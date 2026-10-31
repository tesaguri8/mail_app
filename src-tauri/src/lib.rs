mod commands;
pub mod models;
pub mod services;

use services::datadir;
use services::deep_link;
use services::store::Store;
use services::tsg_one::{self, TsgOne};
use tauri::{Emitter, Manager};

/// ログインの戻り（deep link の URL）を受ける。TSG One の戻り以外は無視する。
///
/// Windows / Linux は 2 つ目の起動の引数（[`on_second_instance`]）、macOS は deep link のプラグインから届く。
/// 引換券をトークンに替えるのは通信を待つので別タスクで行い、結果は "tsg-one:changed" で画面へ知らせる
fn handle_deep_link(app: &tauri::AppHandle, url: &str) {
    let Some(tsg) = app.try_state::<TsgOne>() else {
        return;
    };
    if !deep_link::is_tsg_callback(url, &tsg.redirect_scheme()) {
        return;
    }
    let app = app.clone();
    let url = url.to_string();
    // 投げっぱなしでよい（結果はイベントで知らせ、失敗の理由は状態に残る）
    tauri::async_runtime::spawn(async move {
        let tsg = app.state::<TsgOne>();
        if let Err(e) = tsg.finish_login(&url).await {
            log::warn!("[TsgOne] ログインの戻りを受け付けませんでした: {e}");
        }
        let _ = app.emit(tsg_one::CHANGED_EVENT, tsg.status());
        bring_main_window_to_front(&app);
    });
}

/// 2 つ目の起動（ブラウザからの戻り・アイコンの二度押し）。今の窓を前に出し、引数の deep link を受ける。
#[cfg(desktop)]
fn on_second_instance(app: &tauri::AppHandle, argv: Vec<String>, _cwd: String) {
    bring_main_window_to_front(app);
    for arg in &argv {
        handle_deep_link(app, arg);
    }
}

/// 主窓を前に出す（最小化していれば戻す）。
fn bring_main_window_to_front(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

/// 単一インスタンス（デスクトップだけ。モバイルは OS がアプリを 1 つに保つ）。
///
/// Linux の開発版は、名前を実行ファイルごとに分ける（[`deep_link::dev_instance_id`]）。
#[cfg(desktop)]
fn single_instance_plugin() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    let builder = tauri_plugin_single_instance::Builder::new().callback(on_second_instance);
    #[cfg(all(target_os = "linux", debug_assertions))]
    let builder = match (
        std::env::current_exe(),
        tsg_one::identity::AppIdentity::load(),
    ) {
        (Ok(exe), Ok(identity)) => {
            builder.dbus_id(deep_link::dev_instance_id(&identity.identifier, &exe))
        }
        _ => builder,
    };
    builder.build()
}

