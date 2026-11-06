//! スレッド一覧の、日ごとのスレッド数（メール一覧の日付のつまみ）。docs/MAIL_LIST_TIMELINE.md。
//!
//! 一覧（`list_threads`）は `ORDER BY date_ts DESC, id DESC` で並ぶ。日付のキーはその並びの式の先頭
//! （`date_ts` を端末ローカルの日付にしたもの）から取るので、同じ日のスレッドは一覧の中で連続し、
//! **日ごとの件数の累計が、そのまま一覧の `OFFSET` になる**（試験 `day_offsets_match_list_paging`）。

use rusqlite::params;

use super::emails::folder_predicate;
use super::Store;
use crate::models::{ThreadDayCount, ThreadDayUnread};

impl Store {
    /// フォルダ内のスレッドを、代表メールの日付（端末ローカル）ごとに数える。
    ///
    /// 絞り込み（アカウント・フォルダ・迷惑の隔離・代表フラグ）は `list_threads` と同じ。
    /// 並びは一覧と同じ新しい順で、日時の無いスレッド（`date_key` = None）は最後に 1 つにまとめる。
    /// `account_id` が None なら全アカウント横断。
    ///
    /// ローカル日付は SQLite の `'localtime'`（このプロセスの時間帯＝WebView と同じ機械の設定）で出す。
    pub fn thread_day_counts(
        &self,
        account_id: Option<i64>,
        folder: &str,
    ) -> rusqlite::Result<Vec<ThreadDayCount>> {
        // 参照専用接続（一覧の読み取りと同じく、書き込みに待たされない）。
        let conn = self.read_conn.lock().unwrap();
        let acct = if account_id.is_some() {
            "AND account_id = ?2"
        } else {
            ""
        };
        // ORDER BY は一覧の並び（date_ts DESC。NULL は最後）に合わせる。
        let sql = format!(
            "SELECT date(date_ts, 'unixepoch', 'localtime') AS d, count(*)
             FROM emails
             WHERE {fp} {acct} AND is_folder_rep = 1
             GROUP BY d
             ORDER BY d IS NULL, d DESC",
            fp = folder_predicate(folder, "", "?1"),
        );
        let mut stmt = conn.prepare(&sql)?;
        let map = |r: &rusqlite::Row| -> rusqlite::Result<ThreadDayCount> {
            Ok(ThreadDayCount {
                date_key: r.get(0)?,
                count: r.get::<_, i64>(1)? as i32,
            })
        };
        query_by_account(&mut stmt, folder, account_id, map)
    }

    /// フォルダ内の未読のあるスレッドを、代表メールの日付（端末ローカル）ごとに数える（未読のある日だけ）。
    ///
    /// 未読の判定は一覧の未読（`list_threads` の `unread_cnt > 0`: そのフォルダのスレッドのメールに
    /// 未読が 1 通でもある）と同じ。代表の行ごとに未読を探すと全スレッドぶん引くことになり遅い
    /// （`[実測]` vaio の約 2.8 万スレッドで件数だけの 4〜5 倍）ので、**未読のメールから逆に引く**:
    /// 未読だけの部分索引（`idx_emails_unread`）で未読のスレッドを集め、その代表の日付で数える。
    pub fn thread_day_unread(
        &self,
        account_id: Option<i64>,
        folder: &str,
    ) -> rusqlite::Result<Vec<ThreadDayUnread>> {
        let conn = self.read_conn.lock().unwrap();
        let (acct_t, acct) = if account_id.is_some() {
            ("AND t.account_id = ?2", "AND account_id = ?2")
        } else {
            ("", "")
        };
        // 論理スレッドのある未読 → そのフォルダの代表。論理スレッドの無い行は、その行が代表で未読なら数える。
        let sql = format!(
            "SELECT date(r.date_ts, 'unixepoch', 'localtime') AS d, count(*)
             FROM emails r
             WHERE r.id IN (
                 SELECT r2.id FROM emails t
                 JOIN emails r2 INDEXED BY idx_emails_thread_folder
                   ON r2.logical_thread_id = t.logical_thread_id AND {fp_r2} AND r2.is_folder_rep = 1
                 WHERE {fp_t} {acct_t} AND t.is_read = 0 AND t.logical_thread_id IS NOT NULL
                 UNION
                 SELECT id FROM emails
                 WHERE {fp} {acct} AND is_read = 0 AND logical_thread_id IS NULL AND is_folder_rep = 1
             )
             GROUP BY d
             ORDER BY d IS NULL, d DESC",
            fp = folder_predicate(folder, "", "?1"),
            fp_t = folder_predicate(folder, "t.", "?1"),
            fp_r2 = folder_predicate(folder, "r2.", "?1"),
        );
        let mut stmt = conn.prepare(&sql)?;
        let map = |r: &rusqlite::Row| -> rusqlite::Result<ThreadDayUnread> {
            Ok(ThreadDayUnread {
                date_key: r.get(0)?,
                unread: r.get::<_, i64>(1)? as i32,
            })
        };
        query_by_account(&mut stmt, folder, account_id, map)
    }
}

