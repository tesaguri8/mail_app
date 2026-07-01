use crate::models::{
    AccountInput, AccountSummary, AppInfo, AttachmentSummary, AutoconfigResult, DbInfo,
    EvictionReport, MailDetail, MailSummary, ServerAccountSummary, SignatureSummary, StorageInfo,
    SyncResult, TagSummary,
};
use crate::services::autoconfig;
use crate::services::imap_sync;
use crate::services::media;
use crate::services::store::{NewAccount, NewServerAccount, Store};
use tauri::{AppHandle, Manager, State};

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

    // メールサーバーアカウント設定を再利用 or 作成して紐づける
    let login_user = input
        .username
        .clone()
        .unwrap_or_else(|| input.email.clone());
    let server_account_id = store
        .find_or_create_server_account(&NewServerAccount {
            imap_host: input.imap_host.clone(),
            imap_port: input.imap_port,
            smtp_host: input.smtp_host.clone(),
            smtp_port: input.smtp_port,
            username: login_user,
        })
        .map_err(|e| e.to_string())?;

    let id = store
        .insert_account(&NewAccount {
            email: input.email.clone(),
            display_name: input.display_name.clone(),
            username: input.username.clone(),
            imap_host: input.imap_host.clone(),
            imap_port: input.imap_port,
            smtp_host: input.smtp_host.clone(),
            smtp_port: input.smtp_port,
            server_account_id: Some(server_account_id),
        })
        .map_err(|e| e.to_string())?;

    Ok(AccountSummary {
        id: id as i32,
        email: input.email,
        display_name: input.display_name,
        imap_host: input.imap_host,
        smtp_host: input.smtp_host,
        sync_window: "6m".to_string(),
        signature_id: None,
        unread_count: 0,
        total_count: 0,
    })
}

/// 登録済みアカウント一覧（資格情報は含めない）。
#[tauri::command]
pub fn account_list(store: State<Store>) -> Result<Vec<AccountSummary>, String> {
    store.list_accounts().map_err(|e| e.to_string())
}

/// 登録済みのメールサーバーアカウント設定一覧（再利用の選択肢）。
#[tauri::command]
pub fn server_account_list(store: State<Store>) -> Result<Vec<ServerAccountSummary>, String> {
    store.list_server_accounts().map_err(|e| e.to_string())
}

/// アカウントの編集（差出人名・既定署名）。
#[tauri::command]
pub fn account_update(
    store: State<Store>,
    account_id: i64,
    display_name: Option<String>,
    signature_id: Option<i64>,
) -> Result<(), String> {
    // 空文字は未設定として扱う
    let dn = display_name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    store
        .update_account(account_id, dn, signature_id)
        .map_err(|e| e.to_string())
}

/// 署名一覧。
#[tauri::command]
pub fn signature_list(store: State<Store>) -> Result<Vec<SignatureSummary>, String> {
    store.list_signatures().map_err(|e| e.to_string())
}

/// 署名を新規作成（作成した署名を返す）。
#[tauri::command]
pub fn signature_create(
    store: State<Store>,
    name: String,
    body: String,
) -> Result<SignatureSummary, String> {
    let id = store
        .insert_signature(&name, &body)
        .map_err(|e| e.to_string())?;
    Ok(SignatureSummary {
        id: id as i32,
        name,
        body,
    })
}

/// 署名を更新。
#[tauri::command]
pub fn signature_update(
    store: State<Store>,
    id: i64,
    name: String,
    body: String,
) -> Result<(), String> {
    store
        .update_signature(id, &name, &body)
        .map_err(|e| e.to_string())
}

