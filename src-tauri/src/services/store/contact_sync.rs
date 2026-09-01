//! Google 連絡先（People API）取り込みの台帳操作（マイグレーション 0054）。
//!
//! 取り込んだ連絡先は `contacts` へ直接入れず、いったん `contact_identities` に溜める。
//! 既存の住所録と全件重複させないためで、「ローカルの誰と同じ人か」の判定は照合が行う。
//!
//! このモジュールの責務は 2 つ。取り込み（pull）については **Google 側の状態をそのまま鏡写しに
//! 保つ**こと。照合については、判定そのものは `services::contact_match`（DB を見ない）に任せ、
//! **その結果を住所録と台帳へ書き込む**こと。

use super::{ApplyOutcome, Store};
use crate::models::GcontactsMatchResult;
use crate::services::contact_match::{self, MatchDecision, MatchOutcome};
use crate::services::vcard::ImportedContact;
use rusqlite::{params, OptionalExtension};
use std::collections::HashSet;

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

    /// 未照合の台帳を（外部 ID, 取り込んだ内容）で返す（照合フェーズの入力）。
    /// 内容を読み戻せない行は判定材料が無いので飛ばす。順序は external_id 昇順で固定し、
    /// 同じ台帳からは何度計画しても同じ結果が出るようにする。
    pub fn unlinked_identities(
        &self,
        account_id: i64,
    ) -> rusqlite::Result<Vec<(String, ImportedContact)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT external_id, snapshot FROM contact_identities \
             WHERE provider = 'google' AND account_id = ?1 \
               AND contact_id IS NULL AND remote_deleted = 0 \
             ORDER BY external_id",
        )?;
        let rows = stmt.query_map(params![account_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (external_id, snapshot) = row?;
            if let Some(c) = snapshot.as_deref().and_then(|s| serde_json::from_str(s).ok()) {
                out.push((external_id, c));
            }
        }
        Ok(out)
    }

    /// このアカウントの台帳がすでに掴んでいるローカル連絡先 ID。
    /// 1 人のローカル連絡先を 2 つの外部 ID が掴まないよう、紐付け先から外すために使う。
    fn linked_contact_ids(&self, account_id: i64) -> rusqlite::Result<HashSet<i64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT DISTINCT contact_id FROM contact_identities \
             WHERE provider = 'google' AND account_id = ?1 AND contact_id IS NOT NULL",
        )?;
        let rows = stmt.query_map(params![account_id], |r| r.get::<_, i64>(0))?;
        rows.collect()
    }

    /// 照合の計画を立てる（読み取りのみ）。プレビューと適用が同じ道を通るよう 1 か所に集める。
    /// 戻り値は（材料にした台帳, 1 件ずつの判定）。判定は台帳と同じ順で並ぶ。
    fn build_contact_match_plan(
        &self,
        account_id: i64,
    ) -> rusqlite::Result<(Vec<(String, ImportedContact)>, Vec<MatchOutcome>)> {
        // ロックは各ヘルパーの内側で完結させる（Mutex は非再入）。
        let remote = self.unlinked_identities(account_id)?;
        let already_linked = self.linked_contact_ids(account_id)?;
        let locals = self.contacts_for_dedupe()?;
        let plan = contact_match::plan(&remote, &locals, &already_linked);
        Ok((remote, plan))
    }

    /// 照合の下見（件数だけ。DB は変えない）。
    pub fn preview_contact_matches(
        &self,
        account_id: i64,
    ) -> rusqlite::Result<GcontactsMatchResult> {
        let (_, plan) = self.build_contact_match_plan(account_id)?;
        Ok(summarize(&plan))
    }

    /// 照合を適用する。高確信は既存へ紐付け、それ以外は新規として住所録に起こして紐付ける。
    ///
    /// 起こした連絡先のうち「似た相手が居たもの」は、既存の重複整理が同じ物差しで拾う
    /// （判定に `services::dedupe` を使っているため）。ここで人に代わって統合はしない。
    pub fn apply_contact_matches(
        &self,
        account_id: i64,
    ) -> rusqlite::Result<GcontactsMatchResult> {
        let (remote, plan) = self.build_contact_match_plan(account_id)?;
        let report = summarize(&plan);

        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        // 判定は台帳と同じ順に並ぶので、位置で対応付けられる。
        for (outcome, (external_id, contact)) in plan.iter().zip(remote.iter()) {
            debug_assert_eq!(&outcome.external_id, external_id);
            let contact_id = match outcome.decision {
                MatchDecision::Link(id) => id,
                MatchDecision::Create => super::contacts::insert_from_import(&tx, contact)?,
            };
            tx.execute(
                "UPDATE contact_identities SET contact_id = ?1 \
                 WHERE provider = 'google' AND account_id = ?2 AND external_id = ?3",
                params![contact_id, account_id, external_id],
            )?;
        }
        tx.commit()?;
        Ok(report)
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

/// 計画を UI 向けの件数にまとめる。
fn summarize(plan: &[MatchOutcome]) -> GcontactsMatchResult {
    let linked = plan
        .iter()
        .filter(|o| matches!(o.decision, MatchDecision::Link(_)))
        .count();
    // 「似た相手が居たのに決めきれず新規にした」件数＝このあと重複整理に出る見込み。
    let ambiguous = plan.iter().filter(|o| !o.rivals.is_empty()).count();
    GcontactsMatchResult {
        linked: linked as i32,
        created: (plan.len() - linked) as i32,
        ambiguous: ambiguous as i32,
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

    /// ローカル連絡先を 1 件起こす（照合の相手役）。
    fn local_contact(s: &Store, name: &str, email: &str) -> i64 {
        let conn = s.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO contacts (display_name, email) VALUES (?1, ?2)",
            params![name, email],
        )
        .unwrap();
        conn.last_insert_rowid()
    }

    fn remote_with_email(id: &str, name: &str, email: &str) -> RemoteContact {
        RemoteContact {
            external_id: id.into(),
            etag: Some("e1".into()),
            deleted: false,
            contact: Some(ImportedContact {
                display_name: name.into(),
                email: Some(email.into()),
                source: "google".into(),
                external_id: Some(id.into()),
                ..Default::default()
            }),
        }
    }

    #[test]
    fn matching_links_the_known_one_and_creates_the_rest() {
        let s = mem_store();
        let acct = account(&s);
        let known = local_contact(&s, "末松 信吾", "s@x.jp");
        s.apply_remote_contact(acct, &remote_with_email("people/c1", "末松信吾", "s@x.jp"))
            .unwrap();
        s.apply_remote_contact(acct, &remote_with_email("people/c2", "山田太郎", "t@y.jp"))
            .unwrap();

        // 下見は DB を変えない。
        let preview = s.preview_contact_matches(acct).unwrap();
        assert_eq!((preview.linked, preview.created, preview.ambiguous), (1, 1, 0));
        assert_eq!(s.count_unlinked_identities(acct).unwrap(), 2);

        let applied = s.apply_contact_matches(acct).unwrap();
        assert_eq!((applied.linked, applied.created, applied.ambiguous), (1, 1, 0));
        // 既知の相手は既存の連絡先へ紐付き、住所録は増えない。
        assert_eq!(
            s.contact_identity(acct, "people/c1").unwrap().unwrap().contact_id,
            Some(known)
        );
        // 未知の相手は新規として起こし、その ID で紐付く。
        let created = s
            .contact_identity(acct, "people/c2")
            .unwrap()
            .unwrap()
            .contact_id
            .expect("新規として起こした連絡先に紐付くはず");
        assert_ne!(created, known);
        assert_eq!(s.get_contact(created).unwrap().display_name, "山田太郎");
        assert_eq!(s.count_unlinked_identities(acct).unwrap(), 0);
    }

    #[test]
    fn applying_again_has_nothing_left_to_do() {
        let s = mem_store();
        let acct = account(&s);
        s.apply_remote_contact(acct, &remote_with_email("people/c1", "山田太郎", "t@y.jp"))
            .unwrap();
        s.apply_contact_matches(acct).unwrap();

        // 2 度目は未照合が無いので何も起こさない（押し直しで二重登録しない）。
        let again = s.apply_contact_matches(acct).unwrap();
        assert_eq!((again.linked, again.created, again.ambiguous), (0, 0, 0));
        let n: i64 = {
            let conn = s.conn.lock().unwrap();
            conn.query_row("SELECT count(*) FROM contacts", [], |r| r.get(0))
                .unwrap()
        };
        assert_eq!(n, 1);
    }

    #[test]
    fn an_ambiguous_one_is_created_and_counted_for_review() {
        let s = mem_store();
        let acct = account(&s);
        // 同名だけの一致（同姓同名の別人があり得る）は自動で寄せない。
        local_contact(&s, "山田太郎", "a@x.jp");
        s.apply_remote_contact(acct, &remote_with_email("people/c1", "山田太郎", "b@y.jp"))
            .unwrap();

        let r = s.apply_contact_matches(acct).unwrap();
        assert_eq!((r.linked, r.created, r.ambiguous), (0, 1, 1));
        // 起こした側は既存の重複整理が同じ物差しで拾う。
        let groups = s.find_duplicate_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].contacts.len(), 2);
    }

    #[test]
    fn merging_contacts_hands_the_link_over_to_the_survivor() {
        let s = mem_store();
        let acct = account(&s);
        let keep = local_contact(&s, "末松 信吾", "s@x.jp");
        s.apply_remote_contact(acct, &remote_with_email("people/c1", "末松信吾", "g@x.jp"))
            .unwrap();
        s.apply_contact_matches(acct).unwrap();
        let drop_id = s
            .contact_identity(acct, "people/c1")
            .unwrap()
            .unwrap()
            .contact_id
            .expect("新規として起こされているはず");
        assert_ne!(drop_id, keep);

        s.merge_contacts(keep, &[drop_id]).unwrap();

        // 統合で消えた側に付いていた紐付けは、残した側へ移る（外れると次の同期で二重に起こる）。
        assert_eq!(
            s.contact_identity(acct, "people/c1").unwrap().unwrap().contact_id,
            Some(keep)
        );
        assert_eq!(s.count_unlinked_identities(acct).unwrap(), 0);
    }
}