/// deep link の受け口を OS に登録し、プラグインから届く戻りを受ける。
fn register_deep_link(app: &tauri::App) {
    use tauri_plugin_deep_link::DeepLinkExt;
    // 開発版（`tauri dev`）はインストーラーを通らないので、ここで OS に登録しないと
    // 「scheme does not have a registered handler」になる（Primadoc と同じ）。
    // **Windows の登録は raytrek では確かめられない**（zv で確かめる）
    #[cfg(any(target_os = "linux", target_os = "windows"))]
    if let Err(e) = app.deep_link().register_all() {
        log::warn!("deep link のスキームを登録できません: {e}");
    }
    // macOS とモバイルは 2 つ目の起動が無く、プラグインから届く。
    // Windows / Linux は単一インスタンスの引数で受けるので、ここで受けると二重になる
    #[cfg(any(mobile, target_os = "macos"))]
    {
        let handle = app.handle().clone();
        app.deep_link().on_open_url(move |event| {
            for url in event.urls() {
                handle_deep_link(&handle, url.as_str());
            }
        });
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 開発時: プロジェクト直下（または親）の .env を環境変数へ読み込む。
    // Google OAuth の Client ID/Secret を dev で渡すのに使う（無ければ何もしない）。
    let _ = dotenvy::dotenv();

    let builder = tauri::Builder::default();
    // 単一インスタンスは最初に登録する（2 つ目の起動が、ほかのプラグインを起こす前に今の窓へ渡すため）
    #[cfg(desktop)]
    let builder = builder.plugin(single_instance_plugin());
    builder
        .plugin(tauri_plugin_deep_link::init())
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            // データルート配下に mail.db を開き、マイグレーションを適用（docs/DATA_STORAGE.md）。
            let base = app.path().app_data_dir().expect("app_data_dir");
            let db_path = datadir::db_path(&base);
            log::info!("opening database at {}", db_path.display());
            let store = Store::open(&db_path).expect("failed to open database");
            // 起動時に、保持期間を過ぎたゴミ箱（連絡先・組織の論理削除）を完全削除する。
            if let Ok(days) = store.trash_retention_days() {
                if let Err(e) = store.purge_expired_trash(days) {
                    log::warn!("trash purge on startup failed: {e}");
                }
            }
            // 起動時に、保持期間を過ぎたメールのゴミ箱を完全削除する（0=無期限は何もしない）。
            if let Ok(days) = store.mail_trash_retention_days() {
                if let Err(e) = store.purge_expired_mail_trash(days) {
                    log::warn!("mail trash purge on startup failed: {e}");
                }
            }
            app.manage(store);
            // 同期のキャンセル状態（中断ボタン用）。
            app.manage(commands::SyncControl::default());
            // アカウントごとの IMAP 接続プール（接続＋ログインを使い回す）。
            app.manage(commands::ImapPool::default());
            // アカウントごとのメールの同期の最後の結果（アカウント選択の接続状況の印）。
            app.manage(crate::services::sync_status::SyncStatusBoard::default());
            // 添付ごとの取得の順番待ち（同じ添付を二重に取らない）。
            app.manage(crate::services::attachment_fetch::AttachmentFetches::default());
            // TSG One のログイン。端末の鍵は、データの置き場（移動できる）ではなく、この機械のアプリの
            // フォルダーに置く（鍵を別の機械へ写さない）
            match services::tsg_one::build(&base, &app.config().identifier) {
                Ok(tsg) => {
                    app.manage(tsg);
                }
                Err(e) => {
                    log::error!("[TsgOne] ログインの準備ができません（TSG One は使えません）: {e}")
                }
            }
            register_deep_link(app);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::db_info,
            commands::account_autoconfig,
            commands::account_add,
            commands::account_profiles,
            commands::account_profile_reorder,
            commands::account_profile_rename,
            commands::account_list,
            commands::home_unread_counts,
            commands::server_account_list,
            commands::account_test_connection,
            commands::account_test_login,
            commands::account_check,
            commands::account_ping,
            commands::account_delete,
            commands::account_set_password,
            commands::account_reorder,
            commands::mail_sync,
            commands::account_sync_statuses,
            commands::tsg_one_status,
            commands::tsg_one_login,
            commands::tsg_one_cancel_login,
            commands::tsg_one_logout,
            commands::tsg_one_recheck_device,
            commands::tsg_one_open_member_site,
            commands::mail_sync_cancel,
            commands::mail_send,
            commands::attachment_meta,
            commands::attachment_stage,
            commands::mail_load_remote,
            commands::sender_remote_allowed,
            commands::sender_set_remote_policy,
            commands::mail_list,
            commands::thread_list,
            commands::thread_day_counts,
            commands::mail_search,
            commands::mail_get,
            commands::thread_view,
            commands::thread_rename,
            commands::thread_split,
            commands::thread_merge,
            commands::message_reassign,
            commands::thread_rebuild,
            commands::mail_reprocess,
            commands::rebuild_plan,
            commands::mail_refetch,
            commands::mail_attachments,
            commands::attachment_download,
            commands::attachment_view,
            commands::attachment_open,
            commands::attachment_export,
            commands::attachment_eml_contents,
            commands::attachment_eml_open,
            commands::attachment_eml_export,
            commands::attachment_local_path,
            commands::open_local_path,
            commands::account_storage_info,
            commands::account_set_storage_limit,
            commands::storage_optimize,
            commands::mail_resync,
            commands::mail_rederive_attachments,
            commands::mail_set_read,
            commands::mail_set_starred,
            commands::mail_set_bookmarked,
            commands::mail_delete,
            commands::mail_trash,
            commands::mail_restore,
            commands::mail_empty_folder,
            commands::mail_save_draft,
            commands::mail_get_draft,
            commands::mail_draft_sync_remote,
            commands::mail_draft_discard,
            commands::tag_list,
            commands::tag_create,
            commands::tag_update,
            commands::tag_delete,
            commands::tag_set_parent,
            commands::mail_add_tag,
            commands::mail_remove_tag,
            commands::mail_mark_spam,
            commands::mail_mark_not_spam,
            commands::spam_score,
            commands::spam_find_conflicts,
            commands::spam_forgive_sender,
            commands::spam_enforce_sender,
            commands::spam_settings_get,
            commands::spam_settings_set,
            commands::contact_list,
            commands::contact_export,
            commands::postal_lookup_by_code,
            commands::postal_lookup_by_address,
            commands::contact_get,
            commands::contact_lookup_email,
            commands::recipient_suggest,
            commands::contact_upsert,
            commands::contact_delete,
            commands::contact_restore,
            commands::organization_list,
            commands::organization_get,
            commands::organization_detail,
            commands::organization_company_only_candidates,
            commands::organization_convert_contacts,
            commands::organization_find_duplicates,
            commands::organization_merge,
            commands::organization_unlinked_names,
            commands::organization_create_from_name,
            commands::organization_create_from_name_impact,
            commands::organization_link_suggestions,
            commands::organization_matches,
            commands::organization_link_contacts,
            commands::organization_link_contacts_impact,
            commands::trash_retention_get,
            commands::trash_retention_set,
            commands::trash_purge,
            commands::mail_trash_retention_get,
            commands::mail_trash_retention_set,
            commands::mail_trash_purge,
            commands::green_domain_list,
            commands::green_domain_add,
            commands::green_domain_warn,
            commands::green_domain_clear,
            commands::green_address_check,
            commands::contact_import,
            commands::contact_find_duplicates,
            commands::contact_find_matches,
            commands::contact_merge,
            commands::contact_mark_distinct,
            commands::contact_distinct_pairs,
            commands::contact_unmark_distinct,
            commands::contact_merge_preview,
            commands::contact_sure_merge_preview,
            commands::contact_sure_merge,
            commands::contact_google_duplicates,
            commands::contact_google_duplicates_tidy,
            commands::contact_google_duplicate_ids,
            commands::contact_google_duplicates_of,
            commands::contact_google_duplicates_tidy_of,
            commands::event_list,
            commands::event_list_trashed,
            commands::event_search,
            commands::event_location_suggest,
            commands::event_title_suggest,
            commands::event_get,
            commands::event_upsert,
            commands::event_delete,
            commands::event_restore,
            commands::calendar_list,
            commands::calendar_upsert,
            commands::calendar_set_visible,
            commands::calendar_delete,
            commands::event_attendee_list,
            commands::event_attendee_set,
            commands::event_reminder_list,
            commands::event_reminder_set,
            commands::ics_import,
            commands::ics_export,
            commands::google_set_credentials,
            commands::google_credentials_status,
            commands::google_accounts,
            commands::google_connect,
            commands::google_set_service,
            commands::google_disconnect,
            commands::google_sync,
            commands::contact_sync_target_add,
            commands::contact_sync_stop,
            commands::data_location,
            commands::data_relocate,
            commands::data_reset_location,
            commands::account_set_sync_window,
            commands::account_set_full_window,
            commands::account_set_body_window,
            commands::account_update,
            commands::signature_list,
            commands::signature_create,
            commands::signature_update,
            commands::signature_delete
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
