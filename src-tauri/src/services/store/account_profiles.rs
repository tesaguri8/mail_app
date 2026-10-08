//! アカウント（設定のカード。アドレスごとに 1 枚）。docs/ACCOUNTS.md。
//!
//! カードの中身はそれぞれの表にある（メール = `accounts`、Google の OAuth 連携 =
//! `google_accounts`）。この層はカードそのもの（提供元・アドレス・呼び名・並び順）と、
//! 中身からカードへのつなぎ（`profile_id`）を受け持つ（マイグレーション 0063）。
//!
//! カードは中身が付くときに作り（[`ensure_profile`]）、中身が全部なくなったら消す
//! （[`prune_profile`]）。空のカードは残さない。

use super::Store;
use crate::models::{AccountProfile, AccountProvider};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

/// アドレスのカードを返す（無ければ作る）。行 id を返す。
///
/// 既にあって提供元が `imap`（その他）なら、より具体的な提供元（Google / iCloud）に上げる
/// （その他として足したアドレスに、あとから Google でログインした場合など）。
/// 逆向き（Google → その他）には下げない。
pub(super) fn ensure_profile(
    conn: &Connection,
    email: &str,
    provider: AccountProvider,
) -> rusqlite::Result<i64> {
    let existing: Option<(i64, String)> = conn
        .query_row(
            "SELECT id, provider FROM account_profiles WHERE lower(email) = lower(?1)",
            params![email],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match existing {
        Some((id, current)) => {
            if current == AccountProvider::Imap.as_str() && provider != AccountProvider::Imap {
                conn.execute(
                    "UPDATE account_profiles SET provider = ?2 WHERE id = ?1",
                    params![id, provider.as_str()],
                )?;
            }
            Ok(id)
        }
        None => {
            conn.execute(
                "INSERT INTO account_profiles (provider, email, sort_order) VALUES (?1, ?2, \
                 (SELECT COALESCE(max(sort_order), -1) + 1 FROM account_profiles))",
                params![provider.as_str(), email],
            )?;
            Ok(conn.last_insert_rowid())
        }
    }
}

/// 中身（メール・Google 連携）が 1 つも残っていないカードを消す。中身があれば何もしない。
pub(super) fn prune_profile(conn: &Connection, profile_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM account_profiles WHERE id = ?1 \
           AND NOT EXISTS (SELECT 1 FROM accounts WHERE profile_id = ?1) \
           AND NOT EXISTS (SELECT 1 FROM google_accounts WHERE profile_id = ?1)",
        params![profile_id],
    )?;
    Ok(())
}

impl Store {
    /// カードの一覧（並び順）。各カードに、このアドレスのメールアカウントと Google 連携の id を添える。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn list_account_profiles(&self) -> rusqlite::Result<Vec<AccountProfile>> {
        let conn = self.conn.lock().unwrap();
        let mut mail: HashMap<i64, Vec<i32>> = HashMap::new();
        {
            let mut stmt = conn.prepare(
                "SELECT profile_id, id FROM accounts WHERE profile_id IS NOT NULL \
                 ORDER BY COALESCE(sort_order, id), id",
            )?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
            for row in rows {
                let (profile, id) = row?;
                mail.entry(profile).or_default().push(id as i32);
            }
        }
        let mut stmt = conn.prepare(
            "SELECT p.id, p.provider, p.email, p.display_name, \
                    (SELECT g.id FROM google_accounts g WHERE g.profile_id = p.id \
                     ORDER BY g.id LIMIT 1) \
             FROM account_profiles p ORDER BY COALESCE(p.sort_order, p.id), p.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })?;
        rows.map(|row| {
            let (id, provider, email, display_name, google) = row?;
            Ok(AccountProfile {
                id: id as i32,
                // 表の CHECK 制約で 3 つに限られる。万一読めない綴りならその他として出す。
                provider: AccountProvider::from_db(&provider).unwrap_or(AccountProvider::Imap),
                email,
                display_name,
                mail_account_ids: mail.remove(&id).unwrap_or_default(),
                google_account_id: google.map(|g| g as i32),
            })
        })
        .collect()
    }

