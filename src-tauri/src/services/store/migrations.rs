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
        sql: include_str!("migrations/0011_spam.sql"),
    },
];

pub fn run(conn: &Connection) -> rusqlite::Result<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for m in MIGRATIONS {
        if m.version > current {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(m.sql)?;
            tx.execute_batch(&format!("PRAGMA user_version = {};", m.version))?;
            tx.commit()?;
        }
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

        // バージョンが最新に到達
        let v: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 11);

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
}
