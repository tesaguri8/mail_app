use crate::models::{
    AccountInput, AccountSummary, AppInfo, AttachmentMeta, AttachmentSummary, AutoconfigResult,
    AttendeeInput, CalendarInput, CalendarSummary, ContactGroupSummary, ContactInput, ContactMatch,
    ContactSummary, DataLocation, DbInfo, DraftContent, DraftInput, DuplicateGroup, EventAttendee,
    EventInput, EventSummary, GcalCredentialsStatus, GcalSyncResult, GoogleAccount, GreenDomainEntry,
    HomeUnreadCounts, IcsImportReport, ImportReport, MailDetail,
    MailSummary, OrgDuplicateGroup, OrganizationDetail, OrganizationSummary, RebuildAction,
    RebuildPlan, RecipientSuggestion, RemoteImage, RetentionReport, SendInput,
    ServerAccountSummary, SignatureSummary, SpamSenderConflict, SpamSettings, SpamVerdict,
    StorageInfo, SyncProgress,
    SyncResult, TagSummary, ThreadListItem, ThreadView,
};
use crate::services::autoconfig;
use crate::services::datadir;
use crate::services::dataver;
use crate::services::gcal;
use crate::services::gcsv;
use crate::services::imap_sync;
use crate::services::media;
use crate::services::smtp;
use crate::services::spam;
use crate::services::store::{NewAccount, NewAttachment, NewServerAccount, PurgeRef, Store};
use crate::services::vcard;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

/// 実行中の同期のキャンセルフラグを account_id ごとに管理する（中断ボタン用）。
#[derive(Default)]
pub struct SyncControl(Mutex<HashMap<i64, Arc<AtomicBool>>>);

impl SyncControl {
    /// 同期開始を試みる。そのアカウントが既に同期中なら None（＝二重実行を防ぐ）。
    /// 空きがあればキャンセルフラグを登録して返す。
    fn try_begin(&self, account_id: i64) -> Option<Arc<AtomicBool>> {
        let mut map = self.0.lock().unwrap();
        if map.contains_key(&account_id) {
            return None; // 既に同期/再取り込みが進行中
        }
        let flag = Arc::new(AtomicBool::new(false));
        map.insert(account_id, flag.clone());
        Some(flag)
    }
    /// 同期終了時に取り除く。
    fn end(&self, account_id: i64) {
        self.0.lock().unwrap().remove(&account_id);
    }
    /// そのアカウントが同期中か。
    fn is_running(&self, account_id: i64) -> bool {
        self.0.lock().unwrap().contains_key(&account_id)
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

/// アカウントごとに 1 本の IMAP セッションを保持して使い回す接続プール。
/// 毎回の接続＋ログインを避けて体感を滑らかにする（Thunderbird 等と同じ持続接続方針）。
#[derive(Default)]
pub struct ImapPool(Mutex<HashMap<i64, Arc<Mutex<Option<imap_sync::ImapSession>>>>>);

impl ImapPool {
    /// アカウントの接続スロット（無ければ作る）を返す。
    fn slot(&self, account_id: i64) -> Arc<Mutex<Option<imap_sync::ImapSession>>> {
        self.0
            .lock()
            .unwrap()
            .entry(account_id)
            .or_default()
            .clone()
    }
    /// アカウントの接続を破棄する（アカウント削除時など）。
    fn evict(&self, account_id: i64) {
        self.0.lock().unwrap().remove(&account_id);
    }
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
        server_total_count: 0,
    })
}

/// 登録済みアカウント一覧（資格情報は含めない）。
#[tauri::command]
pub fn account_list(store: State<Store>) -> Result<Vec<AccountSummary>, String> {
    store.list_accounts().map_err(|e| e.to_string())
}

/// ホームのアカウント別バッジ用: inbox の未読数をカテゴリ別（全体/グリーン/住所録/お気に入り）に返す。
#[tauri::command]
pub fn home_unread_counts(store: State<Store>) -> Result<Vec<HomeUnreadCounts>, String> {
    store.home_unread_counts().map_err(|e| e.to_string())
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
    pool: State<'_, ImapPool>,
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
    // このアカウントが既に同期中なら二重実行しない（自動同期と手動/再取り込みの衝突で
    // 進捗が上下してブレるのを防ぐ）。取得 0 件のスキップ結果を返す。
    let Some(cancel) = control.try_begin(account_id) else {
        return Ok(SyncResult {
            fetched: 0,
            stored: 0,
            backfilled: 0,
        });
    };

    // 進捗を "sync:progress" イベントで UI に通知する（フォルダ / 取得済み / 予定）。
    let app_ev = app.clone();
    let cancel_task = cancel.clone();
    // アカウントの接続スロット（使い回し）。
    let session_slot = pool.slot(account_id);
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
        let res = imap_sync::sync_account(
            &db_path,
            account_id,
            &host,
            port,
            &login_user,
            &password,
            &progress,
            &cancel_task,
            &session_slot,
        );
        // 取り込み後のローカル加工（スレッド割当・代表フラグ）は「このブロッキングスレッド上で」
        // 実行する。async ランタイム上で同期的に走らせると、その間 Tauri の IPC 配送が止まり、
        // 一覧取得の呼び出しまで待たされる（起動直後に一覧が数十秒出ない原因だった）。
        // 別接続・小分けなので UI 用接続はブロックしない（docs/THREADING.md §5）。
        if res.is_ok() {
            if let Err(e) = crate::services::store::process_pending_at(&db_path, account_id) {
                log::warn!("取り込み後の加工に失敗: {e}");
            }
        }
        res
    })
    .await;
    // JoinError（タスクパニック）でも同期枠を必ず解放してから伝播する
    // （解放漏れは以後の同期が全てスキップされる原因になるため）。
    control.end(account_id);
    let result = result.map_err(|e| e.to_string())?;
    if result.is_ok() {
        // 保持ポリシーを適用（古い添付の削除・本文の要約保存・容量保険）。best-effort。
        let _ = store.apply_retention(account_id);
    }
    result
}

/// プレーン本文から最小限の HTML を作る（エスケープ＋改行を <br> 化）。
/// multipart/alternative の HTML パート用。改行は CSS(pre-wrap) ではなく <br> で表現する
/// （テキスト主体の安全描画でも確実に改行されるように）。リンク化などは後続。
/// プレーン本文を HTML エスケープ＋改行を <br> 化した断片にする（本文/引用で共有）。
fn plain_to_html_fragment(plain: &str) -> String {
    plain
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace("\r\n", "\n")
        .replace('\n', "<br>\n")
}

fn plain_to_html(plain: &str) -> String {
    compose_html(plain, None)
}

