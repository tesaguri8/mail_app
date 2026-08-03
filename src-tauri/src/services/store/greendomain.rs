//! グリーンドメイン（ユーザーが認めた安全な差出人ドメイン）の判定と管理。docs/GREEN_DOMAINS.md。
//!
//! is_green の判定材料:
//!  1) 差出人が住所録の本人（アドレス完全一致＝is_known 相当）
//!  2) 差出人ドメインがグリーン集合に含まれる
//!     グリーン集合 =（手動認定 green_domains ∪ 住所録由来ドメイン）− フリーメール − 警告 warning_domains
//! 「解除」は警告 warning_domains へ移し、住所録由来の自動グリーンが再登録されないようにする。
//!
//! フリーメール（gmail.com 等）は**ドメイン単位では信頼しない**（誰でも取得できるため、
//! 1 人を信頼するとドメイン全体が信頼されてしまう）。手動認定も拒否し、本人一致だけを
//! グリーンとする。

use super::Store;
use crate::models::GreenDomainEntry;
use rusqlite::{params, Connection};
use std::collections::{HashMap, HashSet};

/// 代表的なフリーメール（無料メール）ドメイン。住所録に 1 人いても
/// ドメイン単位で自動グリーンにはしない（本人＝is_known だけグリーン扱い）。
const FREEMAIL_DOMAINS: &[&str] = &[
    "gmail.com",
    "googlemail.com",
    "yahoo.com",
    "yahoo.co.jp",
    "ymail.com",
    "rocketmail.com",
    "outlook.com",
    "outlook.jp",
    "hotmail.com",
    "hotmail.co.jp",
    "hotmail.co.uk",
    "live.com",
    "live.jp",
    "msn.com",
    "icloud.com",
    "me.com",
    "mac.com",
    "aol.com",
    "gmx.com",
    "gmx.net",
    "gmx.de",
    "web.de",
    "t-online.de",
    "proton.me",
    "protonmail.com",
    "pm.me",
    "zoho.com",
    "mail.com",
    "yandex.com",
    "yandex.ru",
    "mail.ru",
    "qq.com",
    "163.com",
    "126.com",
    "sina.com",
    "naver.com",
    "hanmail.net",
    "daum.net",
    "nifty.com",
    "so-net.ne.jp",
    "docomo.ne.jp",
    "ezweb.ne.jp",
    "au.com",
    "softbank.ne.jp",
    "i.softbank.jp",
    "ybb.ne.jp",
    "ocn.ne.jp",
    "biglobe.ne.jp",
    "plala.or.jp",
    "excite.co.jp",
    "infoseek.jp",
    "orange.fr",
    "free.fr",
    "laposte.net",
    "libero.it",
];

/// メールアドレスからドメイン部（小文字・trim）を取り出す。'@' が無ければ None。
pub(crate) fn domain_of(email: &str) -> Option<String> {
    let e = email.trim();
    let at = e.rfind('@')?;
    let dom = e[at + 1..].trim().trim_end_matches('.').to_lowercase();
    if dom.is_empty() {
        None
    } else {
        Some(dom)
    }
}

/// フリーメール（無料メール）ドメインか。
pub(crate) fn is_freemail(domain: &str) -> bool {
    let d = domain.to_lowercase();
    FREEMAIL_DOMAINS.contains(&d.as_str())
}

/// 旧バージョンで手動グリーンに登録されたフリーメールドメイン（gmail.com 等）を取り除く。
/// ドメイン単位の信頼はフリーメールでは成立しない（誰でも取得できる）ため、起動時に掃除する。
/// 冪等（該当が無ければ 0 件）。戻り値は削除した行数。
pub(crate) fn purge_freemail_green_domains(conn: &Connection) -> rusqlite::Result<usize> {
    let placeholders = FREEMAIL_DOMAINS
        .iter()
        .map(|_| "?")
        .collect::<Vec<_>>()
        .join(",");
    let sql = format!("DELETE FROM green_domains WHERE domain IN ({placeholders})");
    conn.execute(&sql, rusqlite::params_from_iter(FREEMAIL_DOMAINS.iter()))
}

