mod commands;
pub mod models;
pub mod services;

use services::store::Store;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_log::Builder::default().build())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // データルート配下に mail.db を開き、マイグレーションを適用（docs/DATA_STORAGE.md）
            let db_path = app
                .path()
                .app_data_dir()
                .expect("app_data_dir")
                .join("data")
                .join("mail.db");
            let store = Store::open(&db_path).expect("failed to open database");
            app.manage(store);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::db_info,
            commands::account_autoconfig,
            commands::account_add,
            commands::account_list,
            commands::server_account_list,
            commands::account_test_connection,
            commands::account_test_login,
            commands::account_check,
            commands::account_delete,
            commands::mail_sync,
            commands::mail_list,
            commands::mail_get,
            commands::mail_attachments,
            commands::attachment_download,
            commands::attachment_view,
            commands::attachment_open,
            commands::attachment_export,
            commands::account_storage_info,
            commands::account_set_storage_limit,
            commands::storage_optimize,
            commands::mail_resync,
            commands::mail_set_read,
            commands::mail_set_starred,
            commands::mail_set_bookmarked,
            commands::mail_delete,
            commands::tag_list,
            commands::tag_create,
            commands::tag_update,
            commands::tag_delete,
            commands::mail_add_tag,
            commands::mail_remove_tag,
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
