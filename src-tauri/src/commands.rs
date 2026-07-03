use crate::models::{
    AccountInput, AccountSummary, AppInfo, AttachmentSummary, AutoconfigResult,
    ContactGroupSummary, ContactInput, ContactMatch, ContactSummary, DataLocation, DbInfo,
    DraftContent, DraftInput, DuplicateGroup, GreenDomainEntry, ImportReport, MailDetail,
    MailSummary, OrgDuplicateGroup,
    OrganizationDetail, OrganizationSummary, RecipientSuggestion, RemoteImage, RetentionReport,
    SendInput, ServerAccountSummary, SignatureSummary, SpamSettings, SpamVerdict, StorageInfo,
    SyncProgress, SyncResult, TagSummary,
};
use crate::services::autoconfig;
use crate::services::datadir;
use crate::services::gcsv;
use crate::services::imap_sync;
use crate::services::media;
use crate::services::smtp;
use crate::services::spam;
use crate::services::store::{NewAccount, NewServerAccount, Store};
use crate::services::vcard;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

/// 実行中の同期のキャンセルフラグを account_id ごとに管理する（中断ボタン用）。
#[derive(Default)]
pub struct SyncControl(Mutex<HashMap<i64, Arc<AtomicBool>>>);

impl SyncControl {
    /// 同期開始時にフラグを登録して返す（既存があれば置き換え）。
    fn begin(&self, account_id: i64) -> Arc<AtomicBool> {
        let flag = Arc::new(AtomicBool::new(false));
        self.0.lock().unwrap().insert(account_id, flag.clone());
        flag
    }
    /// 同期終了時に取り除く。
    fn end(&self, account_id: i64) {
        self.0.lock().unwrap().remove(&account_id);
    }
    /// 中断要求を立てる（対象が動作中なら true）。
    fn request_cancel(&self, account_id: i64) -> bool {
        if let Some(flag) = self.0.lock().unwrap().get(&account_id) {
            flag.store(true, Ordering::Relaxed);
            true
        } else {
            false
        }
    }
}

/// 実行中の同期/再取り込みを中断する。中断は次のチャンク境界で反映される。
#[tauri::command]
pub fn mail_sync_cancel(control: State<SyncControl>, account_id: i64) -> bool {
    control.request_cancel(account_id)
}

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
        path: store.path().to_string_lossy().to_string(),
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
        sync_window: "all".to_string(),
        full_window: "all".to_string(),
        body_window: "off".to_string(),
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

/// アカウントの並び順を設定する（渡された ID 順に永続化）。
#[tauri::command]
pub fn account_reorder(store: State<Store>, ids: Vec<i64>) -> Result<(), String> {
    store.reorder_accounts(&ids).map_err(|e| e.to_string())
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
    control: State<'_, SyncControl>,
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
    let db_path = store.path();
    let cancel = control.begin(account_id);

    // 進捗を "sync:progress" イベントで UI に通知する（フォルダ / 取得済み / 予定）。
    let app_ev = app.clone();
    let cancel_task = cancel.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        let progress = |folder: &str, current: i32, total: i32| {
            let _ = app_ev.emit(
                "sync:progress",
                SyncProgress {
                    folder: folder.to_string(),
                    current,
                    total,
                },
            );
        };
        imap_sync::sync_account(
            &db_path,
            account_id,
            &host,
            port,
            &login_user,
            &password,
            &progress,
            &cancel_task,
        )
    })
    .await
    .map_err(|e| e.to_string())?;
    control.end(account_id);
    // 同期後に保持ポリシーを適用（古い添付の削除・本文の要約保存・容量保険）。best-effort。
    if result.is_ok() {
        let _ = store.apply_retention(account_id);
    }
    result
}

/// プレーン本文から最小限の HTML を作る（エスケープ＋改行を <br> 化）。
/// multipart/alternative の HTML パート用。改行は CSS(pre-wrap) ではなく <br> で表現する
/// （テキスト主体の安全描画でも確実に改行されるように）。リンク化などは後続。
fn plain_to_html(plain: &str) -> String {
    let escaped = plain
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let with_breaks = escaped.replace("\r\n", "\n").replace('\n', "<br>\n");
    format!(
        "<!DOCTYPE html><html><body>\
         <div style=\"font-family:sans-serif;font-size:14px;line-height:1.5\">{with_breaks}</div>\
         </body></html>"
    )
}