/// 送信用 HTML 本文を組み立てる。新規本文（プレーン→HTML）の後ろに、指定があれば
/// サニタイズ済みの HTML 引用（オリジナル HTML の blockquote 等）をそのまま足す。
fn compose_html(new_body_plain: &str, quoted_html: Option<&str>) -> String {
    let body = plain_to_html_fragment(new_body_plain);
    let quote = quoted_html.unwrap_or("");
    format!(
        "<!DOCTYPE html><html><body>\
         <div style=\"font-family:sans-serif;font-size:14px;line-height:1.5\">{body}{quote}</div>\
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

    // プレーン本文＝新規本文＋プレーン引用。HTML 本文＝新規本文の HTML に、あれば
    // サニタイズ済みの HTML 引用（オリジナル HTML の blockquote）をそのまま足す（B 案）。
    // HTML 引用が無い（新規メール・元が text/plain）ときは従来どおり全プレーンを HTML 化する。
    let quoted_plain = input.quoted_plain.as_deref().unwrap_or("");
    let body_plain = format!("{}{}", input.body, quoted_plain);
    let body_html = match input.quoted_html.as_deref().filter(|s| !s.trim().is_empty()) {
        Some(qh) => compose_html(&input.body, Some(qh)),
        None => plain_to_html(&body_plain),
    };
    // References チェーン: フロント指定が無ければ、返信元（in_reply_to）から親の祖先連鎖を組む。
    let references = match input.references.filter(|s| !s.trim().is_empty()) {
        Some(r) => Some(r),
        None => store
            .references_chain_for(input.in_reply_to.as_deref())
            .map_err(|e| e.to_string())?,
    };
    // 添付を読み込む（ローカルパスから Rust が直接読む）。合計サイズを検証する。
    let mut attachments: Vec<(String, Vec<u8>, String)> = Vec::new();
    let mut total: u64 = 0;
    for path in &input.attachments {
        let p = std::path::Path::new(path);
        let bytes = std::fs::read(p).map_err(|e| format!("添付を読み込めません（{path}）: {e}"))?;
        total += bytes.len() as u64;
        if total > MAX_ATTACHMENT_TOTAL {
            return Err("添付の合計が25MBを超えています。ファイルを減らしてください".to_string());
        }
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("attachment")
            .to_string();
        let ct = guess_content_type(&name).to_string();
        attachments.push((name, bytes, ct));
    }

    // 自分宛（自分の口座アドレスが宛先に含まれる）なら「本物の自分から」検証マークを付ける。
    // Message-ID を自前で採番し、その HMAC を X-Rondine-Self ヘッダに載せる（docs/SPAM.md）。
    let self_addr = acct.email.trim().to_lowercase();
    let norm_addr = |s: &str| -> String {
        let s = s.trim();
        let core = match (s.rfind('<'), s.rfind('>')) {
            (Some(l), Some(r)) if l < r => &s[l + 1..r],
            _ => s,
        };
        core.trim().to_lowercase()
    };
    let is_self_send = to
        .iter()
        .chain(cc.iter())
        .chain(bcc.iter())
        .any(|a| norm_addr(a) == self_addr);
    let (self_message_id, self_mark) = if is_self_send && !self_addr.is_empty() {
        match store.get_or_create_self_secret(input.account_id as i64) {
            Ok(secret) => {
                let domain = self_addr.split('@').nth(1).unwrap_or("localhost");
                let mut rnd = [0u8; 8];
                let _ = getrandom::getrandom(&mut rnd);
                let rnd_hex: String = rnd.iter().map(|b| format!("{b:02x}")).collect();
                let nanos = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                let mid = format!("{nanos}.{rnd_hex}@{domain}");
                let mark = crate::services::selfmark::compute_mark(&secret, &mid);
                (Some(mid), mark)
            }
            Err(_) => (None, None),
        }
    } else {
        (None, None)
    };

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
        body_plain,
        body_html: Some(body_html),
        in_reply_to: input.in_reply_to,
        references,
        // 自分宛は自前採番の Message-ID（HMAC 検証のため）。それ以外は lettre の自動採番でよい。
        message_id: self_message_id,
        attachments,
        self_mark,
    };

    // 送信メッセージを 1 度だけ組み立て、SMTP 送信と Sent 保存で共有する。
    let email = smtp::build_message(&message)?;
    let raw = email.formatted(); // Sent へ APPEND する RFC822 バイト列（Bcc は含まれない）

    tauri::async_runtime::spawn_blocking(move || smtp::send(&config, &email))
        .await
        .map_err(|e| e.to_string())??;

    // ドロップ由来の一時添付は送信後に掃除する（picker で選んだ実ファイルは消さない）。
    let stage_root = drop_stage_root();
    for path in &input.attachments {
        let p = std::path::Path::new(path);
        if p.starts_with(&stage_root) {
            let _ = std::fs::remove_file(p);
            if let Some(parent) = p.parent() {
                let _ = std::fs::remove_dir(parent); // 空になった一意フォルダを消す（best-effort）
            }
        }
    }

    // 送信成功後、送信控えを IMAP の Sent フォルダへ保存する（best-effort）。
    // 失敗しても送信自体は成功しているので、警告ログにとどめてエラーにはしない。
    // ただし Gmail 等はサーバーが送信時に自動で控えを保存するため、APPEND すると二重に
    // なる。該当プロバイダでは APPEND をスキップし、サーバー保存分を次回同期で取り込む。
    //
    // Sent への保存は本文をもう一度アップロードするので、添付つきの大きなメールでは
    // 送信の完了体感が大きく遅くなる。ここでは待たずにバックグラウンドで走らせ、SMTP が
    // 受理した時点で送信完了とする（次回同期でも Sent は取り込まれる）。
    if let Ok(Some((_email, login, host, port))) = store.get_account_imap(input.account_id as i64) {
        if imap_sync::server_saves_sent_copy(&host) {
            log::info!("Sent への APPEND をスキップ（サーバーが自動保存: {host}）");
        } else {
            tauri::async_runtime::spawn(async move {
                let res = tauri::async_runtime::spawn_blocking(move || {
                    imap_sync::append_to_sent(&host, port, &login, &password, &raw)
                })
                .await;
                match res {
                    Ok(Ok(())) => {}
                    Ok(Err(e)) => log::warn!("送信は成功、Sent への保存に失敗: {e}"),
                    Err(e) => log::warn!("Sent 保存タスクに失敗: {e}"),
                }
            });
        }
    }
    Ok(())
}

/// 添付の合計サイズ上限（多くの SMTP サーバーの制限に合わせて 25MB）。
const MAX_ATTACHMENT_TOTAL: u64 = 25 * 1024 * 1024;

/// 拡張子から content-type を推定する（既定は application/octet-stream）。
fn guess_content_type(name: &str) -> &'static str {
    let ext = name.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "pdf" => "application/pdf",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "txt" | "log" => "text/plain",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "md" => "text/markdown",
        "json" => "application/json",
        "xml" => "application/xml",
        "zip" => "application/zip",
        "gz" | "tgz" => "application/gzip",
        "7z" => "application/x-7z-compressed",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "mov" => "video/quicktime",
        _ => "application/octet-stream",
    }
}

/// 添付候補ファイルのメタ（名前・サイズ）を返す。作成画面の一覧表示・事前検証に使う。
#[tauri::command]
pub fn attachment_meta(paths: Vec<String>) -> Result<Vec<AttachmentMeta>, String> {
    let mut out = Vec::with_capacity(paths.len());
    for path in paths {
        let p = std::path::Path::new(&path);
        let meta =
            std::fs::metadata(p).map_err(|e| format!("ファイルを確認できません（{path}）: {e}"))?;
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("attachment")
            .to_string();
        out.push(AttachmentMeta {
            path: path.clone(),
            name,
            size: meta.len() as i64,
        });
    }
    Ok(out)
}

/// ドロップされた添付を書き出す一時フォルダの土台（OS の一時ディレクトリ配下）。
/// 送信後にこの配下のファイルは掃除する（picker で選んだ実ファイルは消さない）。
fn drop_stage_root() -> std::path::PathBuf {
    std::env::temp_dir().join("rondine-drop-attachments")
}

