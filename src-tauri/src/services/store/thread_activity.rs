//! 一覧の並び＝会話の「最後のやり取り」（docs/THREADING.md §5）。
//!
//! 自分の返信は送信済みフォルダにあるので、受信箱の代表（フォルダ内の最新）の日時だけで並べると、
//! 返信した会話が相手のメールの時刻の位置に残る。会話ごとに「自分が最後に送った時刻」を
//! `thread_sent_ts` として会話の行（下書きを除く）に持ち、並びの鍵を [`activity_ts`] の式にする。
//! 列を書き直すのは自分の送信が会話に出入りしたときだけ（代表の付け替えでは書かない）。

use rusqlite::{params, Connection};

/// 一覧の並びの鍵＝会話の「最後のやり取り」（docs/THREADING.md §5）。その行の日時と、同じ会話で
/// 自分が最後に送った時刻（`thread_sent_ts`）の新しい方。`p` は列プレフィックス（"" / "r."）。
///
/// 代表の行だけの式索引（`idx_emails_folder_rep_activity`。0071）と**同じ式**でないと索引が効かない
/// ので、一覧・日付の区切りはどれもこの関数で式を作る（`p` は索引を使う問い合わせでは ""）。
/// 日時の無い行は NULL のまま（`max` は引数に NULL があれば NULL）。
pub(super) fn activity_ts(p: &str) -> String {
    format!("max({p}date_ts, coalesce({p}thread_sent_ts, {p}date_ts))")
}

/// 会話の「自分が最後に送った時刻」の数え方: 送信済みフォルダの、迷惑に隔離していない自分の送信。
/// 下書き・ゴミ箱・迷惑は数えない（ゴミ箱へ移した送信は folder が 'trash' になる）。
/// `+folder` で folder の索引を使わせない: (folder, date_ts) の索引で MAX を取ろうとして、
/// 送信済みの全行を新しい順に舐める計画になる（5 万行で 1 会話 8 ms）。会話の索引で引けば数行で済む。
const SENT_OF_THREAD: &str = "SELECT MAX(date_ts) AS ts FROM emails
     WHERE logical_thread_id = ?1 AND +folder = 'sent' AND is_junk = 0";

/// 会話に入った 1 通に、その会話の `thread_sent_ts` を付ける（取り込みの割当ごと。O(会話の送信数)）。
/// 入ったのが自分の送信なら、会話の全部の行を配り直す（受信箱の代表の並びが上がる）。
pub(super) fn note_thread_sent(
    conn: &Connection,
    email_id: i64,
    tid: i64,
    folder: &str,
) -> rusqlite::Result<()> {
    if folder == "sent" {
        return refresh_thread_sent_ts(conn, tid);
    }
    if folder == "drafts" {
        return Ok(());
    }
    conn.execute(
        &format!("UPDATE emails SET thread_sent_ts = ({SENT_OF_THREAD}) WHERE id = ?2"),
        params![tid, email_id],
    )?;
    Ok(())
}

/// 会話の「自分が最後に送った時刻」（`thread_sent_ts`）を、その会話の行（下書きを除く）に配り直す。
/// 自分の送信が会話に出入りしたとき（削除・ゴミ箱・復元・迷惑・分割/結合）に呼ぶ。
/// 値の変わる行だけ書く（大きな会話でも、取り込みのたびに全行を書き直さない）。
///
/// # Errors
/// 更新に失敗したとき。
pub fn refresh_thread_sent_ts(conn: &Connection, tid: i64) -> rusqlite::Result<()> {
    conn.execute(
        &format!(
            "UPDATE emails SET thread_sent_ts = s.ts FROM ({SENT_OF_THREAD}) AS s
             WHERE emails.logical_thread_id = ?1 AND emails.folder <> 'drafts'
               AND emails.thread_sent_ts IS NOT s.ts"
        ),
        params![tid],
    )?;
    Ok(())
}

