//! Google 連絡先（People API）取り込みの台帳操作（マイグレーション 0054）。
//!
//! 取り込んだ連絡先は `contacts` へ直接入れず、いったん `contact_identities` に溜める。
//! 既存の住所録と全件重複させないためで、「ローカルの誰と同じ人か」の判定は照合が行う。
//!
//! このモジュールの責務は 2 つ。取り込み（pull）については **Google 側の状態をそのまま鏡写しに
//! 保つ**こと。照合については、判定そのものは `services::contact_match`（DB を見ない）に任せ、
//! **その結果を住所録と台帳へ書き込む**こと。

use super::{ApplyOutcome, Store};
use crate::models::{ContactSummary, GcontactsMatchResult};
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

/// push 対象のローカル変更（`contacts.dirty = 1`）1 件。Google へ送る素材。
#[derive(Debug, Clone)]
pub struct ContactPush {
    pub contact_id: i64,
    /// 連携済みなら People API の resourceName。None＝Google 側にまだ無い（作成する）。
    pub external_id: Option<String>,
    /// 更新に必須の etag（読んだ版のものを送らないと People API に弾かれる）。
    pub etag: Option<String>,
    /// ローカルで論理削除された（Google 側も削除する）。
    pub deleted: bool,
    /// 送る中身（削除のときは使わない）。
    pub contact: ContactSummary,
}

