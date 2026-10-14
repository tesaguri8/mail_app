//! 添付の本体を IMAP から取ってきて、手元のファイルにする（オンデマンド取得）。
//!
//! - **分割取得**: `BODY.PEEK[section]<offset.length>` で [`CHUNK`] ずつ取る。取れた分は
//!   添付のフォルダの [`PARTIAL`] に貯めるので、回線が途中で止まって時間切れになっても、
//!   もう一度押せば続きから取る（遅い回線では大きな応答が途中で止まることがある / 2026-10-14 vaio）
//! - **締め切り**: 1 回の取得は全体で [`ImapTimeouts::ATTACHMENT`] の `total` まで。過ぎたら
//!   必ず時間切れで戻す。`spawn_blocking` 側が戻らなくても、少し待って画面には戻す
//! - **進み具合**: 段（準備・接続・取得・保存）と取れたバイト数を [`PROGRESS_EVENT`] で送る
//! - **合流**: 同じ添付の取得が走っていれば、それが終わるのを待ってから結果を使う（二重に取らない）
//! - **async のスレッドを塞がない**: DB・keyring・IMAP・ファイルはすべて `spawn_blocking` の中で扱う

use crate::models::{AttachmentProgress, AttachmentStage};
use crate::services::checksum::simple_checksum;
use crate::services::imap_conn::{connect_client, ImapError, ImapSession, ImapTimeouts};
use crate::services::imap_sync::{combine_mime_part, imap_mailbox_for_tag, parse_section_path};
use crate::services::parser;
use crate::services::store::Store;
use imap_proto::types::{MessageSection, SectionPath};
use mail_parser::MessageParser;
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// 進み具合のイベント名（ペイロードは [`AttachmentProgress`]）。
pub const PROGRESS_EVENT: &str = "attachment:progress";
/// 分割取得の 1 回の大きさ。遅い回線（TLS の握手に 3 秒かかる経路）でも、1 回が読み書きの
/// 上限（60 秒）に余裕で収まり、かつ往復の回数が増えすぎない大きさ。
pub const CHUNK: u64 = 64 * 1024;
/// 途中まで取れた本体（転送エンコードのまま）を貯めるファイル名。
pub const PARTIAL: &str = "__rondine_partial";
/// 途中まで取れた添付の MIME ヘッダを置くファイル名。
const PARTIAL_MIME: &str = "__rondine_partial.mime";
/// 進み具合を送る間隔。
const PROGRESS_EVERY: Duration = Duration::from_millis(250);
/// `spawn_blocking` 側が締め切りで戻らなかったときの保険（締め切り＋この余裕で画面に戻す）。
const DEADLINE_SLACK: Duration = Duration::from_secs(15);

/// 添付の取得の失敗。
#[derive(Debug, thiserror::Error)]
pub enum AttachmentError {
    #[error("添付が見つかりません")]
    NotFound,
    #[error("再取得に必要な情報がありません。アカウントを再同期してください。")]
    NeedResync,
    #[error("アカウントが見つかりません")]
    NoAccount,
    #[error("資格情報を取得できません: {0}")]
    Credentials(#[from] keyring::Error),
    #[error(transparent)]
    Imap(#[from] ImapError),
    #[error("{0}")]
    Parse(&'static str),
    #[error("ファイルを読み書きできませんでした: {0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Db(#[from] rusqlite::Error),
    #[error("取得の処理が止まりました: {0}")]
    Join(String),
}

impl AttachmentError {
    /// サーバーが応答しなかった（締め切りまでに終わらなかった）か。
    pub fn is_timeout(&self) -> bool {
        matches!(self, Self::Imap(ImapError::Timeout))
    }

    /// 張り直せば続きから取れる失敗か（時間切れ・切断）。
    fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::Imap(
                ImapError::Timeout
                    | ImapError::Imap(imap::Error::Io(_) | imap::Error::ConnectionLost)
            )
        )
    }
}

/// 取得の進み具合（`spawn_blocking` の中から書き、async 側が読んで送る）。
#[derive(Debug, Default)]
pub struct Progress {
    stage: AtomicU8,
    received: AtomicU64,
    total: AtomicU64,
}

impl Progress {
    fn set_stage(&self, stage: AttachmentStage) {
        let n = match stage {
            AttachmentStage::Preparing => 0,
            AttachmentStage::Connecting => 1,
            AttachmentStage::Fetching => 2,
            AttachmentStage::Saving => 3,
        };
        self.stage.store(n, Ordering::Relaxed);
    }

