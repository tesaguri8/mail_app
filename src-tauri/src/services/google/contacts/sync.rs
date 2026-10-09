//! 同期エンジン: ローカルの変更を送り（push）、Google の正本を取り込む（pull）。
//!
//! 順序はカレンダーと同じ **push → pull**。ローカルの変更を先に送ってから取り込むことで、
//! 双方の状態が収束する（競合は後勝ち）。
//!
//! **まだ住所録の誰とも結び付いていない連絡先は `contact_identities`（台帳）に留まる。**
//! 「ローカルの誰と同じ人か」を決めるのは照合（`services::contact_match`。利用者が
//! 「住所録へ反映」を押したとき）の役目で、取り込みでは決めない。

use super::api::{self, ApiError, GPerson};
use super::{incoming, outgoing, STARRED_GROUP};
use crate::models::{ContactFields, GcontactsSyncResult};
use crate::services::store::{ApplyOutcome, ContactPush, GoogleService, RemoteContact, Store};
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::Mutex;

/// 同期のエラー。
#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("{0}")]
    Client(String),
    #[error(transparent)]
    Api(#[from] ApiError),
    #[error("データベースの操作に失敗しました: {0}")]
    Db(#[from] rusqlite::Error),
}

/// Google への送信（作成／更新／削除）をプロセス全体で直列化するロック。
///
/// 同じアカウントの同期が重なると、同一の未送信連絡先を二重に作成してしまう（カレンダーの
/// `push_lock` と同じ理由）。先の送信が未送信の印を落としてから後続が送信対象を読む。
fn push_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 1 回の同期で送る更新・作成の上限。残りは未送信の印のまま次の同期で送る（1 件ごとに
/// 読み直しと更新の 2 回を呼ぶので、Google の毎分の上限に収まるよう抑える）。
const WRITE_LIMIT_PER_SYNC: usize = 200;
/// 1 回の同期で送る削除の上限（`batchDeleteContacts` 4 回分）。
const DELETE_LIMIT_PER_SYNC: usize = 4 * api::BATCH_DELETE_MAX;

/// 1 回の同期で送る件数の割り振り（上限を超えた分は次の同期へ回す）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PushPlan {
    /// この同期で送る削除の件数。
    pub deletes_now: usize,
    /// この同期で送る更新・作成の件数。
    pub writes_now: usize,
    /// 次の同期へ回す件数。
    pub deferred: usize,
}

impl PushPlan {
    /// 未送信の削除・更新作成の件数から、この同期で送る分を決める。
    pub(crate) fn of(deletes: usize, writes: usize) -> Self {
        let deletes_now = deletes.min(DELETE_LIMIT_PER_SYNC);
        let writes_now = writes.min(WRITE_LIMIT_PER_SYNC);
        Self {
            deletes_now,
            writes_now,
            deferred: (deletes - deletes_now) + (writes - writes_now),
        }
    }
}

/// まとめて送るラベル・スターの付け外し（グループごとに、付ける人・外す人）。
#[derive(Debug, Default)]
struct MembershipChanges {
    /// ラベル名 → （付ける resourceName, 外す resourceName）。
    by_tag: std::collections::BTreeMap<String, (Vec<String>, Vec<String>)>,
    /// スター（お気に入り）の（付ける, 外す）。
    starred: (Vec<String>, Vec<String>),
}

impl MembershipChanges {
    /// 1 人分の差（`wanted` が Rondine、`current` が読み直した Google 側）を足す。
    fn add(&mut self, rn: &str, wanted: &ContactFields, current: &ContactFields) {
        if wanted.is_favorite != current.is_favorite {
            let side = if wanted.is_favorite {
                &mut self.starred.0
            } else {
                &mut self.starred.1
            };
            side.push(rn.to_string());
        }
        for name in wanted.tags.iter().filter(|w| !current.tags.contains(w)) {
            self.by_tag
                .entry(name.clone())
                .or_default()
                .0
                .push(rn.to_string());
        }
        for name in current.tags.iter().filter(|c| !wanted.tags.contains(c)) {
            self.by_tag
                .entry(name.clone())
                .or_default()
                .1
                .push(rn.to_string());
        }
    }
}

/// 送信を続けるか（Google の上限で止めたら残りを次の同期へ回す）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Flow {
    Continue,
    Stopped,
}