/// 照合の計画: （材料にした台帳, 1 件ずつの判定）。両者は同じ順・同じ数で並ぶ。
type ContactMatchPlan = (Vec<(String, ImportedContact)>, Vec<MatchOutcome>);

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
    /// Google 側の連絡先 1 件を、台帳と（紐付いていれば）住所録へ反映する。
    ///
    /// 既存行の `contact_id`（照合済みの紐付け）は保持する。紐付いたローカル連絡先は、
    /// **未送信のローカル変更（`dirty = 1`）が無いときだけ**上書きする。push → pull の順で
    /// 走るので、送信に成功したものは既に `dirty = 0` になっている。送信に失敗して残った
    /// 変更を取り込みで潰さないための条件。
    pub fn apply_remote_contact(
        &self,
        account_id: i64,
        remote: &RemoteContact,
    ) -> rusqlite::Result<ApplyOutcome> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let outcome = if remote.deleted {
            let n = tx.execute(
                "UPDATE contact_identities SET remote_deleted = 1, fetched_at = CURRENT_TIMESTAMP \
                 WHERE provider = 'google' AND account_id = ?1 AND external_id = ?2",
                params![account_id, remote.external_id],
            )?;
            if n > 0 {
                // Google 側で消えたので、紐付いたローカル連絡先もゴミ箱へ（完全削除はしない）。
                tx.execute(
                    "UPDATE contacts SET deleted_at = CURRENT_TIMESTAMP \
                     WHERE dirty = 0 AND deleted_at IS NULL AND id IN ( \
                         SELECT contact_id FROM contact_identities \
                         WHERE provider = 'google' AND account_id = ?1 AND external_id = ?2 \
                           AND contact_id IS NOT NULL)",
                    params![account_id, remote.external_id],
                )?;
                ApplyOutcome::Deleted
            } else {
                // 取り込んだ覚えのない ID の削除通知は無視する。
                ApplyOutcome::Skipped
            }
        } else if let Some(contact) = remote.contact.as_ref() {
            // 保存に失敗する JSON は無いはずだが、失敗しても同期全体は止めない。
            let snapshot = serde_json::to_string(contact).ok();
            tx.execute(
                "INSERT INTO contact_identities \
                     (provider, account_id, external_id, etag, snapshot, remote_deleted, fetched_at) \
                 VALUES ('google', ?1, ?2, ?3, ?4, 0, CURRENT_TIMESTAMP) \
                 ON CONFLICT(provider, account_id, external_id) DO UPDATE SET \
                     etag = ?3, snapshot = ?4, remote_deleted = 0, fetched_at = CURRENT_TIMESTAMP",
                params![account_id, remote.external_id, remote.etag, snapshot],
            )?;
            // 紐付いたローカル連絡先があれば、Google の正本で更新する。
            let linked: Option<i64> = tx
                .query_row(
                    "SELECT ci.contact_id FROM contact_identities ci \
                     JOIN contacts c ON c.id = ci.contact_id \
                     WHERE ci.provider = 'google' AND ci.account_id = ?1 \
                       AND ci.external_id = ?2 AND c.dirty = 0",
                    params![account_id, remote.external_id],
                    |r| r.get(0),
                )
                .optional()?
                .flatten();
            if let Some(cid) = linked {
                super::contacts::update_from_import(&tx, cid, contact)?;
                // ラベル（Google のグループ）を付け外しする。Google が持っている名前だけを
                // 対象にし、アプリ内だけで付けたタグは触らない。
                let managed = managed_group_names(&tx, account_id)?;
                super::contacts::reconcile_managed_tags(&tx, cid, &contact.labels, &managed)?;
            }
            ApplyOutcome::Upserted
        } else {
            ApplyOutcome::Skipped
        };
        tx.commit()?;
        Ok(outcome)
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
    fn unlinked_identities(
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
    ) -> rusqlite::Result<ContactMatchPlan> {
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

    /// Google の連絡先グループ（ラベル）一覧で台帳を洗い替える。
    ///
    /// 取り込みのたびに Google の一覧そのままに置き換える。Google 側で消えたラベルの行が
    /// 残っていると、そのタグが「Google の持ち物」と誤判定され、取り込みで外されてしまう。
    pub fn replace_contact_groups(
        &self,
        account_id: i64,
        groups: &[(String, String)],
    ) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "DELETE FROM contact_group_identities WHERE provider = 'google' AND account_id = ?1",
            params![account_id],
        )?;
        for (external_id, name) in groups {
            tx.execute(
                "INSERT OR REPLACE INTO contact_group_identities \
                     (provider, account_id, external_id, name, fetched_at) \
                 VALUES ('google', ?1, ?2, ?3, CURRENT_TIMESTAMP)",
                params![account_id, external_id, name],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// ラベル名 → Google のグループ ID。未知の名前は None（送信側が作る）。
    pub fn contact_group_id(
        &self,
        account_id: i64,
        name: &str,
    ) -> rusqlite::Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT external_id FROM contact_group_identities \
             WHERE provider = 'google' AND account_id = ?1 AND name = ?2",
            params![account_id, name],
            |r| r.get(0),
        )
        .optional()
    }

    /// Google 側に作ったラベルを台帳へ覚える（次の取り込みまで ID を引けるように）。
    pub fn remember_contact_group(
        &self,
        account_id: i64,
        external_id: &str,
        name: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO contact_group_identities \
                 (provider, account_id, external_id, name, fetched_at) \
             VALUES ('google', ?1, ?2, ?3, CURRENT_TIMESTAMP)",
            params![account_id, external_id, name],
        )?;
        Ok(())
    }

    /// 「Rondine で新しく作った連絡先も Google 側に作る」設定。
    /// 既定は false。住所録を Google へ上げるかは利用者が決めることなので、明示的に
    /// 有効にしたときだけローカル生まれの連絡先を送る。
    fn push_new_contacts(&self, account_id: i64) -> rusqlite::Result<bool> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT push_new_contacts FROM google_accounts WHERE id = ?1",
            params![account_id],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .map(|v| v.unwrap_or(0) != 0)
    }

    /// 上の設定を変える。
    pub fn set_push_new_contacts(&self, account_id: i64, enabled: bool) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE google_accounts SET push_new_contacts = ?2 WHERE id = ?1",
            params![account_id, enabled as i64],
        )?;
        Ok(())
    }

    /// 未送信のローカル変更（`contacts.dirty = 1`）を送信順に返す。
    ///
    /// 対象は 2 種類:
    /// - このアカウントの台帳を持つ連絡先（更新・削除）
    /// - どの台帳にも無いローカル生まれの連絡先（新規作成）。
    ///   ただし `push_new_contacts` が有効なときだけ
    pub fn list_contacts_to_push(&self, account_id: i64) -> rusqlite::Result<Vec<ContactPush>> {
        let push_new = self.push_new_contacts(account_id)?;
        // (contact_id, external_id, etag, deleted) をまず集め、ロックを離してから中身を読む
        // （get_contact が再ロックするため。Mutex は非再入）。
        let rows: Vec<(i64, Option<String>, Option<String>, bool)> = {
            let conn = self.conn.lock().unwrap();
            let mut out = Vec::new();
            let mut stmt = conn.prepare(
                "SELECT c.id, ci.external_id, ci.etag, c.deleted_at IS NOT NULL \
                 FROM contact_identities ci JOIN contacts c ON c.id = ci.contact_id \
                 WHERE ci.provider = 'google' AND ci.account_id = ?1 AND c.dirty = 1 \
                 ORDER BY c.id",
            )?;
            let linked = stmt.query_map(params![account_id], |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)? != 0,
                ))
            })?;
            for row in linked {
                out.push(row?);
            }
            if push_new {
                // ローカル生まれ＝どのアカウントの台帳にも現れない連絡先。削除済みは送らない
                // （Google 側に存在しないので消すものが無い）。
                let mut stmt = conn.prepare(
                    "SELECT c.id FROM contacts c \
                     WHERE c.dirty = 1 AND c.deleted_at IS NULL \
                       AND NOT EXISTS ( \
                           SELECT 1 FROM contact_identities ci WHERE ci.contact_id = c.id) \
                     ORDER BY c.id",
                )?;
                let fresh = stmt.query_map([], |r| r.get::<_, i64>(0))?;
                for id in fresh {
                    out.push((id?, None, None, false));
                }
            }
            out
        };

        rows.into_iter()
            .map(|(contact_id, external_id, etag, deleted)| {
                Ok(ContactPush {
                    contact_id,
                    external_id,
                    etag,
                    deleted,
                    contact: self.get_contact(contact_id)?,
                })
            })
            .collect()
    }

    /// 送信に成功した連絡先を連携済みにする（台帳に resourceName/etag を保存し dirty を落とす）。
    pub fn mark_contact_pushed(
        &self,
        account_id: i64,
        contact_id: i64,
        external_id: &str,
        etag: Option<&str>,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO contact_identities \
                 (provider, account_id, external_id, contact_id, etag, remote_deleted, fetched_at) \
             VALUES ('google', ?1, ?2, ?3, ?4, 0, CURRENT_TIMESTAMP) \
             ON CONFLICT(provider, account_id, external_id) DO UPDATE SET \
                 contact_id = ?3, etag = ?4, remote_deleted = 0",
            params![account_id, external_id, contact_id, etag],
        )?;
        conn.execute(
            "UPDATE contacts SET dirty = 0 WHERE id = ?1",
            params![contact_id],
        )?;
        Ok(())
    }

    /// 送信すべきものが無かった／送信が済んだので未送信の印を落とす。
    pub fn clear_contact_dirty(&self, contact_id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE contacts SET dirty = 0 WHERE id = ?1",
            params![contact_id],
        )?;
        Ok(())
    }

    /// Google 側で削除した連絡先の台帳に印を付ける（行は消さない）。
    pub fn mark_identity_pushed_delete(
        &self,
        account_id: i64,
        external_id: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE contact_identities SET remote_deleted = 1, fetched_at = CURRENT_TIMESTAMP \
             WHERE provider = 'google' AND account_id = ?1 AND external_id = ?2",
            params![account_id, external_id],
        )?;
        Ok(())
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

