//! 差出人の行が無い「>」だけの引用に、返信の相手から推定した差出人を入れ、手元のメールと
//! 照らし合わせる（docs/THREADING.md §2。規則は `quotes::guess`）。
//!
//! 推定した差出人は `message_quotes.guessed_from` に入れる（読めた `quoted_from` は上書きしない）。
//! 日時は分からないので、照合はその相手から届いた、このメールより前のメールに限り、本文で比べる。

use rusqlite::{params, Connection, OptionalExtension};

use super::quote_match::{normalize_body, MIN_KNOWN_CHARS};
use crate::services::quotes::guess::{guess_quote_sender, ReplyContext};
use crate::services::quotes::sender::split_sender;

/// 照合の相手として見る、推定した差出人の直近のメールの数。
const MATCH_CANDIDATES: i64 = 50;

/// 推定に使うメールの列（件名・返信の印・差出人・宛先・Cc・List-Id・日時）。
type EmailRow = (
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
);

/// 自分のアドレス（すべてのアカウントとプロフィール。小文字）。
fn self_addresses(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT lower(trim(email)) FROM accounts WHERE email IS NOT NULL
         UNION SELECT lower(trim(email)) FROM account_profiles WHERE email IS NOT NULL",
    )?;
    let rows = stmt.query_map([], |r| r.get(0))?;
    rows.collect()
}

