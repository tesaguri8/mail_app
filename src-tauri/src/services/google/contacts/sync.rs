//! 同期エンジン: ローカルの変更を送り（push）、Google の正本を取り込む（pull）。
//!
//! 順序はカレンダーと同じ **push → pull**。ローカルの変更を先に送ってから取り込むことで、
//! 双方の状態が収束する（競合は概ね後勝ち）。
//!
//! **まだ住所録の誰とも結び付いていない連絡先は `contact_identities`（台帳）に留まる。**
//! 初回は Google 側と住所録に同じ人が別 ID で並ぶので、そのまま住所録へ入れると丸ごと二重に
//! なるため。「ローカルの誰と同じ人か」を決めるのは照合（`services::contact_match`。利用者が
//! 「住所録へ反映」を押したとき）の役目で、取り込みでは決めない。

use super::api::{self, ApiError};
use super::convert;
use crate::models::GcontactsSyncResult;
use crate::services::store::{ApplyOutcome, GoogleService, RemoteContact, Store};
use std::collections::HashMap;
use std::sync::OnceLock;
use tokio::sync::Mutex;

/// Google への送信（作成／更新／削除）をプロセス全体で直列化するロック。
///
/// 同じアカウントの同期が重なると、同一の未送信連絡先（dirty=1）を二重に作成してしまう
/// （カレンダーの `push_lock` と同じ理由）。ここで直列化すると、先の送信が
/// `mark_contact_pushed` で dirty を落としてから後続が `list_contacts_to_push` を読む。
fn push_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 未送信のローカル変更（`contacts.dirty = 1`）を Google へ送る。
///
/// 1 件の失敗で全体を止めない（ログして次へ）。壊れた 1 件が他の送信や後続の取り込みを
/// 阻害しないようにするため。etag 不一致は**送らずに未送信のまま残す**: 次の取り込みで
/// 新しい etag を受け取り、その次の送信で通る（結果として後勝ち）。
async fn push_contacts(
    store: &Store,
    client: &reqwest::Client,
    token: &str,
    account_id: i64,
    result: &mut GcontactsSyncResult,
) -> Result<(), String> {
    let _guard = push_lock().lock().await;
    let changes = store
        .list_contacts_to_push(account_id)
        .map_err(|e| e.to_string())?;
    log::info!("push_contacts: account {account_id} 未送信 {} 件", changes.len());

    for ch in changes {
        match (ch.deleted, ch.external_id.as_deref()) {
            // ローカルで削除 → Google 側も削除。未連携なら送るものは無い。
            (true, gid) => {
                if let Some(gid) = gid {
                    if let Err(e) = api::delete_contact(client, token, gid).await {
                        log::warn!(
                            "push_contacts: DELETE 失敗 id={} gid={gid}（スキップ）: {e}",
                            ch.contact_id
                        );
                        continue;
                    }
                    let _ = store.mark_identity_pushed_delete(account_id, gid);
                    result.deleted_out += 1;
                }
                let _ = store.clear_contact_dirty(ch.contact_id);
            }
            // 連携済み → 更新（etag 必須）。
            (false, Some(gid)) => {
                let body = convert::person_write_from_contact(&ch.contact, ch.etag.as_deref());
                match api::update_contact(client, token, gid, &body).await {
                    Ok(g) => {
                        let _ = store.mark_contact_pushed(
                            account_id,
                            ch.contact_id,
                            g.resource_name.as_deref().unwrap_or(gid),
                            g.etag.as_deref(),
                        );
                        result.pushed += 1;
                    }
                    Err(ApiError::EtagConflict) => {
                        // Google 側が先に更新されている。未送信のまま残して次回に持ち越す。
                        log::warn!(
                            "push_contacts: etag 不一致 id={} gid={gid}（次回に持ち越し）",
                            ch.contact_id
                        );
                        result.conflicts += 1;
                    }
                    Err(e) => {
                        log::warn!(
                            "push_contacts: UPDATE 失敗 id={} gid={gid}（スキップ）: {e}",
                            ch.contact_id
                        );
                    }
                }
            }
            // ローカル生まれ → 新規作成（push_new_contacts が有効なときだけここへ来る）。
            (false, None) => match api::create_contact(client, token, &convert::person_write_from_contact(&ch.contact, None)).await {
                Ok(g) => {
                    match g.resource_name.as_deref() {
                        Some(rn) => {
                            let _ = store.mark_contact_pushed(
                                account_id,
                                ch.contact_id,
                                rn,
                                g.etag.as_deref(),
                            );
                        }
                        // resourceName が返らないことは無いはずだが、返らなければ
                        // 紐付けようが無いので未送信の印だけ落として二重作成を防ぐ。
                        None => {
                            let _ = store.clear_contact_dirty(ch.contact_id);
                        }
                    }
                    result.pushed += 1;
                }
                Err(e) => {
                    log::warn!(
                        "push_contacts: CREATE 失敗 id={}（スキップ）: {e}",
                        ch.contact_id
                    );
                }
            },
        }
    }
    Ok(())
}