/// グリーンとみなすドメイン集合（手動 ∪ 住所録由来 − フリーメール − 警告）。
/// 一覧・詳細の is_green 判定で使う（呼び出し側の接続を借りる＝再ロックしない）。
pub(crate) fn green_domain_set(conn: &Connection) -> rusqlite::Result<HashSet<String>> {
    let mut set: HashSet<String> = HashSet::new();
    // 手動認定（フリーメールはドメイン単位で信頼しない。旧バージョンで登録された行への防御）。
    {
        let mut stmt = conn.prepare("SELECT domain FROM green_domains")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for d in rows {
            let dom = d?.to_lowercase();
            if !is_freemail(&dom) {
                set.insert(dom);
            }
        }
    }
    // 住所録由来（削除済み連絡先は除く。フリーメールは除外）。
    for sql in [
        "SELECT DISTINCT ce.value FROM contact_emails ce JOIN contacts c ON c.id = ce.contact_id \
         WHERE ce.value IS NOT NULL AND c.deleted_at IS NULL",
        "SELECT DISTINCT c.email FROM contacts c WHERE c.email IS NOT NULL AND c.deleted_at IS NULL",
    ] {
        let mut stmt = conn.prepare(sql)?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for v in rows {
            if let Some(dom) = domain_of(&v?) {
                if !is_freemail(&dom) {
                    set.insert(dom);
                }
            }
        }
    }
    // 警告ドメインを除外。
    {
        let mut stmt = conn.prepare("SELECT domain FROM warning_domains")?;
        let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
        for d in rows {
            set.remove(&d?.to_lowercase());
        }
    }
    Ok(set)
}

/// アドレスが住所録の本人（完全一致・非削除）か。
pub(crate) fn address_is_known(conn: &Connection, address: &str) -> rusqlite::Result<bool> {
    let addr = address.trim();
    if addr.is_empty() {
        return Ok(false);
    }
    let n: i64 = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM contacts c WHERE c.deleted_at IS NULL AND lower(c.email) = lower(?1)) \
              OR EXISTS (SELECT 1 FROM contact_emails ce JOIN contacts c ON c.id = ce.contact_id \
                         WHERE c.deleted_at IS NULL AND lower(ce.value) = lower(?1))",
        params![addr],
        |r| r.get(0),
    )?;
    Ok(n != 0)
}

/// アドレスが住所録のお気に入り（VIP）連絡先か（完全一致・非削除）。
pub(crate) fn address_is_vip(conn: &Connection, address: &str) -> rusqlite::Result<bool> {
    let addr = address.trim();
    if addr.is_empty() {
        return Ok(false);
    }
    let n: i64 = conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM contacts c \
                        WHERE c.deleted_at IS NULL AND c.is_favorite = 1 AND lower(c.email) = lower(?1)) \
              OR EXISTS (SELECT 1 FROM contact_emails ce JOIN contacts c ON c.id = ce.contact_id \
                         WHERE c.deleted_at IS NULL AND c.is_favorite = 1 AND lower(ce.value) = lower(?1))",
        params![addr],
        |r| r.get(0),
    )?;
    Ok(n != 0)
}

/// アドレスがグリーンか（本人一致 or ドメインがグリーン集合）。
pub(crate) fn address_is_green(
    conn: &Connection,
    set: &HashSet<String>,
    address: Option<&str>,
) -> rusqlite::Result<bool> {
    let Some(addr) = address.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(false);
    };
    if let Some(dom) = domain_of(addr) {
        if set.contains(&dom) {
            return Ok(true);
        }
    }
    address_is_known(conn, addr)
}

