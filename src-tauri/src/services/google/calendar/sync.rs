//! 同期エンジン: カレンダー一覧の取り込み → 各カレンダーで push（送信）→ pull（取り込み）。
//!
//! 順序は「push してから pull」。ローカルの変更を先に Google へ送り、その後 Google の正本を
//! 取り込むことで、双方の状態を収束させる（競合は概ね後勝ち = 最後に同期した側が残る。v1）。

use super::api::{self, ApiError};
use super::convert;
use crate::models::{GcalFullReason, GcalFullResync, GcalSyncResult};
use crate::services::store::{
    FullPullReconciled, GoogleService, RemoteApplied, Store, SyncedCalendar,
};
use chrono::{DateTime, Duration, Local};
use std::collections::HashSet;
use std::sync::OnceLock;
use tokio::sync::Mutex;

/// Google への push（INSERT/PATCH/DELETE）をプロセス全体で直列化するロック。
///
/// 保存直後の即時 push（`gcal_try_autopush` → `push_calendar_only`）と、定期自動同期
/// （`sync_account`）の push が同時に走ると、同一の未送信予定（dirty=1）を Google へ二重に
/// INSERT してしまい、pull で取り込んだ結果 Google 上・ローカル双方に予定が複製される。
/// push をここで直列化すると、先の push が `mark_event_pushed` で dirty を落としてから後続が
/// `list_local_changes` を読むため、二重送信が起きない。pull はロック対象外なので、取り込み中
/// でも即時 push は待たされない（push 同士だけが直列化される）。
fn push_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// 1 アカウントぶんの Google カレンダーを同期する。access_token は呼び出し側で更新済みのものを渡す。
pub async fn sync_account(
    store: &Store,
    access_token: &str,
    account_id: i64,
) -> Result<GcalSyncResult, String> {
    let client = crate::services::google::http_client()?;
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
    //    1 カレンダーの失敗で残り全カレンダーの同期を止めないよう、各カレンダーの
    //    push/pull はエラーをログして次へ進む（push_calendar 内の per-event と同じ方針）。
    let synced = store
        .list_synced_google_calendars(account_id)
        .map_err(|e| e.to_string())?;
    result.calendars = synced.len() as i32;
    for cal in synced {
        let (local_id, ext_id) = (cal.local_id, cal.external_id.as_str());
        // 書き込み可能なカレンダーのみローカル変更を送信する。
        if matches!(cal.access_role.as_str(), "owner" | "writer") {
            if let Err(e) =
                push_calendar(store, &client, access_token, local_id, ext_id, &mut result).await
            {
                log::warn!("sync_account: push 失敗 cal {local_id} (ext {ext_id})（スキップ）: {e}");
            }
        }
        if let Err(e) = pull_calendar(store, &client, access_token, &cal, &mut result).await {
            log::warn!("sync_account: pull 失敗 cal {local_id} (ext {ext_id})（スキップ）: {e}");
        }
    }

    store
        .touch_google_account_synced(account_id, GoogleService::Calendar)
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
    let client = crate::services::google::http_client()?;
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
    // 即時 push と定期同期 push の二重 INSERT を防ぐため直列化する（push_lock のコメント参照）。
    // ガードは関数終了まで保持し、この間の list_local_changes → 送信 → mark_event_pushed を
    // 他の push と重ならせない。
    let _push_guard = push_lock().lock().await;
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

/// 全件を取り直す期間の始まり（過去 1 年）。
fn full_time_min() -> DateTime<Local> {
    Local::now() - Duration::days(365)
}

/// 返ってこなかった予定を消す対象の始まり（ローカル表現の日付）。取り直しの timeMin から 2 日
/// 後ろへ寄せる（Google の timeMin は「終わりがそれより後」の予定を返す。境目の予定を、時差や
/// 終日の扱いの違いで誤って消さない）。
fn reconcile_cutoff(time_min: DateTime<Local>) -> String {
    (time_min + Duration::days(2))
        .format("%Y-%m-%d")
        .to_string()
}

/// 1 回の取り込みの集計（全件の取り直しの報告に使う）。
#[derive(Default)]
struct PullTally {
    fetched: usize,
    inserted: usize,
    restored: usize,
    updated: usize,
    deleted: usize,
    /// 全件のときだけ: 返ってきた予定の Google の ID（cancelled も含む）。
    seen: HashSet<String>,
}

impl PullTally {
    fn count(&mut self, applied: RemoteApplied) {
        match applied {
            RemoteApplied::Inserted => self.inserted += 1,
            RemoteApplied::Restored => self.restored += 1,
            RemoteApplied::Updated => self.updated += 1,
            RemoteApplied::Deleted => self.deleted += 1,
            RemoteApplied::Unchanged => {}
        }
    }
}

/// 全件の取り直しの報告（同期結果とログ用）。
fn full_report(
    local_id: i64,
    reason: GcalFullReason,
    t: &PullTally,
    rec: FullPullReconciled,
) -> GcalFullResync {
    let n = |v: usize| i32::try_from(v).unwrap_or(i32::MAX);
    GcalFullResync {
        calendar_id: i32::try_from(local_id).unwrap_or(i32::MAX),
        reason,
        fetched: n(t.fetched),
        inserted: n(t.inserted),
        restored: n(t.restored),
        updated: n(t.updated),
        deleted: n(t.deleted),
        removed: n(rec.removed),
        uncancelled: n(rec.uncancelled),
    }
}

/// Google 側の予定を取り込む。
///
/// 次のときは増分ではなく全件（過去 1 年〜）で取り直し、手元を合わせる（docs/CALENDAR_SYNC.md）:
/// - 同期トークンが無い（初めて / 「最初から同期し直す」）
/// - `full_due`（最後に全件を取り直してから 24 時間を超えた）
/// - 同期トークンが失効した（410）
///
/// 全件のときは、返ってきた予定を手元で削除扱いでも戻し、期間の中で返ってこなかった予定を削除扱いに
/// する（未送信のローカル変更は残す）。増分が予定を取りこぼしたまま「取得 0 件」を返し続けても、
/// 1 日以内に直る（`[実測]` vaio 2026-11-10）。
async fn pull_calendar(
    store: &Store,
    client: &reqwest::Client,
    token: &str,
    cal: &SyncedCalendar,
    result: &mut GcalSyncResult,
) -> Result<(), String> {
    let (local_id, ext_id) = (cal.local_id, cal.external_id.as_str());
    let sync_token = cal.sync_token.as_deref();
    // 全件で取り直す理由（None なら増分）。
    let mut full: Option<GcalFullReason> = match (sync_token, cal.full_due) {
        (None, _) => Some(GcalFullReason::Fresh),
        (Some(_), true) => Some(GcalFullReason::Daily),
        (Some(_), false) => None,
    };
    let mut use_sync: Option<String> = if full.is_none() {
        sync_token.map(str::to_string)
    } else {
        None
    };
    let mut time_min_at = full_time_min();
    let mut page_token: Option<String> = None;
    let mut tally = PullTally::default();

    loop {
        let time_min = full.map(|_| time_min_at.to_rfc3339());
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
                // トークン失効 → 全件へ。Google の祝日カレンダーなど、発行したトークンを毎回 410 で
                // 返すカレンダーがある（`[実測]` 2026-10-09）。そこでは毎回全件になるが、同じ版の予定は
                // store 側で落とす（取り込みに数えない）。全件は 1 回として数える（ここでやり直す）。
                log::debug!(
                    "pull_calendar: cal {local_id} の同期トークンが失効（410）→ 全件で取り直す"
                );
                store
                    .set_calendar_sync_token(local_id, None)
                    .map_err(|e| e.to_string())?;
                use_sync = None;
                page_token = None;
                full = Some(GcalFullReason::Expired);
                time_min_at = full_time_min();
                tally = PullTally::default();
                continue;
            }
            Err(e) => return Err(e.to_string()),
        };

        tally.fetched += page.items.len();
        for gev in &page.items {
            if full.is_some() {
                if let Some(id) = &gev.id {
                    tally.seen.insert(id.clone());
                }
            }
            if let Some(re) = convert::remote_from_gevent(gev) {
                let applied = store
                    .apply_remote_event_detail(local_id, &re)
                    .map_err(|e| e.to_string())?;
                tally.count(applied);
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
        let next_token = if page.next_sync_token.is_some() {
            "あり"
        } else {
            "なし"
        };
        match full {
            Some(reason) => {
                let rec = store
                    .reconcile_full_pull(local_id, &tally.seen, &reconcile_cutoff(time_min_at))
                    .map_err(|e| e.to_string())?;
                store
                    .mark_calendar_full_synced(local_id)
                    .map_err(|e| e.to_string())?;
                let report = full_report(local_id, reason, &tally, rec);
                log_full(&report, next_token);
                result.pulled += report.inserted + report.restored + report.updated;
                result.deleted_in += report.deleted + report.removed;
                result.full.push(report);
            }
            None => {
                result.pulled += i32::try_from(tally.inserted + tally.restored + tally.updated)
                    .unwrap_or(i32::MAX);
                result.deleted_in += i32::try_from(tally.deleted).unwrap_or(i32::MAX);
                // 同期のたびに出るので、取得が 0 件のときは debug に落とす。
                let level = if tally.fetched > 0 {
                    log::Level::Info
                } else {
                    log::Level::Debug
                };
                log::log!(
                    level,
                    "pull_calendar: cal {local_id} (増分) 取得 {} 件 → 新規 {} / 戻した {} / 更新 {} / 削除 {} / 次のトークン {next_token}",
                    tally.fetched,
                    tally.inserted,
                    tally.restored,
                    tally.updated,
                    tally.deleted,
                );
            }
        }
        break;
    }
    Ok(())
}

/// 全件の取り直しをログに出す。手動・初回・1 日 1 回は必ず INFO。トークンが毎回失効する
/// カレンダー（祝日）は同期のたびに来るので、何も変わらなければ debug に落とす。
///
/// 「戻した」「返ってこなかったので消した」が 0 でなければ、増分が取りこぼしていた跡
/// （増分では届かなかった差）なので WARN で残す（後から食い違いを追えるように）。
fn log_full(r: &GcalFullResync, next_token: &str) {
    let changed = r.inserted + r.restored + r.updated + r.deleted + r.removed + r.uncancelled;
    let drift = r.restored + r.removed + r.uncancelled > 0;
    let level = if drift {
        log::Level::Warn
    } else if r.reason == GcalFullReason::Expired && changed == 0 {
        log::Level::Debug
    } else {
        log::Level::Info
    };
    let why = match r.reason {
        GcalFullReason::Fresh => "最初から",
        GcalFullReason::Daily => "1 日 1 回",
        GcalFullReason::Expired => "トークン失効",
    };
    log::log!(
        level,
        "pull_calendar: cal {} 全件を取り直し（{why}）取得 {} 件 → 新規 {} / 戻した {} / 更新 {} / 削除 {} / 返ってこなかったので消した {} / 1 回だけの削除の記録を外した {} / 次のトークン {next_token}{}",
        r.calendar_id,
        r.fetched,
        r.inserted,
        r.restored,
        r.updated,
        r.deleted,
        r.removed,
        r.uncancelled,
        if drift { "（増分と食い違っていた）" } else { "" },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn 消す対象の始まりは取り直しの期間より2日後ろ() {
        let t = Local.with_ymd_and_hms(2025, 11, 12, 23, 30, 0).unwrap();
        assert_eq!(reconcile_cutoff(t), "2025-11-14");
    }

    #[test]
    fn 取り込みの結果を種類ごとに数える() {
        let mut t = PullTally::default();
        for a in [
            RemoteApplied::Inserted,
            RemoteApplied::Restored,
            RemoteApplied::Restored,
            RemoteApplied::Updated,
            RemoteApplied::Deleted,
            RemoteApplied::Unchanged,
        ] {
            t.count(a);
        }
        t.fetched = 6;
        let r = full_report(
            5,
            GcalFullReason::Daily,
            &t,
            FullPullReconciled {
                removed: 1,
                uncancelled: 0,
            },
        );
        assert_eq!(
            (r.fetched, r.inserted, r.restored, r.updated, r.deleted, r.removed),
            (6, 1, 2, 1, 1, 1)
        );
        assert_eq!(r.calendar_id, 5);
    }
}
