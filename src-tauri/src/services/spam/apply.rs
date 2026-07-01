//! 受信時の自動採点（docs/SPAM.md §8）。
//! commands は Store 経由だが、同期（imap_sync）は生 `Connection` を使うため、
//! ここに `&Connection` 版の採点・隔離ロジックを置く。

use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};

/// 自動判定の方針（app_settings から読む）。
pub struct SpamPolicy {
    pub enabled: bool,
    pub threshold_high: f64,
}

fn get_setting(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        params![key],
        |r| r.get(0),
    )
    .optional()
}

/// 判定の有効/無効と隔離しきい値を読む（未設定は既定値）。
pub fn read_policy(conn: &Connection) -> rusqlite::Result<SpamPolicy> {
    let enabled = get_setting(conn, "spam.enabled")?
        .map(|v| v != "false" && v != "0")
        .unwrap_or(true);
    let threshold_high = get_setting(conn, "spam.threshold_high")?
        .and_then(|v| v.parse().ok())
        .unwrap_or(super::DEFAULT_THRESHOLD_HIGH);
    Ok(SpamPolicy {
        enabled,
        threshold_high,
    })
}

fn token_counts(
    conn: &Connection,
    tokens: &[String],
) -> rusqlite::Result<HashMap<String, (i64, i64)>> {
    let mut stmt =
        conn.prepare("SELECT spam_count, ham_count FROM spam_tokens WHERE token = ?1")?;
    let mut map = HashMap::new();
    let mut seen = HashSet::new();
    for t in tokens {
        if !seen.insert(t.as_str()) {
            continue;
        }
        if let Some(c) = stmt
            .query_row(params![t], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
            })
            .optional()?
        {
            map.insert(t.clone(), c);
        }
    }
    Ok(map)
}

fn totals(conn: &Connection) -> rusqlite::Result<(i64, i64)> {
    let g = |k: &str| -> rusqlite::Result<i64> {
        conn.query_row(
            "SELECT value FROM spam_meta WHERE key = ?1",
            params![k],
            |r| r.get(0),
        )
        .optional()
        .map(|v| v.unwrap_or(0))
    };
    Ok((g("n_spam")?, g("n_ham")?))
}

/// 受信メール 1 件を採点して spam_score を保存し、しきい値超なら is_junk=1 で隔離する。
/// 手動で付けた is_junk は自動では 0 に戻さない（手動優先。§8.3）ので、隔離時のみ 1 を立てる。
#[allow(clippy::too_many_arguments)]
pub fn score_incoming(
    conn: &Connection,
    email_id: i64,
    from_address: Option<&str>,
    subject: Option<&str>,
    body: &str,
    auth_result: Option<&str>,
    list_id: Option<&str>,
    threshold_high: f64,
) -> rusqlite::Result<()> {
    let tokens = super::tokenize::tokenize(from_address, subject, body, auth_result, list_id);
    let counts = token_counts(conn, &tokens)?;
    let (n_spam, n_ham) = totals(conn)?;
    let (score, _) = super::classifier::score(&counts, &tokens, n_spam, n_ham);
    if score >= threshold_high {
        conn.execute(
            "UPDATE emails SET spam_score = ?1, is_junk = 1 WHERE id = ?2",
            params![score, email_id],
        )?;
    } else {
        conn.execute(
            "UPDATE emails SET spam_score = ?1 WHERE id = ?2",
            params![score, email_id],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // apply.rs は store の外なので、必要最小限のテーブルだけ用意する。
    fn conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE emails (id INTEGER PRIMARY KEY, spam_score REAL, is_junk INTEGER DEFAULT 0);
             CREATE TABLE spam_tokens (token TEXT PRIMARY KEY, spam_count INTEGER, ham_count INTEGER, updated_at INTEGER);
             CREATE TABLE spam_meta (key TEXT PRIMARY KEY, value INTEGER);
             CREATE TABLE app_settings (key TEXT PRIMARY KEY, value TEXT);
             INSERT INTO emails (id) VALUES (1);
             INSERT INTO spam_meta VALUES ('n_spam', 0), ('n_ham', 0);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn read_policy_defaults_to_enabled() {
        let c = conn();
        let p = read_policy(&c).unwrap();
        assert!(p.enabled);
        assert_eq!(p.threshold_high, super::super::DEFAULT_THRESHOLD_HIGH);
        // 明示的に無効化すると反映される。
        c.execute(
            "INSERT INTO app_settings VALUES ('spam.enabled','false')",
            [],
        )
        .unwrap();
        assert!(!read_policy(&c).unwrap().enabled);
    }

    #[test]
    fn quarantines_when_learned_spam() {
        let c = conn();
        c.execute("UPDATE spam_meta SET value=20 WHERE key='n_spam'", [])
            .unwrap();
        c.execute("UPDATE spam_meta SET value=20 WHERE key='n_ham'", [])
            .unwrap();
        c.execute("INSERT INTO spam_tokens VALUES ('w:viagra', 20, 0, 0)", [])
            .unwrap();

        score_incoming(
            &c,
            1,
            Some("x@y"),
            Some("hi"),
            "viagra now",
            None,
            None,
            0.9,
        )
        .unwrap();

        let (score, junk): (f64, i64) = c
            .query_row(
                "SELECT spam_score, is_junk FROM emails WHERE id=1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert!(score >= 0.9, "score={score}");
        assert_eq!(junk, 1, "学習済み spam 語で隔離される");
    }

    #[test]
    fn keeps_in_inbox_when_untrained() {
        let c = conn();
        // 学習ゼロ＝中立。隔離しない（is_junk は 0 のまま）。
        score_incoming(
            &c,
            1,
            Some("x@y"),
            Some("hi"),
            "viagra now",
            None,
            None,
            0.9,
        )
        .unwrap();
        let junk: i64 = c
            .query_row("SELECT is_junk FROM emails WHERE id=1", [], |r| r.get(0))
            .unwrap();
        assert_eq!(junk, 0);
    }
}