/// メールを送信する（SMTP）。差出人アカウントの設定と keyring の資格情報を使う。
/// ブロッキング送信は spawn_blocking に載せ、UI を止めない（docs/COMPOSE.md）。
#[tauri::command]
pub async fn mail_send(
    app: AppHandle,
    store: State<'_, Store>,
    input: SendInput,
) -> Result<(), String> {
    // 宛先の正規化（空白のみの行を除去）。To は最低 1 件必須。
    let norm = |v: Vec<String>| -> Vec<String> {
        v.into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };
    let to = norm(input.to);
    let cc = norm(input.cc);
    let bcc = norm(input.bcc);
    if to.is_empty() {
        return Err("宛先（To）を 1 件以上入力してください".to_string());
    }

    let acct = store
        .get_account_smtp(input.account_id as i64)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "アカウントが見つかりません".to_string())?;

    // 資格情報は email をキーに keyring から取得（IMAP と同じ規約）。
    let service = app.config().identifier.clone();
    let password = keyring::Entry::new(&service, &acct.email)
        .and_then(|e| e.get_password())
        .map_err(|e| format!("資格情報を取得できません: {e}"))?;

    let body_html = plain_to_html(&input.body);
    let config = smtp::SmtpConfig {
        host: acct.smtp_host,
        port: acct.smtp_port,
        security: acct.smtp_security,
        user: acct.login_user,
        password: password.clone(),
    };
    let message = smtp::OutgoingMessage {
        from_name: acct.display_name,
        from_email: acct.email,
        to,
        cc,
        bcc,
        subject: input.subject,
        body_plain: input.body,
        body_html: Some(body_html),
        in_reply_to: input.in_reply_to,
        message_id: None, // 実送信は lettre の自動採番でよい
    };

    // 送信メッセージを 1 度だけ組み立て、SMTP 送信と Sent 保存で共有する。
    let email = smtp::build_message(&message)?;
    let raw = email.formatted(); // Sent へ APPEND する RFC822 バイト列（Bcc は含まれない）

    tauri::async_runtime::spawn_blocking(move || smtp::send(&config, &email))
        .await
        .map_err(|e| e.to_string())??;

    // 送信成功後、送信控えを IMAP の Sent フォルダへ保存する（best-effort）。
    // 失敗しても送信自体は成功しているので、警告ログにとどめてエラーにはしない。
    if let Ok(Some((_email, login, host, port))) = store.get_account_imap(input.account_id as i64) {
        let res = tauri::async_runtime::spawn_blocking(move || {
            imap_sync::append_to_sent(&host, port, &login, &password, &raw)
        })
        .await;
        match res {
            Ok(Ok(())) => {}
            Ok(Err(e)) => log::warn!("送信は成功、Sent への保存に失敗: {e}"),
            Err(e) => log::warn!("Sent 保存タスクに失敗: {e}"),
        }
    }
    Ok(())
}

/// リモート画像のディスクキャッシュ用フォルダ。**許可済み差出人**のときだけ Some を返す。
/// 添付と同じく data_dir 配下（`<data>/remote_images/<差出人hash>`）。解除時にこの単位で削除する。
fn remote_cache_dir(store: &Store, sender: Option<&str>) -> Option<std::path::PathBuf> {
    let addr = sender.map(str::trim).filter(|s| !s.is_empty())?;
    if !store.remote_images_allowed_for(addr).unwrap_or(false) {
        return None;
    }
    Some(
        store
            .data_dir()
            .join("remote_images")
            .join(simple_checksum(addr.to_lowercase().as_bytes())),
    )
}

/// 明示許可された外部画像を取得し、サニタイズ（再エンコード）した data URL を返す。
/// http(s) のみ・サイズ/タイムアウト上限つき。取得失敗や非画像は黙って飛ばす（best-effort）。
/// 取得はユーザー操作（「画像を表示」/差出人許可）時のみ行い、既定では読み込まない
/// （開封トラッキング防止）。再エンコードでデコーダ攻撃・EXIF も無害化する（docs/MAIL_SECURITY.md §1.1）。
/// `sender` が許可済みなら添付と同じくディスクキャッシュし、初回だけ取得→以降は再アクセスしない
/// （トラッキング ping の反復を抑止）。未許可（この1通だけ）はキャッシュせず毎回取得する。
#[tauri::command]
pub async fn mail_load_remote(
    store: State<'_, Store>,
    urls: Vec<String>,
    sender: Option<String>,
) -> Result<Vec<RemoteImage>, String> {
    const MAX_BYTES: u64 = 15 * 1024 * 1024;
    let cache_dir = remote_cache_dir(&store, sender.as_deref());

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let mut out = Vec::new();
    for url in urls {
        // http(s) のみ許可（javascript:/file: などを弾く）。
        let lower = url.trim().to_lowercase();
        if !(lower.starts_with("http://") || lower.starts_with("https://")) {
            continue;
        }

        // キャッシュ命中ならネットワークへ行かずに即返す（＝トラッキング ping を出さない）。
        let cache_file = cache_dir
            .as_ref()
            .map(|d| d.join(simple_checksum(url.as_bytes())));
        if let Some(cf) = cache_file.as_ref() {
            if let Ok(jpeg) = std::fs::read(cf) {
                out.push(RemoteImage {
                    url,
                    data_url: media::jpeg_bytes_to_data_url(&jpeg),
                });
                continue;
            }
        }

        let Ok(resp) = client.get(url.as_str()).send().await else {
            continue;
        };
        if !resp.status().is_success() {
            continue;
        }
        // Content-Length があれば先に上限チェック（無駄なダウンロードを避ける）。
        if resp.content_length().is_some_and(|len| len > MAX_BYTES) {
            continue;
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(';').next().unwrap_or(s).trim().to_string());
        let Ok(bytes) = resp.bytes().await else {
            continue;
        };
        if bytes.len() as u64 > MAX_BYTES {
            continue;
        }

        // URL パスからファイル名（拡張子）を拾う（media の形式判定の補助）。
        let filename = url
            .split(['?', '#'])
            .next()
            .unwrap_or(&url)
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string();

        // 画像でなければ飛ばす。画像なら再エンコードして JPEG バイト列にする（＝サニタイズ）。
        if !media::is_image(content_type.as_deref(), &filename) {
            continue;
        }
        let jpeg = tauri::async_runtime::spawn_blocking(move || {
            media::to_web_jpeg_bytes(
                bytes.as_ref(),
                content_type.as_deref(),
                &filename,
                media::VIEW_MAX,
            )
        })
        .await
        .map_err(|e| e.to_string())?;
        let Ok(jpeg) = jpeg else {
            continue;
        };

        // 許可済み差出人ならキャッシュに保存（best-effort。失敗しても表示は続行）。
        if let Some(cf) = cache_file.as_ref() {
            if let Some(parent) = cf.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(cf, &jpeg);
        }
        out.push(RemoteImage {
            url,
            data_url: media::jpeg_bytes_to_data_url(&jpeg),
        });
    }
    Ok(out)
}