    fn set_received(&self, n: u64) {
        self.received.store(n, Ordering::Relaxed);
    }

    /// いまの進み具合（イベントのペイロード）。
    fn snapshot(&self, attachment_id: i64) -> AttachmentProgress {
        let stage = match self.stage.load(Ordering::Relaxed) {
            1 => AttachmentStage::Connecting,
            2 => AttachmentStage::Fetching,
            3 => AttachmentStage::Saving,
            _ => AttachmentStage::Preparing,
        };
        AttachmentProgress {
            attachment_id: attachment_id as i32,
            stage,
            received: self.received.load(Ordering::Relaxed) as f64,
            total: self.total.load(Ordering::Relaxed) as f64,
        }
    }
}

/// 添付ごとの取得の順番待ち（同じ添付を二重に取らない）。`app.manage` で 1 つ持つ。
#[derive(Default)]
pub struct AttachmentFetches(Mutex<HashMap<i64, Arc<tokio::sync::Mutex<()>>>>);

impl AttachmentFetches {
    /// 添付の順番待ちの札（無ければ作る）。札は添付ごとに 1 つで、小さいので消さずに残す。
    fn gate(&self, attachment_id: i64) -> Arc<tokio::sync::Mutex<()>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .entry(attachment_id)
            .or_default()
            .clone()
    }
}

/// 取りに行くメッセージの在りかと、どの添付か。
#[derive(Debug, Clone, Copy)]
pub struct Target<'a> {
    pub attachment_id: i64,
    pub host: &'a str,
    pub port: u16,
    pub user: &'a str,
    pub password: &'a str,
    /// メールのフォルダ（ローカルタグ: inbox/sent/…）。
    pub folder: &'a str,
    pub uid: u32,
    /// IMAP の section（"2" / "1.2"）。無ければメッセージ全体を取って `part_index` で取り出す。
    pub section: Option<&'a str>,
    pub part_index: usize,
}

/// 取れた添付の本体。
#[derive(Debug)]
pub struct Fetched {
    pub bytes: Vec<u8>,
    pub filename: String,
}

/// 添付の本体を手元のファイルとして用意し、そのパスを返す（取得済みならそれを使う）。
/// 締め切りを過ぎたら、`spawn_blocking` 側が戻らなくても時間切れで戻す。
///
/// # Errors
/// 添付・アカウント・資格情報が無いとき、IMAP の取得・解析・書き込みに失敗したとき。
pub async fn ensure_file(app: &AppHandle, attachment_id: i64) -> Result<PathBuf, AttachmentError> {
    let started = Instant::now();
    log::info!("添付 {attachment_id}: 開始");
    let gate = app.state::<AttachmentFetches>().gate(attachment_id);
    let progress = Arc::new(Progress::default());
    let emitter = spawn_emitter(app.clone(), attachment_id, Arc::clone(&progress));
    let result = async {
        // 同じ添付の取得が走っていれば、それが終わるまで待つ（終われば取得済みとして使う）。
        let _turn = gate.lock().await;
        let job = {
            let app = app.clone();
            let progress = Arc::clone(&progress);
            tauri::async_runtime::spawn_blocking(move || {
                fetch_blocking(&app, attachment_id, &progress, started)
            })
        };
        let limit = ImapTimeouts::ATTACHMENT
            .total
            .map_or(Duration::from_secs(600), |t| t + DEADLINE_SLACK);
        match tokio::time::timeout(limit, job).await {
            Ok(joined) => joined.map_err(|e| AttachmentError::Join(e.to_string()))?,
            Err(_) => Err(ImapError::Timeout.into()),
        }
    }
    .await;
    emitter.abort();
    match &result {
        Ok(_) => log::info!(
            "添付 {attachment_id}: 完了（{}ms）",
            started.elapsed().as_millis()
        ),
        Err(e) => log::warn!(
            "添付 {attachment_id} の取得に失敗（{}ms）: {e}",
            started.elapsed().as_millis()
        ),
    }
    result
}