/// 送信の文脈（1 アカウントぶん）。
struct Pusher<'a> {
    store: &'a Store,
    client: &'a reqwest::Client,
    token: &'a str,
    account_id: i64,
    /// 連絡先グループ ID → 名前（Person のラベル所属をタグ名に戻すため）。
    groups: &'a HashMap<String, String>,
}

impl Pusher<'_> {
    /// 未送信のローカル変更を Google へ送る。
    ///
    /// 削除を先に、[`api::BATCH_DELETE_MAX`] 件ずつまとめて送る（統合で数千件の削除が溜まることが
    /// ある）。続けて更新・作成を 1 件ずつ送る。1 回の同期で送る件数は [`PushPlan`] で決め
    /// （削除 [`DELETE_LIMIT_PER_SYNC`] 件・更新と作成 [`WRITE_LIMIT_PER_SYNC`] 件まで）、残りは
    /// 未送信の印のまま次の同期で送る（`deferred`）。Google の上限（429）は [`api::with_retry`] が待って送り直し、それでも上限なら
    /// そこで止めて残りを次の同期へ回す。
    ///
    /// 1 件の失敗で全体を止めない（ログして次へ）。etag 不一致は送らずに未送信のまま残す
    /// （次の同期で読み直して送る）。
    async fn push_all(&self, result: &mut GcontactsSyncResult) -> Result<(), SyncError> {
        let _guard = push_lock().lock().await;
        let changes = self.store.list_contacts_to_push(self.account_id)?;
        log::info!(
            "push_contacts: account {} 未送信 {} 件",
            self.account_id,
            changes.len()
        );
        let (deletes, writes): (Vec<ContactPush>, Vec<ContactPush>) =
            changes.into_iter().partition(|c| c.deleted);
        // 削除で向こうの ID が無いものは送信対象に出てこない（作成待ちは削除済みを除く）。
        let deletes: Vec<String> = deletes.into_iter().filter_map(|c| c.external_id).collect();
        // 中身が Google と同じ更新は送らずに印を落とす（上限の枠を使わせない）。
        let writes = self.drop_unchanged(writes, result)?;
        let plan = PushPlan::of(deletes.len(), writes.len());
        result.deferred += plan.deferred as i32;
        log::info!(
            "push_contacts: この同期で 削除 {} 件・更新/作成 {} 件を送り、{} 件は次の同期へ",
            plan.deletes_now,
            plan.writes_now,
            plan.deferred
        );
        let writes = &writes[..plan.writes_now];
        if self
            .push_deletes(&deletes[..plan.deletes_now], result)
            .await?
            == Flow::Stopped
        {
            result.deferred += writes.len() as i32;
            return Ok(());
        }
        // 更新は [`api::BATCH_UPDATE_MAX`] 件ずつまとめて送る（1 件ずつだと 1 件あたり 2〜3 回の
        // 呼び出しで約 5 秒かかり、200 件で 17 分かかっていた。vaio の実測 2026-10-09）。
        let (updates, creates): (Vec<&ContactPush>, Vec<&ContactPush>) =
            writes.iter().partition(|c| c.external_id.is_some());
        for (i, chunk) in updates.chunks(api::BATCH_UPDATE_MAX).enumerate() {
            if self.push_update_batch(chunk, result).await? == Flow::Stopped {
                let rest = updates.len() - i * api::BATCH_UPDATE_MAX + creates.len();
                result.deferred += rest as i32;
                log::warn!("push_contacts: Google の上限で止めました。残り {rest} 件は次の同期で");
                return Ok(());
            }
        }
        for (i, ch) in creates.iter().enumerate() {
            match self.push_create(ch).await {
                Ok(()) => result.pushed += 1,
                Err(SyncError::Api(ApiError::RateLimited(_))) => {
                    result.deferred += (creates.len() - i) as i32;
                    log::warn!(
                        "push_contacts: Google の上限で止めました。残り {} 件は次の同期で",
                        creates.len() - i
                    );
                    break;
                }
                Err(e) => log::warn!(
                    "push_contacts: 作成失敗 id={}（スキップ）: {e}",
                    ch.contact_id
                ),
            }
        }
        Ok(())
    }

    /// 更新のうち、送る中身が Google から最後に読んだ内容と同じものを送らずに片付け（未送信の印を
    /// 落とす）、送る必要のあるものだけを返す。作成（向こうの ID が無い）と、snapshot が無いものは送る。
    fn drop_unchanged(
        &self,
        writes: Vec<ContactPush>,
        result: &mut GcontactsSyncResult,
    ) -> Result<Vec<ContactPush>, SyncError> {
        let mut keep = Vec::with_capacity(writes.len());
        for ch in writes {
            let snapshot = match ch.external_id.as_deref() {
                Some(gid) => self
                    .store
                    .contact_identity(self.account_id, gid)?
                    .and_then(|i| i.snapshot)
                    .map(|s| (gid.to_string(), s)),
                None => None,
            };
            match snapshot {
                Some((gid, s)) if outgoing::same_as_google(&ch.contact, &s) => {
                    self.store
                        .mark_contact_identity_clean(self.account_id, &gid)?;
                    result.unchanged += 1;
                }
                _ => keep.push(ch),
            }
        }
        if result.unchanged > 0 {
            log::info!(
                "push_contacts: 中身が Google と同じ {} 件は送らずに片付けました",
                result.unchanged
            );
        }
        Ok(keep)
    }

    /// 削除をまとめて送る（件数は [`PushPlan`] で決めた分）。送れたつながりは片付ける。まとめて消せなかった組は 1 件ずつに戻す
    /// （1 件でも消せないものがあると全体が失敗しうるため）。Google の上限で止めたら
    /// [`Flow::Stopped`]（残りは次の同期）。
    async fn push_deletes(
        &self,
        now: &[String],
        result: &mut GcontactsSyncResult,
    ) -> Result<Flow, SyncError> {
        let mut done = 0;
        for chunk in now.chunks(api::BATCH_DELETE_MAX) {
            match api::with_retry(|| api::batch_delete_contacts(self.client, self.token, chunk))
                .await
            {
                Ok(()) => {
                    for gid in chunk {
                        self.store.forget_contact_identity(self.account_id, gid)?;
                    }
                    result.deleted_out += chunk.len() as i32;
                    done += chunk.len();
                }
                Err(ApiError::RateLimited(_)) => {
                    result.deferred += (now.len() - done) as i32;
                    log::warn!(
                        "push_contacts: Google の上限で削除を止めました。残り {} 件は次の同期で",
                        now.len() - done
                    );
                    return Ok(Flow::Stopped);
                }
                Err(e) => {
                    log::warn!("push_contacts: まとめて削除できなかったので 1 件ずつ送ります: {e}");
                    for (j, gid) in chunk.iter().enumerate() {
                        match self.push_delete(gid).await {
                            Ok(()) => result.deleted_out += 1,
                            Err(SyncError::Api(ApiError::RateLimited(_))) => {
                                result.deferred += (now.len() - done - j) as i32;
                                return Ok(Flow::Stopped);
                            }
                            Err(e) => log::warn!("push_contacts: 削除失敗 {gid}（スキップ）: {e}"),
                        }
                    }
                    done += chunk.len();
                }
            }
        }
        Ok(Flow::Continue)
    }

    /// 1 件削除し、つながりを外す。Google 側にもう無ければ（404）消えているので成功とみなす。
    async fn push_delete(&self, gid: &str) -> Result<(), SyncError> {
        match api::with_retry(|| api::delete_contact(self.client, self.token, gid)).await {
            Ok(()) | Err(ApiError::NotFound) => {}
            Err(e) => return Err(e.into()),
        }
        self.store.forget_contact_identity(self.account_id, gid)?;
        Ok(())
    }

    /// 更新をまとめて送る（最大 [`api::BATCH_UPDATE_MAX`] 件）。送る直前にまとめて読み直し
    /// （`people:batchGet`）、それを土台に Rondine が扱う部分だけを上書きして、まとめて更新する
    /// （`people:batchUpdateContacts`）。etag 不一致の分は未送信のまま次回へ（今の作法のまま）。
    /// ラベル・スターの所属は、送れた分をグループごとにまとめて `members:modify` で送る。
    /// Google の上限（429）で止めたら [`Flow::Stopped`]。
    async fn push_update_batch(
        &self,
        chunk: &[&ContactPush],
        result: &mut GcontactsSyncResult,
    ) -> Result<Flow, SyncError> {
        let names: Vec<String> = chunk.iter().filter_map(|c| c.external_id.clone()).collect();
        let bases = match api::with_retry(|| api::batch_get_people(self.client, self.token, &names))
            .await
        {
            Ok(b) => b,
            Err(ApiError::RateLimited(_)) => return Ok(Flow::Stopped),
            Err(e) => return Err(e.into()),
        };
        let mut bodies = HashMap::new();
        let mut current: HashMap<String, ContactFields> = HashMap::new();
        for ch in chunk {
            let Some(gid) = ch.external_id.as_deref() else {
                continue;
            };
            match bases.get(gid) {
                Some(base) => {
                    bodies.insert(
                        gid.to_string(),
                        outgoing::person_body(&ch.contact, Some(base)),
                    );
                    current.insert(gid.to_string(), person_fields(base, self.groups));
                }
                // Google 側にもう無い（向こうで消された）。次の取り込みの削除通知で片付く。
                None => log::warn!("push_contacts: Google 側に無いので更新しません {gid}"),
            }
        }
        let outcomes =
            match api::with_retry(|| api::batch_update_contacts(self.client, self.token, &bodies))
                .await
            {
                Ok(o) => o,
                Err(ApiError::RateLimited(_)) => return Ok(Flow::Stopped),
                Err(e) => return Err(e.into()),
            };
        let mut members = MembershipChanges::default();
        for ch in chunk {
            let Some(gid) = ch.external_id.as_deref() else {
                continue;
            };
            match outcomes.get(gid) {
                Some(api::BatchUpdateOutcome::Updated(sent)) => {
                    let rn = sent
                        .resource_name
                        .clone()
                        .unwrap_or_else(|| gid.to_string());
                    let snapshot = incoming::fields_from_person(sent, self.groups);
                    self.store.mark_contact_pushed(
                        self.account_id,
                        ch.contact_id,
                        &rn,
                        sent.etag.as_deref(),
                        snapshot.as_ref(),
                    )?;
                    if let Some(cur) = current.get(gid) {
                        members.add(&rn, &ch.contact, cur);
                    }
                    result.pushed += 1;
                }
                Some(api::BatchUpdateOutcome::EtagConflict) => {
                    log::warn!(
                        "push_contacts: etag 不一致 id={}（次回に持ち越し）",
                        ch.contact_id
                    );
                    result.conflicts += 1;
                }
                Some(api::BatchUpdateOutcome::Failed(why)) => log::warn!(
                    "push_contacts: 更新失敗 id={}（スキップ）: {why}",
                    ch.contact_id
                ),
                None => {}
            }
        }
        if let Err(e) = self.push_membership_changes(&members).await {
            log::warn!("push_contacts: ラベルの反映に失敗: {e}");
        }
        Ok(Flow::Continue)
    }

    /// まとめたラベル・スターの付け外しを、グループごとに `members:modify` で送る。
    async fn push_membership_changes(&self, m: &MembershipChanges) -> Result<(), SyncError> {
        for (name, (add, remove)) in &m.by_tag {
            let id = if add.is_empty() {
                self.store.contact_group_id(self.account_id, name)?
            } else {
                self.group_id_or_create(name).await?
            };
            if let Some(id) = id {
                self.modify_members(&id, add, remove).await?;
            }
        }
        let (add, remove) = &m.starred;
        self.modify_members(STARRED_GROUP, add, remove).await
    }

    /// `members:modify` を上限ごとに分けて送る。
    async fn modify_members(
        &self,
        group_id: &str,
        add: &[String],
        remove: &[String],
    ) -> Result<(), SyncError> {
        for a in add.chunks(api::MEMBERS_MODIFY_MAX) {
            api::with_retry(|| {
                api::modify_contact_group_members(self.client, self.token, group_id, a, &[])
            })
            .await?;
        }
        for r in remove.chunks(api::MEMBERS_MODIFY_MAX) {
            api::with_retry(|| {
                api::modify_contact_group_members(self.client, self.token, group_id, &[], r)
            })
            .await?;
        }
        Ok(())
    }

    /// 作成待ち → 新規作成（利用者がこの人の同期先にこのアカウントを選んだときだけここへ来る）。
    async fn push_create(&self, ch: &ContactPush) -> Result<(), SyncError> {
        let body = outgoing::person_body(&ch.contact, None);
        let g = api::with_retry(|| api::create_contact(self.client, self.token, &body)).await?;
        match g.resource_name.clone() {
            Some(rn) => self.finish(ch, &rn, &g, &ContactFields::default()).await,
            // resourceName が返らないことは無いはずだが、返らなければ紐付けようが無いので
            // 作成待ちを取り下げて二重作成を防ぐ。
            None => Ok(self
                .store
                .drop_contact_create_request(self.account_id, ch.contact_id)?),
        }
    }

    /// 送信できたつながりを記録し、ラベルとスターを合わせる。ラベルの失敗は本体の送信を
    /// 取り消さない（ログするに留める）。
    async fn finish(
        &self,
        ch: &ContactPush,
        rn: &str,
        sent: &GPerson,
        current: &ContactFields,
    ) -> Result<(), SyncError> {
        let snapshot = incoming::fields_from_person(sent, self.groups);
        self.store.mark_contact_pushed(
            self.account_id,
            ch.contact_id,
            rn,
            sent.etag.as_deref(),
            snapshot.as_ref(),
        )?;
        if let Err(e) = self.push_memberships(rn, &ch.contact, current).await {
            log::warn!(
                "push_contacts: ラベルの反映に失敗 id={}: {e}",
                ch.contact_id
            );
        }
        Ok(())
    }

    /// ラベル（タグ）とスター（お気に入り）の所属を Google 側へ合わせる。
    ///
    /// 所属は `people:updateContact` では変えられないので、グループごとに
    /// `contactGroups/*/members:modify` を呼ぶ。Rondine 側にしか無いラベルは Google に作る。
    /// `current` は送る直前に読み直した Google 側の状態。
    async fn push_memberships(
        &self,
        rn: &str,
        wanted: &ContactFields,
        current: &ContactFields,
    ) -> Result<(), SyncError> {
        let me = [rn.to_string()];
        if wanted.is_favorite != current.is_favorite {
            let (add, remove): (&[String], &[String]) = if wanted.is_favorite {
                (&me, &[])
            } else {
                (&[], &me)
            };
            api::modify_contact_group_members(self.client, self.token, STARRED_GROUP, add, remove)
                .await?;
        }
        for name in wanted.tags.iter().filter(|w| !current.tags.contains(w)) {
            let Some(id) = self.group_id_or_create(name).await? else {
                continue;
            };
            api::modify_contact_group_members(self.client, self.token, &id, &me, &[]).await?;
        }
        for name in current.tags.iter().filter(|c| !wanted.tags.contains(c)) {
            // 外す側は Google に既にあるはずなので、無ければ何もしない。
            if let Some(id) = self.store.contact_group_id(self.account_id, name)? {
                api::modify_contact_group_members(self.client, self.token, &id, &[], &me).await?;
            }
        }
        Ok(())
    }

    /// ラベル名 → Google のグループ ID。未知のラベルは Google に作って台帳へ覚える。
    async fn group_id_or_create(&self, name: &str) -> Result<Option<String>, SyncError> {
        if let Some(id) = self.store.contact_group_id(self.account_id, name)? {
            return Ok(Some(id));
        }
        let g = api::create_contact_group(self.client, self.token, name).await?;
        let id = g
            .resource_name
            .as_deref()
            .and_then(|r| r.strip_prefix("contactGroups/"))
            .map(str::to_string);
        match &id {
            Some(id) => self
                .store
                .remember_contact_group(self.account_id, id, name)?,
            None => log::warn!("push_labels: 作成したラベル '{name}' の ID が返りませんでした"),
        }
        Ok(id)
    }
}