/// 差出人アドレスの外部画像を常に許可するか（住所録の信頼設定も見る。docs/MAIL_SECURITY.md §1）。
#[tauri::command]
pub fn sender_remote_allowed(store: State<Store>, address: String) -> Result<bool, String> {
    store
        .remote_images_allowed_for(&address)
        .map_err(|e| e.to_string())
}

/// 差出人アドレスの外部画像許可（常に許可/解除）を保存する。
/// 解除時は、その差出人のキャッシュ画像を丸ごと削除する（もう表示しないため手元にも残さない）。
#[tauri::command]
pub fn sender_set_remote_policy(
    store: State<Store>,
    address: String,
    allow: bool,
) -> Result<(), String> {
    store
        .set_remote_images_allowed_for(&address, allow)
        .map_err(|e| e.to_string())?;
    if !allow {
        let addr = address.trim().to_lowercase();
        if !addr.is_empty() {
            let dir = store
                .data_dir()
                .join("remote_images")
                .join(simple_checksum(addr.as_bytes()));
            let _ = std::fs::remove_dir_all(&dir);
        }
    }
    Ok(())
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

/// フルデータ保持期間を設定する（'7d'/'30d'/…/'all'）。設定後すぐ保持ポリシーを適用。
#[tauri::command]
pub fn account_set_full_window(
    store: State<Store>,
    account_id: i64,
    window: String,
) -> Result<RetentionReport, String> {
    store
        .set_full_window(account_id, &window)
        .map_err(|e| e.to_string())?;
    store.apply_retention(account_id).map_err(|e| e.to_string())
}

/// 本文の全文保持期間を設定する（'off'/'3m'/…/'2y'）。設定後すぐ保持ポリシーを適用。
#[tauri::command]
pub fn account_set_body_window(
    store: State<Store>,
    account_id: i64,
    window: String,
) -> Result<RetentionReport, String> {
    store
        .set_body_window(account_id, &window)
        .map_err(|e| e.to_string())?;
    store.apply_retention(account_id).map_err(|e| e.to_string())
}

/// 指定フォルダ（'inbox' | 'sent' | 'drafts' | 'trash' | 'spam'）のメール一覧を返す。
/// `account_id` が None（未指定）なら全アカウント横断の「全て」表示。
#[tauri::command]
pub fn mail_list(
    store: State<Store>,
    account_id: Option<i64>,
    folder: String,
    limit: i64,
    offset: i64,
) -> Result<Vec<MailSummary>, String> {
    store
        .list_emails(account_id, &folder, limit, offset)
        .map_err(|e| e.to_string())
}

/// 全文検索。件名・差出人・本文を対象に絞り込む。`account_id` が None なら全アカウント横断。
#[tauri::command]
pub fn mail_search(
    store: State<Store>,
    account_id: Option<i64>,
    folder: String,
    query: String,
    limit: i64,
) -> Result<Vec<MailSummary>, String> {
    store
        .search_emails(account_id, &folder, &query, limit)
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

/// 指定フォルダ（trash/spam 等）を空にする。`account_id` が None なら全アカウント。削除件数を返す。
#[tauri::command]
pub fn mail_empty_folder(
    store: State<Store>,
    account_id: Option<i64>,
    folder: String,
) -> Result<i32, String> {
    store
        .empty_folder(account_id, &folder)
        .map_err(|e| e.to_string())
}

/// 書きかけのメールを下書き（drafts）へ保存/更新する。保存した下書きの emails.id を返す。
/// `input.draft_id` があれば更新、無ければ新規作成。破棄は `mail_delete` を使う。
#[tauri::command]
pub fn mail_save_draft(store: State<Store>, input: DraftInput) -> Result<i64, String> {
    store.save_draft(&input).map_err(|e| e.to_string())
}

/// 下書き 1 件を作成画面へ読み戻す内容（宛先・件名・本文・In-Reply-To）を取得する。
#[tauri::command]
pub fn mail_get_draft(store: State<Store>, id: i64) -> Result<DraftContent, String> {
    store
        .get_draft(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "下書きが見つかりません".to_string())
}

/// カンマ/改行区切りのアドレス文字列を配列へ（空要素を除去）。
fn split_addr_list(s: &str) -> Vec<String> {
    s.split([',', '\n'])
        .map(|x| x.trim().to_string())
        .filter(|x| !x.is_empty())
        .collect()
}

/// 下書き（drafts）をサーバーの Drafts フォルダへ同期する（APPEND。既存の同 Message-ID は
/// 削除して入れ直し、常に最新版が 1 通だけになるようにする）。作成画面を閉じて残すときに呼ぶ。
/// サーバー設定や Drafts フォルダが無い等の失敗はエラーを返すが、呼び出し側は best-effort 扱い
/// （ローカルの下書きは保持される）。
#[tauri::command]
pub async fn mail_draft_sync_remote(
    app: AppHandle,
    store: State<'_, Store>,
    id: i64,
) -> Result<(), String> {
    let draft = store
        .get_draft(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "下書きが見つかりません".to_string())?;
    let (_account_id, message_id) = store
        .draft_remote_ref(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "下書きの Message-ID がありません".to_string())?;
    let acct = store
        .get_account_smtp(draft.account_id as i64)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "アカウントが見つかりません".to_string())?;

    let body_html = plain_to_html(&draft.body);
    let message = smtp::OutgoingMessage {
        from_name: acct.display_name,
        from_email: acct.email,
        to: split_addr_list(&draft.to),
        cc: split_addr_list(&draft.cc),
        bcc: vec![],
        subject: draft.subject,
        body_plain: draft.body,
        body_html: Some(body_html),
        in_reply_to: draft.in_reply_to,
        message_id: Some(message_id.clone()),
    };
    let email = smtp::build_message(&message)?;
    let raw = email.formatted();

    let (imap_email, login, host, port) = store
        .get_account_imap(draft.account_id as i64)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "アカウントが見つかりません".to_string())?;
    let service = app.config().identifier.clone();
    let password = keyring::Entry::new(&service, &imap_email)
        .and_then(|e| e.get_password())
        .map_err(|e| format!("資格情報を取得できません: {e}"))?;
    let inner = message_id
        .trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_string();

    tauri::async_runtime::spawn_blocking(move || {
        imap_sync::upsert_draft(&host, port, &login, &password, &raw, &inner)
    })
    .await
    .map_err(|e| e.to_string())?
}