/// 進み具合を一定の間隔で送る（変わったときだけ）。取得が終わったら `abort` する。
fn spawn_emitter(
    app: AppHandle,
    attachment_id: i64,
    progress: Arc<Progress>,
) -> tauri::async_runtime::JoinHandle<()> {
    tauri::async_runtime::spawn(async move {
        let mut last: Option<AttachmentProgress> = None;
        loop {
            let now = progress.snapshot(attachment_id);
            if last.as_ref() != Some(&now) {
                let _ = app.emit(PROGRESS_EVENT, &now);
                last = Some(now);
            }
            tokio::time::sleep(PROGRESS_EVERY).await;
        }
    })
}

/// 取得の本体（`spawn_blocking` の中）。DB・keyring・IMAP・ファイルの順に進め、段ごとにログを残す。
fn fetch_blocking(
    app: &AppHandle,
    attachment_id: i64,
    progress: &Progress,
    started: Instant,
) -> Result<PathBuf, AttachmentError> {
    let ms = || started.elapsed().as_millis();
    let store = app.state::<Store>();
    let info = store
        .attachment_fetch_info(attachment_id)?
        .ok_or(AttachmentError::NotFound)?;
    log::info!("添付 {attachment_id}: 取得情報（{}ms）", ms());

    // 取得済みでファイルが残っていればそのまま使う（LRU の最終アクセスを更新）。
    if let Some(path) = info.file_path.as_deref().map(PathBuf::from) {
        if path.exists() {
            let _ = store.touch_attachment(attachment_id);
            return Ok(path);
        }
    }
    let uid = info.email_uid.ok_or(AttachmentError::NeedResync)?;
    progress.total.store(info.size.max(0) as u64, Ordering::Relaxed);
    let (email, login, host, port) = store
        .get_account_imap(info.account_id)?
        .ok_or(AttachmentError::NoAccount)?;
    let password = keyring::Entry::new(&app.config().identifier, &email)?.get_password()?;
    log::info!("添付 {attachment_id}: 資格情報（{}ms）", ms());

    let dir = store
        .data_dir()
        .join("attachments")
        .join(attachment_id.to_string());
    let target = Target {
        attachment_id,
        host: &host,
        port,
        user: &login,
        password: &password,
        folder: &info.folder,
        uid: uid as u32,
        section: info.section.as_deref(),
        part_index: info.part_index as usize,
    };
    let fetched = download(&target, &dir, ImapTimeouts::ATTACHMENT, progress)?;

    let path = dir.join(sanitize_filename(&info.filename));
    std::fs::write(&path, &fetched.bytes)?;
    store.set_attachment_downloaded(
        attachment_id,
        &path.to_string_lossy(),
        Some(&simple_checksum(&fetched.bytes)),
    )?;
    log::info!(
        "添付 {attachment_id}: 書き込み（{} バイト・{}ms）",
        fetched.bytes.len(),
        ms()
    );
    Ok(path)
}

/// 添付の本体を IMAP から取る。section があれば分割して取り、途中まで取れた分は `dir` に貯める。
///
/// # Errors
/// 接続・取得・解析・途中のファイルの読み書きに失敗したとき。締め切りを過ぎたら時間切れ。
pub fn download(
    t: &Target<'_>,
    dir: &Path,
    timeouts: ImapTimeouts,
    progress: &Progress,
) -> Result<Fetched, AttachmentError> {
    std::fs::create_dir_all(dir)?;
    match t.section.and_then(parse_section_path) {
        Some(path) => download_in_chunks(t, &path, dir, timeouts, progress),
        None => download_whole(t, timeouts, progress),
    }
}

