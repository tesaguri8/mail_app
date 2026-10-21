//! 差出人は読めたのに日時の無い引用（Thunderbird 日本語版の、日付の行が無い「転送メッセージ」等）を、
//! 差出人と本文で手元のメールと照らし合わせる（docs/THREADING.md §2）。
//!
//! 日時が無いので [`super::quote_match::find_quoted_email`] では探せない。差出人を推定した引用と同じく、
//! その差出人の、このメールより前のメールと本文で比べる（[`super::quote_guess::match_by_body`]）。
//! 見つかれば `message_quotes.matched_email_id` に入れ、会話ビューでは枠にせず畳む。

use rusqlite::{params, Connection, OptionalExtension};

use super::quote_guess::{match_by_body, self_addresses};
use crate::services::quotes::when::parse_quoted_time;

/// 差出人は読めたのに、日時が無い（または読めない）引用か。
pub fn is_undated(quoted_from: Option<&str>, quoted_at: Option<&str>) -> bool {
    quoted_from.is_some_and(|f| !f.trim().is_empty())
        && quoted_at.and_then(parse_quoted_time).is_none()
}

/// 差出人あり・日時なしの引用 1 つを、その差出人の、このメール（`email_id`）より前のメールと本文で
/// 照らし合わせる（指紋の一致か、正規化した本文 20 文字以上が引用に入っていること）。見つからなければ None。
///
/// # Errors
/// 問い合わせに失敗したとき。
pub fn find_undated_quote(
    conn: &Connection,
    email_id: i64,
    from: &str,
    fingerprint: Option<&str>,
    body: &str,
) -> rusqlite::Result<Option<i64>> {
    let date_ts: Option<i64> = conn
        .query_row(
            "SELECT date_ts FROM emails WHERE id = ?1",
            params![email_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    let Some(before_ts) = date_ts else {
        return Ok(None);
    };
    let self_addrs = self_addresses(conn)?;
    match_by_body(
        conn,
        email_id,
        before_ts,
        from,
        &self_addrs,
        fingerprint,
        body,
    )
}

/// 1 通のメールの、差出人あり・日時なしの引用（`blocks` = (block_order, 差出人, 日時, 引用の本文)）を
/// 照らし合わせ、見つかったものに matched_email_id を入れる（取り込みの直後と再解析）。結び付けた数を返す。
/// 日時のある引用・差出人の無い引用・本文の空の引用は飛ばす。
///
/// # Errors
/// 問い合わせ・更新に失敗したとき。
pub fn match_undated_quotes_of_email(
    conn: &Connection,
    email_id: i64,
    blocks: &[(i64, Option<&str>, Option<&str>, &str)],
) -> rusqlite::Result<usize> {
    let mut fp_of = conn.prepare_cached(
        "SELECT fingerprint FROM message_quotes WHERE email_id = ?1 AND block_order = ?2",
    )?;
    let mut up = conn.prepare_cached(
        "UPDATE message_quotes SET matched_email_id = ?3
         WHERE email_id = ?1 AND block_order = ?2 AND matched_email_id IS NULL",
    )?;
    let mut n = 0;
    for &(order, from, at, body) in blocks {
        let Some(from) = from.filter(|_| is_undated(from, at) && !body.trim().is_empty()) else {
            continue;
        };
        let fingerprint: Option<String> = fp_of
            .query_row(params![email_id, order], |r| r.get(0))
            .optional()?
            .flatten();
        if let Some(m) = find_undated_quote(conn, email_id, from, fingerprint.as_deref(), body)? {
            n += up.execute(params![email_id, order, m])?;
        }
    }
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::quotes::split_reply;
    use crate::services::store::emails::{insert_email, NewEmail, NewQuote};
    use crate::services::store::quote_match::unseen_quotes;
    use crate::services::store::threads::reprocess_all_conn;
    use crate::services::store::Store;

    /// Thunderbird 日本語版の転送（見出しに日付の行が無い）。名前・アドレスは作り物。
    const TB_FORWARD: &str = "鈴木様\n\n前田さんからの連絡を転送します。\n\n\
-------- 転送メッセージ --------\n\
件名: \t図面の件\n\
差出人: \t前田 三郎 <maeda@example.invalid>\n\
宛先: \t高橋 花子 <takahashi@example.invalid>\n\
\n\
高橋様\n図面の修正版を送ります。来週の打ち合わせまでにご確認ください。\n\n前田";

    /// 手元にある前田さんのメールの本文（転送の中身と同じ）。
    const ORIGINAL: &str =
        "高橋様\n図面の修正版を送ります。来週の打ち合わせまでにご確認ください。\n\n前田";

    fn store() -> Store {
        let store = Store::open_in_memory_for_test();
        store
            .conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'takahashi@example.invalid','i','s')",
                [],
            )
            .unwrap();
        store
    }

    /// 試験用のメール（RFC 3339 の日時）を入れて id を返す（本文は body_plain と clean_body の両方）。
    fn put(conn: &Connection, key: &str, from: &str, date: &str, body: &str) -> i64 {
        let ts = chrono::DateTime::parse_from_rfc3339(date)
            .unwrap()
            .timestamp();
        conn.execute(
            "INSERT INTO emails (account_id, canonical_key, subject, from_address, date, date_ts,
                                 folder, body_plain, clean_body)
             VALUES (1, ?1, '図面の件', ?2, ?3, ?4, 'inbox', ?5, ?5)",
            params![key, from, date, ts, body],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn matched_of(conn: &Connection, email: i64) -> Vec<Option<i64>> {
        let mut stmt = conn
            .prepare("SELECT matched_email_id FROM message_quotes WHERE email_id = ?1 ORDER BY block_order")
            .unwrap();
        let rows = stmt.query_map(params![email], |r| r.get(0)).unwrap();
        rows.map(Result::unwrap).collect()
    }

    #[test]
    fn detects_undated_quotes() {
        assert!(is_undated(Some("前田 <m@example.invalid>"), None));
        assert!(is_undated(Some("前田"), Some("先週の金曜")));
        assert!(!is_undated(Some("前田"), Some("2026/10/14 12:50")));
        assert!(!is_undated(None, None));
        assert!(!is_undated(Some("  "), None));
    }

    /// 日付の行が無い TB の転送は、差出人の読めた日時不明の 1 通の枠になる（推定の印は付かない）。
    #[test]
    fn tb_forward_without_date_becomes_undated_frame() {
        let store = store();
        let mut conn = store.conn.lock().unwrap();
        let fwd = put(
            &conn,
            "f",
            "suzuki@example.invalid",
            "2026-10-15T09:00:00+09:00",
            TB_FORWARD,
        );
        reprocess_all_conn(&mut conn, 1).unwrap();
        assert_eq!(matched_of(&conn, fwd), vec![None]);
        let frames = unseen_quotes(&conn, fwd, TB_FORWARD, &[]).unwrap();
        assert_eq!(frames.len(), 1, "{frames:?}");
        let f = &frames[0];
        assert_eq!(f.sender, "前田 三郎");
        assert!(!f.guessed);
        assert_eq!((f.at.as_deref(), f.date.as_deref()), (None, None));
        assert_eq!(f.subject.as_deref(), Some("図面の件"));
        assert_eq!(f.body, ORIGINAL);
    }

    /// その差出人の、このメールより前のメールの本文が入っていれば結び付け、枠にしない（再解析の経路）。
    #[test]
    fn undated_quote_matches_earlier_mail_of_that_sender_on_reprocess() {
        let store = store();
        let mut conn = store.conn.lock().unwrap();
        let orig = put(
            &conn,
            "o",
            "maeda@example.invalid",
            "2026-10-14T12:50:00+09:00",
            ORIGINAL,
        );
        let fwd = put(
            &conn,
            "f",
            "suzuki@example.invalid",
            "2026-10-15T09:00:00+09:00",
            TB_FORWARD,
        );
        reprocess_all_conn(&mut conn, 1).unwrap();
        assert_eq!(matched_of(&conn, fwd), vec![Some(orig)]);
        assert!(unseen_quotes(&conn, fwd, TB_FORWARD, &[])
            .unwrap()
            .is_empty());
    }

    /// 結び付けない: 本文が違う・差出人が違う・このメールより後のメール。
    #[test]
    fn undated_quote_does_not_match_other_mail() {
        let store = store();
        let mut conn = store.conn.lock().unwrap();
        put(
            &conn,
            "other-body",
            "maeda@example.invalid",
            "2026-10-14T12:00:00+09:00",
            "高橋様\n別の件で、見積書を送ります。金額をご確認ください。\n\n前田",
        );
        put(
            &conn,
            "other-sender",
            "sato@example.invalid",
            "2026-10-14T12:50:00+09:00",
            ORIGINAL,
        );
        put(
            &conn,
            "later",
            "maeda@example.invalid",
            "2026-10-16T12:50:00+09:00",
            ORIGINAL,
        );
        let fwd = put(
            &conn,
            "f",
            "suzuki@example.invalid",
            "2026-10-15T09:00:00+09:00",
            TB_FORWARD,
        );
        reprocess_all_conn(&mut conn, 1).unwrap();
        assert_eq!(matched_of(&conn, fwd), vec![None]);
        assert_eq!(unseen_quotes(&conn, fwd, TB_FORWARD, &[]).unwrap().len(), 1);
    }

    /// 転送が先に取り込まれ、元のメールが後から入っても（照合を保存していなくても）、会話を開いたときに
    /// 照らし合わせて枠にしない。
    #[test]
    fn undated_quote_is_matched_on_open_when_the_original_comes_later() {
        let store = store();
        let mut conn = store.conn.lock().unwrap();
        let fwd = put(
            &conn,
            "f",
            "suzuki@example.invalid",
            "2026-10-15T09:00:00+09:00",
            TB_FORWARD,
        );
        reprocess_all_conn(&mut conn, 1).unwrap();
        assert_eq!(unseen_quotes(&conn, fwd, TB_FORWARD, &[]).unwrap().len(), 1);
        put(
            &conn,
            "o",
            "maeda@example.invalid",
            "2026-10-14T12:50:00+09:00",
            ORIGINAL,
        );
        assert_eq!(matched_of(&conn, fwd), vec![None]);
        assert!(unseen_quotes(&conn, fwd, TB_FORWARD, &[])
            .unwrap()
            .is_empty());
    }

    /// 取り込みの経路（insert_email）でも、取り込んだ直後に結び付く。
    #[test]
    fn undated_quote_matches_on_insert() {
        let store = store();
        let conn = store.conn.lock().unwrap();
        let orig = put(
            &conn,
            "o",
            "maeda@example.invalid",
            "2026-10-14T12:50:00+09:00",
            ORIGINAL,
        );
        let split = split_reply(TB_FORWARD);
        let quotes = split
            .quotes
            .into_iter()
            .map(|q| NewQuote {
                order: q.order,
                quoted_from: q.quoted_from,
                quoted_at: q.quoted_at,
                fingerprint: q.fingerprint,
                bare_gt: q.bare_gt,
                gt_depth: q.gt_depth,
                body: q.body,
            })
            .collect();
        let e = NewEmail {
            account_id: 1,
            message_id: Some("fwd@example.invalid".into()),
            canonical_key: "fwd@example.invalid".into(),
            subject: Some("Fwd: 図面の件".into()),
            from_address: Some("suzuki@example.invalid".into()),
            from_name: None,
            to_addresses: None,
            to_name: None,
            reply_to: None,
            cc_addresses: None,
            date: Some("2026-10-15T09:00:00+09:00".into()),
            date_ts: Some(
                chrono::DateTime::parse_from_rfc3339("2026-10-15T09:00:00+09:00")
                    .unwrap()
                    .timestamp(),
            ),
            body_plain: Some(TB_FORWARD.into()),
            clean_body: Some(split.clean),
            body_html: None,
            auth_result: None,
            list_id: None,
            in_reply_to: None,
            references_ids: None,
            thread_index: None,
            raw_headers: None,
            has_attachments: false,
            uid: Some(1),
            folder: "inbox".into(),
            is_read: false,
            attachments: Vec::new(),
            quotes,
            verified_self: false,
        };
        insert_email(&conn, &e).unwrap();
        let fwd: i64 = conn
            .query_row(
                "SELECT id FROM emails WHERE canonical_key = 'fwd@example.invalid'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(matched_of(&conn, fwd), vec![Some(orig)]);
        assert!(unseen_quotes(&conn, fwd, TB_FORWARD, &[])
            .unwrap()
            .is_empty());
    }

    /// 日時のある枠と日時の無い枠が混ざっても、日時の無い引用は前の枠に混ざらず、1 通ずつの枠になる
    /// （本文の順。並べ替えは画面の oldestFirst）。
    #[test]
    fn undated_frame_is_separate_from_dated_frames() {
        let store = store();
        let mut conn = store.conn.lock().unwrap();
        let body = "転送します。\n\n\
-----Original Message-----\n\
From: 佐藤 一郎 <sato@example.invalid>\n\
Sent: Friday, October 16, 2026 2:50 PM\n\
Subject: RE: 図面の件\n\
\n\
高橋様\n修正版を確認しました。\n\
\n\
-------- 転送メッセージ --------\n\
件名: \t図面の件\n\
差出人: \t前田 三郎 <maeda@example.invalid>\n\
宛先: \t佐藤 一郎 <sato@example.invalid>\n\
\n\
佐藤様\n図面の修正版を送ります。\n\
\n\
-----Original Message-----\n\
From: 佐藤 一郎 <sato@example.invalid>\n\
Sent: Monday, October 12, 2026 9:00 AM\n\
\n\
前田様\n図面の修正をお願いします。";
        let fwd = put(
            &conn,
            "f",
            "suzuki@example.invalid",
            "2026-10-17T09:00:00+09:00",
            body,
        );
        reprocess_all_conn(&mut conn, 1).unwrap();
        let frames = unseen_quotes(&conn, fwd, body, &[]).unwrap();
        let shape: Vec<(&str, Option<&str>, &str)> = frames
            .iter()
            .map(|f| (f.sender.as_str(), f.date.as_deref(), f.body.as_str()))
            .collect();
        assert_eq!(
            shape,
            vec![
                (
                    "佐藤 一郎",
                    Some("2026-10-16T14:50:00"),
                    "高橋様\n修正版を確認しました。"
                ),
                ("前田 三郎", None, "佐藤様\n図面の修正版を送ります。"),
                (
                    "佐藤 一郎",
                    Some("2026-10-12T09:00:00"),
                    "前田様\n図面の修正をお願いします。"
                ),
            ]
        );
    }
}
