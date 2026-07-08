//! 同期エンジン: カレンダー一覧の取り込み → 各カレンダーで push（送信）→ pull（取り込み）。
//!
//! 順序は「push してから pull」。ローカルの変更を先に Google へ送り、その後 Google の正本を
//! 取り込むことで、双方の状態を収束させる（競合は概ね後勝ち = 最後に同期した側が残る。v1）。

use super::api::{self, ApiError};
use super::convert;
use crate::models::GcalSyncResult;
use crate::services::store::{ApplyOutcome, Store};
use chrono::{Duration, Local};

/// 1 アカウントぶんの Google カレンダーを同期する。access_token は呼び出し側で更新済みのものを渡す。
pub async fn sync_account(
    store: &Store,
    access_token: &str,
    account_id: i64,
) -> Result<GcalSyncResult, String> {
    let client = super::http_client()?;
    let mut result = GcalSyncResult::default();

    // 1) カレンダー一覧を取り込み、ローカル calendars に upsert。
    let cals = api::list_calendars(&client, access_token)
        .await
        .map_err(|e| e.to_string())?;
    for c in &cals {
        if c.deleted == Some(true) {
            continue;
        }
        let name = c.summary.clone().unwrap_or_default();
        store
            .upsert_google_calendar(
                account_id,
                &c.id,
                &name,
                c.background_color.as_deref(),
                c.access_role.as_deref().unwrap_or("reader"),
                c.primary == Some(true),
            )
            .map_err(|e| e.to_string())?;
    }

    // 2) 同期対象カレンダーごとに push → pull。
    let synced = store
        .list_synced_google_calendars(account_id)
        .map_err(|e| e.to_string())?;
    result.calendars = synced.len() as i32;
    for (local_id, ext_id, sync_token, access_role) in synced {
        // 書き込み可能なカレンダーのみローカル変更を送信する。
        if matches!(access_role.as_str(), "owner" | "writer") {
            push_calendar(store, &client, access_token, local_id, &ext_id, &mut result).await?;
        }
        pull_calendar(
            store,
            &client,
            access_token,
            local_id,
            &ext_id,
            sync_token.as_deref(),
            &mut result,
        )
        .await?;
    }

    store
        .touch_calendar_account_synced(account_id)
        .map_err(|e| e.to_string())?;
    Ok(result)
}

/// 単一カレンダーの未送信ローカル変更（dirty=1）だけを Google へ送る（pull はしない）。
/// 予定の保存/削除時に、その予定の所属カレンダーだけを即送信する用途（低レイテンシ）。
pub async fn push_calendar_only(
    store: &Store,
    access_token: &str,
    calendar_local_id: i64,
    calendar_ext_id: &str,
) -> Result<GcalSyncResult, String> {
    let client = super::http_client()?;
    let mut result = GcalSyncResult::default();
    push_calendar(
        store,
        &client,
        access_token,
        calendar_local_id,
        calendar_ext_id,
        &mut result,
    )
    .await?;
    Ok(result)
}

/// ローカルの未送信変更（dirty=1）を Google へ送る。
async fn push_calendar(
    store: &Store,
    client: &reqwest::Client,
    token: &str,
    local_id: i64,
    ext_id: &str,
    result: &mut GcalSyncResult,
) -> Result<(), String> {
    let changes = store.list_local_changes(local_id).map_err(|e| e.to_string())?;
    log::info!(
        "push_calendar: cal {local_id} (ext {ext_id}) 未送信 {} 件",
        changes.len()
    );
    // 1 件の失敗で全体を止めないよう、各予定はエラーをログして次へ進む（continue）。
    // これにより、壊れた 1 件が他の送信や後続の pull（取り込み）を阻害しない。
    for ch in changes {
        if ch.deleted {
            // 削除: Google 側にも存在すれば削除。未連携なら送るものは無い。
            if let Some(gid) = &ch.external_id {
                log::info!("push_calendar: DELETE id={} gid={gid}", ch.id);
                if let Err(e) = api::delete_event(client, token, ext_id, gid).await {
                    log::warn!("push_calendar: DELETE 失敗 id={}（スキップ）: {e}", ch.id);
                    continue;
                }
                result.deleted_out += 1;
            }
            let _ = store.clear_event_dirty(ch.id);
        } else if let Some(gid) = ch.external_id.clone() {
            // 既存の更新（PATCH）。
            log::info!("push_calendar: PATCH id={} gid={gid} title={}", ch.id, ch.title);
            let body = convert::gevent_write_from_local(&ch);
            match api::patch_event(client, token, ext_id, &gid, &body).await {
                Ok(g) => {
                    let _ = store.mark_event_pushed(
                        ch.id,
                        g.id.as_deref().unwrap_or(&gid),
                        g.etag.as_deref(),
                    );
                    result.pushed += 1;
                }
                Err(e) => {
                    log::warn!("push_calendar: PATCH 失敗 id={} gid={gid}（スキップ）: {e}", ch.id);
                    continue;
                }
            }
        } else {
            // 新規作成（INSERT）。
            log::info!("push_calendar: INSERT id={} title={}", ch.id, ch.title);
            let body = convert::gevent_write_from_local(&ch);
            match api::insert_event(client, token, ext_id, &body).await {
                Ok(g) => {
                    match g.id.clone() {
                        Some(new_id) => {
                            let _ = store.mark_event_pushed(ch.id, &new_id, g.etag.as_deref());
                        }
                        None => {
                            let _ = store.clear_event_dirty(ch.id);
                        }
                    }
                    result.pushed += 1;
                }
                Err(e) => {
                    log::warn!("push_calendar: INSERT 失敗 id={}（スキップ）: {e}", ch.id);
                    continue;
                }
            }
        }
    }
    Ok(())
}

/// Google 側の予定を取り込む。sync_token があれば増分、なければフル（過去 1 年〜）。
async fn pull_calendar(
    store: &Store,
    client: &reqwest::Client,
    token: &str,
    local_id: i64,
    ext_id: &str,
    sync_token: Option<&str>,
    result: &mut GcalSyncResult,
) -> Result<(), String> {
    let full_time_min = || (Local::now() - Duration::days(365)).to_rfc3339();
    let mut use_sync: Option<String> = sync_token.map(str::to_string);
    let mut time_min: Option<String> = if use_sync.is_none() {
        Some(full_time_min())
    } else {
        None
    };
    let mut page_token: Option<String> = None;

    loop {
        let page = match api::list_events(
            client,
            token,
            ext_id,
            use_sync.as_deref(),
            time_min.as_deref(),
            page_token.as_deref(),
        )
        .await
        {
            Ok(p) => p,
            Err(ApiError::SyncTokenExpired) => {
                // トークン失効 → フル同期へフォールバック。
                store
                    .set_calendar_sync_token(local_id, None)
                    .map_err(|e| e.to_string())?;
                use_sync = None;
                page_token = None;
                time_min = Some(full_time_min());
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };

        for gev in &page.items {
            if let Some(re) = convert::remote_from_gevent(gev) {
                match store
                    .apply_remote_event(local_id, &re)
                    .map_err(|e| e.to_string())?
                {
                    ApplyOutcome::Upserted => result.pulled += 1,
                    ApplyOutcome::Deleted => result.deleted_in += 1,
                    ApplyOutcome::Skipped => {}
                }
            }
        }

        if let Some(next) = page.next_page_token {
            page_token = Some(next);
            continue;
        }
        // 最終ページ: 次回の増分同期トークンを保存して終了。
        store
            .set_calendar_sync_token(local_id, page.next_sync_token.as_deref())
            .map_err(|e| e.to_string())?;
        break;
    }
    Ok(())
}
