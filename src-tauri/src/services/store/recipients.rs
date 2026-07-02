use super::Store;
use crate::models::RecipientSuggestion;
use rusqlite::params;
use std::collections::HashMap;

/// "Some Name <a@b.com>" -> (Some("Some Name"), "a@b.com")、素の "a@b.com" -> (None, "a@b.com")。
/// メールらしくない（'@' を含まない）ものは None。表示名の前後の引用符は剥がす。
fn parse_addr(raw: &str) -> Option<(Option<String>, String)> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let (name, email) = match (raw.rfind('<'), raw.rfind('>')) {
        (Some(lt), Some(gt)) if gt > lt => {
            let email = raw[lt + 1..gt].trim();
            let name = raw[..lt].trim().trim_matches('"').trim();
            let name = if name.is_empty() {
                None
            } else {
                Some(name.to_string())
            };
            (name, email.to_string())
        }
        _ => (None, raw.to_string()),
    };
    if email.contains('@') && !email.is_empty() {
        Some((name, email))
    } else {
        None
    }
}

/// ヘッダのアドレス列 "A <a@b>, c@d" をカンマ/改行/セミコロンで分割し、各要素を解析する。
fn split_header_addrs(raw: &str) -> Vec<(Option<String>, String)> {
    raw.split([',', '\n', ';'])
        .filter_map(parse_addr)
        .collect()
}

/// 候補の作業用エントリ（重複排除・並び替え前）。
struct Cand {
    email: String,
    name: Option<String>,
    is_contact: bool,
    is_favorite: bool,
    contact_id: Option<i32>,
    /// 履歴での登場回数（住所録由来は 0）。並びの補助に使う。
    freq: i64,
}

impl Store {
    /// 宛先オートコンプリート候補を返す（docs/RECIPIENT_AUTOCOMPLETE.md）。
    /// 住所録（名前・よみ・メール・組織で部分一致）と、過去のやり取り相手
    /// （emails.from_address / to_addresses をパース）を統合し、メールアドレス
    /// （小文字化）で重複排除して住所録を優先する。空クエリは空配列。
    /// 並び: お気に入り > 住所録 > 履歴頻度 > 名前/メール。
    pub fn suggest_recipients(
        &self,
        query: &str,
        limit: i64,
    ) -> rusqlite::Result<Vec<RecipientSuggestion>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let like = format!("%{}%", q.replace('%', "\\%").replace('_', "\\_"));
        // 小文字メールアドレスをキーに集約（住所録優先、履歴は頻度加算）。
        let mut by_email: HashMap<String, Cand> = HashMap::new();
        let conn = self.conn.lock().unwrap();

