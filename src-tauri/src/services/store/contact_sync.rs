//! 連絡先の同期とつながり表（`contact_identities`）の操作。docs/CONTACT_MODEL.md §3。
//!
//! 1 人の連絡先は複数のサービスに同時につながってよく、つながりごとに「未送信」の印（`dirty`）を
//! 持つ。利用者の変更はつながっている全サービスへ送り、向こうでの削除はそのつながりだけを外す。
//!
//! 取り込み（pull）は台帳に向こうの内容（snapshot）を写し、紐付いた連絡先があれば向こうが扱う
//! 項目だけを更新する。まだ誰とも結び付いていない分は照合（`services::contact_match` の判定）で
//! 既存へ寄せるか新規に起こす。

use super::contact_groups::managed_group_names;
use super::contact_rows::load_contact;
use super::contact_tags::{add_tags, reconcile_managed_tags};
use super::contact_write::{write_contact, OrgLinking, WriteOptions};
use super::{ApplyOutcome, Store};
use crate::models::{ContactFields, GcontactsMatchResult};
use crate::services::contact_fields::overlay_google;
use crate::services::contact_match::{self, MatchDecision, MatchOutcome};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashSet;

/// 同期エンジン（services/google/contacts）が Store へ渡す「Google 側の連絡先」1 件。
#[derive(Debug, Clone)]
pub struct RemoteContact {
    /// People API の resourceName（'people/c1234567890'）。
    pub external_id: String,
    pub etag: Option<String>,
    /// Google 側で削除された（増分同期の metadata.deleted）。
    pub deleted: bool,
    /// 取り込んだ内容。削除通知のときは None。
    pub contact: Option<ContactFields>,
}

/// 送るべきローカル変更 1 件（このアカウントへのつながりが未送信・向こうも消す外し方、または
/// 作成待ち）。
#[derive(Debug, Clone)]
pub struct ContactPush {
    pub contact_id: i64,
    /// 連携済みなら People API の resourceName。None＝Google 側にまだ無い（作成する）。
    pub external_id: Option<String>,
    /// Google 側を削除する（ローカルで論理削除した、または同期をやめて向こうも消す）。
    pub deleted: bool,
    /// 送る中身（削除のときは使わない）。
    pub contact: ContactFields,
}

/// 台帳 1 件。
#[derive(Debug, Clone)]
pub struct ContactIdentity {
    /// 紐付いたローカル連絡先。None＝未照合。
    pub contact_id: Option<i64>,
    pub etag: Option<String>,
    /// このつながりへ未送信の変更がある。
    pub dirty: bool,
    /// 取り込んだ内容（保存された JSON を読み戻せなければ None）。
    pub snapshot: Option<ContactFields>,
}

/// 照合の計画: （材料にした台帳, 1 件ずつの判定）。両者は同じ順・同じ数で並ぶ。
type ContactMatchPlan = (Vec<(String, ContactFields)>, Vec<MatchOutcome>);

/// 取り込み・同期での書き込み（利用者の変更ではないので未送信の印は立てない）。
const PULL: WriteOptions = WriteOptions {
    mark_dirty: false,
    org_linking: OrgLinking::ExistingOnly,
};

/// 連絡先に未送信の変更があるか（取り込みで上書きしない条件。通常の取り込みと同じ）。
fn contact_dirty(conn: &Connection, contact_id: i64) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT dirty FROM contacts WHERE id = ?1",
        params![contact_id],
        |r| r.get::<_, i64>(0),
    )
    .map(|d| d != 0)
}

/// 連絡先の「未送信」の印を、つながりの印から決め直す（未送信のつながりが残っていれば 1）。
pub(super) fn refresh_contact_dirty(conn: &Connection, contact_id: i64) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE contacts SET dirty = EXISTS (SELECT 1 FROM contact_identities \
         WHERE contact_id = ?1 AND dirty = 1) WHERE id = ?1",
        params![contact_id],
    )?;
    Ok(())
}

/// 台帳の行を指す条件（Google・アカウント・resourceName）。
const GOOGLE_ROW: &str = "provider = 'google' AND account_id = ?1 AND external_id = ?2";