/// 読み直した Person（JSON）の中身。読めなければ空（ラベル・スターを付けていない扱い）。
fn person_fields(base: &serde_json::Value, groups: &HashMap<String, String>) -> ContactFields {
    serde_json::from_value::<GPerson>(base.clone())
        .ok()
        .and_then(|p| incoming::fields_from_person(&p, groups))
        .unwrap_or_default()
}

/// 連絡先グループ ID → 名前の対応を作る。システムグループ（myContacts・starred 等）は
/// タグにしても意味が無いので除く（スターはお気に入りとして別に扱う）。
async fn group_names(
    client: &reqwest::Client,
    token: &str,
) -> Result<Vec<(String, String)>, ApiError> {
    let groups = api::list_contact_groups(client, token).await?;
    Ok(groups
        .into_iter()
        .filter(|g| g.group_type.as_deref() != Some("SYSTEM_CONTACT_GROUP"))
        .filter_map(|g| {
            // resourceName は 'contactGroups/{id}'。membership 側は id だけを返す。
            let id = g.resource_name?.strip_prefix("contactGroups/")?.to_string();
            Some((id, g.name?))
        })
        .collect())
}

/// Person 1 件を台帳へ渡す形にする。連絡先として成立しないものは None。
fn remote_from_person(person: &GPerson, groups: &HashMap<String, String>) -> Option<RemoteContact> {
    let external_id = person.resource_name.clone()?;
    if person.metadata.deleted {
        return Some(RemoteContact {
            external_id,
            etag: None,
            deleted: true,
            contact: None,
        });
    }
    let contact = incoming::fields_from_person(person, groups)?;
    Some(RemoteContact {
        external_id,
        etag: person.etag.clone(),
        deleted: false,
        contact: Some(contact),
    })
}

