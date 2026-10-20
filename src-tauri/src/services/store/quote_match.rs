//! 引用ブロックを手元のメールと照らし合わせる（docs/THREADING.md §2「引用属性行に (from + 時刻) が埋まっている」）。
//!
//! 引用の見出しの差出人と日時で手元のメールを探し、見つかれば `message_quotes.matched_email_id`
//! に入れる。見つからない引用は「このメールで初めて届いた内容」なので、会話ビューで畳まずに出す。

use chrono::{DateTime, NaiveDateTime};
use rusqlite::{params, Connection};

use crate::models::{ThreadMessage, UnseenQuote};
use crate::services::quotes::{
    self,
    sender::split_sender,
    when::{parse_quoted_time, QuotedTime},
};

/// 時刻の許容幅（秒）。Outlook の "Sent:" は秒を落とし、分も丸めることがある。
const TOLERANCE_SECS: i64 = 120;

/// 時差の最大（秒）。時差の書かれていない日時は、この幅で候補を引いてから送信者の時刻で比べる。
const MAX_OFFSET_SECS: i64 = 14 * 3600;

/// 名前を比べる形にそろえる（空白・引用符を除き、小文字に）。
fn normalize_name(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '"' | '\'' | '“' | '”'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// 候補のメール 1 通。
struct Candidate {
    id: i64,
    from_address: Option<String>,
    from_name: Option<String>,
    /// RFC 3339（送信者の時差つき）。
    date: Option<String>,
    date_ts: i64,
}

/// 候補が引用の差出人と同じ人か（アドレスか、名前が一致）。
fn same_sender(addr: Option<&str>, name: Option<&str>, c: &Candidate) -> bool {
    let by_addr = addr.is_some_and(|a| {
        c.from_address
            .as_deref()
            .is_some_and(|f| f.trim().eq_ignore_ascii_case(a))
    });
    let by_name = name.is_some_and(|n| {
        c.from_name
            .as_deref()
            .is_some_and(|f| !f.trim().is_empty() && normalize_name(f) == normalize_name(n))
    });
    by_addr || by_name
}

/// 候補の時刻と引用の時刻の差（秒）。時差が書かれていれば UTC で、無ければ送信者の時刻どうしで比べる。
fn time_gap(t: &QuotedTime, c: &Candidate) -> Option<i64> {
    match t.offset_secs {
        Some(o) => Some((c.date_ts - (t.local.and_utc().timestamp() - i64::from(o))).abs()),
        None => {
            let local: NaiveDateTime = DateTime::parse_from_rfc3339(c.date.as_deref()?)
                .ok()?
                .naive_local();
            Some((local - t.local).num_seconds().abs())
        }
    }
}

/// 引用元の差出人と日時から、手元のメールを探す（自分自身と下書きは除く）。
/// どちらかが無い・読めないときや、見つからないときは None。複数あれば時刻の近いもの。
///
/// # Errors
/// 問い合わせに失敗したとき。
pub fn find_quoted_email(
    conn: &Connection,
    self_id: i64,
    from: Option<&str>,
    at: Option<&str>,
) -> rusqlite::Result<Option<i64>> {
    let (Some(from), Some(t)) = (from, at.and_then(parse_quoted_time)) else {
        return Ok(None);
    };
    let (addr, name) = split_sender(from);
    if addr.is_none() && name.is_none() {
        return Ok(None);
    }
    let naive = t.local.and_utc().timestamp();
    let (lo, hi) = match t.offset_secs {
        Some(o) => {
            let utc = naive - i64::from(o);
            (utc - TOLERANCE_SECS, utc + TOLERANCE_SECS)
        }
        None => (
            naive - MAX_OFFSET_SECS - TOLERANCE_SECS,
            naive + MAX_OFFSET_SECS + TOLERANCE_SECS,
        ),
    };
    let mut stmt = conn.prepare_cached(
        "SELECT id, from_address, from_name, date, date_ts FROM emails
         WHERE date_ts BETWEEN ?1 AND ?2 AND id <> ?3 AND folder <> 'drafts'",
    )?;
    let candidates = stmt.query_map(params![lo, hi, self_id], |r| {
        Ok(Candidate {
            id: r.get(0)?,
            from_address: r.get(1)?,
            from_name: r.get(2)?,
            date: r.get(3)?,
            date_ts: r.get(4)?,
        })
    })?;
    let candidates = candidates.collect::<rusqlite::Result<Vec<_>>>()?;
    let best = candidates
        .iter()
        .filter(|c| same_sender(addr.as_deref(), name.as_deref(), c))
        .filter_map(|c| {
            time_gap(&t, c)
                .filter(|g| *g <= TOLERANCE_SECS)
                .map(|g| (g, c.id))
        })
        .min_by_key(|(g, _)| *g);
    Ok(best.map(|(_, id)| id))
}

/// まだ結び付いていない引用ブロック（email_id, block_order, quoted_from, quoted_at）を照らし合わせ、
/// 見つかったものに matched_email_id を入れる。結び付けた数を返す。
fn match_rows(
    conn: &Connection,
    rows: Vec<(i64, i64, Option<String>, Option<String>)>,
) -> rusqlite::Result<usize> {
    let mut up = conn.prepare_cached(
        "UPDATE message_quotes SET matched_email_id = ?3 WHERE email_id = ?1 AND block_order = ?2",
    )?;
    let mut n = 0;
    for (email_id, order, from, at) in rows {
        if let Some(m) = find_quoted_email(conn, email_id, from.as_deref(), at.as_deref())? {
            up.execute(params![email_id, order, m])?;
            n += 1;
        }
    }
    Ok(n)
}

/// 1 通のメールの引用ブロックを照らし合わせる（取り込みの直後）。結び付けた数を返す。
///
/// # Errors
/// 問い合わせ・更新に失敗したとき。
pub fn match_quotes_of_email(conn: &Connection, email_id: i64) -> rusqlite::Result<usize> {
    let rows = {
        let mut stmt = conn.prepare_cached(
            "SELECT email_id, block_order, quoted_from, quoted_at FROM message_quotes
             WHERE email_id = ?1 AND matched_email_id IS NULL AND quoted_at IS NOT NULL",
        )?;
        let mapped = stmt.query_map(params![email_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    };
    match_rows(conn, rows)
}

/// アカウントのメールの引用ブロックをまとめて照らし合わせる（ローカル再解析のあと）。結び付けた数を返す。
///
/// # Errors
/// 問い合わせ・更新に失敗したとき。
pub fn match_quotes_of_account(conn: &Connection, account_id: i64) -> rusqlite::Result<usize> {
    let rows = {
        let mut stmt = conn.prepare(
            "SELECT q.email_id, q.block_order, q.quoted_from, q.quoted_at
             FROM message_quotes q JOIN emails e ON e.id = q.email_id
             WHERE e.account_id = ?1 AND q.matched_email_id IS NULL AND q.quoted_at IS NOT NULL",
        )?;
        let mapped = stmt.query_map(params![account_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?;
        mapped.collect::<rusqlite::Result<Vec<_>>>()?
    };
    match_rows(conn, rows)
}

/// 同じ会話にあるとみなす本文の最短（正規化後の文字数）。挨拶だけの短い本文で一致させないため。
const MIN_KNOWN_CHARS: usize = 20;

/// 引用の本文を比べる形にそろえる（行頭の `>` と空白を落とし、1 本につなぐ）。
fn normalize_body(text: &str) -> String {
    text.lines()
        .map(quotes::anchor::normalize_quote_line)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// 会話の各メールの新規部分（正規化済み）。引用の本文がこれを含むなら、その引用は同じ会話に
/// 既にある（照らし合わせが外れても、手元に無いとはみなさない）。
pub fn thread_bodies(messages: &[ThreadMessage]) -> Vec<String> {
    messages
        .iter()
        .filter_map(|m| m.clean_body.as_deref())
        .map(normalize_body)
        .filter(|b| b.chars().count() >= MIN_KNOWN_CHARS)
        .collect()
}

/// メール 1 通の本文から、手元に無いメールの引用を拾う。差出人と日時の両方が読めて、
/// 照らし合わせで見つからず、同じ会話（`known`。[`thread_bodies`]）にも無いものだけ。
///
/// 差出人・日時の分からない引用（"…が書きました:" や、読めない見出しの転送）は、1 通の枠に
/// できない。直前の枠を出していれば、その続きとして見出しごと残す（黙って捨てない）。直前の
/// 1 通が手元にある（または最初の引用）ときは、そのメールの側で読めるので、いままでどおり畳む。
///
/// # Errors
/// 照らし合わせの問い合わせに失敗したとき。
pub fn unseen_quotes(
    conn: &Connection,
    email_id: i64,
    body_plain: &str,
    known: &[String],
) -> rusqlite::Result<Vec<UnseenQuote>> {
    let mut out: Vec<UnseenQuote> = Vec::new();
    // 直前の引用を枠として出したか（読めない引用をその枠に残すため）。
    let mut last_shown = false;
    for q in quotes::split_reply(body_plain).quotes {
        let time = q.quoted_at.as_deref().and_then(parse_quoted_time);
        let (Some(from), Some(at), Some(time)) = (q.quoted_from, q.quoted_at, time) else {
            if let Some(prev) = out.last_mut().filter(|_| last_shown && !q.text.is_empty()) {
                prev.body.push_str("\n\n");
                prev.body.push_str(&q.text);
            }
            continue;
        };
        last_shown = false;
        if q.body.is_empty() {
            continue;
        }
        let norm = normalize_body(&q.body);
        if known.iter().any(|k| norm.contains(k.as_str())) {
            continue;
        }
        if find_quoted_email(conn, email_id, Some(&from), Some(&at))?.is_some() {
            continue;
        }
        last_shown = true;
        out.push(UnseenQuote {
            sender: quotes::sender::sender_label(&from),
            date: time.to_iso(),
            from,
            at,
            subject: q.subject,
            body: q.body,
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::store::Store;

    /// 試験用のメール（差出人・名前・RFC 3339 の日時・フォルダ）を入れて id を返す。
    fn put(
        conn: &Connection,
        key: &str,
        from: &str,
        name: Option<&str>,
        date: &str,
        folder: &str,
    ) -> i64 {
        let ts = DateTime::parse_from_rfc3339(date).unwrap().timestamp();
        conn.execute(
            "INSERT INTO emails (account_id, canonical_key, from_address, from_name, date, date_ts, folder)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6)",
            params![key, from, name, date, ts, folder],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn store() -> Store {
        let store = Store::open_in_memory_for_test();
        store
            .conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'me@example.com','i','s')",
                [],
            )
            .unwrap();
        store
    }

    #[test]
    fn finds_by_address_and_local_time_without_offset() {
        let store = store();
        let conn = store.conn.lock().unwrap();
        let orig = put(
            &conn,
            "a",
            "tanaka@example.jp",
            None,
            "2026-10-02T17:55:41+09:00",
            "inbox",
        );
        // 同じ人の 1 時間後のメールと、同じ時刻の別人は選ばない。
        put(
            &conn,
            "b",
            "tanaka@example.jp",
            None,
            "2026-10-02T18:55:00+09:00",
            "inbox",
        );
        put(
            &conn,
            "c",
            "other@example.jp",
            None,
            "2026-10-02T17:55:00+09:00",
            "inbox",
        );
        let fwd = put(
            &conn,
            "d",
            "suzuki@example.jp",
            None,
            "2026-10-03T12:37:00+09:00",
            "inbox",
        );
        let found = find_quoted_email(
            &conn,
            fwd,
            Some("田中 一郎 <Tanaka@Example.jp>"),
            Some("Friday, October 2, 2026 5:55 PM"),
        )
        .unwrap();
        assert_eq!(found, Some(orig));
    }

    /// 太字の Outlook 見出し（"*From:*" / "*Sent:*"）から取った差出人・日時でも照らし合わせられる。
    #[test]
    fn finds_by_bold_outlook_header() {
        let store = store();
        let conn = store.conn.lock().unwrap();
        let orig = put(
            &conn,
            "a",
            "i-sato@kensetsu.example.co.jp",
            Some("佐藤一郎"),
            "2026-10-06T11:28:37+09:00",
            "inbox",
        );
        let fwd = put(
            &conn,
            "b",
            "hanako@mokuzai.example.jp",
            None,
            "2026-10-07T19:36:00+09:00",
            "inbox",
        );
        let body = "山田です。\n\n\
*From:*佐藤一郎<i-sato@kensetsu.example.co.jp>\n\
*Sent:* Tuesday, October 6, 2026 11:28 AM\n\
*To:* 山田花子<hanako@mokuzai.example.jp>\n\
\n\
山田様\n図面を送ります。";
        let split = quotes::split_reply(body);
        assert_eq!(split.quotes.len(), 1);
        let q = &split.quotes[0];
        let found =
            find_quoted_email(&conn, fwd, q.quoted_from.as_deref(), q.quoted_at.as_deref())
                .unwrap();
        assert_eq!(found, Some(orig));
    }

    #[test]
    fn finds_by_name_when_outlook_drops_the_address_and_skips_drafts() {
        let store = store();
        let conn = store.conn.lock().unwrap();
        put(
            &conn,
            "x",
            "takahashi@example.com",
            Some("高橋 花子"),
            "2026-10-02T14:50:00+09:00",
            "drafts",
        );
        let orig = put(
            &conn,
            "a",
            "takahashi@example.com",
            Some("高橋　花子"),
            "2026-10-02T14:50:10+09:00",
            "sent",
        );
        let found =
            find_quoted_email(&conn, 0, Some("高橋 花子"), Some("2026年10月2日 14:50")).unwrap();
        assert_eq!(found, Some(orig));
    }

    #[test]
    fn uses_utc_when_the_offset_is_written_and_gives_up_without_a_match() {
        let store = store();
        let conn = store.conn.lock().unwrap();
        let orig = put(
            &conn,
            "a",
            "jiro@example.com",
            None,
            "2026-10-01T15:00:00+00:00",
            "inbox",
        );
        let at = Some("Fri, 2 Oct 2026 00:00:30 +0900");
        assert_eq!(
            find_quoted_email(&conn, 0, Some("<jiro@example.com>"), at).unwrap(),
            Some(orig)
        );
        // 時刻が読めない・差出人が違う・手元に無いときは結び付けない。
        assert_eq!(
            find_quoted_email(&conn, 0, Some("<jiro@example.com>"), Some("昨日")).unwrap(),
            None
        );
        assert_eq!(
            find_quoted_email(&conn, 0, Some("<saburo@example.com>"), at).unwrap(),
            None
        );
        assert_eq!(
            find_quoted_email(
                &conn,
                0,
                Some("<jiro@example.com>"),
                Some("Fri, 2 Oct 2026 03:00:00 +0900")
            )
            .unwrap(),
            None
        );
    }

    #[test]
    fn match_quotes_of_email_fills_only_the_found_blocks() {
        let store = store();
        let conn = store.conn.lock().unwrap();
        let orig = put(
            &conn,
            "a",
            "tanaka@example.jp",
            None,
            "2026-10-02T17:55:00+09:00",
            "inbox",
        );
        let fwd = put(
            &conn,
            "d",
            "suzuki@example.jp",
            None,
            "2026-10-03T12:37:00+09:00",
            "inbox",
        );
        conn.execute(
            "INSERT INTO message_quotes (email_id, block_order, quoted_from, quoted_at, fingerprint) VALUES
             (?1, 0, 'tanaka@example.jp', 'Friday, October 2, 2026 5:55 PM', 'f0'),
             (?1, 1, 'takahashi@example.com', 'Friday, October 2, 2026 2:50 PM', 'f1')",
            params![fwd],
        )
        .unwrap();
        assert_eq!(match_quotes_of_email(&conn, fwd).unwrap(), 1);
        let matched: Vec<Option<i64>> = conn
            .prepare("SELECT matched_email_id FROM message_quotes WHERE email_id = ?1 ORDER BY block_order")
            .unwrap()
            .query_map(params![fwd], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(matched, vec![Some(orig), None]);
        assert_eq!(
            match_quotes_of_account(&conn, 1).unwrap(),
            0,
            "手元に無い 1 通は結び付かないまま"
        );
    }

    /// 1 通として読めない引用（見出しの無い区切り）は、直前の枠の続きとして残す（名前は作り物）。
    #[test]
    fn unreadable_quote_stays_in_the_previous_frame() {
        let store = store();
        let conn = store.conn.lock().unwrap();
        let body = "転送します。\n\n\
-----Original Message-----\n\
From: 高橋 花子 <takahashi@example.com>\n\
Sent: Friday, October 2, 2026 2:50 PM\n\
Subject: 図面\n\
\n\
田中様\n図面を送ります。\n\
\n\
-----Original Message-----\n\
見出しの読めない 1 通\nhttps://files.example.jp/d/xyz";
        let quotes = unseen_quotes(&conn, 0, body, &[]).unwrap();
        assert_eq!(quotes.len(), 1, "{quotes:?}");
        let q = &quotes[0];
        assert_eq!(q.sender, "高橋 花子");
        assert!(
            q.body.starts_with("田中様\n図面を送ります。"),
            "{:?}",
            q.body
        );
        assert!(q.body.contains("-----Original Message-----"));
        assert!(q.body.contains("https://files.example.jp/d/xyz"));
    }

    /// Outlook の見出しの間に挟まった iPhone の「日付、名前 <addr> のメール:」も、日時が読めるので
    /// 1 通の枠になる（名前は作り物）。
    #[test]
    fn iphone_date_first_attribution_becomes_its_own_frame() {
        let store = store();
        let conn = store.conn.lock().unwrap();
        let body = "了解しました。\n\n\
-----Original Message-----\n\
From: 高橋 花子 <takahashi@example.com>\n\
Sent: Friday, October 2, 2026 5:55 PM\n\
Subject: RE: 打ち合わせ\n\
\n\
佐藤様\n10/2 夕方の本文です。\n\
\n\
2026/10/02 12:10、佐藤 一郎 <ichiro@example.jp> のメール:\n\
\n\
> 高橋様\n> 10/2 昼の本文です。\n\
\n\
-----Original Message-----\n\
From: 鈴木 次郎 <jiro@example.com>\n\
Sent: Thursday, October 1, 2026 9:00 AM\n\
\n\
佐藤様\n10/1 の本文です。";
        let quotes = unseen_quotes(&conn, 0, body, &[]).unwrap();
        let senders: Vec<&str> = quotes.iter().map(|q| q.sender.as_str()).collect();
        assert_eq!(senders, ["高橋 花子", "佐藤 一郎", "鈴木 次郎"]);
        assert_eq!(quotes[1].date, "2026-10-02T12:10:00");
        assert_eq!(quotes[0].body, "佐藤様\n10/2 夕方の本文です。");
    }
}