/// 下書きをサーバーとローカルの両方から削除する（破棄・送信後の後片付け）。
/// ローカルは即削除して返し、サーバーのコピー削除はバックグラウンドで行う（best-effort。
/// サーバー設定が無い/失敗しても UI は待たせない）。
#[tauri::command]
pub async fn mail_draft_discard(
    app: AppHandle,
    store: State<'_, Store>,
    id: i64,
) -> Result<(), String> {
    // サーバー削除に要る情報は、ローカル削除で消える前に読み出しておく。
    let remote = store
        .draft_remote_ref(id)
        .ok()
        .flatten()
        .and_then(|(account_id, mid)| {
            store
                .get_account_imap(account_id)
                .ok()
                .flatten()
                .map(|(imap_email, login, host, port)| (imap_email, login, host, port, mid))
        });
    // ローカルは即削除（UI 反映を待たせない）。
    store.delete_emails(&[id]).map_err(|e| e.to_string())?;
    // サーバーのコピー削除はバックグラウンドで（best-effort）。
    if let Some((imap_email, login, host, port, mid)) = remote {
        let service = app.config().identifier.clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(password) =
                keyring::Entry::new(&service, &imap_email).and_then(|e| e.get_password())
            {
                let inner = mid
                    .trim()
                    .trim_start_matches('<')
                    .trim_end_matches('>')
                    .to_string();
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    imap_sync::delete_draft_remote(&host, port, &login, &password, &inner)
                })
                .await;
            }
        });
    }
    Ok(())
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

/// タグを削除（メール/連絡先との紐づけも解除。子は親へ繰り上げ）。
#[tauri::command]
pub fn tag_delete(store: State<Store>, id: i64) -> Result<(), String> {
    store.delete_tag(id).map_err(|e| e.to_string())
}

