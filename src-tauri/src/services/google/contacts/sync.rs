//! 同期エンジン: 連絡先グループ（ラベル）を引いてから、連絡先を取り込む（pull）。
//!
//! **取り込みは台帳止まり。** 取り込んだ内容は `contact_identities` に溜まるだけで、住所録
//! （`contacts`）には現れない。既存の住所録と全件重複させないためで、「ローカルの誰と同じ人か」
//! を決めるのは照合（`services::contact_match`。利用者が「住所録へ反映」を押したとき）の役目。
//! ローカル変更の送信（push）は後続。

use super::api::{self, ApiError};
use super::convert;
use crate::models::GcontactsSyncResult;
use crate::services::store::{ApplyOutcome, GoogleService, RemoteContact, Store};
use std::collections::HashMap;

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
