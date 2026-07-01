//! ローカル保存の管理（2軸: 添付は期間で削除、本文は保証期間＋容量で要約）。
//!
//! 添付ファイルは「手元に残す期間」より古いものは実ファイルを削除する（メタは残し再DL可）。
//! 本文は「テキスト全文を確実に残す期間」内は保証保持し、容量上限を超えたらそれより古い本文を
//! 古い順に要約保存（clean_body だけ残す）へ落として上限に収める。
//! いずれも正本はサーバーにあるため、削っても開く/再取得で復元できる（ローカル＝キャッシュ）。

use super::Store;
use crate::models::RetentionReport;
use rusqlite::params;

/// 保持期間ウィンドウ（'7d'/'30d'/'3m'/'6m'/'1y'/'2y'）を日数に変換する。
/// 'all'（常に保持）/'off'（無効）や未知値は None（＝そのティアを働かせない）。
fn window_days(w: &str) -> Option<i64> {
    match w.trim().to_lowercase().as_str() {
        "all" | "off" | "" => None,
        "7d" => Some(7),
        "30d" => Some(30),
        "3m" => Some(90),
        "6m" => Some(180),
        "1y" => Some(365),
        "2y" => Some(730),
        other => {
            // 汎用パース: "<n>d" / "<n>m" / "<n>y"。
            let parse = |suffix: char| {
                other
                    .strip_suffix(suffix)
                    .and_then(|s| s.parse::<i64>().ok())
            };
            if let Some(n) = parse('d') {
                Some(n)
            } else if let Some(n) = parse('m') {
                Some(n * 30)
            } else {
                parse('y').map(|n| n * 365)
            }
        }
    }
}