/// タグの親を設定（フォルダ整理。parent=None でルートへ）。
#[tauri::command]
pub fn tag_set_parent(store: State<Store>, id: i64, parent: Option<i64>) -> Result<(), String> {
    store.set_tag_parent(id, parent).map_err(|e| e.to_string())
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

/// 迷惑としてマーク（学習＋隔離）。既存の一括操作規約に合わせ ids を受ける（docs/SPAM.md §7.5）。
#[tauri::command]
pub fn mail_mark_spam(store: State<Store>, ids: Vec<i64>) -> Result<(), String> {
    for id in &ids {
        if let Some(f) = store.email_spam_text(*id).map_err(|e| e.to_string())? {
            let tokens = spam::tokenize(
                f.from_address.as_deref(),
                f.subject.as_deref(),
                &f.body,
                f.auth_result.as_deref(),
                f.list_id.as_deref(),
            );
            store
                .spam_learn(*id, &tokens, true)
                .map_err(|e| e.to_string())?;
        }
    }
    store.set_emails_junk(&ids, true).map_err(|e| e.to_string())
}

/// 非迷惑に戻す（学習＋隔離解除）。誤検知リカバリ（§8.4）から呼ぶ。
#[tauri::command]
pub fn mail_mark_not_spam(store: State<Store>, ids: Vec<i64>) -> Result<(), String> {
    for id in &ids {
        if let Some(f) = store.email_spam_text(*id).map_err(|e| e.to_string())? {
            let tokens = spam::tokenize(
                f.from_address.as_deref(),
                f.subject.as_deref(),
                &f.body,
                f.auth_result.as_deref(),
                f.list_id.as_deref(),
            );
            store
                .spam_learn(*id, &tokens, false)
                .map_err(|e| e.to_string())?;
        }
    }
    store
        .set_emails_junk(&ids, false)
        .map_err(|e| e.to_string())
}

/// メールの迷惑スコアを算出して保存し、判定（バンド・根拠）を返す（§7.5）。
/// 迷惑判定が無効（spam.enabled=false）なら中立を返す。しきい値はユーザー設定から読む（§9）。
/// 隔離（is_junk）は自動では変えず、手動マークを優先する（§8.3）。
#[tauri::command]
pub fn spam_score(store: State<Store>, id: i64) -> Result<SpamVerdict, String> {
    let settings = store.spam_settings().map_err(|e| e.to_string())?;
    // 迷惑判定が無効なら中立（clean）を返し、スコアも保存しない（§9.1 spam.enabled）。
    if !settings.enabled {
        return Ok(SpamVerdict {
            score: 0.0,
            band: "clean".to_string(),
            top_tokens: Vec::new(),
        });
    }
    let f = store
        .email_spam_text(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "メールが見つかりません".to_string())?;
    let tokens = spam::tokenize(
        f.from_address.as_deref(),
        f.subject.as_deref(),
        &f.body,
        f.auth_result.as_deref(),
        f.list_id.as_deref(),
    );
    let counts = store
        .spam_token_counts(&tokens)
        .map_err(|e| e.to_string())?;
    let (n_spam, n_ham) = store.spam_totals().map_err(|e| e.to_string())?;
    let (score, top_tokens) = spam::classifier::score(&counts, &tokens, n_spam, n_ham);
    store.set_spam_score(id, score).map_err(|e| e.to_string())?;
    let band = spam::band(score, settings.threshold_low, settings.threshold_high);
    Ok(SpamVerdict {
        score,
        band: band.to_string(),
        top_tokens,
    })
}

/// 迷惑メール設定を取得する（未設定キーは既定値で補完。docs/SPAM.md §9）。
#[tauri::command]
pub fn spam_settings_get(store: State<Store>) -> Result<SpamSettings, String> {
    store.spam_settings().map_err(|e| e.to_string())
}

/// 迷惑メール設定を保存する。しきい値は 0..1・low<=high に正規化してから保存する。
#[tauri::command]
pub fn spam_settings_set(store: State<Store>, settings: SpamSettings) -> Result<(), String> {
    let low = settings.threshold_low.clamp(0.0, 1.0);
    let high = settings.threshold_high.clamp(0.0, 1.0);
    let (low, high) = if low <= high {
        (low, high)
    } else {
        (high, low)
    };
    let normalized = SpamSettings {
        enabled: settings.enabled,
        threshold_low: low,
        threshold_high: high,
    };
    store
        .set_spam_settings(&normalized)
        .map_err(|e| e.to_string())
}

/// 連絡先一覧（`query` で名前/よみ/メール/組織を絞り込み、`groups` のいずれかのタグで絞り込み）。
#[tauri::command]
pub fn contact_list(
    store: State<Store>,
    query: Option<String>,
    groups: Option<Vec<i64>>,
    include_deleted: Option<bool>,
) -> Result<Vec<ContactSummary>, String> {
    store
        .list_contacts(
            query.as_deref(),
            &groups.unwrap_or_default(),
            include_deleted.unwrap_or(false),
        )
        .map_err(|e| e.to_string())
}

/// 単一の連絡先を取得。
#[tauri::command]
pub fn contact_get(store: State<Store>, id: i64) -> Result<ContactSummary, String> {
    store.get_contact(id).map_err(|e| e.to_string())
}

/// 指定メールアドレスを持つ連絡先を返す（非削除）。メールの ＋/編集 切替・重複数表示に使う。
#[tauri::command]
pub fn contact_lookup_email(
    store: State<Store>,
    email: String,
) -> Result<Vec<ContactSummary>, String> {
    store
        .lookup_contacts_by_email(&email)
        .map_err(|e| e.to_string())
}

/// メール作成の宛先オートコンプリート候補（住所録＋過去のやり取り相手）。
/// docs/RECIPIENT_AUTOCOMPLETE.md
#[tauri::command]
pub fn recipient_suggest(
    store: State<Store>,
    query: String,
    limit: i64,
) -> Result<Vec<RecipientSuggestion>, String> {
    store
        .suggest_recipients(&query, limit)
        .map_err(|e| e.to_string())
}

/// 連絡先を作成または更新（確定後の行を返す）。`input.id` が無ければ新規。
#[tauri::command]
pub fn contact_upsert(store: State<Store>, input: ContactInput) -> Result<ContactSummary, String> {
    if input.display_name.trim().is_empty() {
        return Err("名前を入力してください".to_string());
    }
    store.upsert_contact(&input).map_err(|e| e.to_string())
}

/// 連絡先を論理削除（ゴミ箱へ。保持期間後に完全削除）。
#[tauri::command]
pub fn contact_delete(store: State<Store>, id: i64) -> Result<(), String> {
    store.delete_contact(id).map_err(|e| e.to_string())
}

/// 論理削除した連絡先を復元。
#[tauri::command]
pub fn contact_restore(store: State<Store>, id: i64) -> Result<(), String> {
    store.restore_contact(id).map_err(|e| e.to_string())
}

/// 連絡先グループ一覧（所属件数つき）。
#[tauri::command]
pub fn contact_group_list(store: State<Store>) -> Result<Vec<ContactGroupSummary>, String> {
    store.list_contact_groups().map_err(|e| e.to_string())
}

/// 組織一覧（所属件数つき）。`query` があれば名前で部分一致。組織コンボボックス用。
#[tauri::command]
pub fn organization_list(
    store: State<Store>,
    query: Option<String>,
    include_deleted: Option<bool>,
) -> Result<Vec<OrganizationSummary>, String> {
    store
        .list_organizations(query.as_deref(), include_deleted.unwrap_or(false))
        .map_err(|e| e.to_string())
}

/// 組織の詳細（所属連絡先＋共有アドレスを件数つきで）。住所録の「組織」タブ用。
#[tauri::command]
pub fn organization_detail(store: State<Store>, id: i64) -> Result<OrganizationDetail, String> {
    store.organization_detail(id).map_err(|e| e.to_string())
}

/// 組織を論理削除する。所属している連絡先があるときは削除しない（安全側）。
#[tauri::command]
pub fn organization_delete(store: State<Store>, id: i64) -> Result<(), String> {
    if store.delete_organization(id).map_err(|e| e.to_string())? {
        Ok(())
    } else {
        Err("所属している連絡先がある組織は削除できません".to_string())
    }
}

/// 論理削除した組織を復元。
#[tauri::command]
pub fn organization_restore(store: State<Store>, id: i64) -> Result<(), String> {
    store.restore_organization(id).map_err(|e| e.to_string())
}

/// ゴミ箱の保持日数を取得（既定 7 日）。
#[tauri::command]
pub fn trash_retention_get(store: State<Store>) -> Result<i64, String> {
    store.trash_retention_days().map_err(|e| e.to_string())
}

/// ゴミ箱の保持日数を保存。
#[tauri::command]
pub fn trash_retention_set(store: State<Store>, days: i64) -> Result<(), String> {
    store.set_trash_retention_days(days).map_err(|e| e.to_string())
}

/// 保持期間を過ぎたゴミ箱を今すぐ完全削除する（設定変更後などに呼べる）。
#[tauri::command]
pub fn trash_purge(store: State<Store>) -> Result<(), String> {
    let days = store.trash_retention_days().map_err(|e| e.to_string())?;
    store.purge_expired_trash(days).map_err(|e| e.to_string())
}

/// 組織を作成/編集する（名前・メモ）。id 指定で更新、無ければ新規。
#[tauri::command]
pub fn organization_upsert(
    store: State<Store>,
    id: Option<i64>,
    name: String,
    name_kana: Option<String>,
    note: Option<String>,
) -> Result<OrganizationSummary, String> {
    if name.trim().is_empty() {
        return Err("組織名を入力してください".to_string());
    }
    store
        .upsert_organization(id, &name, name_kana.as_deref(), note.as_deref())
        .map_err(|e| e.to_string())
}

/// 組織名の重複候補（正規化名で束ねたグループ）を返す。組織の統一 UI 用。
#[tauri::command]
pub fn organization_find_duplicates(
    store: State<Store>,
) -> Result<Vec<OrgDuplicateGroup>, String> {
    store
        .find_organization_duplicates()
        .map_err(|e| e.to_string())
}

/// 複数の組織を 1 件（keep_id）に統一し、統一後の組織を返す。`name` が統一名。
#[tauri::command]
pub fn organization_merge(
    store: State<Store>,
    keep_id: i64,
    drop_ids: Vec<i64>,
    name: String,
) -> Result<OrganizationSummary, String> {
    if name.trim().is_empty() {
        return Err("組織名を入力してください".to_string());
    }
    store
        .merge_organizations(keep_id, &drop_ids, &name)
        .map_err(|e| e.to_string())
}

/// 連絡先ファイルを取り込む。拡張子で判定し vCard(.vcf) と Google CSV(.csv) に対応。
/// 完全重複は取り込み時に集約し、件数レポートを返す。
#[tauri::command]
pub fn contact_import(store: State<Store>, path: String) -> Result<ImportReport, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("ファイルを読めません: {e}"))?;
    let parsed = if path.to_lowercase().ends_with(".csv") {
        gcsv::parse(&text)
    } else {
        vcard::parse(&text)
    };
    store.import_contacts(&parsed).map_err(|e| e.to_string())
}

