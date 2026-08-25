//! Google 連絡先（People API）取り込みの台帳操作（マイグレーション 0054）。
//!
//! 取り込んだ連絡先は `contacts` へ直接入れず、いったん `contact_identities` に溜める。
//! 既存の住所録と全件重複させないためで、「ローカルの誰と同じ人か」の判定は照合フェーズが行う。
//! ここでは Google 側の状態をそのまま鏡写しに保つことだけを責務にする。

use super::{ApplyOutcome, Store};
use crate::services::vcard::ImportedContact;
use rusqlite::{params, OptionalExtension};

/// 同期エンジン（services/google/contacts）が Store へ渡す「Google 側の連絡先」1 件。
/// Store 層を同期エンジンに依存させないため、境界の受け渡し型はここ（store 側）に置く。
#[derive(Debug, Clone)]
pub struct RemoteContact {
    /// People API の resourceName（'people/c1234567890'）。
    pub external_id: String,
    /// 更新時に必須の etag。
    pub etag: Option<String>,
    /// Google 側で削除された（増分同期の metadata.deleted）。
    pub deleted: bool,
    /// 取り込んだ内容。削除通知のときは None。
    pub contact: Option<ImportedContact>,
}

/// 台帳 1 件の読み出し結果。
#[derive(Debug, Clone)]
pub struct ContactIdentity {
    /// 紐付いたローカル連絡先。None＝未照合。
    pub contact_id: Option<i64>,
    /// 保存してある etag（送信時に必要）。
    pub etag: Option<String>,
    /// Google 側で削除された印。
    pub remote_deleted: bool,
    /// 取り込んだ内容（保存された JSON を読み戻せなければ None）。
    pub snapshot: Option<ImportedContact>,
}

impl Store {
    /// Google 側の連絡先 1 件を台帳へ反映する。
    ///
    /// 既存行の `contact_id`（照合済みの紐付け）は保持する。削除通知は印を付けるだけで行を
    /// 消さない（ローカル連絡先をどう扱うかは送信フェーズの判断で、取り込みでは決めない）。
    pub fn apply_remote_contact(
        &self,
        account_id: i64,
        remote: &RemoteContact,
    ) -> rusqlite::Result<ApplyOutcome> {
        let conn = self.conn.lock().unwrap();
        if remote.deleted {
            let n = conn.execute(
                "UPDATE contact_identities SET remote_deleted = 1, fetched_at = CURRENT_TIMESTAMP \
                 WHERE provider = 'google' AND account_id = ?1 AND external_id = ?2",
                params![account_id, remote.external_id],
            )?;
            // 取り込んだ覚えのない ID の削除通知は無視する。
            return Ok(if n > 0 {
                ApplyOutcome::Deleted
            } else {
                ApplyOutcome::Skipped
            });
        }

        let Some(contact) = remote.contact.as_ref() else {
            return Ok(ApplyOutcome::Skipped);
        };
        // 保存に失敗する JSON は無いはずだが、失敗しても同期全体は止めない。
        let snapshot = serde_json::to_string(contact).ok();
        conn.execute(
            "INSERT INTO contact_identities \
                 (provider, account_id, external_id, etag, snapshot, remote_deleted, fetched_at) \
             VALUES ('google', ?1, ?2, ?3, ?4, 0, CURRENT_TIMESTAMP) \
             ON CONFLICT(provider, account_id, external_id) DO UPDATE SET \
                 etag = ?3, snapshot = ?4, remote_deleted = 0, fetched_at = CURRENT_TIMESTAMP",
            params![account_id, remote.external_id, remote.etag, snapshot],
        )?;
        Ok(ApplyOutcome::Upserted)
    }