    /// カードの並び順を設定する（渡された id 順に 0, 1, 2…）。
    ///
    /// メールの一覧（左の欄・ホーム）はメールアカウントの並び順を使うので、そちらもカードの順に
    /// 振り直す（同じカードに複数あれば、その中の今の順を保つ）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき（全体を巻き戻す）。
    pub fn reorder_account_profiles(&self, ids: &[i64]) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        {
            let mut set_profile =
                tx.prepare("UPDATE account_profiles SET sort_order = ?1 WHERE id = ?2")?;
            ids.iter()
                .enumerate()
                .try_for_each(|(i, id)| set_profile.execute(params![i as i64, id]).map(|_| ()))?;
            let mail: Vec<i64> = {
                let mut stmt = tx.prepare(
                    "SELECT a.id FROM accounts a \
                     LEFT JOIN account_profiles p ON p.id = a.profile_id \
                     ORDER BY COALESCE(p.sort_order, p.id), COALESCE(a.sort_order, a.id), a.id",
                )?;
                let rows = stmt.query_map([], |r| r.get(0))?;
                rows.collect::<rusqlite::Result<_>>()?
            };
            let mut set_mail = tx.prepare("UPDATE accounts SET sort_order = ?1 WHERE id = ?2")?;
            mail.iter()
                .enumerate()
                .try_for_each(|(i, id)| set_mail.execute(params![i as i64, id]).map(|_| ()))?;
        }
        tx.commit()
    }

    /// カードの呼び名を変える（空なら消してアドレスで出す）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn rename_account_profile(&self, id: i64, name: Option<&str>) -> rusqlite::Result<()> {
        let name = name.map(str::trim).filter(|n| !n.is_empty());
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE account_profiles SET display_name = ?2 WHERE id = ?1",
            params![id, name],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::store::NewAccount;

    fn mail(s: &Store, email: &str, provider: AccountProvider) -> i64 {
        s.insert_account(&NewAccount {
            email: email.into(),
            display_name: None,
            username: None,
            imap_host: "imap.example.com".into(),
            imap_port: 993,
            smtp_host: "smtp.example.com".into(),
            smtp_port: 587,
            server_account_id: None,
            provider,
        })
        .unwrap()
    }

    #[test]
    fn mail_and_google_of_one_address_share_a_card() {
        let s = Store::open_in_memory_for_test();
        let m = mail(&s, "Smt@Gmail.com", AccountProvider::Imap);
        let g = s
            .upsert_google_account("smt@gmail.com", None, None)
            .unwrap();
        let other = mail(&s, "info@example.com", AccountProvider::Imap);

        let cards = s.list_account_profiles().unwrap();
        assert_eq!(cards.len(), 2);
        // その他で足したアドレスに Google でログインすると、カードは Google になる。
        assert_eq!(cards[0].provider, AccountProvider::Google);
        assert_eq!(cards[0].mail_account_ids, vec![m as i32]);
        assert_eq!(cards[0].google_account_id, Some(g as i32));
        assert_eq!(cards[1].provider, AccountProvider::Imap);
        assert_eq!(cards[1].mail_account_ids, vec![other as i32]);
        assert_eq!(cards[1].google_account_id, None);
    }

    #[test]
    fn card_disappears_when_its_last_service_goes() {
        let s = Store::open_in_memory_for_test();
        let m = mail(&s, "a@gmail.com", AccountProvider::Google);
        let g = s.upsert_google_account("a@gmail.com", None, None).unwrap();

        // Google を完全に解除しても、メールが残るのでカードは残る。
        s.purge_google_account(g).unwrap();
        let cards = s.list_account_profiles().unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].google_account_id, None);
        // 解除中（一時的）では行が残るので、カードも中身もそのまま。
        let g = s.upsert_google_account("a@gmail.com", None, None).unwrap();
        s.disconnect_google_account(g).unwrap();
        s.delete_account(m).unwrap();
        let cards = s.list_account_profiles().unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].google_account_id, Some(g as i32));
        // 最後の中身が消えるとカードも消える。
        s.purge_google_account(g).unwrap();
        assert!(s.list_account_profiles().unwrap().is_empty());
    }

    #[test]
    fn reorder_moves_cards_and_their_mail_accounts() {
        let s = Store::open_in_memory_for_test();
        let a = mail(&s, "a@example.com", AccountProvider::Imap);
        let b = mail(&s, "b@example.com", AccountProvider::Imap);
        s.upsert_google_account("c@gmail.com", None, None).unwrap();
        let cards = s.list_account_profiles().unwrap();
        let ids: Vec<i64> = [2, 0, 1].iter().map(|&i| i64::from(cards[i].id)).collect();

        s.reorder_account_profiles(&ids).unwrap();
        let emails: Vec<String> = s
            .list_account_profiles()
            .unwrap()
            .into_iter()
            .map(|c| c.email)
            .collect();
        assert_eq!(emails, ["c@gmail.com", "a@example.com", "b@example.com"]);
        let mail_order: Vec<i64> = s
            .list_accounts()
            .unwrap()
            .iter()
            .map(|x| i64::from(x.id))
            .collect();
        assert_eq!(mail_order, vec![a, b]);
    }

    #[test]
    fn rename_trims_and_clears() {
        let s = Store::open_in_memory_for_test();
        mail(&s, "a@example.com", AccountProvider::Imap);
        let id = i64::from(s.list_account_profiles().unwrap()[0].id);
        s.rename_account_profile(id, Some("  仕事  ")).unwrap();
        assert_eq!(
            s.list_account_profiles().unwrap()[0]
                .display_name
                .as_deref(),
            Some("仕事")
        );
        s.rename_account_profile(id, Some(" ")).unwrap();
        assert_eq!(s.list_account_profiles().unwrap()[0].display_name, None);
    }
}