/// 重複候補（同一の正規化表示名でまとめたグループ）を返す。整理 UI 用。
#[tauri::command]
pub fn contact_find_duplicates(store: State<Store>) -> Result<Vec<DuplicateGroup>, String> {
    store.find_duplicate_groups().map_err(|e| e.to_string())
}

/// 入力（メール/電話/FAX/氏名）に一致する既存連絡先を返す。新規登録前チェック・
/// 編集中の赤字警告・メールからの＋追加で使う。`exclude_id` は編集中の自分自身を除く。
#[tauri::command]
pub fn contact_find_matches(
    store: State<Store>,
    emails: Vec<String>,
    phones: Vec<String>,
    display_name: Option<String>,
    exclude_id: Option<i64>,
) -> Result<Vec<ContactMatch>, String> {
    store
        .find_contact_matches(&emails, &phones, display_name.as_deref(), exclude_id)
        .map_err(|e| e.to_string())
}

/// 複数の連絡先を 1 件（keep_id）に統合し、統合後の連絡先を返す。
#[tauri::command]
pub fn contact_merge(
    store: State<Store>,
    keep_id: i64,
    drop_ids: Vec<i64>,
) -> Result<ContactSummary, String> {
    if drop_ids.is_empty() {
        return store.get_contact(keep_id).map_err(|e| e.to_string());
    }
    store
        .merge_contacts(keep_id, &drop_ids)
        .map_err(|e| e.to_string())
}

