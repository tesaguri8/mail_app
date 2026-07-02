//! 迷惑メール学習のストア層（docs/SPAM.md §7.4）。
//! spam_tokens / spam_meta の読み書きと学習トランザクション。
//! カウンタ整合（§4.3）は同一 tx で担保する。分類ロジックは services/spam を使う。

use super::Store;
use rusqlite::{params, OptionalExtension, Transaction};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

/// 迷惑判定・学習に使うメールの素性（保存済み emails 行から取り出す）。
pub struct SpamFeatures {
    pub from_address: Option<String>,
    pub subject: Option<String>,
    /// clean_body（引用除去後）優先、無ければ body_plain。
    pub body: String,
    /// Authentication-Results 生テキスト（§7.7）。
    pub auth_result: Option<String>,
    /// List-Id 生テキスト（§7.7）。
    pub list_id: Option<String>,
}

/// spam_tokens への加算/打ち消しを 1 文字列（upsert）で行う。
/// `dir`: 1=spam 方向 / -1=ham 方向。`sign`: +1=加算 / -1=打ち消し。
/// カウントは MAX(0, ...) で負に落ちないようにする。
fn apply_counts(tx: &Transaction, tokens: &[&String], dir: i64, sign: i64) -> rusqlite::Result<()> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // spam 方向なら spam_count を、ham 方向なら ham_count を sign 分動かす。
    let (ds, dh) = if dir > 0 { (sign, 0) } else { (0, sign) };
    let mut stmt = tx.prepare(
        "INSERT INTO spam_tokens (token, spam_count, ham_count, updated_at)
         VALUES (?1, ?2, ?3, ?6)
         ON CONFLICT(token) DO UPDATE SET
             spam_count = MAX(0, spam_count + ?4),
             ham_count  = MAX(0, ham_count + ?5),
             updated_at = ?6",
    )?;
    for t in tokens {
        stmt.execute(params![t, ds.max(0), dh.max(0), ds, dh, now])?;
    }
    Ok(())
}

/// 学習総数 spam_meta.n_spam / n_ham を増減する（負にはしない）。
fn bump_total(tx: &Transaction, dir: i64, sign: i64) -> rusqlite::Result<()> {
    let key = if dir > 0 { "n_spam" } else { "n_ham" };
    tx.execute(
        "UPDATE spam_meta SET value = MAX(0, value + ?1) WHERE key = ?2",
        params![sign, key],
    )?;
    Ok(())
}