/// このアカウントで Google が持っているラベル名の集合。
/// 取り込みで「外してよいタグ」を見分けるために使う（ここに無い名前は触らない）。
fn managed_group_names(
    conn: &rusqlite::Connection,
    account_id: i64,
) -> rusqlite::Result<HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT name FROM contact_group_identities WHERE provider = 'google' AND account_id = ?1",
    )?;
    let rows = stmt.query_map(params![account_id], |r| r.get::<_, String>(0))?;
    rows.collect()
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

    /// 台帳に取り込み、照合して紐付けたローカル連絡先の ID を返す（送信テストの下ごしらえ）。
    fn linked_contact(s: &Store, acct: i64, rid: &str, name: &str, email: &str) -> i64 {
        s.apply_remote_contact(acct, &remote_with_email(rid, name, email))
            .unwrap();
        s.apply_contact_matches(acct).unwrap();
        s.contact_identity(acct, rid)
            .unwrap()
            .unwrap()
            .contact_id
            .expect("照合で紐付いているはず")
    }

    #[test]
    fn editing_a_linked_contact_queues_it_for_push() {
        let s = mem_store();
        let acct = account(&s);
        let id = linked_contact(&s, acct, "people/c1", "山田太郎", "t@y.jp");
        // 取り込み直後は送るものが無い（Google から来たままなので）。
        assert!(s.list_contacts_to_push(acct).unwrap().is_empty());

        s.upsert_contact(&crate::models::ContactInput {
            id: Some(id as i32),
            display_name: "山田 太郎".into(),
            ..Default::default()
        })
        .unwrap();

        let push = s.list_contacts_to_push(acct).unwrap();
        assert_eq!(push.len(), 1);
        assert_eq!(push[0].contact_id, id);
        assert_eq!(push[0].external_id.as_deref(), Some("people/c1"));
        assert_eq!(push[0].etag.as_deref(), Some("e1"), "更新には読んだ版の etag が要る");
        assert!(!push[0].deleted);

        // 送信できたら未送信の印が落ち、新しい etag が台帳に載る。
        s.mark_contact_pushed(acct, id, "people/c1", Some("etag-new"))
            .unwrap();
        assert!(s.list_contacts_to_push(acct).unwrap().is_empty());
        assert_eq!(
            s.contact_identity(acct, "people/c1").unwrap().unwrap().etag.as_deref(),
            Some("etag-new")
        );
    }

    #[test]
    fn deleting_a_linked_contact_queues_a_remote_delete() {
        let s = mem_store();
        let acct = account(&s);
        let id = linked_contact(&s, acct, "people/c1", "山田太郎", "t@y.jp");
        s.delete_contact(id).unwrap();

        let push = s.list_contacts_to_push(acct).unwrap();
        assert_eq!(push.len(), 1);
        assert!(push[0].deleted);
        assert_eq!(push[0].external_id.as_deref(), Some("people/c1"));
    }

    #[test]
    fn locally_born_contacts_are_pushed_only_when_enabled() {
        let s = mem_store();
        let acct = account(&s);
        s.upsert_contact(&crate::models::ContactInput {
            display_name: "手元で作った人".into(),
            email: Some("local@x.jp".into()),
            ..Default::default()
        })
        .unwrap();

        // 既定では住所録を Google へ上げない。
        assert!(!s.push_new_contacts(acct).unwrap());
        assert!(s.list_contacts_to_push(acct).unwrap().is_empty());

        s.set_push_new_contacts(acct, true).unwrap();
        let push = s.list_contacts_to_push(acct).unwrap();
        assert_eq!(push.len(), 1);
        assert_eq!(push[0].external_id, None, "Google 側にまだ無いので作成する");
        assert_eq!(push[0].contact.display_name, "手元で作った人");
    }

    #[test]
    fn pulling_refreshes_a_linked_contact() {
        let s = mem_store();
        let acct = account(&s);
        let id = linked_contact(&s, acct, "people/c1", "山田太郎", "t@y.jp");

        // Google 側で組織が入った → 紐付いたローカル連絡先にも反映される。
        let mut updated = remote_with_email("people/c1", "山田太郎", "t@y.jp");
        if let Some(c) = updated.contact.as_mut() {
            c.organization = Some("株式会社ヤマダ".into());
        }
        s.apply_remote_contact(acct, &updated).unwrap();
        assert_eq!(
            s.get_contact(id).unwrap().organization.as_deref(),
            Some("株式会社ヤマダ")
        );
    }

    #[test]
    fn pulling_does_not_clobber_unsent_local_changes() {
        let s = mem_store();
        let acct = account(&s);
        let id = linked_contact(&s, acct, "people/c1", "山田太郎", "t@y.jp");
        // ローカルで編集（未送信）。
        s.upsert_contact(&crate::models::ContactInput {
            id: Some(id as i32),
            display_name: "山田 太郎（編集済み）".into(),
            ..Default::default()
        })
        .unwrap();

        let mut updated = remote_with_email("people/c1", "山田太郎", "t@y.jp");
        if let Some(c) = updated.contact.as_mut() {
            c.display_name = "Google 側の名前".into();
        }
        s.apply_remote_contact(acct, &updated).unwrap();

        // 送れていない変更を取り込みで潰さない（次の送信で Google 側へ出る）。
        assert_eq!(
            s.get_contact(id).unwrap().display_name,
            "山田 太郎（編集済み）"
        );
        // 台帳の内容と etag は更新されている（次の送信で新しい etag を使える）。
        assert_eq!(
            s.contact_identity(acct, "people/c1")
                .unwrap()
                .unwrap()
                .snapshot
                .unwrap()
                .display_name,
            "Google 側の名前"
        );
    }

    #[test]
    fn a_contact_deleted_on_google_goes_to_the_local_trash() {
        let s = mem_store();
        let acct = account(&s);
        let id = linked_contact(&s, acct, "people/c1", "山田太郎", "t@y.jp");

        let del = RemoteContact {
            external_id: "people/c1".into(),
            etag: None,
            deleted: true,
            contact: None,
        };
        s.apply_remote_contact(acct, &del).unwrap();

        // 完全削除はしない（ゴミ箱に落とすだけ。誤削除から戻せるように）。
        let conn = s.conn.lock().unwrap();
        let deleted_at: Option<String> = conn
            .query_row("SELECT deleted_at FROM contacts WHERE id = ?1", params![id], |r| r.get(0))
            .unwrap();
        assert!(deleted_at.is_some());
    }

    #[test]
    fn a_remote_delete_leaves_unsent_local_changes_alone() {
        let s = mem_store();
        let acct = account(&s);
        let id = linked_contact(&s, acct, "people/c1", "山田太郎", "t@y.jp");
        s.upsert_contact(&crate::models::ContactInput {
            id: Some(id as i32),
            display_name: "まだ送っていない編集".into(),
            ..Default::default()
        })
        .unwrap();

        let del = RemoteContact {
            external_id: "people/c1".into(),
            etag: None,
            deleted: true,
            contact: None,
        };
        s.apply_remote_contact(acct, &del).unwrap();

        // ローカルに未送信の変更がある間は消さない（消してよいかは人が決める）。
        let conn = s.conn.lock().unwrap();
        let deleted_at: Option<String> = conn
            .query_row("SELECT deleted_at FROM contacts WHERE id = ?1", params![id], |r| r.get(0))
            .unwrap();
        assert!(deleted_at.is_none());
    }
    /// ラベル付きの Google 連絡先。
    fn remote_with_labels(id: &str, name: &str, labels: &[&str]) -> RemoteContact {
        RemoteContact {
            external_id: id.into(),
            etag: Some("e1".into()),
            deleted: false,
            contact: Some(ImportedContact {
                display_name: name.into(),
                email: Some("t@y.jp".into()),
                labels: labels.iter().map(|l| l.to_string()).collect(),
                source: "google".into(),
                external_id: Some(id.into()),
                ..Default::default()
            }),
        }
    }

    fn tags_of(s: &Store, id: i64) -> Vec<String> {
        s.get_contact(id).unwrap().tags
    }

    #[test]
    fn the_group_ledger_round_trips_and_is_replaced_wholesale() {
        let s = mem_store();
        let acct = account(&s);
        s.replace_contact_groups(acct, &[("g1".into(), "取引先".into())])
            .unwrap();
        assert_eq!(s.contact_group_id(acct, "取引先").unwrap().as_deref(), Some("g1"));

        // 洗い替え: Google 側で消えたラベルの行は残さない（残すと取り込みで誤って外れる）。
        s.replace_contact_groups(acct, &[("g2".into(), "友人".into())])
            .unwrap();
        assert_eq!(s.contact_group_id(acct, "取引先").unwrap(), None);
        assert_eq!(s.contact_group_id(acct, "友人").unwrap().as_deref(), Some("g2"));

        // 送信で作ったラベルは、次の取り込みまで台帳に覚えておく。
        s.remember_contact_group(acct, "g3", "新しいラベル").unwrap();
        assert_eq!(
            s.contact_group_id(acct, "新しいラベル").unwrap().as_deref(),
            Some("g3")
        );
    }

    #[test]
    fn google_labels_arrive_as_tags() {
        let s = mem_store();
        let acct = account(&s);
        s.replace_contact_groups(acct, &[("g1".into(), "取引先".into())])
            .unwrap();
        s.apply_remote_contact(acct, &remote_with_labels("people/c1", "山田太郎", &["取引先"]))
            .unwrap();
        s.apply_contact_matches(acct).unwrap();
        let id = s
            .contact_identity(acct, "people/c1")
            .unwrap()
            .unwrap()
            .contact_id
            .unwrap();
        assert_eq!(tags_of(&s, id), vec!["取引先"]);
    }

    #[test]
    fn a_label_removed_on_google_is_removed_locally() {
        let s = mem_store();
        let acct = account(&s);
        s.replace_contact_groups(acct, &[("g1".into(), "取引先".into())])
            .unwrap();
        s.apply_remote_contact(acct, &remote_with_labels("people/c1", "山田太郎", &["取引先"]))
            .unwrap();
        s.apply_contact_matches(acct).unwrap();
        let id = s
            .contact_identity(acct, "people/c1")
            .unwrap()
            .unwrap()
            .contact_id
            .unwrap();

        // Google 側でラベルを外した → ローカルのタグも外れる。
        s.apply_remote_contact(acct, &remote_with_labels("people/c1", "山田太郎", &[]))
            .unwrap();
        assert!(tags_of(&s, id).is_empty());
    }

    #[test]
    fn tags_that_google_does_not_know_survive_a_pull() {
        let s = mem_store();
        let acct = account(&s);
        s.replace_contact_groups(acct, &[("g1".into(), "取引先".into())])
            .unwrap();
        s.apply_remote_contact(acct, &remote_with_labels("people/c1", "山田太郎", &["取引先"]))
            .unwrap();
        s.apply_contact_matches(acct).unwrap();
        let id = s
            .contact_identity(acct, "people/c1")
            .unwrap()
            .unwrap()
            .contact_id
            .unwrap();
        // アプリ内だけで付けたタグ（Google は知らない）。
        {
            let conn = s.conn.lock().unwrap();
            conn.execute("INSERT INTO tags (name) VALUES ('自分用')", []).unwrap();
            conn.execute(
                "INSERT INTO contact_tags (contact_id, tag_id) \
                 SELECT ?1, id FROM tags WHERE name = '自分用'",
                params![id],
            )
            .unwrap();
        }

        // Google 側から全ラベルが消えても、Google が知らないタグは触らない。
        s.apply_remote_contact(acct, &remote_with_labels("people/c1", "山田太郎", &[]))
            .unwrap();
        assert_eq!(tags_of(&s, id), vec!["自分用"]);
    }

    #[test]
    fn labels_are_left_alone_while_local_changes_are_unsent() {
        let s = mem_store();
        let acct = account(&s);
        s.replace_contact_groups(acct, &[("g1".into(), "取引先".into())])
            .unwrap();
        s.apply_remote_contact(acct, &remote_with_labels("people/c1", "山田太郎", &["取引先"]))
            .unwrap();
        s.apply_contact_matches(acct).unwrap();
        let id = s
            .contact_identity(acct, "people/c1")
            .unwrap()
            .unwrap()
            .contact_id
            .unwrap();
        // ローカルで編集（未送信）。
        s.upsert_contact(&crate::models::ContactInput {
            id: Some(id as i32),
            display_name: "山田 太郎".into(),
            tags: vec!["取引先".into()],
            ..Default::default()
        })
        .unwrap();

        // 送れていない間は取り込みでタグを動かさない（次の送信で Google 側へ出る）。
        s.apply_remote_contact(acct, &remote_with_labels("people/c1", "山田太郎", &[]))
            .unwrap();
        assert_eq!(tags_of(&s, id), vec!["取引先"]);
    }
}