/// グリーン／警告ドメインの一覧（管理タブ用。住所録由来の自動グリーンも含む）。
#[tauri::command]
pub fn green_domain_list(store: State<Store>) -> Result<Vec<GreenDomainEntry>, String> {
    store.list_green_domains().map_err(|e| e.to_string())
}

/// ドメインをグリーンに認定（警告から外し、手動グリーンに登録）。
#[tauri::command]
pub fn green_domain_add(
    store: State<Store>,
    domain: String,
    note: Option<String>,
) -> Result<(), String> {
    store
        .add_green_domain(&domain, note.as_deref())
        .map_err(|e| e.to_string())
}

/// ドメインを警告（グリーン解除）にする。住所録由来の自動グリーンを上書き除外し、再登録を防ぐ。
#[tauri::command]
pub fn green_domain_warn(
    store: State<Store>,
    domain: String,
    note: Option<String>,
) -> Result<(), String> {
    store
        .warn_green_domain(&domain, note.as_deref())
        .map_err(|e| e.to_string())
}

/// ドメインを中立に戻す（グリーン・警告の両方から外す）。
#[tauri::command]
pub fn green_domain_clear(store: State<Store>, domain: String) -> Result<(), String> {
    store.clear_green_domain(&domain).map_err(|e| e.to_string())
}

/// 単一アドレスがグリーンか（詳細画面のバッジ・ボタン用）。
#[tauri::command]
pub fn green_address_check(store: State<Store>, address: String) -> Result<bool, String> {
    store.address_green(&address).map_err(|e| e.to_string())
}

/// 現在のデータ保存先と使用量を返す。
#[tauri::command]
pub fn data_location(app: AppHandle, store: State<Store>) -> Result<DataLocation, String> {
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(build_data_location(&base, &store))
}

/// データ（mail.db + 添付）を指定フォルダへ移動する（再起動不要）。
#[tauri::command]
pub fn data_relocate(
    app: AppHandle,
    store: State<Store>,
    dir: String,
) -> Result<DataLocation, String> {
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let pointer = datadir::pointer_file(&base);
    store.relocate(std::path::Path::new(&dir), &pointer)?;
    Ok(build_data_location(&base, &store))
}

