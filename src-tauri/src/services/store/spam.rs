//! 迷惑メール学習のストア層（docs/SPAM.md §7.4）。
//! spam_tokens / spam_meta の読み書きと学習トランザクション。
//! カウンタ整合（§4.3）は同一 tx で担保する。分類ロジックは services/spam を使う。

use super::Store;
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use std::collections::{HashMap, HashSet};
use std::time::{SystemTime, UNIX_EPOCH};

/// epoch 秒（取得できなければ 0）。
fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// 差出人アドレスを一致判定用に正規化する。`"Name <addr>"` 形式なら `<>` の中を採用し、
/// 前後空白を除いて小文字化する（保存済み from_address は素のアドレスだが防御的に <> も処理）。
pub fn normalize_sender_address(raw: &str) -> String {
    let s = raw.trim();
    let core = match (s.rfind('<'), s.rfind('>')) {
        (Some(l), Some(r)) if l < r => &s[l + 1..r],
        _ => s,
    };
    core.trim().to_lowercase()
}

/// このアドレスが「迷惑差出人」に登録済みか（挿入は同期スレッドの別接続から呼ぶため
/// `&Connection` 版。docs/SPAM.md）。
pub fn is_spam_sender_conn(conn: &Connection, address: &str) -> rusqlite::Result<bool> {
    let norm = normalize_sender_address(address);
    if norm.is_empty() {
        return Ok(false);
    }
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM spam_senders WHERE address = ?1",
        params![norm],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// このアドレスが「このまま迷惑（強制適用）」で登録されているか（docs/SPAM.md §8.5）。
/// ユーザーが注意喚起に対して明示的に選んだ差出人なので、信頼シグナルより優先する。
fn is_enforced_spam_sender_conn(conn: &Connection, norm: &str) -> rusqlite::Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM spam_senders WHERE address = ?1 AND enforced = 1",
        params![norm],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// 差出人が「許可リスト」に該当するか（本人検証 / 住所録に登録済みの本人）。
/// 該当すれば、迷惑差出人として登録済みでも受信時に自動隔離しない（誤登録・共有アドレスの
/// 取り違え等での取りこぼしを防ぐ）。
///
/// 優先順位は**具体的な指定が勝つ**（docs/SPAM.md §8.5）:
/// 本人検証 ＞ 強制適用の迷惑登録（enforced）＞ 住所録の本人一致 ＞ アドレス単位の迷惑登録
/// ＞ グリーン**ドメイン**。グリーンはドメイン単位の緩い信頼なので、アドレス単位の迷惑登録には
/// 勝たせない（フリーメールを 1 件グリーンにするとそのドメイン全員が許可される、を防ぐ）。
pub fn is_allowlisted_sender_conn(
    conn: &Connection,
    address: &str,
    verified_self: bool,
) -> rusqlite::Result<bool> {
    if verified_self {
        return Ok(true);
    }
    let norm = normalize_sender_address(address);
    if norm.is_empty() {
        return Ok(false);
    }
    // ユーザーが「このまま迷惑」を選んだ差出人は、住所録に居ても隔離する。
    if is_enforced_spam_sender_conn(conn, &norm)? {
        return Ok(false);
    }
    // 住所録に登録済み（アドレス完全一致）の差出人は信頼する。
    super::greendomain::address_is_known(conn, &norm)
}

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
    let now = now_secs();
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

    /// 指定メール群の差出人アドレス（正規化・重複排除）から、自分の口座アドレスを除いた集合を返す。
    /// 「この差出人を迷惑扱いにする」対象を決めるのに使う（自分自身は迷惑差出人にしない）。docs/SPAM.md。
    pub fn spam_sender_candidates(&self, ids: &[i64]) -> rusqlite::Result<Vec<String>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().unwrap();
        // 自分の口座アドレス（正規化）。
        let mut own = HashSet::new();
        {
            let mut stmt = conn.prepare("SELECT email FROM accounts")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for e in rows {
                own.insert(normalize_sender_address(&e?));
            }
        }
        let placeholders = ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
        let sql = format!(
            "SELECT DISTINCT from_address FROM emails
             WHERE id IN ({placeholders}) AND from_address IS NOT NULL AND from_address <> ''"
        );
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(ids.iter()), |r| {
            r.get::<_, String>(0)
        })?;
        let mut seen = HashSet::new();
        let mut out = Vec::new();
        for a in rows {
            let norm = normalize_sender_address(&a?);
            if norm.is_empty() || own.contains(&norm) {
                continue;
            }
            if seen.insert(norm.clone()) {
                out.push(norm);
            }
        }
        Ok(out)
    }

    /// アドレスを「迷惑差出人」に登録する（既存なら何もしない）。docs/SPAM.md。
    pub fn add_spam_sender(&self, address: &str) -> rusqlite::Result<()> {
        let norm = normalize_sender_address(address);
        if norm.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR IGNORE INTO spam_senders(address, created_at) VALUES (?1, ?2)",
            params![norm, now_secs()],
        )?;
        Ok(())
    }

    /// アドレスの「迷惑差出人」登録を解除する（非迷惑に戻す時。docs/SPAM.md）。
    pub fn remove_spam_sender(&self, address: &str) -> rusqlite::Result<()> {
        let norm = normalize_sender_address(address);
        if norm.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM spam_senders WHERE address = ?1", params![norm])?;
        Ok(())
    }

    /// アドレスの迷惑登録を「このまま迷惑（強制適用）」にする（docs/SPAM.md §8.5）。
    /// 住所録／グリーンとの矛盾を知らせた上でユーザーが迷惑を選んだ場合に使う。
    /// 以後この差出人は信頼シグナルより迷惑登録を優先して隔離し、注意喚起にも再掲しない。
    /// 未登録のアドレスなら、強制適用で新規登録する。
    pub fn enforce_spam_sender(&self, address: &str) -> rusqlite::Result<()> {
        let norm = normalize_sender_address(address);
        if norm.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO spam_senders(address, created_at, enforced) VALUES (?1, ?2, 1) \
             ON CONFLICT(address) DO UPDATE SET enforced = 1",
            params![norm, now_secs()],
        )?;
        Ok(())
    }

    /// 迷惑差出人リストと信頼シグナル（住所録/グリーン）の矛盾を列挙する（注意喚起用）。
    /// 誤登録に気付けるよう、迷惑登録済みなのに住所録に居る／グリーン認定の差出人を返す。
    /// 「このまま迷惑」を選んだ差出人（enforced=1）は解決済みなので返さない。
    pub fn find_spam_sender_conflicts(
        &self,
    ) -> rusqlite::Result<Vec<crate::models::SpamSenderConflict>> {
        let conn = self.conn.lock().unwrap();
        let addrs: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT address FROM spam_senders WHERE enforced = 0 ORDER BY address")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        // グリーン集合は表示側と同じ定義（手動 ∪ 住所録由来 − フリーメール − 警告）を使う。
        let green_set = super::greendomain::green_domain_set(&conn)?;
        let mut out = Vec::new();
        for addr in addrs {
            let name: Option<String> = conn
                .query_row(
                    "SELECT c.display_name FROM contact_emails ce JOIN contacts c ON c.id = ce.contact_id \
                     WHERE lower(ce.value) = ?1 AND c.deleted_at IS NULL LIMIT 1",
                    params![addr],
                    |r| r.get(0),
                )
                .optional()?;
            let is_contact = name.is_some() || super::greendomain::address_is_known(&conn, &addr)?;
            let is_green = super::greendomain::domain_of(&addr)
                .is_some_and(|domain| green_set.contains(&domain));
            if is_contact || is_green {
                let reason = match (is_contact, is_green) {
                    (true, true) => "contact_green",
                    (true, false) => "contact",
                    _ => "green",
                };
                out.push(crate::models::SpamSenderConflict {
                    address: addr,
                    display_name: name,
                    reason: reason.to_string(),
                });
            }
        }
        Ok(out)
    }

    /// 差出人アドレス一致のメールを一括で隔離/復帰する（大文字小文字は無視）。
    /// `value=true`: 受信箱にある同アドレスのメールを迷惑へ（is_junk=1）。
    /// `value=false`: 隔離済み（is_junk=1）の同アドレスを受信箱へ戻す。戻り値は更新件数。
    pub fn set_sender_junk(&self, address: &str, value: bool) -> rusqlite::Result<usize> {
        let norm = normalize_sender_address(address);
        if norm.is_empty() {
            return Ok(0);
        }
        let conn = self.conn.lock().unwrap();
        let n = if value {
            conn.execute(
                "UPDATE emails SET is_junk = 1
                 WHERE from_address = ?1 COLLATE NOCASE
                   AND COALESCE(folder,'inbox') = 'inbox'
                   AND is_junk = 0",
                params![norm],
            )?
        } else {
            conn.execute(
                "UPDATE emails SET is_junk = 0
                 WHERE from_address = ?1 COLLATE NOCASE
                   AND is_junk = 1",
                params![norm],
            )?
        };
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store_with_email() -> Store {
        let store = Store::open_in_memory_for_test();
        {
            let conn = store.conn.lock().unwrap();
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
        }
        store
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

    #[test]
    fn normalize_sender_extracts_and_lowercases() {
        assert_eq!(normalize_sender_address("  Foo@Bar.COM "), "foo@bar.com");
        assert_eq!(
            normalize_sender_address("Bad Guy <SPAM@Bad.Example>"),
            "spam@bad.example"
        );
        assert_eq!(normalize_sender_address(""), "");
    }

    #[test]
    fn spam_sender_junks_matching_and_excludes_self() {
        let store = Store::open_in_memory_for_test();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO accounts (id, email, imap_host, smtp_host)
                 VALUES (1, 'me@self.example', 'i', 's')",
                [],
            )
            .unwrap();
            // 迷惑差出人からの受信 2 通（大文字小文字違い）＋自分の送信 1 通。
            conn.execute(
                "INSERT INTO emails (id, account_id, canonical_key, from_address, folder) VALUES
                   (10, 1, 'k10', 'spam@bad.example', 'inbox'),
                   (11, 1, 'k11', 'SPAM@Bad.Example', 'inbox'),
                   (12, 1, 'k12', 'me@self.example', 'sent')",
                [],
            )
            .unwrap();
        }

        // 候補は spam@bad.example のみ（自分の口座アドレスは除外・正規化して小文字）。
        let cands = store.spam_sender_candidates(&[10, 11, 12]).unwrap();
        assert_eq!(cands, vec!["spam@bad.example".to_string()]);

        store.add_spam_sender("spam@bad.example").unwrap();
        // 大文字小文字を無視して受信箱の 2 通が隔離される（送信は対象外）。
        assert_eq!(store.set_sender_junk("spam@bad.example", true).unwrap(), 2);

        // 挿入時判定（同期の別接続想定）も true（表示名つき・大文字でも一致）。
        {
            let conn = store.conn.lock().unwrap();
            assert!(is_spam_sender_conn(&conn, "Bad Guy <SPAM@bad.example>").unwrap());
        }

        // 非迷惑に戻す: 登録解除＋受信箱へ復帰（2 通）。
        store.remove_spam_sender("spam@bad.example").unwrap();
        assert_eq!(store.set_sender_junk("spam@bad.example", false).unwrap(), 2);
        {
            let conn = store.conn.lock().unwrap();
            assert!(!is_spam_sender_conn(&conn, "spam@bad.example").unwrap());
        }
    }

    #[test]
    fn allowlisted_sender_contact_and_self_but_not_green_domain() {
        let store = Store::open_in_memory_for_test();
        let conn = store.conn.lock().unwrap();
        conn.execute("INSERT INTO contacts (id, display_name) VALUES (1, '野崎')", [])
            .unwrap();
        conn.execute(
            "INSERT INTO contact_emails (contact_id, value) VALUES (1, 'nozapi333@icloud.com')",
            [],
        )
        .unwrap();
        conn.execute("INSERT INTO green_domains (domain) VALUES ('example.co.jp')", [])
            .unwrap();

        // 住所録に登録済みの差出人は許可（表示名つき・大文字でも一致）。
        assert!(is_allowlisted_sender_conn(&conn, "野崎 <NOZAPI333@icloud.com>", false).unwrap());
        // グリーン認定は「ドメイン単位の緩い信頼」なので、アドレス単位の迷惑登録には勝たせない。
        assert!(!is_allowlisted_sender_conn(&conn, "who@example.co.jp", false).unwrap());
        // 本人検証は無条件で許可。
        assert!(is_allowlisted_sender_conn(&conn, "anyone@nowhere.example", true).unwrap());
        // どれにも該当しない差出人は非許可。
        assert!(!is_allowlisted_sender_conn(&conn, "stranger@bad.example", false).unwrap());
    }

    #[test]
    fn enforced_spam_sender_beats_contact_and_leaves_conflicts() {
        let store = Store::open_in_memory_for_test();
        {
            let conn = store.conn.lock().unwrap();
            conn.execute("INSERT INTO contacts (id, display_name) VALUES (1, '知人')", [])
                .unwrap();
            conn.execute(
                "INSERT INTO contact_emails (contact_id, value) VALUES (1, 'known@gmail.com')",
                [],
            )
            .unwrap();
        }
        store.add_spam_sender("known@gmail.com").unwrap();

        // 強制適用の前: 住所録一致なので隔離しない＝矛盾として注意喚起に出る。
        {
            let conn = store.conn.lock().unwrap();
            assert!(is_allowlisted_sender_conn(&conn, "known@gmail.com", false).unwrap());
        }
        let conflicts = store.find_spam_sender_conflicts().unwrap();
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].address, "known@gmail.com");

        // 「このまま迷惑」を選ぶと、住所録より迷惑登録が優先され、注意喚起からも消える。
        store.enforce_spam_sender("known@gmail.com").unwrap();
        {
            let conn = store.conn.lock().unwrap();
            assert!(!is_allowlisted_sender_conn(&conn, "known@gmail.com", false).unwrap());
        }
        assert!(store.find_spam_sender_conflicts().unwrap().is_empty());
    }
}
