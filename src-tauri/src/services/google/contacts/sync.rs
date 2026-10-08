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
        for ch in changes {
            let outcome = match (ch.deleted, ch.external_id.as_deref()) {
                (true, Some(gid)) => self
                    .push_delete(gid)
                    .await
                    .map(|()| result.deleted_out += 1),
                // 削除で向こうの ID が無いものは送信対象に出てこない（作成待ちは削除済みを除く）。
                (true, None) => Ok(()),
                (false, Some(gid)) => self
                    .push_update(&ch, gid)
                    .await
                    .map(|()| result.pushed += 1),
                (false, None) => self.push_create(&ch).await.map(|()| result.pushed += 1),
            };
            match outcome {
                Ok(()) => {}
                Err(SyncError::Api(ApiError::EtagConflict)) => {
                    log::warn!(
                        "push_contacts: etag 不一致 id={}（次回に持ち越し）",
                        ch.contact_id
                    );
                    result.conflicts += 1;
                }
                Err(e) => log::warn!(
                    "push_contacts: 送信失敗 id={}（スキップ）: {e}",
                    ch.contact_id
                ),
            }
        }
        Ok(())
    }

    /// ローカルで削除 → Google 側も削除し、つながりを外す。
    async fn push_delete(&self, gid: &str) -> Result<(), SyncError> {
        api::delete_contact(self.client, self.token, gid).await?;
        self.store.forget_contact_identity(self.account_id, gid)?;
        Ok(())
    }

    /// 連携済み → 送る直前に読み直し、それを土台に Rondine が扱う部分だけを上書きして送る。
    async fn push_update(&self, ch: &ContactPush, gid: &str) -> Result<(), SyncError> {
        let base = api::get_person(self.client, self.token, gid).await?;
        let current = person_fields(&base, self.groups);
        let body = outgoing::person_body(&ch.contact, Some(&base));
        let g = api::update_contact(self.client, self.token, gid, &body).await?;
        let rn = g.resource_name.clone().unwrap_or_else(|| gid.to_string());
        self.finish(ch, &rn, &g, &current).await
    }

    /// 作成待ち → 新規作成（利用者がこの人の同期先にこのアカウントを選んだときだけここへ来る）。
    async fn push_create(&self, ch: &ContactPush) -> Result<(), SyncError> {
        let body = outgoing::person_body(&ch.contact, None);
        let g = api::create_contact(self.client, self.token, &body).await?;
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