impl Store {
    /// Google 側の連絡先 1 件を、台帳と（紐付いていれば）住所録へ反映する。
    ///
    /// - 削除通知: そのつながりだけを外す（台帳の行を消す）。連絡先と他のサービスのつながりは残す
    /// - 更新: 台帳の内容と etag を差し替え、紐付いた連絡先は**未送信の変更が無いときだけ**
    ///   Google が扱う項目を更新する（送れていない変更を取り込みで潰さない）
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき（その 1 件ぶんを巻き戻す）。
    pub fn apply_remote_contact(
        &self,
        account_id: i64,
        remote: &RemoteContact,
    ) -> rusqlite::Result<ApplyOutcome> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let outcome = match (remote.deleted, remote.contact.as_ref()) {
            (true, _) => {
                let linked = identity_contact(&tx, account_id, &remote.external_id)?;
                let n = tx.execute(
                    &format!("DELETE FROM contact_identities WHERE {GOOGLE_ROW}"),
                    params![account_id, remote.external_id],
                )?;
                if let Some(cid) = linked {
                    refresh_contact_dirty(&tx, cid)?;
                }
                if n > 0 {
                    ApplyOutcome::Deleted
                } else {
                    // 取り込んだ覚えのない ID の削除通知は無視する。
                    ApplyOutcome::Skipped
                }
            }
            (false, Some(contact)) => {
                apply_remote_update(&tx, account_id, remote, contact)?;
                ApplyOutcome::Upserted
            }
            (false, None) => ApplyOutcome::Skipped,
        };
        tx.commit()?;
        Ok(outcome)
    }

    /// 次回の増分同期トークンを保存する（None でフル同期に戻す）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
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
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
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

    /// 取り込み済みのうち、まだ住所録の誰とも結び付いていない件数（照合の対象数）。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn count_unlinked_identities(&self, account_id: i64) -> rusqlite::Result<i64> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT count(*) FROM contact_identities \
             WHERE provider = 'google' AND account_id = ?1 AND contact_id IS NULL",
            params![account_id],
            |r| r.get(0),
        )
    }

    /// 照合の計画を立てる（読み取りのみ）。下見と適用が同じ道を通るよう 1 か所に集める。
    fn build_contact_match_plan(&self, account_id: i64) -> rusqlite::Result<ContactMatchPlan> {
        let (remote, already_linked) = {
            let conn = self.conn.lock().unwrap();
            (
                unlinked_identities(&conn, account_id)?,
                linked_contact_ids(&conn, account_id)?,
            )
        };
        // 未照合が無ければ住所録を読まない（自動同期は毎回これを通るので、全員を中身まで読む
        // 重い処理を空回りさせない）。
        if remote.is_empty() {
            return Ok((remote, Vec::new()));
        }
        let locals = self.contacts_for_dedupe()?;
        let plan = contact_match::plan(&remote, &locals, &already_linked);
        Ok((remote, plan))
    }

    /// 照合を適用する。高確信は既存へ紐付け、それ以外は新規として住所録に起こして紐付ける。
    ///
    /// 既存へ紐付けるときは、通常の取り込みと同じく Google が扱う項目を Google の値にし、
    /// Rondine にしか無い項目は残す（手元に未送信の変更があれば触らない）。新規に起こした分も
    /// 含め、どちらも送信待ちにしない — 同期のたびに自動で照合するので、確認なしに Google を
    /// 書き換える経路を作らない（利用者の判断 2026-10-09）。似た相手が居たものは、既存の
    /// 重複整理が同じ物差しで拾う（ここで人に代わって統合はしない）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき（全体を巻き戻す）。
    pub fn apply_contact_matches(&self, account_id: i64) -> rusqlite::Result<GcontactsMatchResult> {
        let (remote, plan) = self.build_contact_match_plan(account_id)?;
        let report = summarize(&plan);
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        for (outcome, (external_id, contact)) in plan.iter().zip(remote.iter()) {
            match outcome.decision {
                MatchDecision::Link(id) => {
                    link_identity(&tx, account_id, external_id, id)?;
                    // 通常の取り込みと同じ規則で Google の内容を取り込むだけにし、送信待ちにしない
                    // （同期のたびに自動で照合するので、確認なしに Google を書き換えない）。
                    // Rondine にしか無い項目は、その人を利用者が編集したときに初めて送られる。
                    if !contact_dirty(&tx, id)? {
                        let existing = load_contact(&tx, id)?.fields;
                        write_contact(&tx, Some(id), &overlay_google(&existing, contact), PULL)?;
                        let managed = managed_group_names(&tx, account_id)?;
                        reconcile_managed_tags(&tx, id, &contact.tags, &managed)?;
                    }
                }
                MatchDecision::Create => {
                    let id = write_contact(&tx, None, contact, PULL)?;
                    add_tags(&tx, id, &contact.tags)?;
                    link_identity(&tx, account_id, external_id, id)?;
                }
            }
        }
        tx.commit()?;
        Ok(report)
    }

    /// 上の設定を変える。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn set_push_new_contacts(&self, account_id: i64, enabled: bool) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE google_accounts SET push_new_contacts = ?2 WHERE id = ?1",
            params![account_id, enabled as i64],
        )?;
        Ok(())
    }

    /// このアカウントへ送るべきローカル変更を送信順に返す。
    ///
    /// - このアカウントへのつながりが未送信の連絡先（更新・削除）
    /// - 同期をやめて向こうも消すつながり（削除）
    /// - このアカウントへの作成待ち（新規作成）。どこにもつながっていない連絡先を勝手に作る
    ///   ことはしない（同期先は 1 人ずつ選ぶ。docs/CONTACT_MODEL.md §3）
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn list_contacts_to_push(&self, account_id: i64) -> rusqlite::Result<Vec<ContactPush>> {
        let conn = self.conn.lock().unwrap();
        let mut targets: Vec<(i64, Option<String>, bool)> = {
            let mut stmt = conn.prepare(
                "SELECT c.id, ci.external_id, \
                        c.deleted_at IS NOT NULL OR ci.unlink_requested = 1 \
                 FROM contact_identities ci JOIN contacts c ON c.id = ci.contact_id \
                 WHERE ci.provider = 'google' AND ci.account_id = ?1 \
                   AND (ci.dirty = 1 OR ci.unlink_requested = 1) \
                 ORDER BY c.id",
            )?;
            let rows = stmt.query_map(params![account_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0))
            })?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        {
            // 作成待ち。削除済み（ゴミ箱）の人は作らない（向こうに無いので消すものも無い）。
            let mut stmt = conn.prepare(
                "SELECT r.contact_id FROM contact_create_requests r \
                 JOIN contacts c ON c.id = r.contact_id \
                 WHERE r.provider = 'google' AND r.account_id = ?1 AND c.deleted_at IS NULL \
                 ORDER BY r.contact_id",
            )?;
            let rows = stmt.query_map(params![account_id], |r| r.get::<_, i64>(0))?;
            for id in rows {
                targets.push((id?, None, false));
            }
        }
        targets
            .into_iter()
            .map(|(contact_id, external_id, deleted)| {
                Ok(ContactPush {
                    contact_id,
                    external_id,
                    deleted,
                    contact: load_contact(&conn, contact_id)?.fields,
                })
            })
            .collect()
    }

    /// 送信に成功したつながりを記録する（台帳に resourceName・etag・送った後の内容を保存し、
    /// このつながりの未送信の印を落とす）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn mark_contact_pushed(
        &self,
        account_id: i64,
        contact_id: i64,
        external_id: &str,
        etag: Option<&str>,
        snapshot: Option<&ContactFields>,
    ) -> rusqlite::Result<()> {
        let snapshot = snapshot.and_then(|s| serde_json::to_string(s).ok());
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO contact_identities \
                 (provider, account_id, external_id, contact_id, etag, snapshot, dirty, fetched_at) \
             VALUES ('google', ?1, ?2, ?3, ?4, ?5, 0, CURRENT_TIMESTAMP) \
             ON CONFLICT(provider, account_id, external_id) DO UPDATE SET \
                 contact_id = ?3, etag = ?4, snapshot = coalesce(?5, snapshot), dirty = 0",
            params![account_id, external_id, contact_id, etag, snapshot],
        )?;
        // 作成待ちだったなら、作れたので消す。
        conn.execute(
            "DELETE FROM contact_create_requests \
             WHERE contact_id = ?1 AND provider = 'google' AND account_id = ?2",
            params![contact_id, account_id],
        )?;
        refresh_contact_dirty(&conn, contact_id)
    }

    /// 作成待ちを、作れないまま取り下げる（作成で resourceName が返らなかったとき。残すと
    /// 同期のたびに作成を繰り返して二重になる）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn drop_contact_create_request(
        &self,
        account_id: i64,
        contact_id: i64,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM contact_create_requests \
             WHERE contact_id = ?1 AND provider = 'google' AND account_id = ?2",
            params![contact_id, account_id],
        )?;
        Ok(())
    }

    /// Google 側で削除し終えたつながりを外す（台帳の行を消す）。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn forget_contact_identity(
        &self,
        account_id: i64,
        external_id: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        let linked = identity_contact(&conn, account_id, external_id)?;
        conn.execute(
            &format!("DELETE FROM contact_identities WHERE {GOOGLE_ROW}"),
            params![account_id, external_id],
        )?;
        match linked {
            Some(cid) => refresh_contact_dirty(&conn, cid),
            None => Ok(()),
        }
    }

    /// このつながりへの未送信の印だけを落とす（送る中身が Google と同じだったとき）。台帳の
    /// etag・snapshot はそのまま。
    ///
    /// # Errors
    /// DB の書き込みに失敗したとき。
    pub fn mark_contact_identity_clean(
        &self,
        account_id: i64,
        external_id: &str,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        let linked = identity_contact(&conn, account_id, external_id)?;
        conn.execute(
            &format!("UPDATE contact_identities SET dirty = 0 WHERE {GOOGLE_ROW}"),
            params![account_id, external_id],
        )?;
        match linked {
            Some(cid) => refresh_contact_dirty(&conn, cid),
            None => Ok(()),
        }
    }

    /// 台帳 1 件を読み出す。
    ///
    /// # Errors
    /// DB の読み出しに失敗したとき。
    pub fn contact_identity(
        &self,
        account_id: i64,
        external_id: &str,
    ) -> rusqlite::Result<Option<ContactIdentity>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            &format!(
                "SELECT contact_id, etag, dirty, snapshot FROM contact_identities WHERE {GOOGLE_ROW}"
            ),
            params![account_id, external_id],
            |r| {
                let snapshot: Option<String> = r.get(3)?;
                Ok(ContactIdentity {
                    contact_id: r.get(0)?,
                    etag: r.get(1)?,
                    dirty: r.get::<_, i64>(2)? != 0,
                    snapshot: snapshot.as_deref().and_then(|s| serde_json::from_str(s).ok()),
                })
            },
        )
        .optional()
    }
}