        // 1) 住所録: 主メールアドレスを持つ連絡先を名前・よみ・メール・組織で検索。
        {
            let mut stmt = conn.prepare(
                "SELECT id, display_name, email, is_favorite
                 FROM contacts
                 WHERE email IS NOT NULL AND email <> ''
                   AND (display_name LIKE ?1 ESCAPE '\\' OR name_kana LIKE ?1 ESCAPE '\\'
                        OR email LIKE ?1 ESCAPE '\\' OR organization LIKE ?1 ESCAPE '\\')",
            )?;
            let rows = stmt.query_map(params![like], |r| {
                Ok((
                    r.get::<_, i64>(0)? as i32,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)? != 0,
                ))
            })?;
            for row in rows {
                let (id, display_name, email, is_favorite) = row?;
                let key = email.to_lowercase();
                by_email.entry(key).or_insert(Cand {
                    email,
                    name: display_name,
                    is_contact: true,
                    is_favorite,
                    contact_id: Some(id),
                    freq: 0,
                });
            }
        }

        // 2) 履歴: 差出人/宛先ヘッダから、クエリに一致するアドレスを収集。
        {
            let mut stmt = conn.prepare(
                "SELECT from_address, to_addresses FROM emails
                 WHERE from_address LIKE ?1 ESCAPE '\\' OR to_addresses LIKE ?1 ESCAPE '\\'",
            )?;
            let rows = stmt.query_map(params![like], |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, Option<String>>(1)?,
                ))
            })?;
            let needle = q.to_lowercase();
            for row in rows {
                let (from, to) = row?;
                let mut addrs = Vec::new();
                if let Some(f) = &from {
                    addrs.extend(split_header_addrs(f));
                }
                if let Some(t) = &to {
                    addrs.extend(split_header_addrs(t));
                }
                for (name, email) in addrs {
                    // ヘッダ全体が LIKE に当たっても、個々のアドレス/名前が一致しない
                    // 同乗者は除外する（例: 複数宛先の1人だけがクエリに一致）。
                    let hit = email.to_lowercase().contains(&needle)
                        || name
                            .as_deref()
                            .is_some_and(|n| n.to_lowercase().contains(&needle));
                    if !hit {
                        continue;
                    }
                    let key = email.to_lowercase();
                    match by_email.get_mut(&key) {
                        // 住所録に既出: 頻度だけ数える（住所録を優先表示）。
                        Some(c) => c.freq += 1,
                        None => {
                            by_email.insert(
                                key,
                                Cand {
                                    email,
                                    name,
                                    is_contact: false,
                                    is_favorite: false,
                                    contact_id: None,
                                    freq: 1,
                                },
                            );
                        }
                    }
                }
            }
        }

        // 並び替え: お気に入り DESC → 住所録優先 → 履歴頻度 DESC → 名前(なければメール)。
        let mut cands: Vec<Cand> = by_email.into_values().collect();
        cands.sort_by(|a, b| {
            b.is_favorite
                .cmp(&a.is_favorite)
                .then(b.is_contact.cmp(&a.is_contact))
                .then(b.freq.cmp(&a.freq))
                .then_with(|| {
                    let an = a.name.as_deref().unwrap_or(&a.email).to_lowercase();
                    let bn = b.name.as_deref().unwrap_or(&b.email).to_lowercase();
                    an.cmp(&bn)
                })
        });

        Ok(cands
            .into_iter()
            .take(limit.max(0) as usize)
            .map(|c| RecipientSuggestion {
                email: c.email,
                name: c.name,
                source: if c.is_contact { "contact" } else { "history" }.to_string(),
                is_favorite: c.is_favorite,
                contact_id: c.contact_id,
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn test_store() -> Store {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        super::super::migrations::run(&conn).unwrap();
        conn.execute(
            "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'me@x','i','s')",
            [],
        )
        .unwrap();
        Store {
            conn: Mutex::new(conn),
            path: Mutex::new(PathBuf::from(":memory:")),
        }
    }

    fn add_contact(store: &Store, name: &str, email: &str, favorite: bool) {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO contacts (display_name, email, is_favorite) VALUES (?1, ?2, ?3)",
            params![name, email, favorite as i64],
        )
        .unwrap();
    }

    fn add_email(store: &Store, key: &str, from: Option<&str>, to: Option<&str>) {
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO emails (account_id, canonical_key, from_address, to_addresses, date)
             VALUES (1, ?1, ?2, ?3, '2026-01-01')",
            params![key, from, to],
        )
        .unwrap();
    }

    #[test]
    fn parse_addr_handles_named_and_bare() {
        assert_eq!(
            parse_addr("Alice <alice@corp.com>"),
            Some((Some("Alice".into()), "alice@corp.com".into()))
        );
        assert_eq!(
            parse_addr("\"Bob B\" <bob@x.com>"),
            Some((Some("Bob B".into()), "bob@x.com".into()))
        );
        assert_eq!(parse_addr("carol@y.com"), Some((None, "carol@y.com".into())));
        assert_eq!(parse_addr("not-an-email"), None);
        assert_eq!(parse_addr("   "), None);
    }

    #[test]
    fn split_header_addrs_splits_multiple() {
        let v = split_header_addrs("Alice <a@x>, b@y ; C <c@z>");
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], (Some("Alice".into()), "a@x".into()));
        assert_eq!(v[1], (None, "b@y".into()));
        assert_eq!(v[2], (Some("C".into()), "c@z".into()));
    }

    #[test]
    fn suggest_merges_contacts_and_history_with_dedup() {
        let store = test_store();
        add_contact(&store, "Alice Anderson", "alice@corp.com", false);
        add_contact(&store, "Zoe Zephyr", "zoe@corp.com", true); // favorite
        // 履歴: alice は住所録と重複（頻度加算）、dave は履歴のみ。
        add_email(&store, "k1", Some("Alice Anderson <alice@corp.com>"), None);
        add_email(&store, "k2", None, Some("Dave <dave@ext.com>, alice@corp.com"));

        // 名前一致（住所録）。
        let r = store.suggest_recipients("alice", 10).unwrap();
        assert_eq!(r.len(), 1, "重複は 1 件に集約");
        assert_eq!(r[0].email, "alice@corp.com");
        assert_eq!(r[0].source, "contact", "住所録を優先");
        assert_eq!(r[0].contact_id, Some(1));

        // 履歴のみの相手。
        let r = store.suggest_recipients("dave", 10).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].source, "history");
        assert_eq!(r[0].name.as_deref(), Some("Dave"));

        // メールドメイン一致で複数ヒット、お気に入りが先頭。
        let r = store.suggest_recipients("corp.com", 10).unwrap();
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].email, "zoe@corp.com");
        assert!(r[0].is_favorite);

        // 空クエリは空。
        assert!(store.suggest_recipients("  ", 10).unwrap().is_empty());
    }

    #[test]
    fn suggest_excludes_non_matching_co_recipients() {
        let store = test_store();
        // 1 通に 2 宛先。クエリ "dave" はヘッダ全体に LIKE で当たるが、
        // 同乗者 alice は除外されること。
        add_email(&store, "k1", None, Some("dave@ext.com, alice@corp.com"));
        let r = store.suggest_recipients("dave", 10).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].email, "dave@ext.com");
    }
}