impl Store {
    /// アカウントのローカルキャッシュ使用量（ダウンロード済み添付＋本文のバイト）。
    /// 本文を要約すればこの値が減るので、容量オーバー時の要約判定に使える。
    pub fn storage_used(&self, account_id: i64) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        let attachments: i64 = conn.query_row(
            "SELECT COALESCE(SUM(a.size), 0)
             FROM attachments a JOIN emails e ON e.id = a.email_id
             WHERE e.account_id = ?1 AND a.file_path IS NOT NULL",
            params![account_id],
            |r| r.get(0),
        )?;
        let bodies: i64 = conn.query_row(
            "SELECT COALESCE(SUM(COALESCE(length(body_html_z),0) + COALESCE(length(body_plain),0)), 0)
             FROM emails WHERE account_id = ?1",
            params![account_id],
            |r| r.get(0),
        )?;
        Ok(attachments + bodies)
    }

    /// アカウントの容量上限（バイト）。未設定は既定 2GB。
    pub fn storage_limit(&self, account_id: i64) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(storage_limit, 2147483648) FROM accounts WHERE id = ?1",
            params![account_id],
            |r| r.get(0),
        )
    }

    /// 容量上限を設定する（バイト）。
    pub fn set_storage_limit(&self, account_id: i64, bytes: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET storage_limit = ?1 WHERE id = ?2",
            params![bytes, account_id],
        )?;
        Ok(())
    }

    /// フルデータ保持期間を設定する（'7d'/'30d'/…/'all'）。取り込み範囲は変えない。
    pub fn set_full_window(&self, account_id: i64, window: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET full_window = ?1 WHERE id = ?2",
            params![window, account_id],
        )?;
        Ok(())
    }

    /// テキスト全文を確実に残す期間を設定する（'3m'/…/'2y'/'all'）。この期間内は本文全文を
    /// 保証保持し要約しない。これより古い本文は、容量オーバー時に古い順で要約対象になる。
    pub fn set_body_window(&self, account_id: i64, window: &str) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET body_window = ?1 WHERE id = ?2",
            params![window, account_id],
        )?;
        Ok(())
    }

    /// アカウントの保持設定（full_window, body_window）を読む。
    fn retention_windows(&self, account_id: i64) -> rusqlite::Result<(String, String)> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COALESCE(full_window,'all'), COALESCE(body_window,'off')
             FROM accounts WHERE id = ?1",
            params![account_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
    }

    /// full_window より古い（スター以外の）メールの添付ファイルを削除する（メタは残す）。
    ///
    /// ただし範囲外でも**最終アクセスから GRACE_DAYS 日以内**の添付は残す。古い添付を
    /// 一時的に開いた／再ダウンロードした直後に消えないための猶予。放置すれば次回以降の
    /// 適用で削除される（アクセスのたびに accessed_at が更新され猶予は延びる）。
    /// `accessed_at` が NULL（accessed_at 列導入前の旧DLなど）は猶予対象外＝削除する。
    /// 返り値: (削除した添付数, 解放バイト)。
    fn evict_attachments_outside_window(
        &self,
        account_id: i64,
        days: i64,
    ) -> rusqlite::Result<(i32, i64)> {
        /// 範囲外の添付をDL後に残す猶予（日）。この期間は開いたファイルを使い回せる。
        const GRACE_DAYS: i64 = 3;
        let candidates: Vec<(i64, i64, String)> = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT a.id, COALESCE(a.size, 0), a.file_path
                 FROM attachments a JOIN emails e ON e.id = a.email_id
                 WHERE e.account_id = ?1 AND a.file_path IS NOT NULL AND e.is_flagged = 0
                   AND e.date IS NOT NULL
                   AND datetime(e.date) < datetime('now', ?2)
                   AND (a.accessed_at IS NULL OR datetime(a.accessed_at) < datetime('now', ?3))",
            )?;
            let window_mod = format!("-{} days", days);
            let grace_mod = format!("-{} days", GRACE_DAYS);
            let rows = stmt.query_map(params![account_id, window_mod, grace_mod], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut evicted = 0i32;
        let mut freed = 0i64;
        for (id, size, path) in candidates {
            let _ = std::fs::remove_file(&path);
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "UPDATE attachments SET file_path = NULL, checksum = NULL WHERE id = ?1",
                params![id],
            )?;
            drop(conn);
            freed += size;
            evicted += 1;
        }
        Ok((evicted, freed))
    }

    /// 容量上限を超えていれば、body_window（テキスト全文の保証期間）より古い本文を
    /// 古い順に要約保存へ落として上限に収める（容量オーバー時の要約）。
    /// clean_body（引用除去済みの新規部分）だけ残し、重い body_html_z / body_plain を破棄。
    /// 保証期間内・スター付き・clean_body 空のメールは対象外。
    /// body_window が 'all'/'off'（＝全文を常に保証）なら要約しない。
    /// 返り値: (要約した件数, 解放バイト)。
    fn compact_bodies_to_fit(
        &self,
        account_id: i64,
        body_window: &str,
    ) -> rusqlite::Result<(i32, i64)> {
        let Some(guard_days) = window_days(body_window) else {
            return Ok((0, 0)); // 全文を常に保証（要約しない）。
        };
        let limit = self.storage_limit(account_id)?;
        let mut used = self.storage_used(account_id)?;
        if used <= limit {
            return Ok((0, 0));
        }

        // 保証期間より古い本文を、古い順（要約候補）に集める。
        let candidates: Vec<(i64, i64)> = {
            let conn = self.conn.lock().unwrap();
            let mut stmt = conn.prepare(
                "SELECT id, COALESCE(length(body_html_z),0) + COALESCE(length(body_plain),0)
                 FROM emails
                 WHERE account_id = ?1 AND is_flagged = 0 AND body_compacted = 0
                   AND clean_body IS NOT NULL AND clean_body <> ''
                   AND date IS NOT NULL AND datetime(date) < datetime('now', ?2)
                   AND (body_html_z IS NOT NULL OR body_plain IS NOT NULL)
                 ORDER BY date ASC",
            )?;
            let cutoff = format!("-{} days", guard_days);
            let rows = stmt.query_map(params![account_id, cutoff], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut compacted = 0i32;
        let mut freed = 0i64;
        for (id, bytes) in candidates {
            if used <= limit {
                break;
            }
            let conn = self.conn.lock().unwrap();
            conn.execute(
                "UPDATE emails SET body_html_z = NULL, body_plain = NULL, body_compacted = 1 WHERE id = ?1",
                params![id],
            )?;
            drop(conn);
            used -= bytes;
            freed += bytes;
            compacted += 1;
        }
        Ok((compacted, freed))
    }

    /// 保持ポリシーを適用する（2軸: 添付は期間で削除、本文は保証期間＋容量で要約）。
    /// まず「添付を手元に残す期間」より古い添付ファイルを削除し（年齢ベース。メタは残す）、
    /// 次に容量上限を超えていれば「テキスト全文を確実に残す期間」より古い本文を古い順に
    /// 要約保存へ落として上限に収める。スター付きと各保証期間内は保護。正本はサーバー。
    pub fn apply_retention(&self, account_id: i64) -> rusqlite::Result<RetentionReport> {
        let (full_w, body_w) = self.retention_windows(account_id)?;

        // 1) 添付ファイルの年齢ベース削除。
        let (mut evicted, mut freed) = (0i32, 0i64);
        if let Some(days) = window_days(&full_w) {
            let (e, f) = self.evict_attachments_outside_window(account_id, days)?;
            evicted += e;
            freed += f;
        }

        // 2) 容量オーバー時に、保証期間より古い本文を古い順に要約。
        let (compacted, body_freed) = self.compact_bodies_to_fit(account_id, &body_w)?;

        Ok(RetentionReport {
            evicted,
            compacted,
            freed_bytes: (freed + body_freed) as f64,
        })
    }

    /// 添付が「ダウンロード済み扱い」かどうか（file_path が非 NULL）。テスト補助。
    #[cfg(test)]
    fn attachment_has_file(&self, id: i64) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT file_path FROM attachments WHERE id = ?1",
            params![id],
            |r| r.get::<_, Option<String>>(0),
        )
        .unwrap()
        .is_some()
    }

    /// 同期状態（uid_validity/last_uid）をリセットして次回フル再取得させる。
    pub fn reset_sync_state(&self, account_id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET uid_validity = NULL, last_uid = NULL WHERE id = ?1",
            params![account_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::store::migrations;
    use rusqlite::Connection;
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn test_store() -> Store {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrations::run(&conn).unwrap();
        Store {
            conn: Mutex::new(conn),
            path: Mutex::new(PathBuf::new()),
        }
    }

    #[test]
    fn window_days_parses_vocab_and_generic() {
        assert_eq!(window_days("all"), None);
        assert_eq!(window_days("off"), None);
        assert_eq!(window_days(""), None);
        assert_eq!(window_days("7d"), Some(7));
        assert_eq!(window_days("30d"), Some(30));
        assert_eq!(window_days("3m"), Some(90));
        assert_eq!(window_days("6m"), Some(180));
        assert_eq!(window_days("1y"), Some(365));
        assert_eq!(window_days("2y"), Some(730));
        // 汎用パース（辞書外）。
        assert_eq!(window_days("45d"), Some(45));
        assert_eq!(window_days("nonsense"), None);
    }

    /// 添付は「手元に残す期間」で年齢削除、本文は容量オーバー時に保証期間より古い順で要約。
    /// スター付き・保証期間内は保護。
    #[test]
    fn retention_evicts_old_attachments_and_compacts_bodies_when_over_limit() {
        let store = test_store();
        {
            let conn = store.conn.lock().unwrap();
            // full_window(添付)=30d、body_window(全文保証)=6m、容量上限=1（＝必ず超過）。
            conn.execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host, full_window, body_window, storage_limit)
                 VALUES (1,'a@b','i','s','30d','6m',1)",
                [],
            )
            .unwrap();
            // (id, 経過日数, is_flagged): A=5d(保証内) B=200d C=400d D=400d★
            let seed = [(1, 5, 0), (2, 200, 0), (3, 400, 0), (4, 400, 1)];
            for (id, days, flagged) in seed {
                conn.execute(
                    "INSERT INTO emails (id, account_id, canonical_key, date, clean_body, body_plain, body_html_z, is_flagged)
                     VALUES (?1, 1, ?2, datetime('now', ?3), 'new part', 'plain body', x'01020304', ?4)",
                    params![id, format!("k{id}"), format!("-{days} days"), flagged],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO attachments (id, email_id, filename, size, file_path)
                     VALUES (?1, ?1, 'f.pdf', 1000, ?2)",
                    params![id, format!("/nonexistent/{id}.pdf")],
                )
                .unwrap();
            }
        }

        let r = store.apply_retention(1).unwrap();

        // 添付: 5日前=保持 / 200日・400日=削除 / スター付き=保護。
        assert!(store.attachment_has_file(1), "添付期間内は保持");
        assert!(!store.attachment_has_file(2), "添付期間外は削除");
        assert!(!store.attachment_has_file(3), "添付期間外は削除");
        assert!(store.attachment_has_file(4), "スター付き添付は保護");
        assert_eq!(r.evicted, 2, "削除した添付は2件");

        // 本文: 保証期間(6m)より古い B/C を要約。A(保証内)・D(スター)は保護。
        let conn = store.conn.lock().unwrap();
        let compacted = |id: i64| -> i64 {
            conn.query_row(
                "SELECT body_compacted FROM emails WHERE id=?1",
                params![id],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(compacted(1), 0, "保証期間内は全文保持");
        assert_eq!(compacted(2), 1, "保証期間外は要約");
        assert_eq!(compacted(3), 1, "保証期間外は要約");
        assert_eq!(compacted(4), 0, "スター付き本文は保護");
        assert_eq!(r.compacted, 2, "要約した本文は2件");

        // 要約後: 重い列は NULL、clean_body は残る。
        let (hz, bp, cb): (Option<Vec<u8>>, Option<String>, Option<String>) = conn
            .query_row(
                "SELECT body_html_z, body_plain, clean_body FROM emails WHERE id=3",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert!(hz.is_none() && bp.is_none(), "重い本文は破棄");
        assert!(cb.is_some(), "clean_body は残る");
    }

    /// 容量に収まっていれば、保証期間より古くても本文は要約しない（容量ドリブン）。
    #[test]
    fn retention_does_not_compact_when_within_limit() {
        let store = test_store();
        {
            let conn = store.conn.lock().unwrap();
            // body_window=6m だが容量上限は既定(2GB)で十分 → 要約しない。
            conn.execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host, full_window, body_window)
                 VALUES (1,'a@b','i','s','all','6m')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO emails (id, account_id, canonical_key, date, clean_body, body_html_z)
                 VALUES (1,1,'k1',datetime('now','-400 days'),'new',x'01020304')",
                [],
            )
            .unwrap();
        }
        let r = store.apply_retention(1).unwrap();
        assert_eq!(r.compacted, 0, "容量に収まっていれば要約しない");
        let c: i64 = store
            .conn
            .lock()
            .unwrap()
            .query_row("SELECT body_compacted FROM emails WHERE id=1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(c, 0);
    }

    /// 既定（full='all' / body='off'）では何も落とさない（非破壊）。
    #[test]
    fn retention_defaults_are_non_destructive() {
        let store = test_store();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'a@b','i','s')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO emails (id, account_id, canonical_key, date, clean_body, body_html_z)
                 VALUES (1,1,'k1',datetime('now','-999 days'),'x',x'0102')",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO attachments (id, email_id, filename, size, file_path)
                 VALUES (1,1,'f.pdf',1000,'/nonexistent/1.pdf')",
                [],
            )
            .unwrap();
        }
        let r = store.apply_retention(1).unwrap();
        assert_eq!(r.evicted, 0);
        assert_eq!(r.compacted, 0);
        assert!(store.attachment_has_file(1), "既定では添付を消さない");
    }

    /// 範囲外でも最終アクセスから3日以内の添付は残す（一時的に開いた古い添付の猶予）。
    #[test]
    fn retention_grace_keeps_recently_accessed_out_of_window_attachments() {
        let store = test_store();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host, full_window)
                 VALUES (1,'a@b','i','s','30d')",
                [],
            )
            .unwrap();
            // 3件とも範囲外（100日前）でDL済み。accessed_at だけ変える。
            for id in 1..=3 {
                conn.execute(
                    "INSERT INTO emails (id, account_id, canonical_key, date)
                     VALUES (?1, 1, ?2, datetime('now','-100 days'))",
                    params![id, format!("k{id}")],
                )
                .unwrap();
            }
            // 1: 今アクセス（猶予内）→残る / 2: 10日前（猶予切れ）→削除 / 3: NULL（旧DL）→削除
            conn.execute(
                "INSERT INTO attachments (id, email_id, filename, size, file_path, accessed_at)
                 VALUES (1,1,'a.pdf',1000,'/nonexistent/1.pdf',datetime('now'))",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO attachments (id, email_id, filename, size, file_path, accessed_at)
                 VALUES (2,2,'b.pdf',1000,'/nonexistent/2.pdf',datetime('now','-10 days'))",
                [],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO attachments (id, email_id, filename, size, file_path, accessed_at)
                 VALUES (3,3,'c.pdf',1000,'/nonexistent/3.pdf',NULL)",
                [],
            )
            .unwrap();
        }

        let r = store.apply_retention(1).unwrap();

        assert!(
            store.attachment_has_file(1),
            "3日以内にアクセスした添付は残す"
        );
        assert!(!store.attachment_has_file(2), "猶予切れの添付は削除");
        assert!(
            !store.attachment_has_file(3),
            "旧DL(accessed_at NULL)は削除"
        );
        assert_eq!(r.evicted, 2);
    }
}
