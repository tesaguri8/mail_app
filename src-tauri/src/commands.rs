use crate::models::{
    AccountInput, AccountSummary, AppInfo, AutoconfigResult, DbInfo, MailSummary, SyncResult,
};
use crate::services::autoconfig;
use crate::services::store::{NewAccount, Store};
use crate::services::imap_sync;
use tauri::{AppHandle, State};

/// アプリ識別情報を返す（identifier はハードコードせず Tauri 設定から取得）。
#[tauri::command]
pub fn app_info(app: AppHandle) -> AppInfo {
    let pkg = app.package_info();
    AppInfo {
        name: pkg.name.clone(),
        version: pkg.version.to_string(),
        identifier: app.config().identifier.clone(),
    }
}

/// DB のスキーマバージョンとパスを返す（疎通確認用）。
#[tauri::command]
pub fn db_info(store: State<Store>) -> Result<DbInfo, String> {
    let version = store.schema_version().map_err(|e| e.to_string())?;
    Ok(DbInfo {
        schema_version: version as i32,
        path: store.path.to_string_lossy().to_string(),
    })
}

/// メールアドレスから接続設定を自動判定（docs/ONBOARDING.md）。
/// 内蔵テーブル/さくらで決まらなければ MX レコードからメールサーバーを判定。
#[tauri::command]
pub async fn account_autoconfig(email: String) -> AutoconfigResult {
    let mut r = autoconfig::resolve(&email);
    if r.source == "guess" {
        let domain = email.rsplit('@').next().unwrap_or("").to_lowercase();
        if let Some(mx) = autoconfig::mx_host(&domain).await {
            r.imap_host = mx.clone();
            r.smtp_host = mx;
            r.imap_port = 993;
            r.imap_security = "ssl".to_string();
            r.smtp_port = 587;
            r.smtp_security = "starttls".to_string();
            r.source = "mx".to_string();
            r.note = Some(
                "MX レコードからメールサーバーを判定しました。ユーザー名はメールアドレス全体の場合があります。"
                    .to_string(),
            );
        }
    }
    r
}

/// アカウントを追加。資格情報は keyring（OS 金庫）へ、設定は DB へ保存。
#[tauri::command]
pub fn account_add(
    app: AppHandle,
    store: State<Store>,
    input: AccountInput,
    password: String,
) -> Result<AccountSummary, String> {
    // 資格情報は平文 DB でなく OS 金庫へ（サービス名 = アプリ identifier、ユーザー名 = email）
    let service = app.config().identifier.clone();
    let entry = keyring::Entry::new(&service, &input.email).map_err(|e| e.to_string())?;
    entry.set_password(&password).map_err(|e| e.to_string())?;

    let id = store
        .insert_account(&NewAccount {
            email: input.email.clone(),
            display_name: input.display_name.clone(),
            username: input.username.clone(),
            imap_host: input.imap_host.clone(),
            imap_port: input.imap_port,
            smtp_host: input.smtp_host.clone(),
            smtp_port: input.smtp_port,
        })
        .map_err(|e| e.to_string())?;

    Ok(AccountSummary {
        id: id as i32,
        email: input.email,
        display_name: input.display_name,
        imap_host: input.imap_host,
        smtp_host: input.smtp_host,
    })
}

/// 登録済みアカウント一覧（資格情報は含めない）。
#[tauri::command]
pub fn account_list(store: State<Store>) -> Result<Vec<AccountSummary>, String> {
    store.list_accounts().map_err(|e| e.to_string())
}

/// IMAP に接続して INBOX を同期し、新着を DB に保存（PoC）。
/// ブロッキング処理は spawn_blocking に載せ、UI を止めない。
#[tauri::command]
pub async fn mail_sync(
    app: AppHandle,
    store: State<'_, Store>,
    account_id: i64,
) -> Result<SyncResult, String> {
    let (email, login_user, host, port) = store
        .get_account_imap(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "アカウントが見つかりません".to_string())?;
    // 資格情報は email をキーに保存（アカウント識別子）。ログインは login_user を使う。
    let service = app.config().identifier.clone();
    let password = keyring::Entry::new(&service, &email)
        .and_then(|e| e.get_password())
        .map_err(|e| format!("資格情報を取得できません: {e}"))?;
    let db_path = store.path.clone();

    tauri::async_runtime::spawn_blocking(move || {
        imap_sync::sync_account(&db_path, account_id, &host, port, &login_user, &password, 50)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// メール一覧を返す。
#[tauri::command]
pub fn mail_list(store: State<Store>, account_id: i64, limit: i64) -> Result<Vec<MailSummary>, String> {
    store
        .list_emails(account_id, limit)
        .map_err(|e| e.to_string())
}

/// ホスト:ポートへの TCP 疎通テスト（認証は行わない。オンボーディングの確認用）。
#[tauri::command]
pub fn account_test_connection(host: String, port: u16) -> Result<(), String> {
    use std::net::{TcpStream, ToSocketAddrs};
    use std::time::Duration;
    let addr = format!("{host}:{port}");
    let sock = addr
        .to_socket_addrs()
        .map_err(|e| format!("名前解決に失敗: {e}"))?
        .next()
        .ok_or_else(|| "アドレスを解決できませんでした".to_string())?;
    TcpStream::connect_timeout(&sock, Duration::from_secs(8))
        .map(|_| ())
        .map_err(|e| format!("接続できませんでした: {e}"))
}