/// 署名を削除（参照していたアカウントの紐づけは解除）。
#[tauri::command]
pub fn signature_delete(store: State<Store>, id: i64) -> Result<(), String> {
    store.delete_signature(id).map_err(|e| e.to_string())
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
        imap_sync::sync_account(&db_path, account_id, &host, port, &login_user, &password)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 同期範囲（取り込み期間/件数）を設定する。値: "n50" / "3d" / "30d" / "3m" / "all" 等。
#[tauri::command]
pub fn account_set_sync_window(
    store: State<Store>,
    account_id: i64,
    window: String,
) -> Result<(), String> {
    store
        .set_sync_window(account_id, &window)
        .map_err(|e| e.to_string())
}

/// メール一覧を返す。
#[tauri::command]
pub fn mail_list(
    store: State<Store>,
    account_id: i64,
    limit: i64,
) -> Result<Vec<MailSummary>, String> {
    store
        .list_emails(account_id, limit)
        .map_err(|e| e.to_string())
}

/// 複数メールの既読/未読を一括設定。
#[tauri::command]
pub fn mail_set_read(store: State<Store>, ids: Vec<i64>, read: bool) -> Result<(), String> {
    store.set_emails_read(&ids, read).map_err(|e| e.to_string())
}

/// 複数メールのスター（お気に入り）を一括設定。
#[tauri::command]
pub fn mail_set_starred(store: State<Store>, ids: Vec<i64>, value: bool) -> Result<(), String> {
    store
        .set_emails_starred(&ids, value)
        .map_err(|e| e.to_string())
}

/// 複数メールのブックマークを一括設定。
#[tauri::command]
pub fn mail_set_bookmarked(store: State<Store>, ids: Vec<i64>, value: bool) -> Result<(), String> {
    store
        .set_emails_bookmarked(&ids, value)
        .map_err(|e| e.to_string())
}

/// 複数メールを一括削除。
#[tauri::command]
pub fn mail_delete(store: State<Store>, ids: Vec<i64>) -> Result<(), String> {
    store.delete_emails(&ids).map_err(|e| e.to_string())
}

/// タグ一覧（使用件数つき）。
#[tauri::command]
pub fn tag_list(store: State<Store>) -> Result<Vec<TagSummary>, String> {
    store.list_tags().map_err(|e| e.to_string())
}

/// タグを新規作成（作成したタグを返す）。
#[tauri::command]
pub fn tag_create(
    store: State<Store>,
    name: String,
    color: Option<String>,
) -> Result<TagSummary, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("タグ名を入力してください".to_string());
    }
    store
        .insert_tag(name, color.as_deref())
        .map_err(|e| e.to_string())
}

/// タグの名前・色を更新。
#[tauri::command]
pub fn tag_update(
    store: State<Store>,
    id: i64,
    name: String,
    color: Option<String>,
) -> Result<(), String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("タグ名を入力してください".to_string());
    }
    store
        .update_tag(id, name, color.as_deref())
        .map_err(|e| e.to_string())
}

/// タグを削除（メールとの紐づけも解除）。
#[tauri::command]
pub fn tag_delete(store: State<Store>, id: i64) -> Result<(), String> {
    store.delete_tag(id).map_err(|e| e.to_string())
}

/// 複数メールにタグを付与。
#[tauri::command]
pub fn mail_add_tag(store: State<Store>, ids: Vec<i64>, tag_id: i64) -> Result<(), String> {
    store
        .add_tag_to_emails(&ids, tag_id)
        .map_err(|e| e.to_string())
}

/// 複数メールからタグを外す。
#[tauri::command]
pub fn mail_remove_tag(store: State<Store>, ids: Vec<i64>, tag_id: i64) -> Result<(), String> {
    store
        .remove_tag_from_emails(&ids, tag_id)
        .map_err(|e| e.to_string())
}

/// メール本文を取得し、既読にする。
#[tauri::command]
pub fn mail_get(store: State<Store>, id: i64) -> Result<MailDetail, String> {
    let detail = store
        .get_email(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "メールが見つかりません".to_string())?;
    let _ = store.mark_read(id);
    Ok(detail)
}

/// メールの添付メタ一覧を返す（本体未取得のものは is_downloaded=false）。
#[tauri::command]
pub fn mail_attachments(
    store: State<Store>,
    email_id: i64,
) -> Result<Vec<AttachmentSummary>, String> {
    store.list_attachments(email_id).map_err(|e| e.to_string())
}