static STAGE_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// 一時ファイル名の衝突を避ける一意 ID（ナノ秒＋連番）。
fn stage_uid() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let seq = STAGE_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!("{nanos}-{seq}")
}

/// パーセントエンコード（%XX）を素朴にデコードする（ヘッダで渡すファイル名の復元用）。
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// ドラッグ＆ドロップされたファイルの中身を一時ファイルへ書き出し、追加用メタを返す。
/// ブラウザ側からはパスが取れないため、本体を生バイト、ファイル名をヘッダ x-name
/// （percent-encoded）で受け取る。送信時にこのパスを読み込んで MIME に同梱する。
#[tauri::command]
pub fn attachment_stage(request: tauri::ipc::Request<'_>) -> Result<AttachmentMeta, String> {
    let name_enc = request
        .headers()
        .get("x-name")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("attachment");
    let name = {
        // パス区切りを潰して一時フォルダの外に出られないようにする。
        let n = percent_decode(name_enc).replace(['/', '\\'], "_");
        let n = n.trim().to_string();
        if n.is_empty() {
            "attachment".to_string()
        } else {
            n
        }
    };
    let bytes = match request.body() {
        tauri::ipc::InvokeBody::Raw(b) => b,
        _ => return Err("添付データの受け取りに失敗しました".to_string()),
    };
    if bytes.len() as u64 > MAX_ATTACHMENT_TOTAL {
        return Err("添付の合計が25MBを超えています。ファイルを減らしてください".to_string());
    }
    let dir = drop_stage_root().join(stage_uid());
    std::fs::create_dir_all(&dir).map_err(|e| format!("一時フォルダを作成できません: {e}"))?;
    let path = dir.join(&name);
    std::fs::write(&path, bytes).map_err(|e| format!("添付を書き出せません: {e}"))?;
    Ok(AttachmentMeta {
        path: path.to_string_lossy().into_owned(),
        name,
        size: bytes.len() as i64,
    })
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

/// フォルダ内のスレッド総数（一覧の「表示 X / 全 Y」表示用）。`account_id` が None なら全アカウント。
#[tauri::command]
pub fn thread_count(
    store: State<Store>,
    account_id: Option<i64>,
    folder: String,
) -> Result<i64, String> {
    store
        .thread_count(account_id, &folder)
        .map_err(|e| e.to_string())
}

/// スレッド単位のメール一覧（代表＝フォルダ内最新＋件数バッジ）。docs/THREADING.md §5。
#[tauri::command]
pub fn thread_list(
    store: State<Store>,
    account_id: Option<i64>,
    folder: String,
    limit: i64,
    offset: i64,
) -> Result<Vec<ThreadListItem>, String> {
    store
        .list_threads(account_id, &folder, limit, offset)
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

/// 複数メールを一括削除（完全削除。ゴミ箱内からの削除等）。
/// ローカルは即削除し、サーバー上のコピーも「Trash へ移動→完全削除」でバックグラウンド反映する（best-effort）。
#[tauri::command]
pub async fn mail_delete(
    app: AppHandle,
    store: State<'_, Store>,
    ids: Vec<i64>,
) -> Result<(), String> {
    // サーバー反映に要る情報は、ローカル削除で消える前に読み出しておく。
    let refs = store.purge_refs(&ids).map_err(|e| e.to_string())?;
    store.delete_emails(&ids).map_err(|e| e.to_string())?;
    spawn_remote_purge(&app, &store, refs);
    Ok(())
}

/// ローカルで完全削除したメールを、サーバー上でも「Trash へ移動→完全削除」する（best-effort・非同期）。
/// アカウントごとに資格情報を取り出してからバックグラウンドで実行する（UI は待たせない）。
fn spawn_remote_purge(app: &AppHandle, store: &Store, refs: Vec<PurgeRef>) {
    if refs.is_empty() {
        return;
    }
    let identifier = app.config().identifier.clone();
    // アカウント別に PurgeItem（Message-ID は山括弧を外す）へまとめる。
    let mut by_acct: HashMap<i64, Vec<imap_sync::PurgeItem>> = HashMap::new();
    for r in refs {
        let inner = r
            .message_id
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .to_string();
        if inner.is_empty() {
            continue;
        }
        by_acct.entry(r.account_id).or_default().push(imap_sync::PurgeItem {
            source_tag: r.source_tag,
            message_id_inner: inner,
        });
    }
    for (account_id, items) in by_acct {
        // 資格情報の取得は削除前後どちらでもよいが、spawn へ移す前に owned にしておく。
        let Some((email, login, host, port)) = store.get_account_imap(account_id).ok().flatten()
        else {
            continue;
        };
        let identifier = identifier.clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(password) =
                keyring::Entry::new(&identifier, &email).and_then(|e| e.get_password())
            {
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    imap_sync::purge_emails_remote(&host, port, &login, &password, &items)
                })
                .await;
            }
        });
    }
}

/// 複数メールをゴミ箱（trash フォルダ）へ移動する（既定の削除操作）。復元可能。
/// 下書き（drafts）だけは、サーバーの Drafts フォルダのコピーも背景で削除する。
/// 下書きは「破棄＝サーバーからも消える」が自然で、ローカルのゴミ箱に残したまま
/// サーバー Drafts に居残ると、別端末や Webメールでゴミのままになるため避ける。
/// （ゴミ箱から復元した場合は、次回の保存でサーバーへ再同期される。）
#[tauri::command]
pub async fn mail_trash(
    app: AppHandle,
    store: State<'_, Store>,
    ids: Vec<i64>,
) -> Result<(), String> {
    // 移動前に下書きのサーバー参照（account_id, Message-ID）を控える（移動後は folder が変わるため）。
    let draft_refs: Vec<PurgeRef> = store
        .purge_refs(&ids)
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|r| r.source_tag == "drafts")
        .collect();
    store.move_emails_to_trash(&ids).map_err(|e| e.to_string())?;
    spawn_remote_draft_delete(&app, &store, draft_refs);
    Ok(())
}

/// ローカルでゴミ箱へ移した/破棄した下書きについて、サーバーの Drafts フォルダのコピーを
/// バックグラウンドで削除する（best-effort・非同期。UI は待たせない）。
fn spawn_remote_draft_delete(app: &AppHandle, store: &Store, refs: Vec<PurgeRef>) {
    if refs.is_empty() {
        return;
    }
    let identifier = app.config().identifier.clone();
    for r in refs {
        let inner = r
            .message_id
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>')
            .to_string();
        if inner.is_empty() {
            continue;
        }
        let Some((email, login, host, port)) = store.get_account_imap(r.account_id).ok().flatten()
        else {
            continue;
        };
        let identifier = identifier.clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(password) =
                keyring::Entry::new(&identifier, &email).and_then(|e| e.get_password())
            {
                let _ = tauri::async_runtime::spawn_blocking(move || {
                    imap_sync::delete_draft_remote(&host, port, &login, &password, &inner)
                })
                .await;
            }
        });
    }
}

/// ゴミ箱の複数メールを元のフォルダ（prev_folder、無ければ inbox）へ復元する。
#[tauri::command]
pub fn mail_restore(store: State<Store>, ids: Vec<i64>) -> Result<(), String> {
    store
        .restore_emails_from_trash(&ids)
        .map_err(|e| e.to_string())
}