/// アカウント全体の `thread_sent_ts` を付け直す（再構築の後。スレッドはアカウントごと）。
///
/// # Errors
/// 更新に失敗したとき。
pub fn refresh_thread_sent_ts_for_account(
    conn: &Connection,
    account_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE emails SET thread_sent_ts = NULL
         WHERE account_id = ?1 AND thread_sent_ts IS NOT NULL",
        params![account_id],
    )?;
    conn.execute(
        "UPDATE emails SET thread_sent_ts = s.ts
         FROM (SELECT logical_thread_id AS tid, MAX(date_ts) AS ts FROM emails
               WHERE account_id = ?1 AND folder = 'sent' AND is_junk = 0
                 AND logical_thread_id IS NOT NULL
               GROUP BY logical_thread_id) AS s
         WHERE emails.logical_thread_id = s.tid AND emails.folder <> 'drafts'",
        params![account_id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::store::emails::{insert_email, NewEmail};
    use crate::services::store::threads::process_pending_conn;
    use crate::services::store::Store;

    /// 2026-11-10 00:00 UTC。並びだけを見る試験は時間帯に依らない（日付を見る試験は日を大きく離す）。
    const DAY: i64 = 1_794_268_800;
    const H: i64 = 3600;

    fn test_store() -> Store {
        let store = Store::open_in_memory_for_test();
        store
            .conn
            .lock()
            .unwrap()
            .execute_batch(
                "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'me@example.invalid','i','s');",
            )
            .unwrap();
        store
    }

    /// 1 通。`reply_to` があれば、その Message-ID への返信（同じ会話に入る）。
    fn mail(mid: &str, ts: i64, folder: &str, from: &str, reply_to: Option<&str>) -> NewEmail {
        NewEmail {
            account_id: 1,
            message_id: Some(mid.into()),
            canonical_key: mid.into(),
            subject: Some(format!("件名 {}", mid.split('-').next().unwrap_or(mid))),
            from_address: Some(from.into()),
            from_name: None,
            to_addresses: Some("someone@example.invalid".into()),
            to_name: None,
            reply_to: None,
            cc_addresses: None,
            date: chrono::DateTime::from_timestamp(ts, 0).map(|d| d.to_rfc3339()),
            date_ts: Some(ts),
            body_plain: Some("body".into()),
            clean_body: Some("body".into()),
            body_html: None,
            auth_result: None,
            list_id: None,
            in_reply_to: reply_to.map(str::to_string),
            references_ids: reply_to.map(str::to_string),
            thread_index: None,
            raw_headers: None,
            quotes: vec![],
            has_attachments: false,
            is_read: true,
            uid: None,
            folder: folder.into(),
            verified_self: false,
            attachments: vec![],
        }
    }

    fn add(store: &Store, m: NewEmail) {
        let conn = store.conn.lock().unwrap();
        insert_email(&conn, &m).unwrap();
        process_pending_conn(&conn, 1).unwrap();
    }

    fn id_of(store: &Store, mid: &str, folder: &str) -> i64 {
        store
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT id FROM emails WHERE message_id = ?1 AND folder = ?2",
                params![mid, folder],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// 受信箱の並び（件名）。
    fn inbox_subjects(store: &Store) -> Vec<String> {
        store
            .list_threads(None, "inbox", 100, 0)
            .unwrap()
            .into_iter()
            .map(|t| t.subject.unwrap_or_default())
            .collect()
    }

    /// 相手 A（11:04）→ 別の会話 B（15:00）→ 自分が A に返信（18:25）。
    fn seed(store: &Store) {
        add(
            store,
            mail(
                "a1@example.invalid",
                DAY + 11 * H,
                "inbox",
                "aoi@example.invalid",
                None,
            ),
        );
        add(
            store,
            mail(
                "b1@example.invalid",
                DAY + 15 * H,
                "inbox",
                "ben@example.invalid",
                None,
            ),
        );
    }

    fn reply_a(store: &Store, ts: i64, folder: &str) {
        add(
            store,
            mail(
                "a2@example.invalid",
                ts,
                folder,
                "me@example.invalid",
                Some("a1@example.invalid"),
            ),
        );
    }

    #[test]
    fn replied_conversation_moves_up_and_shows_the_reply_time() {
        let store = test_store();
        seed(&store);
        assert_eq!(
            inbox_subjects(&store),
            ["件名 b1@example.invalid", "件名 a1@example.invalid"]
        );

        reply_a(&store, DAY + 18 * H + 25 * 60, "sent");
        let rows = store.list_threads(None, "inbox", 100, 0).unwrap();
        let subjects: Vec<_> = rows
            .iter()
            .map(|t| t.subject.clone().unwrap_or_default())
            .collect();
        assert_eq!(
            subjects,
            ["件名 a1@example.invalid", "件名 b1@example.invalid"]
        );
        // 時刻は最後のやり取り（自分の返信）。差出人は今までどおり受信箱の代表（相手）。
        let a = &rows[0];
        assert_eq!(
            a.date.as_deref(),
            chrono::DateTime::from_timestamp(DAY + 18 * H + 25 * 60, 0)
                .map(|d| d.to_rfc3339())
                .as_deref()
        );
        assert_eq!(a.from_address.as_deref(), Some("aoi@example.invalid"));
        // 送信済みの一覧は今までどおり（返信 1 通の会話）。
        let sent = store.list_threads(None, "sent", 100, 0).unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].date, a.date);
    }

    #[test]
    fn an_older_reply_does_not_move_the_conversation() {
        let store = test_store();
        seed(&store);
        // 相手の 11:04 より前の自分の送信（会話の始まり）は、並びを変えない。
        add(
            &store,
            mail(
                "a0@example.invalid",
                DAY + 9 * H,
                "sent",
                "me@example.invalid",
                None,
            ),
        );
        add(
            &store,
            mail(
                "a1b@example.invalid",
                DAY + 12 * H,
                "inbox",
                "aoi@example.invalid",
                Some("a0@example.invalid"),
            ),
        );
        let rows = store.list_threads(None, "inbox", 100, 0).unwrap();
        assert_eq!(rows[0].subject.as_deref(), Some("件名 b1@example.invalid"));
        assert_eq!(
            rows[1].date.as_deref(),
            chrono::DateTime::from_timestamp(DAY + 12 * H, 0)
                .map(|d| d.to_rfc3339())
                .as_deref()
        );
    }

    #[test]
    fn drafts_do_not_count() {
        let store = test_store();
        seed(&store);
        reply_a(&store, DAY + 18 * H, "drafts");
        assert_eq!(inbox_subjects(&store)[0], "件名 b1@example.invalid");
    }

    #[test]
    fn trash_restore_junk_and_delete_keep_the_order_fresh() {
        let store = test_store();
        seed(&store);
        reply_a(&store, DAY + 18 * H, "sent");
        let a_first = || inbox_subjects(&store)[0] == "件名 a1@example.invalid";
        assert!(a_first());
        let sent = id_of(&store, "a2@example.invalid", "sent");

        store.move_emails_to_trash(&[sent]).unwrap();
        assert!(!a_first(), "ゴミ箱の自分の送信は数えない");
        store.restore_emails_from_trash(&[sent]).unwrap();
        assert!(a_first(), "戻せばまた上がる");

        store.set_emails_junk(&[sent], true).unwrap();
        assert!(!a_first(), "迷惑に隔離した自分の送信は数えない");
        store.set_emails_junk(&[sent], false).unwrap();
        assert!(a_first());

        store.delete_emails(&[sent]).unwrap();
        assert!(!a_first(), "消せば元の位置へ");
    }

    #[test]
    fn rebuilding_threads_keeps_the_last_activity() {
        let store = test_store();
        seed(&store);
        reply_a(&store, DAY + 18 * H, "sent");
        store.rebuild_threads(1).unwrap();
        assert_eq!(inbox_subjects(&store)[0], "件名 a1@example.invalid");
    }

    #[test]
    fn day_counts_follow_the_last_activity() {
        let store = test_store();
        seed(&store);
        // 返信は 2 日あと（どの時間帯でも別の日）。日付の区切りは返信の日に数える。
        reply_a(&store, DAY + 60 * H, "sent");
        let days = store.thread_day_counts(None, "inbox").unwrap();
        let counts: Vec<i32> = days.iter().map(|d| d.count).collect();
        assert_eq!(counts, [1, 1]);
        let rows = store.list_threads(None, "inbox", 100, 0).unwrap();
        assert_eq!(rows[0].subject.as_deref(), Some("件名 a1@example.invalid"));
        // 先頭の日（新しい日）の 1 件が、返信した会話。
        let key = |ts: i64| -> Option<String> {
            store
                .conn
                .lock()
                .unwrap()
                .query_row(
                    "SELECT date(?1, 'unixepoch', 'localtime')",
                    params![ts],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(days[0].date_key, key(DAY + 60 * H));
        // 未読の日も同じ式で数える。
        let a1 = id_of(&store, "a1@example.invalid", "inbox");
        store.set_emails_read(&[a1], false).unwrap();
        let unread = store.thread_day_unread(None, "inbox").unwrap();
        assert_eq!(unread.len(), 1);
        assert_eq!(unread[0].date_key, key(DAY + 60 * H));
    }

    #[test]
    fn unthreaded_rows_keep_their_own_date() {
        let store = test_store();
        let conn = store.conn.lock().unwrap();
        // 割当前（process_pending 前）の行は、自分の日時だけで並ぶ。
        insert_email(
            &conn,
            &mail(
                "x1@example.invalid",
                DAY + 10 * H,
                "inbox",
                "x@example.invalid",
                None,
            ),
        )
        .unwrap();
        insert_email(
            &conn,
            &mail(
                "x2@example.invalid",
                DAY + 20 * H,
                "sent",
                "me@example.invalid",
                Some("x1@example.invalid"),
            ),
        )
        .unwrap();
        drop(conn);
        let rows = store.list_threads(None, "inbox", 100, 0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(
            rows[0].date.as_deref(),
            chrono::DateTime::from_timestamp(DAY + 10 * H, 0)
                .map(|d| d.to_rfc3339())
                .as_deref()
        );
    }

    /// 一覧の先頭 N 件は、並びの式索引で引く（全件を並べ替えない）。
    #[test]
    fn the_page_query_uses_the_activity_index() {
        let store = test_store();
        let conn = store.conn.lock().unwrap();
        let plan: Vec<String> = conn
            .prepare(&format!(
                "EXPLAIN QUERY PLAN SELECT id FROM emails
                 WHERE folder = ?1 AND is_junk = 0 AND is_folder_rep = 1
                 ORDER BY {} DESC, id DESC LIMIT 100",
                activity_ts("")
            ))
            .unwrap()
            .query_map(params!["inbox"], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        let plan = plan.join(" / ");
        assert!(plan.contains("idx_emails_folder_rep_activity"), "{plan}");
        assert!(!plan.contains("TEMP B-TREE FOR ORDER BY"), "{plan}");
    }
}