/// `?1` = フォルダ・`?2` = アカウント（あれば）で問い合わせを流す。
fn query_by_account<T>(
    stmt: &mut rusqlite::Statement<'_>,
    folder: &str,
    account_id: Option<i64>,
    map: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> rusqlite::Result<Vec<T>> {
    match account_id {
        Some(a) => stmt
            .query_map(params![folder, a], map)?
            .collect::<rusqlite::Result<_>>(),
        None => stmt
            .query_map(params![folder], map)?
            .collect::<rusqlite::Result<_>>(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::store::emails::{insert_email, NewEmail};

    fn test_store() -> Store {
        let store = Store::open_in_memory_for_test();
        store
            .conn
            .lock()
            .unwrap()
            .execute_batch(
                "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'a@example.invalid','i','s');
                 INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (2,'b@example.invalid','i','s');",
            )
            .unwrap();
        store
    }

    /// 単独のメール（1 通 = 1 スレッド）。`ts` が None なら日時の無いメール。
    fn mail(account_id: i64, mid: &str, ts: Option<i64>, folder: &str) -> NewEmail {
        NewEmail {
            account_id,
            message_id: Some(mid.into()),
            canonical_key: mid.into(),
            subject: Some(format!("件名 {mid}")),
            from_address: Some("taro@example.invalid".into()),
            from_name: Some("試験 太郎".into()),
            to_addresses: None,
            to_name: None,
            reply_to: None,
            cc_addresses: None,
            date: ts.map(|t| {
                chrono::DateTime::from_timestamp(t, 0)
                    .expect("試験の時刻は範囲内")
                    .to_rfc3339()
            }),
            date_ts: ts,
            body_plain: Some("body".into()),
            clean_body: Some("body".into()),
            body_html: None,
            auth_result: None,
            list_id: None,
            in_reply_to: None,
            references_ids: None,
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

    /// 試験の一覧: 数日にわたり、同じ時刻のスレッド・日時の無いスレッド・別アカウント・別フォルダを混ぜる。
    fn seed(store: &Store) {
        let base = 1_790_000_000; // 2026-09 ごろ
        let conn = store.conn.lock().unwrap();
        let mut n = 0;
        let mut add = |acct: i64, ts: Option<i64>, folder: &str| {
            n += 1;
            insert_email(
                &conn,
                &mail(acct, &format!("m{n}@example.invalid"), ts, folder),
            )
            .unwrap();
        };
        // 6 時間おきに 40 通（10 日分。日付の境目をまたぐ）。
        for i in 0..40 {
            add(1, Some(base + i * 6 * 3600), "inbox");
        }
        // 同じ時刻のスレッドを 5 通（OFFSET の境目で入れ替わらないこと）。
        for _ in 0..5 {
            add(1, Some(base + 7 * 3600), "inbox");
        }
        // 日時の無いスレッド。
        add(1, None, "inbox");
        add(1, None, "inbox");
        // 別アカウント・別フォルダ。
        for i in 0..7 {
            add(2, Some(base + i * 5 * 3600), "inbox");
        }
        add(1, Some(base), "sent");
        drop(conn);
        for a in [1, 2] {
            crate::services::store::threads::process_pending_conn(&store.conn.lock().unwrap(), a)
                .unwrap();
        }
    }

    /// 代表メールの日付を、数えたときと同じ式で引く。
    fn day_of(store: &Store, id: i32) -> Option<String> {
        store
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT date(date_ts, 'unixepoch', 'localtime') FROM emails WHERE id = ?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// 日ごとの件数の累計が、一覧の OFFSET と一致する: k 日目の区間を OFFSET で読むと、
    /// ちょうどその日のスレッドだけが返る。ページの境目で重複・欠落もしない。
    #[test]
    fn day_offsets_match_list_paging() {
        let store = test_store();
        seed(&store);
        for account in [Some(1), None] {
            let days = store.thread_day_counts(account, "inbox").unwrap();
            let total: i32 = days.iter().map(|d| d.count).sum();
            let all = store.list_threads(account, "inbox", 1000, 0).unwrap();
            assert_eq!(total as usize, all.len());
            assert!(days.len() > 5, "日付の境目をまたいでいる");
            let mut offset = 0;
            for d in &days {
                let rows = store
                    .list_threads(account, "inbox", i64::from(d.count), i64::from(offset))
                    .unwrap();
                assert_eq!(rows.len() as i32, d.count);
                for r in &rows {
                    assert_eq!(day_of(&store, r.id), d.date_key, "offset {offset}");
                }
                // 区間ごとに読んだ並びが、一括で読んだ並びと同じ（同じ時刻でも入れ替わらない）。
                let ids: Vec<i32> = rows.iter().map(|r| r.id).collect();
                let expect: Vec<i32> = all[offset as usize..(offset + d.count) as usize]
                    .iter()
                    .map(|r| r.id)
                    .collect();
                assert_eq!(ids, expect);
                offset += d.count;
            }
        }
    }

    /// 新しい順に並び、日時の無いスレッドは最後に 1 つ。フォルダ・アカウントで絞り込む。
    #[test]
    fn days_are_newest_first_with_undated_last() {
        let store = test_store();
        seed(&store);
        let days = store.thread_day_counts(Some(1), "inbox").unwrap();
        let last = days.last().unwrap();
        assert_eq!(last.date_key, None);
        assert_eq!(last.count, 2);
        let keys: Vec<&str> = days.iter().filter_map(|d| d.date_key.as_deref()).collect();
        assert!(keys.windows(2).all(|w| w[0] > w[1]), "{keys:?}");
        let sum = |days: &[ThreadDayCount]| days.iter().map(|d| d.count).sum::<i32>();
        assert_eq!(sum(&days), 47);
        assert_eq!(sum(&store.thread_day_counts(Some(2), "inbox").unwrap()), 7);
        assert_eq!(sum(&store.thread_day_counts(None, "inbox").unwrap()), 54);
        assert_eq!(sum(&store.thread_day_counts(Some(1), "sent").unwrap()), 1);
        assert!(store
            .thread_day_counts(Some(1), "trash")
            .unwrap()
            .is_empty());
    }

    /// 未読のあるスレッドの数は、一覧の未読（unread_count > 0）と日ごとに一致する。
    /// 論理スレッドの途中のメール（代表でない行）だけが未読の場合も数える。
    #[test]
    fn unread_per_day_matches_list() {
        let store = test_store();
        seed(&store);
        {
            let conn = store.conn.lock().unwrap();
            // 何通かを未読にする（日をまたいで散らす）。
            conn.execute(
                "UPDATE emails SET is_read = 0 WHERE folder = 'inbox' AND id % 3 = 0",
                [],
            )
            .unwrap();
            // 返信を 1 通足して元のメールと同じスレッドにし（代表は新しい返信へ移る）、元のメールだけを未読にする。
            let reply = mail(
                1,
                "reply1@example.invalid",
                Some(1_790_000_000 + 3600),
                "inbox",
            );
            insert_email(&conn, &reply).unwrap();
            conn.execute_batch(
                "UPDATE emails SET logical_thread_id =
                     (SELECT logical_thread_id FROM emails WHERE message_id = 'm1@example.invalid')
                 WHERE message_id = 'reply1@example.invalid';
                 UPDATE emails SET is_folder_rep = 0, is_read = 0 WHERE message_id = 'm1@example.invalid';
                 UPDATE emails SET is_folder_rep = 1, is_read = 1 WHERE message_id = 'reply1@example.invalid';",
            )
            .unwrap();
        }
        // 元のメールは返信と同じスレッドに入り、代表ではなくなっている（代表でない行だけが未読）。
        let (rep, read): (i64, i64) = store
            .conn
            .lock()
            .unwrap()
            .query_row(
                "SELECT o.is_folder_rep, r.is_read FROM emails o JOIN emails r
                   ON r.logical_thread_id = o.logical_thread_id AND r.is_folder_rep = 1
                 WHERE o.message_id = 'm1@example.invalid' AND r.folder = 'inbox'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((rep, read), (0, 1));
        for account in [Some(1), None] {
            let days = store.thread_day_counts(account, "inbox").unwrap();
            let unread = store.thread_day_unread(account, "inbox").unwrap();
            let all = store.list_threads(account, "inbox", 1000, 0).unwrap();
            let mut offset = 0usize;
            let mut expect = Vec::new();
            for d in &days {
                let n = d.count as usize;
                let k = all[offset..offset + n]
                    .iter()
                    .filter(|r| r.unread_count > 0)
                    .count();
                if k > 0 {
                    expect.push((d.date_key.clone(), k as i32));
                }
                offset += n;
            }
            let got: Vec<_> = unread
                .iter()
                .map(|u| (u.date_key.clone(), u.unread))
                .collect();
            assert_eq!(got, expect);
            assert!(!got.is_empty());
        }
    }

    /// 未読の数え上げは、未読だけの部分索引から引く（代表の行を全部なめない）。
    #[test]
    fn unread_query_uses_partial_index() {
        let store = test_store();
        let conn = store.read_conn.lock().unwrap();
        let plan: Vec<String> = conn
            .prepare(
                "EXPLAIN QUERY PLAN
                 SELECT 1 FROM emails t
                 WHERE t.folder = 'inbox' AND t.is_junk = 0 AND t.is_read = 0
                   AND t.logical_thread_id IS NOT NULL",
            )
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert!(
            plan.iter().any(|p| p.contains("idx_emails_unread")),
            "{plan:?}"
        );
    }
}