/// 取り込んだ更新を台帳と（紐付いていれば）住所録へ書く。
fn apply_remote_update(
    conn: &Connection,
    account_id: i64,
    remote: &RemoteContact,
    contact: &ContactFields,
) -> rusqlite::Result<()> {
    // 保存に失敗する JSON は無いはずだが、失敗しても同期全体は止めない。
    let snapshot = serde_json::to_string(contact).ok();
    conn.execute(
        "INSERT INTO contact_identities \
             (provider, account_id, external_id, etag, snapshot, fetched_at) \
         VALUES ('google', ?1, ?2, ?3, ?4, CURRENT_TIMESTAMP) \
         ON CONFLICT(provider, account_id, external_id) DO UPDATE SET \
             etag = ?3, snapshot = ?4, fetched_at = CURRENT_TIMESTAMP",
        params![account_id, remote.external_id, remote.etag, snapshot],
    )?;
    let linked: Option<i64> = conn
        .query_row(
            "SELECT ci.contact_id FROM contact_identities ci JOIN contacts c ON c.id = ci.contact_id \
             WHERE ci.provider = 'google' AND ci.account_id = ?1 AND ci.external_id = ?2 \
               AND c.dirty = 0 AND ci.unlink_requested = 0",
            params![account_id, remote.external_id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(cid) = linked {
        let existing = load_contact(conn, cid)?.fields;
        write_contact(conn, Some(cid), &overlay_google(&existing, contact), PULL)?;
        // ラベル: Google が持っている名前だけを付け外しし、アプリ内だけのタグは触らない。
        let managed = managed_group_names(conn, account_id)?;
        reconcile_managed_tags(conn, cid, &contact.tags, &managed)?;
    }
    Ok(())
}

/// つながりが指す連絡先（未照合なら None）。
fn identity_contact(
    conn: &Connection,
    account_id: i64,
    external_id: &str,
) -> rusqlite::Result<Option<i64>> {
    conn.query_row(
        &format!("SELECT contact_id FROM contact_identities WHERE {GOOGLE_ROW}"),
        params![account_id, external_id],
        |r| r.get(0),
    )
    .optional()
    .map(Option::flatten)
}

/// つながりを連絡先へ結び付ける。
fn link_identity(
    conn: &Connection,
    account_id: i64,
    external_id: &str,
    contact_id: i64,
) -> rusqlite::Result<()> {
    conn.execute(
        &format!("UPDATE contact_identities SET contact_id = ?3 WHERE {GOOGLE_ROW}"),
        params![account_id, external_id, contact_id],
    )?;
    Ok(())
}

/// 未照合の台帳を（外部 ID, 取り込んだ内容）で返す。順序は external_id 昇順で固定し、
/// 同じ台帳からは何度計画しても同じ結果が出るようにする。読み戻せない行は飛ばす。
fn unlinked_identities(
    conn: &Connection,
    account_id: i64,
) -> rusqlite::Result<Vec<(String, ContactFields)>> {
    let mut stmt = conn.prepare(
        "SELECT external_id, snapshot FROM contact_identities \
         WHERE provider = 'google' AND account_id = ?1 AND contact_id IS NULL \
         ORDER BY external_id",
    )?;
    let rows = stmt.query_map(params![account_id], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (external_id, snapshot) = row?;
        if let Some(c) = snapshot
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
        {
            out.push((external_id, c));
        }
    }
    Ok(out)
}

/// このアカウントの台帳がすでに掴んでいるローカル連絡先 ID（2 つの外部 ID が同じ人を掴まない）。
fn linked_contact_ids(conn: &Connection, account_id: i64) -> rusqlite::Result<HashSet<i64>> {
    let mut stmt = conn.prepare(
        "SELECT DISTINCT contact_id FROM contact_identities \
         WHERE provider = 'google' AND account_id = ?1 AND contact_id IS NOT NULL",
    )?;
    let rows = stmt.query_map(params![account_id], |r| r.get::<_, i64>(0))?;
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
mod tests;