/// 締め切りまでの残りを上限にした接続の設定（過ぎていたら時間切れ）。
fn remaining(timeouts: ImapTimeouts, deadline: Option<Instant>) -> Result<ImapTimeouts, ImapError> {
    match deadline {
        None => Ok(timeouts),
        Some(d) => {
            let left = d.saturating_duration_since(Instant::now());
            if left.is_zero() {
                Err(ImapError::Timeout)
            } else {
                Ok(ImapTimeouts {
                    total: Some(left),
                    ..timeouts
                })
            }
        }
    }
}

/// 接続・ログインして、メールのフォルダを選ぶ（段ごとの所要時間をログに残す）。
fn open_session(
    t: &Target<'_>,
    timeouts: ImapTimeouts,
    progress: &Progress,
) -> Result<ImapSession, AttachmentError> {
    progress.set_stage(AttachmentStage::Connecting);
    let t0 = Instant::now();
    let client = connect_client(t.host, t.port, timeouts)?;
    let connected = t0.elapsed();
    let mut session = client
        .login(t.user, t.password)
        .map_err(|(e, _)| ImapError::from(e))?;
    let logged_in = t0.elapsed();
    let mailbox = imap_mailbox_for_tag(&mut session, t.folder).map_err(ImapError::Other)?;
    session.select(&mailbox).map_err(ImapError::from)?;
    log::info!(
        "添付 {}: 接続 {}ms / ログイン {}ms / 選択 {}ms（{mailbox}）",
        t.attachment_id,
        connected.as_millis(),
        (logged_in - connected).as_millis(),
        (t0.elapsed() - logged_in).as_millis()
    );
    progress.set_stage(AttachmentStage::Fetching);
    Ok(session)
}

/// section の添付を [`CHUNK`] ずつ取る。途中で切れても、取れた分が増えていて締め切り前なら
/// 張り直して続ける。取り終えたら MIME ヘッダと合わせて解析する。
fn download_in_chunks(
    t: &Target<'_>,
    path: &[u32],
    dir: &Path,
    timeouts: ImapTimeouts,
    progress: &Progress,
) -> Result<Fetched, AttachmentError> {
    let partial = dir.join(PARTIAL);
    let mime_path = dir.join(PARTIAL_MIME);
    let mut offset = std::fs::metadata(&partial).map_or(0, |m| m.len());
    if offset > 0 {
        log::info!("添付 {}: 前回の続き（{offset} バイト）から取る", t.attachment_id);
    }
    progress.set_received(offset);
    let deadline = timeouts.total.map(|d| Instant::now() + d);
    loop {
        let mut session = open_session(t, remaining(timeouts, deadline)?, progress)?;
        let before = offset;
        let fetch_started = Instant::now();
        let r = fetch_chunks(&mut session, t.uid, path, &partial, &mime_path, &mut offset, progress);
        log::info!(
            "添付 {}: 取得 {} バイト（{}ms・計 {offset} バイト）",
            t.attachment_id,
            offset - before,
            fetch_started.elapsed().as_millis()
        );
        match r {
            Ok(()) => {
                let _ = session.logout();
                break;
            }
            // 進んではいたので、張り直して続きから取る（締め切りは remaining が見る）。
            Err(e) if e.is_retryable() && offset > before => {
                log::info!("添付 {}: 途中で止まったので張り直す: {e}", t.attachment_id);
            }
            Err(e) => return Err(e),
        }
    }
    progress.set_stage(AttachmentStage::Saving);
    let mime = std::fs::read(&mime_path).unwrap_or_default();
    let body = std::fs::read(&partial)?;
    let fetched = assemble(&mime, &body, t.part_index);
    // 組み立てられなかったときも途中のファイルは消す（壊れていたら次は最初から取り直す）。
    let _ = std::fs::remove_file(&partial);
    let _ = std::fs::remove_file(&mime_path);
    fetched
}

