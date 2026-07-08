use rusqlite::Connection;

/// 順序付きマイグレーション。PRAGMA user_version でバージョン管理し、
/// 起動時に未適用分をトランザクションで順次適用する（docs/CROSS_CUTTING.md #4）。
struct Migration {
    version: i64,
    sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("migrations/0001_init.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("migrations/0002_account_username.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("migrations/0003_servers.sql"),
    },
    Migration {
        version: 4,
        sql: include_str!("migrations/0004_sync_state.sql"),
    },
    Migration {
        version: 5,
        sql: include_str!("migrations/0005_signatures.sql"),
    },
    Migration {
        version: 6,
        sql: include_str!("migrations/0006_attachments.sql"),
    },
    Migration {
        version: 7,
        sql: include_str!("migrations/0007_attachment_kind.sql"),
    },
    Migration {
        version: 8,
        sql: include_str!("migrations/0008_body_compression.sql"),
    },
    Migration {
        version: 9,
        sql: include_str!("migrations/0009_storage_limit.sql"),
    },
    Migration {
        version: 10,
        sql: include_str!("migrations/0010_tags.sql"),
    },
    Migration {
        version: 11,
        sql: include_str!("migrations/0011_retention_tiers.sql"),
    },
    Migration {
        version: 12,
        sql: include_str!("migrations/0012_fetch_all.sql"),
    },
    Migration {
        version: 13,
        sql: include_str!("migrations/0013_spam.sql"),
    },
    Migration {
        version: 14,
        sql: include_str!("migrations/0014_app_settings.sql"),
    },
    Migration {
        version: 15,
        sql: include_str!("migrations/0015_folders.sql"),
    },
    Migration {
        version: 16,
        sql: include_str!("migrations/0016_contacts.sql"),
    },
    Migration {
        version: 17,
        sql: include_str!("migrations/0017_contact_uid.sql"),
    },
    Migration {
        version: 18,
        sql: include_str!("migrations/0018_contact_model.sql"),
    },
    Migration {
        version: 19,
        sql: include_str!("migrations/0019_contact_tags.sql"),
    },
    Migration {
        version: 20,
        sql: include_str!("migrations/0020_email_names.sql"),
    },
    Migration {
        version: 21,
        sql: include_str!("migrations/0021_contact_email_index.sql"),
    },
    Migration {
        version: 22,
        sql: include_str!("migrations/0022_email_date_ts.sql"),
    },
    Migration {
        version: 23,
        sql: include_str!("migrations/0023_account_order.sql"),
    },
    Migration {
        version: 24,
        sql: include_str!("migrations/0024_email_folder_date_index.sql"),
    },
    Migration {
        version: 25,
        sql: include_str!("migrations/0025_contact_shared_value.sql"),
    },
    Migration {
        version: 26,
        sql: include_str!("migrations/0026_organizations.sql"),
    },
    Migration {
        version: 27,
        sql: include_str!("migrations/0027_trash.sql"),
    },
    Migration {
        version: 28,
        sql: include_str!("migrations/0028_green_domains.sql"),
    },
    Migration {
        version: 29,
        sql: include_str!("migrations/0029_draft_reply.sql"),
    },
    Migration {
        version: 30,
        sql: include_str!("migrations/0030_draft_bcc.sql"),
    },
    Migration {
        version: 31,
        sql: include_str!("migrations/0031_threading.sql"),
    },
    Migration {
        version: 32,
        sql: include_str!("migrations/0032_folder_rep.sql"),
    },
    Migration {
        version: 33,
        sql: include_str!("migrations/0033_thread_folder_index.sql"),
    },
    Migration {
        version: 34,
        sql: include_str!("migrations/0034_data_versions.sql"),
    },
    // 35 は「ゴミ箱(dev)」と「Reply-To(feature)」が別々の枝で衝突したため欠番。
    // 両 DB が user_version=35 で別々の 35 を適用済みだったので、双方を 35 超へ振り直し、
    // 各 DB が「自分に無い方」だけ適用できるようにする（すでにある列は下の run() で許容する）。
    Migration {
        version: 36,
        sql: include_str!("migrations/0036_mail_trash.sql"),
    },
    Migration {
        version: 37,
        sql: include_str!("migrations/0037_reply_to.sql"),
    },
    Migration {
        version: 38,
        sql: include_str!("migrations/0038_calendar.sql"),
    },
    Migration {
        version: 39,
        sql: include_str!("migrations/0039_calendar_meta.sql"),
    },
    Migration {
        // 40 は wt-1 の「全件メタデータ索引」（元 0038_full_index）。dev の 0038(カレンダー)と
        // 38 で衝突したため、dev 側を正として本マイグレーションを 40 へ振り直した。
        version: 40,
        sql: include_str!("migrations/0040_full_index.sql"),
    },
    Migration {
        // 41 は Google カレンダー双方向同期の同期メタ（calendar_accounts + 列追加）。
        version: 41,
        sql: include_str!("migrations/0041_calendar_sync.sql"),
    },
    Migration {
        // 42 は予定の Google 上の実在カレンダー追跡（カレンダー間移動を正しく反映するため）。
        version: 42,
        sql: include_str!("migrations/0042_event_remote_calendar.sql"),
    },
    Migration {
        // 43 は迷惑差出人（このアドレスを迷惑にしたら同アドレスの他メールも迷惑へ。docs/SPAM.md）。
        version: 43,
        sql: include_str!("migrations/0043_spam_senders.sql"),
    },
    Migration {
        // 44 は「本物の自分から」検証（emails.verified_self / accounts.self_secret。docs/SPAM.md）。
        version: 44,
        sql: include_str!("migrations/0044_verified_self.sql"),
    },
    Migration {
        // 45 は添付の再分類（inline 指定の PDF 等を添付一覧に出す。既存メールも直す）。
        version: 45,
        sql: include_str!("migrations/0045_attachment_reclassify.sql"),
    },
];