/// 推定した差出人の、このメールより前のメールのうち、本文が引用に入っているもの（新しい順で最初）。
fn match_guessed(
    conn: &Connection,
    email_id: i64,
    before_ts: i64,
    guessed: &str,
    fingerprint: Option<&str>,
    quote_body: &str,
) -> rusqlite::Result<Option<i64>> {
    let Some(addr) = split_sender(guessed).0 else {
        return Ok(None);
    };
    let quote = normalize_body(quote_body);
    let mut stmt = conn.prepare_cached(
        "SELECT id, clean_body, body_fingerprint FROM emails
         WHERE lower(trim(from_address)) = ?1 AND date_ts < ?2 AND id <> ?3 AND folder <> 'drafts'
         ORDER BY date_ts DESC LIMIT ?4",
    )?;
    let rows = stmt.query_map(params![addr, before_ts, email_id, MATCH_CANDIDATES], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, Option<String>>(1)?,
            r.get::<_, Option<String>>(2)?,
        ))
    })?;
    for row in rows {
        let (id, clean, fp) = row?;
        if fingerprint.is_some_and(|f| fp.as_deref() == Some(f)) {
            return Ok(Some(id));
        }
        let clean = clean.as_deref().map(normalize_body).unwrap_or_default();
        if clean.chars().count() >= MIN_KNOWN_CHARS && quote.contains(&clean) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// 1 通のメールの、差出人の行が無い「>」だけの引用（`blocks` = (block_order, 引用の本文)）に、
/// 推定した差出人を入れ、照らし合わせる。推定は 1 段目（いちばん新しい引用＝block_order 0）だけ。
/// 推定した数を返す。
///
/// # Errors
/// 問い合わせ・更新に失敗したとき。
pub fn guess_quotes_of_email(
    conn: &Connection,
    email_id: i64,
    blocks: &[(i64, &str)],
) -> rusqlite::Result<usize> {
    let Some(&(order, body)) = blocks.first().filter(|(o, _)| *o == 0) else {
        return Ok(0);
    };
    let row: Option<EmailRow> = conn
        .query_row(
            "SELECT subject, in_reply_to, references_ids, from_address, from_name, to_addresses,
                    cc_addresses, list_id, date_ts
             FROM emails WHERE id = ?1",
            params![email_id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                    r.get(6)?,
                    r.get(7)?,
                    r.get(8)?,
                ))
            },
        )
        .optional()?;
    let Some((subject, irt, refs, from, from_name, to, cc, list_id, date_ts)) = row else {
        return Ok(0);
    };
    let ctx = ReplyContext {
        subject: subject.as_deref(),
        in_reply_to: irt.as_deref(),
        references: refs.as_deref(),
        from_address: from.as_deref(),
        from_name: from_name.as_deref(),
        to: to.as_deref(),
        cc: cc.as_deref(),
        list_id: list_id.as_deref(),
    };
    let Some(guessed) = guess_quote_sender(&ctx, &self_addresses(conn)?) else {
        return Ok(0);
    };
    let fingerprint: Option<String> = conn
        .query_row(
            "SELECT fingerprint FROM message_quotes WHERE email_id = ?1 AND block_order = ?2",
            params![email_id, order],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let matched = match date_ts {
        Some(ts) => match_guessed(conn, email_id, ts, &guessed, fingerprint.as_deref(), body)?,
        None => None,
    };
    let n = conn.execute(
        "UPDATE message_quotes SET guessed_from = ?3, matched_email_id = COALESCE(?4, matched_email_id)
         WHERE email_id = ?1 AND block_order = ?2 AND quoted_from IS NULL",
        params![email_id, order, guessed, matched],
    )?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::store::quote_match::unseen_quotes;
    use crate::services::store::threads::reprocess_all_conn;
    use crate::services::store::Store;

    /// 試験用のメール（RFC 3339 の日時）を入れて id を返す。
    #[allow(clippy::too_many_arguments)] // 試験の作り物の列を 1 行で並べるため
    fn put(
        conn: &Connection,
        key: &str,
        subject: &str,
        from: &str,
        to: &str,
        in_reply_to: Option<&str>,
        date: &str,
        body: &str,
    ) -> i64 {
        let ts = chrono::DateTime::parse_from_rfc3339(date)
            .unwrap()
            .timestamp();
        conn.execute(
            "INSERT INTO emails (account_id, canonical_key, subject, from_address, to_addresses,
                                 in_reply_to, date, date_ts, folder, body_plain, clean_body)
             VALUES (1, ?1, ?2, ?3, ?4, ?5, ?6, ?7, 'inbox', ?8, ?8)",
            params![key, subject, from, to, in_reply_to, date, ts, body],
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
                "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'me@example.jp','i','s')",
                [],
            )
            .unwrap();
        store
    }

    fn guessed_of(conn: &Connection, email: i64) -> (Option<String>, Option<i64>) {
        conn.query_row(
            "SELECT guessed_from, matched_email_id FROM message_quotes
             WHERE email_id = ?1 AND block_order = 0",
            params![email],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap()
    }

    const REPLY: &str = "承知しました。\n\n> 来週の打ち合わせは火曜の午後でいかがでしょうか。\n> 資料は前日までにお送りします。";

    /// 自分の返信（宛先が 1 人）の「>」だけの引用は、宛先の人と推定し、日時不明の枠になる。
    #[test]
    fn own_reply_quote_becomes_guessed_frame() {
        let store = store();
        let mut conn = store.conn.lock().unwrap();
        let reply = put(
            &conn,
            "r",
            "Re: 打ち合わせ",
            "me@example.jp",
            "佐藤 一郎 <sato@example.jp>",
            Some("<orig@example.jp>"),
            "2026-10-15T09:00:00+09:00",
            REPLY,
        );
        reprocess_all_conn(&mut conn, 1).unwrap();
        assert_eq!(
            guessed_of(&conn, reply),
            (Some("佐藤 一郎 <sato@example.jp>".into()), None)
        );
        let frames = unseen_quotes(&conn, reply, REPLY, &[]).unwrap();
        assert_eq!(frames.len(), 1);
        assert!(frames[0].guessed);
        assert_eq!(frames[0].sender, "佐藤 一郎");
        assert_eq!(
            (frames[0].date.as_deref(), frames[0].at.as_deref()),
            (None, None)
        );
    }

    /// 推定した相手の、このメールより前のメールの本文が引用に入っていれば結び付け、枠にしない。
    #[test]
    fn guessed_quote_matches_earlier_mail_of_that_person() {
        let store = store();
        let mut conn = store.conn.lock().unwrap();
        let orig = put(
            &conn,
            "o",
            "打ち合わせ",
            "sato@example.jp",
            "me@example.jp",
            None,
            "2026-10-14T18:00:00+09:00",
            "来週の打ち合わせは火曜の午後でいかがでしょうか。\n資料は前日までにお送りします。",
        );
        let reply = put(
            &conn,
            "r",
            "Re: 打ち合わせ",
            "me@example.jp",
            "sato@example.jp",
            None,
            "2026-10-15T09:00:00+09:00",
            REPLY,
        );
        reprocess_all_conn(&mut conn, 1).unwrap();
        assert_eq!(
            guessed_of(&conn, reply),
            (Some("sato@example.jp".into()), Some(orig))
        );
        assert!(unseen_quotes(&conn, reply, REPLY, &[]).unwrap().is_empty());
    }

    /// 転送の「>」は推定しない（枠にもしない）。
    #[test]
    fn forwarded_gt_quote_is_not_guessed() {
        let store = store();
        let mut conn = store.conn.lock().unwrap();
        let fwd = put(
            &conn,
            "f",
            "Fwd: 打ち合わせ",
            "me@example.jp",
            "sato@example.jp",
            Some("<orig@example.jp>"),
            "2026-10-15T09:00:00+09:00",
            REPLY,
        );
        reprocess_all_conn(&mut conn, 1).unwrap();
        assert_eq!(guessed_of(&conn, fwd), (None, None));
        assert!(unseen_quotes(&conn, fwd, REPLY, &[]).unwrap().is_empty());
    }
}