/// 連絡先グループ ID → 名前の対応を作る。システムグループ（myContacts 等）はタグにしても
/// 意味が無いので除く。
async fn group_names(
    client: &reqwest::Client,
    token: &str,
) -> Result<HashMap<String, String>, ApiError> {
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

/// 1 アカウントぶんの Google 連絡先を取り込む。access_token は呼び出し側で更新済みのものを渡す。
pub async fn sync_account(
    store: &Store,
    access_token: &str,
    account_id: i64,
) -> Result<GcontactsSyncResult, String> {
    let client = crate::services::google::http_client()?;
    let mut result = GcontactsSyncResult::default();

    // ラベル解決に失敗しても連絡先の取り込みは続ける（タグが付かないだけ）。
    let groups = match group_names(&client, access_token).await {
        Ok(g) => g,
        Err(e) => {
            log::warn!("gcontacts: ラベル一覧を取得できません（タグ無しで続行）: {e}");
            HashMap::new()
        }
    };

    // 取り込みの前にローカルの変更を送る（送信に成功した分は dirty が落ち、直後の取り込みで
    // Google の正本に上書きされる＝双方が収束する）。送信の失敗で取り込みまで止めない。
    if let Err(e) = push_contacts(store, &client, access_token, account_id, &mut result).await {
        log::warn!("gcontacts: 送信に失敗しました（取り込みは続行）: {e}");
    }

    let mut sync_token = store.contacts_sync_token(account_id).map_err(|e| e.to_string())?;
    let mut page_token: Option<String> = None;

    loop {
        let page = match api::list_connections(
            &client,
            access_token,
            sync_token.as_deref(),
            page_token.as_deref(),
        )
        .await
        {
            Ok(p) => p,
            Err(ApiError::SyncTokenExpired) => {
                // トークン失効 → フル同期へフォールバック（upsert なので再適用は安全）。
                log::info!("gcontacts: 同期トークンが失効。フル同期に切り替えます");
                store
                    .set_contacts_sync_token(account_id, None)
                    .map_err(|e| e.to_string())?;
                sync_token = None;
                page_token = None;
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };

        for person in &page.connections {
            let Some(external_id) = person.resource_name.clone() else {
                result.skipped += 1;
                continue;
            };
            let remote = if person.metadata.deleted {
                RemoteContact {
                    external_id,
                    etag: None,
                    deleted: true,
                    contact: None,
                }
            } else {
                match convert::imported_from_person(person, &groups) {
                    Some(contact) => RemoteContact {
                        external_id,
                        etag: person.etag.clone(),
                        deleted: false,
                        contact: Some(contact),
                    },
                    // 名前もメールも電話も無い＝連絡先として成立しない。
                    None => {
                        result.skipped += 1;
                        continue;
                    }
                }
            };
            match store
                .apply_remote_contact(account_id, &remote)
                .map_err(|e| e.to_string())?
            {
                ApplyOutcome::Upserted => result.pulled += 1,
                ApplyOutcome::Deleted => result.deleted_in += 1,
                ApplyOutcome::Skipped => result.skipped += 1,
            }
        }

        if let Some(next) = page.next_page_token {
            page_token = Some(next);
            continue;
        }
        // 最終ページ: 次回の増分同期トークンを保存して終了。
        store
            .set_contacts_sync_token(account_id, page.next_sync_token.as_deref())
            .map_err(|e| e.to_string())?;
        break;
    }

    store
        .touch_google_account_synced(account_id, GoogleService::Contacts)
        .map_err(|e| e.to_string())?;
    result.unlinked = store
        .count_unlinked_identities(account_id)
        .map_err(|e| e.to_string())? as i32;
    Ok(result)
}