impl Store {
    /// 判定・学習の入力になる素性（差出人・件名・本文・認証結果・List-Id）を取得する。
    /// 本文は clean_body（引用除去後）を優先し、無ければ body_plain。
    pub fn email_spam_text(&self, id: i64) -> rusqlite::Result<Option<SpamFeatures>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT from_address, subject, COALESCE(clean_body, body_plain, ''), auth_result, list_id
             FROM emails WHERE id = ?1",
            params![id],
            |r| {
                Ok(SpamFeatures {
                    from_address: r.get(0)?,
                    subject: r.get(1)?,
                    body: r.get::<_, String>(2)?,
                    auth_result: r.get(3)?,
                    list_id: r.get(4)?,
                })
            },
        )
        .optional()
    }

    /// 対象トークンの (spam_count, ham_count) をまとめて取得する（未知語は含めない）。
    pub fn spam_token_counts(
        &self,
        tokens: &[String],
    ) -> rusqlite::Result<HashMap<String, (i64, i64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT spam_count, ham_count FROM spam_tokens WHERE token = ?1")?;
        let mut map = HashMap::new();
        let mut seen = HashSet::new();
        for t in tokens {
            if !seen.insert(t.as_str()) {
                continue;
            }
            let row = stmt
                .query_row(params![t], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?))
                })
                .optional()?;
            if let Some(c) = row {
                map.insert(t.clone(), c);
            }
        }
        Ok(map)
    }

    /// 学習メール総数 (n_spam, n_ham)。分類の平滑化に使う。
    pub fn spam_totals(&self) -> rusqlite::Result<(i64, i64)> {
        let conn = self.conn.lock().unwrap();
        let n_spam = conn
            .query_row(
                "SELECT value FROM spam_meta WHERE key = 'n_spam'",
                [],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let n_ham = conn
            .query_row("SELECT value FROM spam_meta WHERE key = 'n_ham'", [], |r| {
                r.get(0)
            })
            .optional()?
            .unwrap_or(0);
        Ok((n_spam, n_ham))
    }

    /// 学習フィードバック（§7.3）。dedup 済みトークンで spam/ham カウントと総数を
    /// 同一 tx で更新する。再マーク時は emails.spam_learned を見て旧方向を
    /// 打ち消してから付け替える（同じメールなので tokenize は決定的で一致する）。
    pub fn spam_learn(
        &self,
        email_id: i64,
        tokens: &[String],
        is_spam: bool,
    ) -> rusqlite::Result<()> {
        let new_dir: i64 = if is_spam { 1 } else { -1 };
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;

        let prev: i64 = tx
            .query_row(
                "SELECT spam_learned FROM emails WHERE id = ?1",
                params![email_id],
                |r| r.get(0),
            )
            .optional()?
            .unwrap_or(0);
        if prev == new_dir {
            return Ok(()); // 同じ向きの再学習は冪等（二重計上しない）。
        }

        // 同一メール内の重複トークンは 1 回だけ数える。
        let mut seen = HashSet::new();
        let uniq: Vec<&String> = tokens.iter().filter(|t| seen.insert(t.as_str())).collect();

        if prev != 0 {
            // 旧方向の寄与を打ち消す。
            apply_counts(&tx, &uniq, prev, -1)?;
            bump_total(&tx, prev, -1)?;
        }
        apply_counts(&tx, &uniq, new_dir, 1)?;
        bump_total(&tx, new_dir, 1)?;
        tx.execute(
            "UPDATE emails SET spam_learned = ?1 WHERE id = ?2",
            params![new_dir, email_id],
        )?;
        tx.commit()
    }

    /// 判定スコアを保存する（隔離フラグ is_junk は手動操作と分けて別途扱う）。
    pub fn set_spam_score(&self, email_id: i64, score: f64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE emails SET spam_score = ?1 WHERE id = ?2",
            params![score, email_id],
        )?;
        Ok(())
    }

    /// 迷惑フラグ（隔離）を一括設定する（手動マークの隔離/復帰。§8.2）。
    pub fn set_emails_junk(&self, ids: &[i64], value: bool) -> rusqlite::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare("UPDATE emails SET is_junk = ?1 WHERE id = ?2")?;
            for id in ids {
                stmt.execute(params![value as i64, id])?;
            }
        }
        tx.commit()
    }
}

#[cfg(test)]
mod tests {
    use super::super::migrations;
    use super::*;
    use rusqlite::Connection;

    fn store_with_email() -> Store {
        let conn = Connection::open_in_memory().unwrap();
        migrations::run(&conn).unwrap();
        conn.execute(
            "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1, 'a@b', 'i', 's')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO emails (id, account_id, canonical_key, from_address, subject, clean_body)
             VALUES (1, 1, 'k1', 'x@spam.example', '当選', '無料 当選 しました')",
            [],
        )
        .unwrap();
        Store {
            conn: std::sync::Mutex::new(conn),
            path: std::sync::Mutex::new(std::path::PathBuf::from(":memory:")),
        }
    }

    #[test]
    fn learn_updates_counts_and_totals() {
        let store = store_with_email();
        let toks = vec![
            "w:free".to_string(),
            "w:free".to_string(),
            "url:spam.example".to_string(),
        ];
        store.spam_learn(1, &toks, true).unwrap();

        let (n_spam, n_ham) = store.spam_totals().unwrap();
        assert_eq!((n_spam, n_ham), (1, 0));
        let counts = store.spam_token_counts(&toks).unwrap();
        // 重複 "w:free" は 1 カウント。
        assert_eq!(counts.get("w:free"), Some(&(1, 0)));
        assert_eq!(counts.get("url:spam.example"), Some(&(1, 0)));
    }

    #[test]
    fn remark_reverses_previous_direction() {
        let store = store_with_email();
        let toks = vec!["w:free".to_string()];
        store.spam_learn(1, &toks, true).unwrap();
        // spam→ham へ付け替え: spam 側は打ち消され、ham 側に付く。
        store.spam_learn(1, &toks, false).unwrap();

        let (n_spam, n_ham) = store.spam_totals().unwrap();
        assert_eq!((n_spam, n_ham), (0, 1));
        let counts = store.spam_token_counts(&toks).unwrap();
        assert_eq!(counts.get("w:free"), Some(&(0, 1)));

        // 同じ向きの再学習は冪等。
        store.spam_learn(1, &toks, false).unwrap();
        assert_eq!(store.spam_totals().unwrap(), (0, 1));
    }
}