/// 連絡先を取り込む（増分同期トークンがあれば増分。失効したらフル同期へ切り替える）。
async fn pull(
    store: &Store,
    client: &reqwest::Client,
    token: &str,
    account_id: i64,
    groups: &HashMap<String, String>,
    result: &mut GcontactsSyncResult,
) -> Result<(), SyncError> {
    let mut sync_token = store.contacts_sync_token(account_id)?;
    let mut page_token: Option<String> = None;
    loop {
        let page = match api::list_connections(
            client,
            token,
            sync_token.as_deref(),
            page_token.as_deref(),
        )
        .await
        {
            Ok(p) => p,
            Err(ApiError::SyncTokenExpired) => {
                // トークン失効 → フル同期へフォールバック（upsert なので再適用は安全）。
                log::info!("gcontacts: 同期トークンが失効。フル同期に切り替えます");
                store.set_contacts_sync_token(account_id, None)?;
                sync_token = None;
                page_token = None;
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        for person in &page.connections {
            // 名前もメールも電話も無い＝連絡先として成立しない。
            let Some(remote) = remote_from_person(person, groups) else {
                result.skipped += 1;
                continue;
            };
            match store.apply_remote_contact(account_id, &remote)? {
                ApplyOutcome::Upserted => result.pulled += 1,
                ApplyOutcome::Deleted => result.deleted_in += 1,
                ApplyOutcome::Skipped => result.skipped += 1,
            }
        }
        match page.next_page_token {
            Some(next) => page_token = Some(next),
            None => {
                // 最終ページ: 次回の増分同期トークンを保存して終了。
                store.set_contacts_sync_token(account_id, page.next_sync_token.as_deref())?;
                return Ok(());
            }
        }
    }
}

/// 1 アカウントぶんの Google 連絡先を同期する（送信 → 取り込み）。access_token は呼び出し側で
/// 更新済みのものを渡す。
///
/// # Errors
/// HTTP クライアントの初期化・取り込み・DB の操作に失敗したとき。送信の失敗は取り込みを
/// 止めない（ログして続ける）。
pub async fn sync_account(
    store: &Store,
    access_token: &str,
    account_id: i64,
) -> Result<GcontactsSyncResult, SyncError> {
    let client = crate::services::google::http_client().map_err(SyncError::Client)?;
    let mut result = GcontactsSyncResult::default();

    // ラベル解決に失敗しても連絡先の同期は続ける（タグが付かないだけ）。取得できたときは
    // 台帳を洗い替える（送信でラベル ID を引くため／取り込みで「Google の持ち物であるタグ」を
    // 見分けるため）。
    let groups: HashMap<String, String> = match group_names(&client, access_token).await {
        Ok(list) => {
            store.replace_contact_groups(account_id, &list)?;
            list.into_iter().collect()
        }
        Err(e) => {
            log::warn!("gcontacts: ラベル一覧を取得できません（タグ無しで続行）: {e}");
            HashMap::new()
        }
    };

    let pusher = Pusher {
        store,
        client: &client,
        token: access_token,
        account_id,
        groups: &groups,
    };
    if let Err(e) = pusher.push_all(&mut result).await {
        log::warn!("gcontacts: 送信に失敗しました（取り込みは続行）: {e}");
    }
    pull(
        store,
        &client,
        access_token,
        account_id,
        &groups,
        &mut result,
    )
    .await?;

    store.touch_google_account_synced(account_id, GoogleService::Contacts)?;
    result.unlinked = store.count_unlinked_identities(account_id)? as i32;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::store::test_support::person;
    use rusqlite::params;

    /// 溜まった更新のうち、中身が Google と同じものは送らずに印を落とし、違うものだけを送る。
    #[test]
    fn unchanged_updates_are_settled_without_sending() {
        let store = Store::open_in_memory_for_test();
        let account = store
            .upsert_google_account("a@gmail.com", None, None)
            .unwrap();
        let same = store.upsert_contact(&person("山田", &["y@x.jp"])).unwrap();
        let changed = store.upsert_contact(&person("佐藤", &["s@x.jp"])).unwrap();
        {
            let conn = store.conn.lock().unwrap();
            // 「同じ」は今の内容そのまま、「変わった」はメールが違う内容を Google から読んだことにする。
            let mut old = changed.fields.clone();
            old.emails[0].value = "old@x.jp".into();
            for (cid, ext, snap) in [
                (
                    same.id,
                    "people/same",
                    serde_json::to_string(&same.fields).unwrap(),
                ),
                (
                    changed.id,
                    "people/changed",
                    serde_json::to_string(&old).unwrap(),
                ),
            ] {
                conn.execute(
                    "INSERT INTO contact_identities \
                         (provider, account_id, external_id, contact_id, snapshot, dirty) \
                     VALUES ('google', ?1, ?2, ?3, ?4, 1)",
                    params![account, ext, cid, snap],
                )
                .unwrap();
            }
        }
        let client = reqwest::Client::new();
        let groups = HashMap::new();
        let pusher = Pusher {
            store: &store,
            client: &client,
            token: "",
            account_id: account,
            groups: &groups,
        };
        let writes = store.list_contacts_to_push(account).unwrap();
        assert_eq!(writes.len(), 2);
        let mut result = GcontactsSyncResult::default();
        let left = pusher.drop_unchanged(writes, &mut result).unwrap();

        assert_eq!(result.unchanged, 1);
        assert_eq!(
            left.iter()
                .map(|c| c.external_id.as_deref())
                .collect::<Vec<_>>(),
            vec![Some("people/changed")]
        );
        // 片付けた分は次の送信対象に出てこない。
        let next = store.list_contacts_to_push(account).unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(next[0].external_id.as_deref(), Some("people/changed"));
    }

    /// まとめて送るラベル・スターの付け外しは、グループごとに付ける人・外す人へ集まる。
    #[test]
    fn membership_changes_are_grouped_per_label() {
        let f = |tags: &[&str], fav: bool| ContactFields {
            tags: tags.iter().map(|t| t.to_string()).collect(),
            is_favorite: fav,
            ..Default::default()
        };
        let mut m = MembershipChanges::default();
        m.add(
            "people/1",
            &f(&["家族", "取引先"], true),
            &f(&["取引先"], false),
        );
        m.add("people/2", &f(&["家族"], false), &f(&["旧ラベル"], false));
        m.add("people/3", &f(&[], false), &f(&[], true));
        assert_eq!(
            m.by_tag.get("家族"),
            Some(&(vec!["people/1".to_string(), "people/2".to_string()], vec![]))
        );
        assert_eq!(
            m.by_tag.get("旧ラベル"),
            Some(&(vec![], vec!["people/2".to_string()]))
        );
        assert!(
            !m.by_tag.contains_key("取引先"),
            "変わらないラベルは送らない"
        );
        assert_eq!(
            m.starred,
            (vec!["people/1".to_string()], vec!["people/3".to_string()])
        );
    }

    #[test]
    fn push_plan_sends_up_to_the_limits_and_defers_the_rest() {
        // 統合で溜まった数千件: 1 回目は削除 2000・更新 200、残りは次の同期。
        let p = PushPlan::of(2754, 2639);
        assert_eq!(
            (p.deletes_now, p.writes_now, p.deferred),
            (2000, 200, 754 + 2439)
        );
        // 上限より少なければ全部送る。
        assert_eq!(
            PushPlan::of(3, 2),
            PushPlan {
                deletes_now: 3,
                writes_now: 2,
                deferred: 0
            }
        );
        assert_eq!(PushPlan::of(0, 0).deferred, 0);
    }
}