/// データを既定の場所に戻す。
#[tauri::command]
pub fn data_reset_location(app: AppHandle, store: State<Store>) -> Result<DataLocation, String> {
    let base = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let default_dir = datadir::default_data_dir(&base);
    let pointer = datadir::pointer_file(&base);
    if store.data_dir() != default_dir {
        store.relocate(&default_dir, &pointer)?;
    }
    // ポインタを消して「既定」に戻す（既定と同じ場所なので解決結果は変わらない）。
    let _ = std::fs::remove_file(&pointer);
    Ok(build_data_location(&base, &store))
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

/// 1通の全文をサーバーから再取得して本文キャッシュを復元する（要約保存の解除）。
/// emails.uid で該当メッセージだけを取り直すので、アカウント全体の再同期は不要。
/// 復元後の本文（body_compacted=false）を返す。
#[tauri::command]
pub async fn mail_refetch(
    app: AppHandle,
    store: State<'_, Store>,
    id: i64,
) -> Result<MailDetail, String> {
    let (account_id, uid) = store
        .email_refetch_info(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "メールが見つかりません".to_string())?;
    let uid = uid.ok_or_else(|| {
        "再取得に必要な情報がありません。アカウントを再同期してください。".to_string()
    })?;

    let (email, login_user, host, port) = store
        .get_account_imap(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "アカウントが見つかりません".to_string())?;
    let service = app.config().identifier.clone();
    let password = keyring::Entry::new(&service, &email)
        .and_then(|e| e.get_password())
        .map_err(|e| format!("資格情報を取得できません: {e}"))?;

    let parsed = tauri::async_runtime::spawn_blocking(move || {
        imap_sync::fetch_message(&host, port, &login_user, &password, uid as u32)
    })
    .await
    .map_err(|e| e.to_string())??;

    store
        .update_email_body(
            id,
            parsed.body_plain.as_deref(),
            parsed.clean_body.as_deref(),
            parsed.body_html.as_deref(),
        )
        .map_err(|e| e.to_string())?;

    store
        .get_email(id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "メールが見つかりません".to_string())
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

    // 保存先: <mail.db と同じフォルダ>/attachments/<attachment_id>/<filename>。
    // DB パス（開発ビルドはワークツリー別）から導出し、DB と添付キャッシュを常に同じ場所に置く。
    let dir = store
        .data_dir()
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
    // ダウンロード後に保持ポリシーを適用（best-effort）。
    if let Ok(Some(info)) = store.attachment_fetch_info(attachment_id) {
        let _ = store.apply_retention(info.account_id);
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

/// ストレージ最適化: 保持ポリシー（期間ベースの3ティア＋容量上限の保険）を適用する。
/// 古い添付ファイルを削除し、さらに古い本文を要約保存に落とす。メタは常に残す。
#[tauri::command]
pub fn storage_optimize(store: State<Store>, account_id: i64) -> Result<RetentionReport, String> {
    store.apply_retention(account_id).map_err(|e| e.to_string())
}

/// 点検つき再取り込み: 同期状態をリセットして取り込み範囲をフル再取得し、
/// 既存メールに uid・添付メタを埋め戻す（古いメールの添付を後付け対応）。
#[tauri::command]
pub async fn mail_resync(
    app: AppHandle,
    store: State<'_, Store>,
    control: State<'_, SyncControl>,
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
    let db_path = store.path();
    let cancel = control.begin(account_id);

    let app_ev = app.clone();
    let cancel_task = cancel.clone();
    let out = tauri::async_runtime::spawn_blocking(move || {
        let progress = |folder: &str, current: i32, total: i32| {
            let _ = app_ev.emit(
                "sync:progress",
                SyncProgress {
                    folder: folder.to_string(),
                    current,
                    total,
                },
            );
        };
        imap_sync::sync_account(
            &db_path,
            account_id,
            &host,
            port,
            &login_user,
            &password,
            &progress,
            &cancel_task,
        )
    })
    .await;
    control.end(account_id);
    out.map_err(|e| e.to_string())?
}

/// ファイル名を保存に安全な形へ正規化する（パス区切り・禁止文字を除去）。
/// DataLocation を組み立てる（現在の保存先・既定かどうか・使用量）。
fn build_data_location(base: &std::path::Path, store: &Store) -> DataLocation {
    let dir = store.data_dir();
    let db = dir.join("mail.db");
    let db_bytes =
        file_len(&db) + file_len(&dir.join("mail.db-wal")) + file_len(&dir.join("mail.db-shm"));
    let attachments_bytes = dir_size(&dir.join("attachments"));
    DataLocation {
        dir: dir.to_string_lossy().to_string(),
        is_default: !datadir::pointer_file(base).exists(),
        db_bytes: db_bytes as f64,
        attachments_bytes: attachments_bytes as f64,
    }
}

fn file_len(p: &std::path::Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// ディレクトリ配下の合計バイト（再帰）。存在しなければ 0。
fn dir_size(p: &std::path::Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(p) else {
        return 0;
    };
    let mut total = 0u64;
    for e in entries.flatten() {
        match e.file_type() {
            Ok(t) if t.is_dir() => total += dir_size(&e.path()),
            Ok(_) => total += e.metadata().map(|m| m.len()).unwrap_or(0),
            Err(_) => {}
        }
    }
    total
}

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