/// 添付本体をディスクに用意して保存先パスを返す（既に取得済みならそれを再利用）。
/// emails.uid + attachments.part_index で IMAP から該当パートだけを再取得する。
async fn ensure_attachment_file(
    app: &AppHandle,
    store: &Store,
    attachment_id: i64,
) -> Result<std::path::PathBuf, String> {
    let info = store
        .attachment_fetch_info(attachment_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "添付が見つかりません".to_string())?;

    // 取得済みでファイルが残っていればそのまま使う（LRU の最終アクセスを更新）。
    if let Some(path) = info.file_path.as_ref() {
        let p = std::path::PathBuf::from(path);
        if p.exists() {
            let _ = store.touch_attachment(attachment_id);
            return Ok(p);
        }
    }

    let uid = info.email_uid.ok_or_else(|| {
        "再取得に必要な情報がありません。アカウントを再同期してください。".to_string()
    })?;
    let part_index = info.part_index;
    let filename = info.filename;

    let (email, login, host, port) = store
        .get_account_imap(info.account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "アカウントが見つかりません".to_string())?;
    let service = app.config().identifier.clone();
    let password = keyring::Entry::new(&service, &email)
        .and_then(|e| e.get_password())
        .map_err(|e| format!("資格情報を取得できません: {e}"))?;

    let fetched = tauri::async_runtime::spawn_blocking(move || {
        imap_sync::fetch_attachment(
            &host,
            port,
            &login,
            &password,
            uid as u32,
            part_index as usize,
        )
    })
    .await
    .map_err(|e| e.to_string())??;

    // 保存先: <app_data>/data/attachments/<attachment_id>/<filename>
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join("data")
        .join("attachments")
        .join(attachment_id.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let safe = sanitize_filename(&filename);
    let path = dir.join(&safe);
    std::fs::write(&path, &fetched.bytes).map_err(|e| e.to_string())?;

    let checksum = simple_checksum(&fetched.bytes);
    let path_str = path.to_string_lossy().to_string();
    store
        .set_attachment_downloaded(attachment_id, &path_str, Some(&checksum))
        .map_err(|e| e.to_string())?;

    Ok(path)
}

/// 添付ファイルをオンデマンドで取得して保存する（既に取得済みならそれを返す）。
/// 取得後、アカウントの容量上限を超えていれば古い添付を自動で追い出す。
#[tauri::command]
pub async fn attachment_download(
    app: AppHandle,
    store: State<'_, Store>,
    attachment_id: i64,
) -> Result<AttachmentSummary, String> {
    ensure_attachment_file(&app, &store, attachment_id).await?;
    // 容量超過時は古い添付を追い出す（best-effort）。
    if let Ok(Some(info)) = store.attachment_fetch_info(attachment_id) {
        let _ = store.evict_over_limit(info.account_id);
    }
    store
        .get_attachment(attachment_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "添付が見つかりません".to_string())
}

/// 画像の添付/インラインを web 表示用に変換し、data URL を返す。
/// HEIC は WebView 非対応のため JPEG へ変換し、大きすぎる画像は縮小する。
/// `thumb=true` なら一覧サムネイル用に小さめのレンディションを返す。
#[tauri::command]
pub async fn attachment_view(
    app: AppHandle,
    store: State<'_, Store>,
    attachment_id: i64,
    thumb: bool,
) -> Result<String, String> {
    let att = store
        .get_attachment(attachment_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "添付が見つかりません".to_string())?;
    let ct = att.content_type.as_deref();
    if !media::is_image(ct, &att.filename) {
        return Err("画像ではありません".to_string());
    }

    let path = ensure_attachment_file(&app, &store, attachment_id).await?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let max = if thumb {
        media::THUMB_MAX
    } else {
        media::VIEW_MAX
    };

    let filename = att.filename.clone();
    let content_type = att.content_type.clone();
    tauri::async_runtime::spawn_blocking(move || {
        media::to_web_data_url(&bytes, content_type.as_deref(), &filename, max)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 添付を OS の関連アプリで開く（未取得なら先に取得）。
/// HEIC は Windows 標準で開けないことがあるため、JPEG レンディションを作って開く。
#[tauri::command]
pub async fn attachment_open(
    app: AppHandle,
    store: State<'_, Store>,
    attachment_id: i64,
) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let att = store
        .get_attachment(attachment_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "添付が見つかりません".to_string())?;

    let original = ensure_attachment_file(&app, &store, attachment_id).await?;

    // HEIC はそのままだと Windows で開けない場合があるので JPEG 版を作って開く。
    let to_open = if media::is_heic(att.content_type.as_deref(), &att.filename) {
        let bytes = std::fs::read(&original).map_err(|e| e.to_string())?;
        let jpeg = tauri::async_runtime::spawn_blocking(move || {
            media::heic_to_jpeg_bytes(&bytes, media::VIEW_MAX)
        })
        .await
        .map_err(|e| e.to_string())??;
        let jpeg_path = original.with_extension("jpg");
        std::fs::write(&jpeg_path, &jpeg).map_err(|e| e.to_string())?;
        jpeg_path
    } else {
        original
    };

    app.opener()
        .open_path(to_open.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// 添付をユーザー指定の場所へ保存する（ダウンロード）。未取得なら先に取得してから複製。
/// `dest` は保存先のフルパス（フロントの保存ダイアログで決める）。
#[tauri::command]
pub async fn attachment_export(
    app: AppHandle,
    store: State<'_, Store>,
    attachment_id: i64,
    dest: String,
) -> Result<(), String> {
    let src = ensure_attachment_file(&app, &store, attachment_id).await?;
    std::fs::copy(&src, &dest).map_err(|e| format!("保存に失敗しました: {e}"))?;
    Ok(())
}

/// アカウントのローカル保存容量（使用量と上限）。
#[tauri::command]
pub fn account_storage_info(store: State<Store>, account_id: i64) -> Result<StorageInfo, String> {
    let used = store.storage_used(account_id).map_err(|e| e.to_string())?;
    let limit = store.storage_limit(account_id).map_err(|e| e.to_string())?;
    Ok(StorageInfo {
        used_bytes: used as f64,
        limit_bytes: limit as f64,
    })
}

/// アカウントの容量上限を設定する（バイト）。
#[tauri::command]
pub fn account_set_storage_limit(
    store: State<Store>,
    account_id: i64,
    bytes: f64,
) -> Result<(), String> {
    let bytes = bytes.max(0.0) as i64;
    store
        .set_storage_limit(account_id, bytes)
        .map_err(|e| e.to_string())
}

/// ストレージ最適化: 上限超過分の古い添付バイトを追い出す（メタは残す）。
#[tauri::command]
pub fn storage_optimize(store: State<Store>, account_id: i64) -> Result<EvictionReport, String> {
    store
        .evict_over_limit(account_id)
        .map_err(|e| e.to_string())
}

/// 点検つき再取り込み: 同期状態をリセットして取り込み範囲をフル再取得し、
/// 既存メールに uid・添付メタを埋め戻す（古いメールの添付を後付け対応）。
#[tauri::command]
pub async fn mail_resync(
    app: AppHandle,
    store: State<'_, Store>,
    account_id: i64,
) -> Result<SyncResult, String> {
    store
        .reset_sync_state(account_id)
        .map_err(|e| e.to_string())?;
    let (email, login_user, host, port) = store
        .get_account_imap(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "アカウントが見つかりません".to_string())?;
    let service = app.config().identifier.clone();
    let password = keyring::Entry::new(&service, &email)
        .and_then(|e| e.get_password())
        .map_err(|e| format!("資格情報を取得できません: {e}"))?;
    let db_path = store.path.clone();

    tauri::async_runtime::spawn_blocking(move || {
        imap_sync::sync_account(&db_path, account_id, &host, port, &login_user, &password)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// ファイル名を保存に安全な形へ正規化する（パス区切り・禁止文字を除去）。
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim();
    if trimmed.is_empty() {
        "attachment".to_string()
    } else {
        trimmed.to_string()
    }
}

/// 非暗号の簡易チェックサム（キャッシュ整合の目安。改ざん検知用ではない）。
fn simple_checksum(bytes: &[u8]) -> String {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut h);
    format!("{:016x}", h.finish())
}

/// 実際に IMAP ログインを試す（ユーザー名/パスワードの検証）。
#[tauri::command]
pub async fn account_test_login(
    host: String,
    port: u16,
    username: String,
    password: String,
) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        imap_sync::test_login(&host, port, &username, &password)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 登録済みアカウントの接続状態を確認（保存済み資格情報で実ログイン）。
#[tauri::command]
pub async fn account_check(
    app: AppHandle,
    store: State<'_, Store>,
    account_id: i64,
) -> Result<(), String> {
    let (email, login, host, port) = store
        .get_account_imap(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "アカウントが見つかりません".to_string())?;
    let service = app.config().identifier.clone();
    let password = keyring::Entry::new(&service, &email)
        .and_then(|e| e.get_password())
        .map_err(|e| format!("資格情報を取得できません: {e}"))?;
    tauri::async_runtime::spawn_blocking(move || {
        imap_sync::test_login(&host, port, &login, &password)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// アカウントを削除（受信メールと keyring の資格情報も削除）。
#[tauri::command]
pub fn account_delete(app: AppHandle, store: State<Store>, account_id: i64) -> Result<(), String> {
    if let Some((email, _login, _host, _port)) = store
        .get_account_imap(account_id)
        .map_err(|e| e.to_string())?
    {
        let service = app.config().identifier.clone();
        if let Ok(entry) = keyring::Entry::new(&service, &email) {
            let _ = entry.delete_credential();
        }
    }
    store.delete_account(account_id).map_err(|e| e.to_string())
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