/// `BODY.PEEK[section]<offset.CHUNK>` を最後まで繰り返し、取れた分を `partial` に足していく。
/// 1 回目には MIME ヘッダも取って `mime_path` に置く（再開のときは取り直さない）。
fn fetch_chunks(
    session: &mut ImapSession,
    uid: u32,
    path: &[u32],
    partial: &Path,
    mime_path: &Path,
    offset: &mut u64,
    progress: &Progress,
) -> Result<(), AttachmentError> {
    let sec = path
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(".");
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(partial)?;
    loop {
        let want_mime = !mime_path.exists();
        let range = format!("BODY.PEEK[{sec}]<{offset}.{CHUNK}>");
        let query = if want_mime {
            format!("(BODY.PEEK[{sec}.MIME] {range})")
        } else {
            format!("({range})")
        };
        let msgs = session
            .uid_fetch(uid.to_string(), query)
            .map_err(ImapError::from)?;
        let m = msgs
            .iter()
            .next()
            .ok_or(AttachmentError::Parse("メッセージが見つかりませんでした"))?;
        if want_mime {
            let mime = m
                .section(&SectionPath::Part(path.to_vec(), Some(MessageSection::Mime)))
                .unwrap_or(&[]);
            std::fs::write(mime_path, mime)?;
        }
        // 範囲の外（取り終えた）なら空か NIL が返る。
        let data = m
            .section(&SectionPath::Part(path.to_vec(), None))
            .unwrap_or(&[]);
        let n = data.len() as u64;
        if n > CHUNK && *offset > 0 {
            // 部分取得に応えないサーバーが、続きの位置を無視して全体を返してきた。
            return Err(AttachmentError::Parse(
                "サーバーが部分取得に応えませんでした",
            ));
        }
        file.write_all(data)?;
        *offset += n;
        progress.set_received(*offset);
        // ちょうど CHUNK なら続きがあるかもしれない。それ以外（短い・全体が来た）は取り終えた。
        if n != CHUNK {
            return Ok(());
        }
    }
}

/// section の無い既存行: メッセージ全体を取って `part_index` 番目の実添付を取り出す（後方互換）。
fn download_whole(
    t: &Target<'_>,
    timeouts: ImapTimeouts,
    progress: &Progress,
) -> Result<Fetched, AttachmentError> {
    let mut session = open_session(t, timeouts, progress)?;
    let msgs = session
        .uid_fetch(t.uid.to_string(), "(BODY[])")
        .map_err(ImapError::from)?;
    let raw = msgs
        .iter()
        .next()
        .and_then(|m| m.body())
        .ok_or(AttachmentError::Parse("メッセージが見つかりませんでした"))?;
    progress.set_received(raw.len() as u64);
    progress.set_stage(AttachmentStage::Saving);
    let msg = MessageParser::default()
        .parse(raw)
        .ok_or(AttachmentError::Parse("メッセージを解析できませんでした"))?;
    // 保存時と同じ「実添付パート一覧」から part_index 番目を取り出す（ネストした添付にも届く）。
    let part = parser::real_attachment_parts(&msg)
        .get(t.part_index)
        .copied()
        .ok_or(AttachmentError::Parse("添付が見つかりませんでした"))?;
    let fetched = Fetched {
        bytes: part.contents().to_vec(),
        filename: parser::part_filename(part, t.part_index),
    };
    let _ = session.logout();
    Ok(fetched)
}

/// MIME ヘッダと本体（転送エンコードのまま）を合わせて、添付の中身を取り出す。
fn assemble(mime: &[u8], body: &[u8], part_index: usize) -> Result<Fetched, AttachmentError> {
    let combined = combine_mime_part(mime, body);
    let msg = MessageParser::default()
        .parse(&combined)
        .ok_or(AttachmentError::Parse("添付パートを解析できませんでした"))?;
    let part = msg
        .parts
        .first()
        .ok_or(AttachmentError::Parse("添付パートが空でした"))?;
    Ok(Fetched {
        bytes: part.contents().to_vec(),
        filename: parser::part_filename(part, part_index),
    })
}

/// 保存先のファイル名に使えない文字を `_` に置き換える（空なら `attachment`）。
pub fn sanitize_filename(name: &str) -> String {
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

#[cfg(test)]
mod tests;