impl Store {
    /// グリーン／警告ドメインの一覧（管理タブ用）。住所録由来の自動グリーンも含める。
    pub fn list_green_domains(&self) -> rusqlite::Result<Vec<GreenDomainEntry>> {
        let conn = self.conn.lock().unwrap();

        // ドメインごとの連絡先件数（非削除・フリーメール除く。参考表示＆自動判定用）。
        let mut contact_domains: HashMap<String, i32> = HashMap::new();
        for sql in [
            "SELECT ce.value FROM contact_emails ce JOIN contacts c ON c.id = ce.contact_id \
             WHERE ce.value IS NOT NULL AND c.deleted_at IS NULL",
            "SELECT c.email FROM contacts c WHERE c.email IS NOT NULL AND c.deleted_at IS NULL",
        ] {
            let mut stmt = conn.prepare(sql)?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            for v in rows {
                if let Some(dom) = domain_of(&v?) {
                    if !is_freemail(&dom) {
                        *contact_domains.entry(dom).or_insert(0) += 1;
                    }
                }
            }
        }

        let manual: HashMap<String, Option<String>> = {
            let mut m = HashMap::new();
            let mut stmt = conn.prepare("SELECT domain, note FROM green_domains")?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?;
            for row in rows {
                let (d, n) = row?;
                let dom = d.to_lowercase();
                // フリーメールはドメイン単位で信頼しない（掃除前の旧行が残っていても出さない）。
                if !is_freemail(&dom) {
                    m.insert(dom, n);
                }
            }
            m
        };
        let warning: HashMap<String, Option<String>> = {
            let mut m = HashMap::new();
            let mut stmt = conn.prepare("SELECT domain, note FROM warning_domains")?;
            let rows =
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)))?;
            for row in rows {
                let (d, n) = row?;
                m.insert(d.to_lowercase(), n);
            }
            m
        };

        let mut out: Vec<GreenDomainEntry> = Vec::new();
        // 警告（除外）ドメイン。
        for (dom, note) in &warning {
            out.push(GreenDomainEntry {
                domain: dom.clone(),
                kind: "warning".to_string(),
                auto: contact_domains.contains_key(dom),
                contact_count: *contact_domains.get(dom).unwrap_or(&0),
                note: note.clone(),
            });
        }
        // グリーン = 手動 ∪ 住所録由来 − 警告。
        let mut green_keys: HashSet<String> = manual.keys().cloned().collect();
        for k in contact_domains.keys() {
            green_keys.insert(k.clone());
        }
        for dom in green_keys {
            if warning.contains_key(&dom) {
                continue;
            }
            let is_manual = manual.contains_key(&dom);
            out.push(GreenDomainEntry {
                domain: dom.clone(),
                kind: "green".to_string(),
                auto: contact_domains.contains_key(&dom),
                contact_count: *contact_domains.get(&dom).unwrap_or(&0),
                note: manual.get(&dom).cloned().flatten().filter(|_| is_manual),
            });
        }
        out.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then_with(|| b.contact_count.cmp(&a.contact_count))
                .then_with(|| a.domain.cmp(&b.domain))
        });
        Ok(out)
    }

    /// ドメインをグリーンに認定（警告から外し、手動グリーンに登録）。
    ///
    /// * `domain` - 認定するドメイン（大文字小文字は無視）
    /// * `note` - 任意のメモ
    ///
    /// 戻り値は認定したか。**フリーメール（gmail.com 等）は認定しない**（`Ok(false)`）。
    /// 誰でも取得できるドメインをまとめて信頼すると、無関係な差出人まで信頼扱いになるため。
    /// この場合は差出人を住所録に登録して「本人」として信頼する。
    pub fn add_green_domain(&self, domain: &str, note: Option<&str>) -> rusqlite::Result<bool> {
        let dom = domain.trim().to_lowercase();
        if dom.is_empty() {
            return Ok(false);
        }
        if is_freemail(&dom) {
            return Ok(false);
        }
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM warning_domains WHERE domain = ?1", params![dom])?;
        conn.execute(
            "INSERT INTO green_domains (domain, note) VALUES (?1, ?2) \
             ON CONFLICT(domain) DO UPDATE SET note = ?2",
            params![dom, note],
        )?;
        Ok(true)
    }

    /// ドメインを警告（グリーン解除）に。手動グリーンから外し、警告へ登録（自動再登録を防ぐ）。
    pub fn warn_green_domain(&self, domain: &str, note: Option<&str>) -> rusqlite::Result<()> {
        let dom = domain.trim().to_lowercase();
        if dom.is_empty() {
            return Ok(());
        }
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM green_domains WHERE domain = ?1", params![dom])?;
        conn.execute(
            "INSERT INTO warning_domains (domain, note) VALUES (?1, ?2) \
             ON CONFLICT(domain) DO UPDATE SET note = ?2",
            params![dom, note],
        )?;
        Ok(())
    }

    /// ドメインを中立に戻す（グリーン・警告の両方から外す。住所録由来なら自動グリーンに戻る）。
    pub fn clear_green_domain(&self, domain: &str) -> rusqlite::Result<()> {
        let dom = domain.trim().to_lowercase();
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM green_domains WHERE domain = ?1", params![dom])?;
        conn.execute("DELETE FROM warning_domains WHERE domain = ?1", params![dom])?;
        Ok(())
    }

    /// 単一アドレスがグリーンか（詳細画面のバッジ・ボタン用）。
    pub fn address_green(&self, address: &str) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        let set = green_domain_set(&conn)?;
        address_is_green(&conn, &set, Some(address))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ContactInput;

    fn store() -> Store {
        Store::open_in_memory_for_test()
    }

    #[test]
    fn domain_and_freemail_helpers() {
        assert_eq!(domain_of("a@Sng-Design.com").as_deref(), Some("sng-design.com"));
        assert_eq!(domain_of("noatsign").as_deref(), None);
        assert!(is_freemail("gmail.com"));
        assert!(!is_freemail("sng-design.com"));
    }

    #[test]
    fn auto_green_excludes_freemail_and_warning_overrides() {
        let s = store();
        // 会社ドメインの連絡先 → ドメイン自動グリーン。
        s.upsert_contact(&ContactInput {
            display_name: "会社の人".into(),
            email: Some("taro@acme.co.jp".into()),
            ..Default::default()
        })
        .unwrap();
        // フリーメールの連絡先 → ドメインは自動グリーンにしない（本人だけ）。
        s.upsert_contact(&ContactInput {
            display_name: "個人".into(),
            email: Some("hanako@gmail.com".into()),
            ..Default::default()
        })
        .unwrap();

        assert!(s.address_green("info@acme.co.jp").unwrap(), "会社ドメインは自動グリーン");
        assert!(s.address_green("taro@acme.co.jp").unwrap());
        assert!(s.address_green("hanako@gmail.com").unwrap(), "本人はグリーン(is_known)");
        assert!(!s.address_green("spam@gmail.com").unwrap(), "フリーメール他人はグリーンでない");
        assert!(!s.address_green("x@unknown.co.jp").unwrap());

        // 手動認定。
        assert!(s.add_green_domain("newsletter.example.com", None).unwrap());
        assert!(s.address_green("hi@newsletter.example.com").unwrap());

        // フリーメールは手動でもドメイン単位で認定しない（本人＝住所録一致だけを信頼する）。
        assert!(!s.add_green_domain("gmail.com", None).unwrap());
        assert!(!s.address_green("stranger@gmail.com").unwrap());

        // 解除（警告）→ 住所録由来でもグリーンでなくなる（自動再登録されない）。
        s.warn_green_domain("acme.co.jp", None).unwrap();
        assert!(!s.address_green("info@acme.co.jp").unwrap(), "警告は自動グリーンを上書き");
        assert!(s.address_green("taro@acme.co.jp").unwrap(), "本人(is_known)は残る");

        // 中立に戻すと自動グリーンが復活。
        s.clear_green_domain("acme.co.jp").unwrap();
        assert!(s.address_green("info@acme.co.jp").unwrap());

        // 一覧に会社ドメインが green、gmail は出ない、警告も分けて出る。
        s.warn_green_domain("spam.example.com", None).unwrap();
        let list = s.list_green_domains().unwrap();
        assert!(list.iter().any(|e| e.domain == "acme.co.jp" && e.kind == "green" && e.auto));
        assert!(list.iter().any(|e| e.domain == "spam.example.com" && e.kind == "warning"));
        assert!(!list.iter().any(|e| e.domain == "gmail.com"));
    }

    #[test]
    fn purge_removes_legacy_freemail_green_rows() {
        let s = store();
        {
            // 旧バージョンで登録されたフリーメールの手動グリーンを模す。
            let conn = s.conn.lock().unwrap();
            conn.execute(
                "INSERT INTO green_domains (domain) VALUES ('gmail.com'), ('acme.co.jp')",
                [],
            )
            .unwrap();
            // 掃除前でも、ドメイン単位の信頼としては効かせない。
            let set = green_domain_set(&conn).unwrap();
            assert!(!address_is_green(&conn, &set, Some("x@gmail.com")).unwrap());
            assert_eq!(purge_freemail_green_domains(&conn).unwrap(), 1);
            let left: i64 = conn
                .query_row("SELECT COUNT(*) FROM green_domains", [], |r| r.get(0))
                .unwrap();
            assert_eq!(left, 1, "フリーメールだけ消え、通常ドメインは残る");
        }
        assert!(s.address_green("info@acme.co.jp").unwrap());
    }
}