    /// 次回の増分同期トークンを保存する（None でフル同期に戻す）。
    pub fn set_contacts_sync_token(
        &self,
        account_id: i64,
        token: Option<&str>,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE google_accounts SET contacts_sync_token = ?2 WHERE id = ?1",
            params![account_id, token],
        )?;
        Ok(())
    }

    /// 保存済みの増分同期トークン（未取得なら None＝フル同期）。
    pub fn contacts_sync_token(&self, account_id: i64) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT contacts_sync_token FROM google_accounts WHERE id = ?1",
            params![account_id],
            |r| r.get(0),
        )
        .optional()
        .map(Option::flatten)
    }

    /// 取り込み済みのうち、まだローカル連絡先と結び付いていない件数（照合フェーズの対象数）。
    pub fn count_unlinked_identities(&self, account_id: i64) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT count(*) FROM contact_identities \
             WHERE provider = 'google' AND account_id = ?1 \
               AND contact_id IS NULL AND remote_deleted = 0",
            params![account_id],
            |r| r.get(0),
        )
    }

    /// 台帳 1 件を読み出す（照合フェーズ・送信フェーズ用）。
    pub fn contact_identity(
        &self,
        account_id: i64,
        external_id: &str,
    ) -> rusqlite::Result<Option<ContactIdentity>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT contact_id, etag, remote_deleted, snapshot FROM contact_identities \
             WHERE provider = 'google' AND account_id = ?1 AND external_id = ?2",
            params![account_id, external_id],
            |r| {
                let snapshot: Option<String> = r.get(3)?;
                Ok(ContactIdentity {
                    contact_id: r.get(0)?,
                    etag: r.get(1)?,
                    remote_deleted: r.get::<_, i64>(2)? != 0,
                    snapshot: snapshot
                        .as_deref()
                        .and_then(|s| serde_json::from_str(s).ok()),
                })
            },
        )
        .optional()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem_store() -> Store {
        Store::open_in_memory_for_test()
    }

    fn account(s: &Store) -> i64 {
        s.upsert_google_account("a@gmail.com", None, None).unwrap()
    }

    fn remote(id: &str, name: &str, etag: &str) -> RemoteContact {
        RemoteContact {
            external_id: id.into(),
            etag: Some(etag.into()),
            deleted: false,
            contact: Some(ImportedContact {
                display_name: name.into(),
                source: "google".into(),
                external_id: Some(id.into()),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn upsert_keeps_the_row_unique_and_refreshes_etag() {
        let s = mem_store();
        let acct = account(&s);

        assert_eq!(
            s.apply_remote_contact(acct, &remote("people/c1", "山田太郎", "etag1"))
                .unwrap(),
            ApplyOutcome::Upserted
        );
        // 同じ resourceName の再取得は 1 行のまま内容だけ差し替わる。
        s.apply_remote_contact(acct, &remote("people/c1", "山田 太郎", "etag2"))
            .unwrap();

        let got = s.contact_identity(acct, "people/c1").unwrap().unwrap();
        assert_eq!(got.contact_id, None); // 未照合のまま
        assert_eq!(got.etag.as_deref(), Some("etag2"));
        assert!(!got.remote_deleted);
        assert_eq!(got.snapshot.unwrap().display_name, "山田 太郎");
        assert_eq!(s.count_unlinked_identities(acct).unwrap(), 1);
    }

    #[test]
    fn upsert_does_not_clobber_an_existing_link() {
        let s = mem_store();
        let acct = account(&s);
        s.apply_remote_contact(acct, &remote("people/c1", "山田太郎", "etag1"))
            .unwrap();
        // 照合フェーズが紐付けた状態を作る（contact_id は実在の連絡先しか指せない）。
        let local_id = {
            let conn = s.conn.lock().unwrap();
            conn.execute("INSERT INTO contacts (display_name) VALUES ('山田太郎')", [])
                .unwrap();
            let id = conn.last_insert_rowid();
            conn.execute(
                "UPDATE contact_identities SET contact_id = ?1 WHERE external_id = 'people/c1'",
                params![id],
            )
            .unwrap();
            id
        };

        s.apply_remote_contact(acct, &remote("people/c1", "山田 太郎", "etag2"))
            .unwrap();
        let got = s.contact_identity(acct, "people/c1").unwrap().unwrap();
        assert_eq!(
            got.contact_id,
            Some(local_id),
            "取り込みで紐付けを失ってはならない"
        );
        // 紐付け済みは照合フェーズの対象から外れる。
        assert_eq!(s.count_unlinked_identities(acct).unwrap(), 0);
    }

    #[test]
    fn delete_marks_the_row_instead_of_removing_it() {
        let s = mem_store();
        let acct = account(&s);
        s.apply_remote_contact(acct, &remote("people/c1", "山田太郎", "etag1"))
            .unwrap();

        let del = RemoteContact {
            external_id: "people/c1".into(),
            etag: None,
            deleted: true,
            contact: None,
        };
        assert_eq!(
            s.apply_remote_contact(acct, &del).unwrap(),
            ApplyOutcome::Deleted
        );
        let got = s.contact_identity(acct, "people/c1").unwrap().unwrap();
        assert!(got.remote_deleted);
        // 内容は消さずに残す（送信フェーズが「何が消えたか」を見られるように）。
        assert!(got.snapshot.is_some());
        assert_eq!(s.count_unlinked_identities(acct).unwrap(), 0);
    }

    #[test]
    fn delete_for_an_unknown_id_is_ignored() {
        let s = mem_store();
        let acct = account(&s);
        let del = RemoteContact {
            external_id: "people/unknown".into(),
            etag: None,
            deleted: true,
            contact: None,
        };
        assert_eq!(
            s.apply_remote_contact(acct, &del).unwrap(),
            ApplyOutcome::Skipped
        );
    }

    #[test]
    fn deleting_the_local_contact_only_unlinks_the_identity() {
        let s = mem_store();
        let acct = account(&s);
        s.apply_remote_contact(acct, &remote("people/c1", "山田太郎", "etag1"))
            .unwrap();
        let conn_ops = |sql: &str| {
            let conn = s.conn.lock().unwrap();
            conn.execute(sql, []).unwrap();
        };
        conn_ops("INSERT INTO contacts (id, display_name) VALUES (7, '山田太郎')");
        conn_ops("UPDATE contact_identities SET contact_id = 7 WHERE external_id = 'people/c1'");
        // ローカル側を物理削除しても、取り込み台帳は未照合として残る（ON DELETE SET NULL）。
        conn_ops("DELETE FROM contacts WHERE id = 7");

        let got = s.contact_identity(acct, "people/c1").unwrap().unwrap();
        assert_eq!(got.contact_id, None);
        assert!(got.snapshot.is_some());
        assert_eq!(s.count_unlinked_identities(acct).unwrap(), 1);
    }

    #[test]
    fn sync_token_round_trips_and_clears() {
        let s = mem_store();
        let acct = account(&s);
        assert_eq!(s.contacts_sync_token(acct).unwrap(), None);
        s.set_contacts_sync_token(acct, Some("tok1")).unwrap();
        assert_eq!(s.contacts_sync_token(acct).unwrap().as_deref(), Some("tok1"));
        // 失効時はフル同期へ戻せる。
        s.set_contacts_sync_token(acct, None).unwrap();
        assert_eq!(s.contacts_sync_token(acct).unwrap(), None);
    }

    #[test]
    fn identities_are_scoped_per_account() {
        let s = mem_store();
        let a = account(&s);
        let b = s.upsert_google_account("b@gmail.com", None, None).unwrap();
        s.apply_remote_contact(a, &remote("people/c1", "A の連絡先", "e1"))
            .unwrap();
        s.apply_remote_contact(b, &remote("people/c1", "B の連絡先", "e1"))
            .unwrap();
        // 同じ resourceName でもアカウントが違えば別の行。
        assert_eq!(s.count_unlinked_identities(a).unwrap(), 1);
        assert_eq!(s.count_unlinked_identities(b).unwrap(), 1);
        let got = s.contact_identity(b, "people/c1").unwrap().unwrap();
        assert_eq!(got.snapshot.unwrap().display_name, "B の連絡先");
    }
}