/// 指定フォルダ（trash/spam 等）を空にする。`account_id` が None なら全アカウント。削除件数を返す。
/// ローカルは即削除し、サーバー上のコピーも「Trash へ移動→完全削除」でバックグラウンド反映する（best-effort）。
#[tauri::command]
pub async fn mail_empty_folder(
    app: AppHandle,
    store: State<'_, Store>,
    account_id: Option<i64>,
    folder: String,
) -> Result<i32, String> {
    // サーバー反映に要る情報は、ローカル削除で消える前に読み出しておく。
    let refs = store
        .purge_refs_for_folder(account_id, &folder)
        .map_err(|e| e.to_string())?;
    let n = store
        .empty_folder(account_id, &folder)
        .map_err(|e| e.to_string())?;
    spawn_remote_purge(&app, &store, refs);
    Ok(n)
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
    let references = store
        .references_chain_for(draft.in_reply_to.as_deref())
        .map_err(|e| e.to_string())?;
    let message = smtp::OutgoingMessage {
        from_name: acct.display_name,
        from_email: acct.email,
        to: split_addr_list(&draft.to),
        cc: split_addr_list(&draft.cc),
        bcc: split_addr_list(&draft.bcc),
        subject: draft.subject,
        body_plain: draft.body,
        body_html: Some(body_html),
        in_reply_to: draft.in_reply_to,
        references,
        message_id: Some(message_id.clone()),
        attachments: Vec::new(), // 下書きのサーバー保存は本文のみ（添付の永続化は今回対象外）
        self_mark: None,         // 下書きには検証マークを付けない
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
/// さらに差出人アドレスを「迷惑差出人」に登録し、同アドレスの既存メールも迷惑へ移す
/// （今後の新着は挿入時に自動隔離。「このアドレスを迷惑にしたら他のメールも迷惑へ」）。
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
    store
        .set_emails_junk(&ids, true)
        .map_err(|e| e.to_string())?;
    // 差出人アドレス（自分の口座は除く）を迷惑差出人にし、同アドレスの既存メールも迷惑へ。
    for addr in store
        .spam_sender_candidates(&ids)
        .map_err(|e| e.to_string())?
    {
        store.add_spam_sender(&addr).map_err(|e| e.to_string())?;
        store
            .set_sender_junk(&addr, true)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 非迷惑に戻す（学習＋隔離解除）。誤検知リカバリ（§8.4）から呼ぶ。
/// 迷惑差出人の登録も解除し、同アドレスのメールを受信箱へ戻す（マークと対称。docs/SPAM.md）。
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
        .map_err(|e| e.to_string())?;
    // 迷惑差出人の登録を外し、同アドレスの隔離済みメールを受信箱へ戻す。
    for addr in store
        .spam_sender_candidates(&ids)
        .map_err(|e| e.to_string())?
    {
        store.remove_spam_sender(&addr).map_err(|e| e.to_string())?;
        store
            .set_sender_junk(&addr, false)
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// 迷惑差出人リストと信頼シグナル（住所録/グリーン）の矛盾を列挙する（注意喚起用）。
/// 「グリーン/連絡先なのに迷惑登録されている」誤登録をユーザーに気付かせる。
#[tauri::command]
pub fn spam_find_conflicts(store: State<Store>) -> Result<Vec<SpamSenderConflict>, String> {
    store.find_spam_sender_conflicts().map_err(|e| e.to_string())
}

/// 指定アドレスを迷惑差出人から外し、同アドレスの隔離済みメールを受信箱へ戻す（矛盾の解消）。
#[tauri::command]
pub fn spam_forgive_sender(store: State<Store>, address: String) -> Result<(), String> {
    store
        .remove_spam_sender(&address)
        .map_err(|e| e.to_string())?;
    store
        .set_sender_junk(&address, false)
        .map_err(|e| e.to_string())?;
    Ok(())
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
    store
        .set_trash_retention_days(days)
        .map_err(|e| e.to_string())
}

/// 保持期間を過ぎたゴミ箱を今すぐ完全削除する（設定変更後などに呼べる）。
#[tauri::command]
pub fn trash_purge(store: State<Store>) -> Result<(), String> {
    let days = store.trash_retention_days().map_err(|e| e.to_string())?;
    store.purge_expired_trash(days).map_err(|e| e.to_string())
}

/// メールのゴミ箱の保持日数を取得（既定 30 日。0 = 無期限）。
#[tauri::command]
pub fn mail_trash_retention_get(store: State<Store>) -> Result<i64, String> {
    store.mail_trash_retention_days().map_err(|e| e.to_string())
}

/// メールのゴミ箱の保持日数を保存（0 = 無期限）。
#[tauri::command]
pub fn mail_trash_retention_set(store: State<Store>, days: i64) -> Result<(), String> {
    store
        .set_mail_trash_retention_days(days)
        .map_err(|e| e.to_string())
}

/// 保持期間を過ぎたゴミ箱メールを今すぐ完全削除する（0 = 無期限なら何もしない）。
#[tauri::command]
pub fn mail_trash_purge(store: State<Store>) -> Result<(), String> {
    let days = store
        .mail_trash_retention_days()
        .map_err(|e| e.to_string())?;
    store
        .purge_expired_mail_trash(days)
        .map_err(|e| e.to_string())
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
pub fn organization_find_duplicates(store: State<Store>) -> Result<Vec<OrgDuplicateGroup>, String> {
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

// ─────────────────────────── カレンダー（docs/DATABASE_SCHEMA.md events） ───────────────────────────

/// 期間 [from, to)（'YYYY-MM-DD' 等の ISO 文字列）に重なる予定を開始順で返す。
/// 月/週グリッドの表示範囲を渡す。`include_deleted` が true ならゴミ箱も含める。
#[tauri::command]
pub fn event_list(
    store: State<Store>,
    from: String,
    to: String,
    include_deleted: Option<bool>,
) -> Result<Vec<EventSummary>, String> {
    store
        .list_events(&from, &to, include_deleted.unwrap_or(false))
        .map_err(|e| e.to_string())
}

/// 論理削除済みの予定のみ（ゴミ箱一覧）。
#[tauri::command]
pub fn event_list_trashed(store: State<Store>) -> Result<Vec<EventSummary>, String> {
    store.list_trashed_events().map_err(|e| e.to_string())
}

/// タイトル・メモ・場所を横断して予定を検索する（部分一致・大文字小文字無視）。
/// 期間に依らず全予定を対象にし、開始日時の新しい順に最大 `limit` 件（既定 200）返す。
#[tauri::command]
pub fn event_search(
    store: State<Store>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<EventSummary>, String> {
    store
        .search_events(&query, limit.unwrap_or(200))
        .map_err(|e| e.to_string())
}

/// 場所欄のオートコンプリート候補（過去に入力した場所を頻度順）。
#[tauri::command]
pub fn event_location_suggest(
    store: State<Store>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<String>, String> {
    store
        .suggest_event_locations(&query, limit.unwrap_or(8))
        .map_err(|e| e.to_string())
}

/// タイトル欄のオートコンプリート候補（過去に入力したタイトルを頻度順）。
#[tauri::command]
pub fn event_title_suggest(
    store: State<Store>,
    query: String,
    limit: Option<i64>,
) -> Result<Vec<String>, String> {
    store
        .suggest_event_titles(&query, limit.unwrap_or(8))
        .map_err(|e| e.to_string())
}

/// 単一の予定を取得。
#[tauri::command]
pub fn event_get(store: State<Store>, id: i64) -> Result<EventSummary, String> {
    store.get_event(id).map_err(|e| e.to_string())
}

/// 予定を作成または更新（確定後の行を返す）。`input.id` が無ければ新規。
/// Google カレンダー（書き込み可）所属なら、保存後にその予定を即 Google へ送る（ベストエフォート）。
#[tauri::command]
pub async fn event_upsert(
    app: AppHandle,
    store: State<'_, Store>,
    input: EventInput,
) -> Result<EventSummary, String> {
    if input.title.trim().is_empty() {
        return Err("タイトルを入力してください".to_string());
    }
    if input.start_at.trim().is_empty() {
        return Err("開始日時を入力してください".to_string());
    }
    // 更新前の (external_id, remote_calendar) を控える（カレンダー移動の検出用）。
    // remote_calendar は Google 上で今この予定が実在するカレンダー。
    let prev = match input.id {
        Some(id) => store.event_sync_ref(id as i64).ok().flatten(),
        None => None,
    };
    let saved = store.upsert_event(&input).map_err(|e| e.to_string())?;
    let new_cal = saved.calendar_id.map(|v| v as i64);
    // 新カレンダーが Google 書き込み可なら、その external_id（＝新しい送信先）。
    let new_target_ext = new_cal.and_then(|cid| match store.google_calendar_meta(cid) {
        Ok(Some((_, ext, role))) if matches!(role.as_str(), "owner" | "writer") => Some(ext),
        _ => None,
    });
    // Google 上の実在場所が新カレンダーと違うなら、実在場所から削除して連携解除（新カレンダーへ作成し直す）。
    if let Some((ext, remote_cal)) = prev {
        gcal_handle_move(&app, store.inner(), ext, remote_cal, new_target_ext, saved.id as i64).await;
    }
    // 新カレンダーへ送信（新規 or 既存の更新）。
    gcal_try_autopush(&app, store.inner(), new_cal).await;
    Ok(saved)
}

/// 予定を論理削除（ゴミ箱へ。保持期間後に完全削除）。
/// Google カレンダー所属なら、削除も即 Google へ伝播する（ベストエフォート）。
#[tauri::command]
pub async fn event_delete(
    app: AppHandle,
    store: State<'_, Store>,
    id: i64,
) -> Result<(), String> {
    // 所属カレンダーを削除前に控える（削除後も残るが順序を明確にするため）。
    let cal_id = store.get_event(id).ok().and_then(|e| e.calendar_id).map(|v| v as i64);
    store.delete_event(id).map_err(|e| e.to_string())?;
    gcal_try_autopush(&app, store.inner(), cal_id).await;
    Ok(())
}

/// 論理削除した予定を復元。
#[tauri::command]
pub fn event_restore(store: State<Store>, id: i64) -> Result<(), String> {
    store.restore_event(id).map_err(|e| e.to_string())
}

/// カレンダー一覧（マイ→他）。
#[tauri::command]
pub fn calendar_list(store: State<Store>) -> Result<Vec<CalendarSummary>, String> {
    store.list_calendars().map_err(|e| e.to_string())
}

/// カレンダーを作成または更新（確定後の行を返す）。
#[tauri::command]
pub fn calendar_upsert(
    store: State<Store>,
    input: CalendarInput,
) -> Result<CalendarSummary, String> {
    if input.name.trim().is_empty() {
        return Err("カレンダー名を入力してください".to_string());
    }
    store.upsert_calendar(&input).map_err(|e| e.to_string())
}

/// カレンダーの表示オン/オフを切り替える。
#[tauri::command]
pub fn calendar_set_visible(store: State<Store>, id: i64, visible: bool) -> Result<(), String> {
    store
        .set_calendar_visible(id, visible)
        .map_err(|e| e.to_string())
}

/// カレンダーを削除（既定は不可。所属予定は既定へ付け替え）。削除できたら true。
#[tauri::command]
pub fn calendar_delete(store: State<Store>, id: i64) -> Result<bool, String> {
    store.delete_calendar(id).map_err(|e| e.to_string())
}

/// 予定の参加者（ゲスト）一覧。
#[tauri::command]
pub fn event_attendee_list(store: State<Store>, event_id: i64) -> Result<Vec<EventAttendee>, String> {
    store.list_event_attendees(event_id).map_err(|e| e.to_string())
}

/// 予定の参加者を入力の集合に一致させる（全置き換え）。
#[tauri::command]
pub fn event_attendee_set(
    store: State<Store>,
    event_id: i64,
    attendees: Vec<AttendeeInput>,
) -> Result<(), String> {
    store
        .set_event_attendees(event_id, &attendees)
        .map_err(|e| e.to_string())
}

/// 予定のリマインダー（開始何分前に通知するか）の一覧を昇順で返す。
#[tauri::command]
pub fn event_reminder_list(store: State<Store>, event_id: i64) -> Result<Vec<i32>, String> {
    store
        .list_event_reminders(event_id)
        .map_err(|e| e.to_string())
}

/// 予定のリマインダーを入力の集合に一致させる（全置き換え）。
/// Google カレンダー（書き込み可）所属なら、保存後にその予定を即 Google へ送る（全通知を反映）。
#[tauri::command]
pub async fn event_reminder_set(
    app: AppHandle,
    store: State<'_, Store>,
    event_id: i64,
    minutes: Vec<i32>,
) -> Result<(), String> {
    store
        .set_event_reminders(event_id, &minutes)
        .map_err(|e| e.to_string())?;
    let cal_id = store
        .get_event(event_id)
        .ok()
        .and_then(|e| e.calendar_id)
        .map(|v| v as i64);
    gcal_try_autopush(&app, store.inner(), cal_id).await;
    Ok(())
}

/// .ics ファイルを取り込む（各 VEVENT を予定として追加）。
#[tauri::command]
pub fn ics_import(store: State<Store>, path: String) -> Result<IcsImportReport, String> {
    let text = std::fs::read_to_string(&path).map_err(|e| format!("ファイルを読めません: {e}"))?;
    store.import_ics(&text).map_err(|e| e.to_string())
}

/// 全予定（非削除）を .ics ファイルへ書き出す。
#[tauri::command]
pub fn ics_export(store: State<Store>, path: String) -> Result<(), String> {
    let text = store.export_ics().map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| format!("ファイルを書けません: {e}"))
}

// ────────────────── Google カレンダー同期（docs/CALENDAR_SYNC.md） ──────────────────

/// keyring 内の Client Secret のキー（OAuth アプリは 1 つなので固定）。
const GCAL_CLIENT_SECRET_KEY: &str = "gcal:client_secret";

/// keyring 内の refresh_token のキー（連携アカウントのメールごと）。
fn gcal_refresh_key(email: &str) -> String {
    format!("gcal:refresh:{email}")
}

/// Client ID / Secret を解決する。優先順位は「アプリに保存済み（app_settings/keyring）」→
/// 「環境変数（GCAL_CLIENT_ID / GCAL_CLIENT_SECRET。dev の .env 用）」。無ければ None。
fn gcal_resolve_credentials(app: &AppHandle, store: &Store) -> (Option<String>, Option<String>) {
    let non_empty = |s: String| Some(s).filter(|v| !v.trim().is_empty());
    // Client ID: 保存済み → 環境変数。
    let client_id = store
        .get_setting("gcal_client_id")
        .ok()
        .flatten()
        .and_then(non_empty)
        .or_else(|| std::env::var("GCAL_CLIENT_ID").ok().and_then(non_empty));
    // Client Secret: keyring → 環境変数。
    let service = app.config().identifier.clone();
    let client_secret = keyring::Entry::new(&service, GCAL_CLIENT_SECRET_KEY)
        .and_then(|e| e.get_password())
        .ok()
        .and_then(non_empty)
        .or_else(|| std::env::var("GCAL_CLIENT_SECRET").ok().and_then(non_empty));
    (client_id, client_secret)
}

/// 保存/削除した予定が Google（書き込み可）カレンダー所属なら、その予定の変更だけを即 Google へ
/// 送る（保存時オート送信）。ベストエフォート: 失敗しても保存自体は成功扱い（警告ログのみ）。
/// ローカル専用・読み取り専用カレンダー・未連携なら即 return（ネットワークアクセスなし）。
/// アカウントのアクセストークンを取得（refresh_token → access_token）。失敗時は None。
async fn gcal_account_access(app: &AppHandle, store: &Store, account_id: i64) -> Option<String> {
    let email = store.calendar_account_email(account_id).ok().flatten()?;
    let (client_id, client_secret) = gcal_read_credentials(app, store).ok()?;
    let service = app.config().identifier.clone();
    let refresh = keyring::Entry::new(&service, &gcal_refresh_key(&email))
        .and_then(|e| e.get_password())
        .ok()?;
    match gcal::oauth::refresh_access_token(&client_id, &client_secret, &refresh).await {
        Ok(a) => Some(a),
        Err(e) => {
            log::warn!("Google カレンダー: トークン更新に失敗: {e}");
            None
        }
    }
}

async fn gcal_try_autopush(app: &AppHandle, store: &Store, calendar_local_id: Option<i64>) {
    let Some(cal_id) = calendar_local_id else {
        return;
    };
    let (account_id, ext_id, access_role) = match store.google_calendar_meta(cal_id) {
        Ok(Some(m)) => m,
        _ => {
            log::info!("gcal autopush: カレンダー {cal_id} はローカル/未連携のため送信しません");
            return; // ローカル/未連携カレンダー
        }
    };
    if !matches!(access_role.as_str(), "owner" | "writer") {
        log::info!(
            "gcal autopush: カレンダー {cal_id}（{access_role}）は読み取り専用のため送信しません"
        );
        return; // 読み取り専用（購読カレンダー等）は送れない
    }
    let Some(access) = gcal_account_access(app, store, account_id).await else {
        return;
    };
    match gcal::sync::push_calendar_only(store, &access, cal_id, &ext_id).await {
        Ok(r) => log::info!(
            "gcal autopush: カレンダー {cal_id} 送信 pushed={} deleted_out={}",
            r.pushed,
            r.deleted_out
        ),
        Err(e) => log::warn!("Google カレンダー自動送信に失敗: {e}"),
    }
}

/// 予定が別の Google カレンダーへ移った（またはローカル/読み取り専用へ移った）ときの後始末。
/// `remote_calendar`（＝Google 上で今この予定が実在するカレンダー）から予定を削除し、ローカルの
/// 連携情報を解除して、新カレンダーで新規作成扱いにする（Google は単純 patch でカレンダー間移動を
/// 扱えないため、「実在場所から削除 → 新へ作成」で反映する）。ローカル calendar_id ではなく Google
/// 上の実在場所を使うので、付け替えでズレていても正しい場所から消せる。
async fn gcal_handle_move(
    app: &AppHandle,
    store: &Store,
    external_id: Option<String>,
    remote_calendar: Option<String>,
    new_target_ext: Option<String>,
    event_id: i64,
) {
    let (Some(gid), Some(remote_cal)) = (external_id, remote_calendar) else {
        return; // Google 上に無い予定は、移動元として消すものが無い
    };
    if Some(&remote_cal) == new_target_ext.as_ref() {
        return; // 同じ Google カレンダーのまま＝通常の編集（patch）。移動ではない
    }
    // 実在カレンダー（remote_cal）から削除する。書き込み可のときだけ。
    if let Ok(Some((account_id, role))) = store.google_calendar_by_ext(&remote_cal) {
        if matches!(role.as_str(), "owner" | "writer") {
            if let Some(access) = gcal_account_access(app, store, account_id).await {
                if let Ok(client) = gcal::http_client() {
                    match gcal::api::delete_event(&client, &access, &remote_cal, &gid).await {
                        Ok(()) => log::info!(
                            "gcal move: 実在カレンダー {remote_cal} から gid={gid} を削除"
                        ),
                        Err(e) => log::warn!("gcal move: 旧予定の削除に失敗: {e}"),
                    }
                }
            }
        }
    }
    // 連携情報を解除 → 新カレンダーの autopush で新規作成される（dirty は 1 のまま）。
    if let Err(e) = store.reset_event_remote(event_id) {
        log::warn!("gcal move: 連携情報のリセットに失敗: {e}");
    }
}

/// 解決済み Client ID / Secret を返す。どちらか欠けていれば分かるエラー。
fn gcal_read_credentials(app: &AppHandle, store: &Store) -> Result<(String, String), String> {
    let (client_id, client_secret) = gcal_resolve_credentials(app, store);
    let client_id = client_id.ok_or(
        "Google の Client ID が未設定です。設定 > Google カレンダー で入力（または .env の GCAL_CLIENT_ID）してください",
    )?;
    let client_secret = client_secret
        .ok_or("Google の Client Secret が未設定です（.env の GCAL_CLIENT_SECRET でも可）")?;
    Ok((client_id, client_secret))
}

/// OAuth クライアント資格情報（Client ID / Secret）を保存する。
#[tauri::command]
pub fn gcal_set_credentials(
    app: AppHandle,
    store: State<Store>,
    client_id: String,
    client_secret: String,
) -> Result<(), String> {
    let cid = client_id.trim();
    let cs = client_secret.trim();
    if cid.is_empty() || cs.is_empty() {
        return Err("Client ID と Client Secret を入力してください".into());
    }
    store
        .set_setting("gcal_client_id", cid)
        .map_err(|e| e.to_string())?;
    let service = app.config().identifier.clone();
    keyring::Entry::new(&service, GCAL_CLIENT_SECRET_KEY)
        .and_then(|e| e.set_password(cs))
        .map_err(|e| format!("Client Secret を保存できません: {e}"))?;
    Ok(())
}

/// OAuth クライアント資格情報の設定状況（値は返さず、有無とヒントのみ）。
#[tauri::command]
pub fn gcal_credentials_status(
    app: AppHandle,
    store: State<Store>,
) -> Result<GcalCredentialsStatus, String> {
    // 保存済み・環境変数（.env）どちらでも「設定済み」と見なす。
    let (client_id, client_secret) = gcal_resolve_credentials(&app, store.inner());
    let hint = client_id.as_ref().map(|id| {
        let head: String = id.chars().take(12).collect();
        format!("{head}…")
    });
    Ok(GcalCredentialsStatus {
        configured: client_id.is_some() && client_secret.is_some(),
        client_id_hint: hint,
    })
}

/// 連携済み Google アカウント一覧。
#[tauri::command]
pub fn gcal_accounts(store: State<Store>) -> Result<Vec<GoogleAccount>, String> {
    store.list_calendar_accounts().map_err(|e| e.to_string())
}

/// Google アカウントを連携する（OAuth 同意フロー → refresh_token を keyring に保存）。
#[tauri::command]
pub async fn gcal_connect(
    app: AppHandle,
    store: State<'_, Store>,
) -> Result<GoogleAccount, String> {
    let (client_id, client_secret) = gcal_read_credentials(&app, store.inner())?;
    let (tokens, email) = gcal::oauth::run_flow(&app, &client_id, &client_secret).await?;
    let refresh = tokens.refresh_token.ok_or(
        "refresh_token を取得できませんでした（同意画面でカレンダーの権限を許可してください）",
    )?;
    let service = app.config().identifier.clone();
    keyring::Entry::new(&service, &gcal_refresh_key(&email))
        .and_then(|e| e.set_password(&refresh))
        .map_err(|e| format!("認証情報を保存できません: {e}"))?;
    let id = store
        .upsert_calendar_account(&email, None)
        .map_err(|e| e.to_string())?;
    Ok(GoogleAccount {
        id: id as i32,
        email,
        last_sync_at: None,
    })
}

/// Google アカウントの連携を解除する（refresh_token と、取り込んだカレンダー/予定を削除）。
#[tauri::command]
pub fn gcal_disconnect(
    app: AppHandle,
    store: State<Store>,
    account_id: i64,
) -> Result<(), String> {
    if let Ok(Some(email)) = store.calendar_account_email(account_id) {
        let service = app.config().identifier.clone();
        if let Ok(entry) = keyring::Entry::new(&service, &gcal_refresh_key(&email)) {
            let _ = entry.delete_credential();
        }
    }
    store
        .delete_calendar_account(account_id)
        .map_err(|e| e.to_string())
}

/// 指定アカウントのカレンダーを同期する（push → pull の双方向）。
#[tauri::command]
pub async fn gcal_sync(
    app: AppHandle,
    store: State<'_, Store>,
    account_id: i64,
) -> Result<GcalSyncResult, String> {
    let email = store
        .calendar_account_email(account_id)
        .map_err(|e| e.to_string())?
        .ok_or("連携アカウントが見つかりません")?;
    let (client_id, client_secret) = gcal_read_credentials(&app, store.inner())?;
    let service = app.config().identifier.clone();
    let refresh = keyring::Entry::new(&service, &gcal_refresh_key(&email))
        .and_then(|e| e.get_password())
        .map_err(|_| "保存された認証情報がありません。もう一度連携してください".to_string())?;
    let access = gcal::oauth::refresh_access_token(&client_id, &client_secret, &refresh).await?;
    gcal::sync::sync_account(store.inner(), &access, account_id).await
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
    // 開いた時点では既読にしない。既読化はフロントが「アクティブが外れた」タイミングで
    // mail_set_read で行う（読書中は未読のまま＝未読フィルタから消えない）。
    Ok(detail)
}

/// 指定メールが属する論理スレッドの会話（時系列）を取得する（docs/THREADING.md §5）。
/// 未割当の旧データはここで遅延割当する。
#[tauri::command]
pub fn thread_view(store: State<Store>, email_id: i64) -> Result<ThreadView, String> {
    store
        .thread_view(email_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "スレッドが見つかりません".to_string())
}

/// 論理スレッドにアプリ独自タイトルを付ける（再件名）。title=null で既定へ戻す。
#[tauri::command]
pub fn thread_rename(
    store: State<Store>,
    thread_id: i64,
    title: Option<String>,
) -> Result<(), String> {
    store
        .thread_rename(thread_id, title.as_deref())
        .map_err(|e| e.to_string())
}

/// メールを別スレッドへ切り出す（手動分割）。mode: "this"（この 1 通）| "below"（このメール以降）。
/// 新スレッド id を返す。
#[tauri::command]
pub fn thread_split(store: State<Store>, email_id: i64, mode: String) -> Result<i64, String> {
    store
        .thread_split(email_id, &mode)
        .map_err(|e| e.to_string())
}

/// 2 つの論理スレッドを結合する（source を target へ）。
#[tauri::command]
pub fn thread_merge(
    store: State<Store>,
    source_thread: i64,
    target_thread: i64,
) -> Result<(), String> {
    store
        .thread_merge(source_thread, target_thread)
        .map_err(|e| e.to_string())
}

/// メール 1 通を指定スレッドへ付け替える（手動）。
#[tauri::command]
pub fn message_reassign(
    store: State<Store>,
    email_id: i64,
    target_thread: i64,
) -> Result<(), String> {
    store
        .message_reassign(email_id, target_thread)
        .map_err(|e| e.to_string())
}

/// アカウントの auto スレッド割当を作り直す（manual は保持）。
#[tauri::command]
pub fn thread_rebuild(store: State<Store>, account_id: i64) -> Result<(), String> {
    store.rebuild_threads(account_id).map_err(|e| e.to_string())
}

/// ローカル再加工（再ダウンロード不要）: 保存済み本文から clean_body・引用・スレッド・代表フラグを
/// 作り直す。パーサ改良を既存メールへ反映する用途（docs/THREADING.md §5）。処理件数を返す。
#[tauri::command]
pub fn mail_reprocess(store: State<Store>, account_id: i64) -> Result<i64, String> {
    let n = store.reprocess_all(account_id).map_err(|e| e.to_string())?;
    // 現行パーサで全件を作り直せたので、解析バージョンを現行として記録する。
    store
        .mark_reprocessed(account_id)
        .map_err(|e| e.to_string())?;
    Ok(n as i64)
}

/// 再構築の実行計画: アカウントに記録されたデータ形式バージョンを現行値と比べ、
/// サーバーから全体再取り込みが必要か、ローカル再解析だけで足りるかを判定する。
#[tauri::command]
pub fn rebuild_plan(store: State<Store>, account_id: i64) -> Result<RebuildPlan, String> {
    let (ingest, parse) = store
        .data_versions(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "アカウントが見つかりません".to_string())?;
    let action = if dataver::needs_resync(ingest) {
        RebuildAction::Resync
    } else {
        // 取り込み形式が現行なら、解析が古くても最新でもローカル再解析で足りる
        // （最新の場合も点検を兼ねて作り直す。冪等・通信なし）。
        RebuildAction::Reprocess
    };
    Ok(RebuildPlan {
        action,
        ingest_stored: ingest as i32,
        ingest_current: dataver::INGEST_VERSION as i32,
        parse_stored: parse as i32,
        parse_current: dataver::PARSE_VERSION as i32,
    })
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
    let (account_id, uid, folder) = store
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
        imap_sync::fetch_message(&host, port, &login_user, &password, &folder, uid as u32)
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

    // 添付メタが未保存（absent をヘッダのみで取り込んでいた）なら、ここで復元する。
    let atts: Vec<NewAttachment> = parsed
        .attachments
        .into_iter()
        .map(|a| NewAttachment {
            part_index: a.part_index,
            filename: a.filename,
            content_type: a.content_type,
            size: a.size,
            kind: a.kind,
            content_id: a.content_id,
            section: None,
        })
        .collect();
    store.ensure_attachments(id, &atts).map_err(|e| e.to_string())?;

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
    let section = info.section;

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
            section.as_deref(),
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

/// ローカルパスのファイルを OS の関連アプリで開く（作成画面で、添付を送信前に確認する用）。
/// パスはユーザーが選択／ドロップした添付、または退避済みの一時ファイル。存在するファイルのみ開く。
#[tauri::command]
pub fn open_local_path(app: AppHandle, path: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    if !std::path::Path::new(&path).is_file() {
        return Err("ファイルが見つかりません".to_string());
    }
    app.opener()
        .open_path(path, None::<&str>)
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
    pool: State<'_, ImapPool>,
    account_id: i64,
) -> Result<SyncResult, String> {
    // 資格情報の取得（失敗しうる）は同期枠の確保より前に済ませる（確保後に失敗すると
    // フラグが残り、以後の同期が全てスキップされてしまうため）。
    let (email, login_user, host, port) = store
        .get_account_imap(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "アカウントが見つかりません".to_string())?;
    let service = app.config().identifier.clone();
    let password = keyring::Entry::new(&service, &email)
        .and_then(|e| e.get_password())
        .map_err(|e| format!("資格情報を取得できません: {e}"))?;

    // 明示操作（再取り込み）は自動同期より優先する。実行中（多くは画面に出ないサイレントな
    // 自動同期）なら中断させ、枠が解放されるまで待ってから確保する。
    if control.request_cancel(account_id) {
        // 中断はチャンク境界で反映される。最大 ~10 秒だけ解放を待つ。
        for _ in 0..100 {
            if !control.is_running(account_id) {
                break;
            }
            let _ = tauri::async_runtime::spawn_blocking(|| {
                std::thread::sleep(std::time::Duration::from_millis(100))
            })
            .await;
        }
    }
    let Some(cancel) = control.try_begin(account_id) else {
        return Err(
            "実行中の同期を中断できませんでした。少し待ってから再度お試しください。".to_string(),
        );
    };
    // これ以降のエラーは同期枠を必ず解放してから返す。
    if let Err(e) = store.reset_sync_state(account_id) {
        control.end(account_id);
        return Err(e.to_string());
    }
    let db_path = store.path();

    let app_ev = app.clone();
    let cancel_task = cancel.clone();
    let session_slot = pool.slot(account_id);
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
            &session_slot,
        )
    })
    .await;
    control.end(account_id);
    let result = out.map_err(|e| e.to_string())?;
    if result.is_ok() {
        // フル再取得後は、保存済み本文からローカルで全面再加工（clean_body・スレッド・代表フラグ）。
        // 接続は閉じており、サーバーとは無関係。
        match store.reprocess_all(account_id) {
            Ok(_) => {
                // 中断されず完走したときだけ、データ形式を現行として記録する
                // （中断時は古い形式のメールが残っている可能性があるため記録しない）。
                if !cancel.load(Ordering::Relaxed) {
                    if let Err(e) = store.mark_resynced(account_id) {
                        log::warn!("再取り込みのバージョン記録に失敗: {e}");
                    }
                }
            }
            Err(e) => log::warn!("再取り込み後の再加工に失敗: {e}"),
        }
    }
    result
}

/// 開発用: 添付本体を落とさず BODYSTRUCTURE だけ取り直し、既存メールの添付メタを section 付きで
/// 作り直す（ネスト添付の取りこぼし修正・開発DBの掃除）。本体を落とさないので軽い。作り直した件数を返す。
#[tauri::command]
pub async fn mail_rederive_attachments(
    app: AppHandle,
    store: State<'_, Store>,
    control: State<'_, SyncControl>,
    pool: State<'_, ImapPool>,
    account_id: i64,
) -> Result<u32, String> {
    let (email, login_user, host, port) = store
        .get_account_imap(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "アカウントが見つかりません".to_string())?;
    let service = app.config().identifier.clone();
    let password = keyring::Entry::new(&service, &email)
        .and_then(|e| e.get_password())
        .map_err(|e| format!("資格情報を取得できません: {e}"))?;

    // 明示操作。実行中の自動同期があれば中断させ、枠が解放されるまで待つ。
    if control.request_cancel(account_id) {
        for _ in 0..100 {
            if !control.is_running(account_id) {
                break;
            }
            let _ = tauri::async_runtime::spawn_blocking(|| {
                std::thread::sleep(std::time::Duration::from_millis(100))
            })
            .await;
        }
    }
    let Some(cancel) = control.try_begin(account_id) else {
        return Err(
            "実行中の同期を中断できませんでした。少し待ってから再度お試しください。".to_string(),
        );
    };
    let db_path = store.path();

    let app_ev = app.clone();
    let cancel_task = cancel.clone();
    let session_slot = pool.slot(account_id);
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
        imap_sync::rederive_account_attachments(
            &db_path,
            account_id,
            &host,
            port,
            &login_user,
            &password,
            &progress,
            &cancel_task,
            &session_slot,
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

/// 登録済みアカウントの IMAP サーバーへ TCP 到達確認だけ行う（LOGIN しない・タイムアウトつき）。
/// 接続ドットの軽量チェック用。都度フル LOGIN すると遅いサーバーで緑になるまで待たされ、
/// かつ連続ログインとみなされやすいため、状態表示はこの軽い疎通で済ませる。
#[tauri::command]
pub async fn account_ping(store: State<'_, Store>, account_id: i64) -> Result<(), String> {
    let (_email, _login, host, port) = store
        .get_account_imap(account_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "アカウントが見つかりません".to_string())?;
    tauri::async_runtime::spawn_blocking(move || {
        use std::net::{TcpStream, ToSocketAddrs};
        use std::time::Duration;
        let addr = format!("{host}:{port}")
            .to_socket_addrs()
            .map_err(|e| format!("名前解決に失敗: {e}"))?
            .next()
            .ok_or_else(|| "アドレスを解決できませんでした".to_string())?;
        TcpStream::connect_timeout(&addr, Duration::from_secs(6))
            .map(|_| ())
            .map_err(|e| format!("接続できませんでした: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// アカウントを削除（受信メールと keyring の資格情報も削除）。
#[tauri::command]
pub fn account_delete(
    app: AppHandle,
    store: State<Store>,
    pool: State<ImapPool>,
    account_id: i64,
) -> Result<(), String> {
    if let Some((email, _login, _host, _port)) = store
        .get_account_imap(account_id)
        .map_err(|e| e.to_string())?
    {
        let service = app.config().identifier.clone();
        if let Ok(entry) = keyring::Entry::new(&service, &email) {
            let _ = entry.delete_credential();
        }
    }
    // 保持していた IMAP 接続を破棄する。
    pool.evict(account_id);
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

#[cfg(test)]
mod compose_tests {
    use super::{compose_html, plain_to_html};

    #[test]
    fn quoted_html_is_placed_inside_body() {
        let quote = "<br><br>X さんが書きました:<br><blockquote>元のHTML</blockquote>";
        let html = compose_html("返信本文\n2行目", Some(quote));
        // 新規本文が <br> 化され、引用はその後ろ・body の内側に入る（</html> の外に出ない）。
        assert!(html.contains("返信本文<br>\n2行目"));
        let body_end = html.find("</div></body></html>").expect("has closing");
        let quote_pos = html.find("<blockquote>元のHTML</blockquote>").expect("has quote");
        assert!(quote_pos < body_end, "quote must be inside body div");
    }

    #[test]
    fn html_escapes_new_body_but_keeps_quoted_html_verbatim() {
        // 新規本文の < > & はエスケープ、引用済み HTML はそのまま。
        let html = compose_html("a<b>&c", Some("<blockquote><b>bold</b></blockquote>"));
        assert!(html.contains("a&lt;b&gt;&amp;c"));
        assert!(html.contains("<blockquote><b>bold</b></blockquote>"));
    }

    #[test]
    fn no_quote_matches_plain_to_html() {
        assert_eq!(compose_html("hi", None), plain_to_html("hi"));
    }
}