/// 「既に適用済み」を示すエラーか（別枝で同じ列/表を先に追加していた等）。
/// この場合はスキーマ変更をスキップしてバージョンだけ進めてよい（冪等化）。
fn is_already_applied(e: &rusqlite::Error) -> bool {
    let s = e.to_string().to_lowercase();
    s.contains("duplicate column name") || s.contains("already exists")
}

pub fn run(conn: &Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for m in MIGRATIONS {
        if m.version <= current {
            continue;
        }
        let tx = conn.unchecked_transaction()?;
        match tx.execute_batch(m.sql) {
            Ok(()) => {}
            // 別枝で同じスキーマを適用済み（列/表が既存）なら、その変更は不要。ロールバックして
            // バージョンだけ進める（マイグレーションはトランザクション＝全適用か未適用のどちらか
            // なので、先頭の重複エラーで止まっても残りも既存＝スキップして問題ない）。
            Err(e) if is_already_applied(&e) => {
                drop(tx);
                let bump = conn.unchecked_transaction()?;
                bump.execute_batch(&format!("PRAGMA user_version = {};", m.version))?;
                bump.commit()?;
                continue;
            }
            Err(e) => return Err(e),
        }
        tx.execute_batch(&format!("PRAGMA user_version = {};", m.version))?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_and_fts_works() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();

        // バージョンが最新に到達（MIGRATIONS 末尾の version と一致すること）
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, MIGRATIONS.last().unwrap().version);

        // emails テーブルが存在
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='emails'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);

        // FTS5 が使える（全文検索ヒット）
        conn.execute(
            "INSERT INTO email_fts(rowid, subject, from_address, clean_body) VALUES (1, 'hi', 'a@b', 'hello world')",
            [],
        )
        .unwrap();
        let hit: i64 = conn
            .query_row(
                "SELECT count(*) FROM email_fts WHERE email_fts MATCH 'hello'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit, 1);
    }

    #[test]
    fn migrations_are_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();
        // 2回目の run は no-op（再作成でエラーにならない）
        run(&conn).unwrap();
    }

    /// 別枝で先に列を追加済みの DB（user_version=35 で reply_to だけ既存＝旧 fix 枝の DB を模す）でも、
    /// run() が「既存の列は許容し、無い列だけ追加」して最新版へ到達する（ゴミ箱/Reply-To 衝突対策）。
    #[test]
    fn run_tolerates_columns_added_by_other_branch() {
        let conn = Connection::open_in_memory().unwrap();
        // 実在の v35 DB を忠実に模す: from_address(0001)・body_compacted(0011) と
        // folder_sync(0015) は 35 より前に存在する。reply_to だけ「別枝で既存」の状態を作る。
        conn.execute_batch(
            "CREATE TABLE emails (id INTEGER PRIMARY KEY, folder TEXT, from_address TEXT, body_compacted INTEGER DEFAULT 0, has_attachments INTEGER DEFAULT 0);
             ALTER TABLE emails ADD COLUMN reply_to TEXT;
             CREATE TABLE folder_sync (
               account_id INTEGER NOT NULL, folder TEXT NOT NULL,
               uid_validity INTEGER, last_uid INTEGER,
               PRIMARY KEY (account_id, folder));
             -- 実在の v35 DB にある基盤テーブル（accounts=0001 / attachments=0006）。
             -- 0044(accounts 列追加)・0045(attachments/emails のデータ更新)が動くよう用意する。
             CREATE TABLE accounts (id INTEGER PRIMARY KEY, email TEXT);
             CREATE TABLE attachments (id INTEGER PRIMARY KEY, email_id INTEGER NOT NULL,
               kind TEXT, content_type TEXT, content_id TEXT, size INTEGER);
             PRAGMA user_version = 35;",
        )
        .unwrap();
        run(&conn).unwrap();
        let cols: Vec<String> = {
            let mut s = conn.prepare("PRAGMA table_info(emails)").unwrap();
            s.query_map([], |r| r.get::<_, String>(1))
                .unwrap()
                .collect::<rusqlite::Result<_>>()
                .unwrap()
        };
        for c in ["reply_to", "trashed_at", "prev_folder"] {
            assert!(cols.contains(&c.to_string()), "column {c} missing");
        }
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, MIGRATIONS.last().unwrap().version);
    }

    #[test]
    fn tags_can_be_assigned_and_queried() {
        let conn = Connection::open_in_memory().unwrap();
        run(&conn).unwrap();

        // tag_id 索引が存在する（0006_tags.sql）
        let idx: i64 = conn
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE type='index' AND name='idx_email_tags_tag'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(idx, 1);

        // 最小限のアカウント・メール・タグを作って紐づけ → タグ ID でメールを引ける
        conn.execute(
            "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1, 'a@b', 'imap', 'smtp')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO emails (id, account_id, canonical_key) VALUES (1, 1, 'k1')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO tags (id, name) VALUES (10, '案件A')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO email_tags (email_id, tag_id) VALUES (1, 10)",
            [],
        )
        .unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT count(*) FROM email_tags WHERE tag_id = 10",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 1);
    }

    #[test]
    fn contacts_tables_exist_and_cascade_membership() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        run(&conn).unwrap();

        // contacts / contact_groups / contact_group_members が作成されている
        for name in ["contacts", "contact_groups", "contact_group_members"] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [name],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "table {name} missing");
        }

        // 連絡先をグループに入れ、連絡先削除で所属が CASCADE で外れる
        conn.execute(
            "INSERT INTO contacts (id, display_name) VALUES (1, '山田太郎')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO contact_groups (id, name) VALUES (5, '取引先')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO contact_group_members (contact_id, group_id) VALUES (1, 5)",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM contacts WHERE id = 1", [])
            .unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM contact_group_members", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn events_tables_exist_and_attendees_cascade() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        run(&conn).unwrap();

        // events / event_attendees が作成されている
        for name in ["events", "event_attendees"] {
            let n: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table' AND name=?1",
                    [name],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "table {name} missing");
        }

        // 予定に参加者を紐づけ、予定削除で参加者が CASCADE で外れる
        conn.execute(
            "INSERT INTO contacts (id, display_name) VALUES (1, '山田太郎')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO events (id, title, start_at) VALUES (7, '打ち合わせ', '2026-07-06T10:00')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO event_attendees (event_id, contact_id) VALUES (7, 1)",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM events WHERE id = 7", []).unwrap();
        let n: i64 = conn
            .query_row("SELECT count(*) FROM event_attendees", [], |r| r.get(0))
            .unwrap();
        assert_eq!(n, 0);
    }
}
