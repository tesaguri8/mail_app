//! 論理スレッドの割当・再構築・分割/結合（docs/THREADING.md §2〜§4）。
//!
//! 束ね方（ルートキー方式・order 非依存）:
//! - 各メールから「ルートキー」を決める。References があればその先頭（真のルート Message-ID）、
//!   無ければ In-Reply-To、無ければ自分の Message-ID を "mid:<id>" とする。ヘッダが全く無ければ
//!   件名正規化＋相手アドレスの "subj:<件名>#<相手>" にフォールバック（控えめ）。
//! - 同じ (account_id, root_key) のメールは同じ論理スレッドに束ねる（find-or-create）。
//! - ルートメール自身のキー "mid:<自分のMessage-ID>" と、その返信の "mid:<References先頭>" は一致するので
//!   取得順に依存せず同じスレッドに集まる。
//! - ユーザーの手動操作（分割/結合/再割当）は thread_assignment='manual' で固定し、再解析で動かさない。

use super::Store;
use crate::models::{ThreadMessage, ThreadSummary, ThreadView};
use rusqlite::{params, Connection, OptionalExtension};

/// 件名の正規化（Re:/Fwd:/RE：/転送/Fw: などの接頭辞を繰り返し剥がし、小文字化・空白畳み）。
pub fn normalize_subject(subject: Option<&str>) -> String {
    let mut s = subject.unwrap_or("").trim().to_string();
    loop {
        let lower = s.to_lowercase();
        let trimmed = lower.trim_start();
        let mut cut = 0usize;
        for p in [
            "re:",
            "fwd:",
            "fw:",
            "re：",
            "転送:",
            "転送：",
            "fwd：",
            "fw：",
        ] {
            if trimmed.starts_with(p) {
                cut = p.len();
                break;
            }
        }
        // "転送" 単独の接頭（コロン無し）も剥がす。
        if cut == 0 && trimmed.starts_with("転送") && !trimmed.starts_with("転送し") {
            cut = "転送".len();
        }
        if cut == 0 {
            break;
        }
        // 元の（小文字化前の）文字列から同じバイト数分だけ前方を落とす。
        let skip = s.len() - s.trim_start().len();
        s = s[skip + cut..].trim_start().to_string();
    }
    s.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// 表示用文字列（"名前 <addr>" や "addr"）から素のメールアドレス（小文字）を取り出す。
fn bare_addr(s: Option<&str>) -> Option<String> {
    let raw = s?.trim();
    if raw.is_empty() {
        return None;
    }
    // 先頭要素（カンマ区切りの 1 件目）を対象にする。
    let first = raw.split(',').next().unwrap_or(raw).trim();
    let inner = if let (Some(a), Some(b)) = (first.find('<'), first.find('>')) {
        if a < b {
            first[a + 1..b].trim()
        } else {
            first
        }
    } else {
        first
    };
    let addr = inner.trim();
    if addr.is_empty() {
        None
    } else {
        Some(addr.to_lowercase())
    }
}

/// 1 通のメールからルートキーを算出する（上記方針）。
#[allow(clippy::too_many_arguments)]
fn compute_root_key(
    conn: &Connection,
    account_id: i64,
    message_id: Option<&str>,
    in_reply_to: Option<&str>,
    references_ids: Option<&str>,
    subject: Option<&str>,
    from_address: Option<&str>,
    to_addresses: Option<&str>,
    folder: Option<&str>,
) -> rusqlite::Result<String> {
    // 既知の祖先（In-Reply-To または References のいずれか）が手元にあれば、そのスレッドを継承する。
    // References 先頭が真の root でないクライアント（Spark/titan.email 等、参照順が非標準）でも、
    // 参照している祖先の 1 つが手元にあれば同じスレッドに束ねられる（古い順処理で親は割当済み）。
    let mut ancestors: Vec<&str> = Vec::new();
    if let Some(irt) = in_reply_to.map(str::trim).filter(|s| !s.is_empty()) {
        ancestors.push(irt);
    }
    if let Some(refs) = references_ids {
        ancestors.extend(refs.split_whitespace());
    }
    let mut stmt = conn.prepare_cached(
        "SELECT thread_id FROM emails
         WHERE account_id = ?1 AND message_id = ?2 AND thread_id IS NOT NULL LIMIT 1",
    )?;
    for cand in &ancestors {
        let key: Option<String> = stmt
            .query_row(params![account_id, cand], |r| r.get(0))
            .optional()?;
        if let Some(k) = key {
            return Ok(k);
        }
    }
    // 祖先が手元に無い: root をキーにする。References 先頭 → In-Reply-To → 自分の Message-ID。
    if let Some(root) = references_ids
        .and_then(|r| r.split_whitespace().next())
        .filter(|s| !s.is_empty())
    {
        return Ok(format!("mid:{root}"));
    }
    if let Some(irt) = in_reply_to.filter(|s| !s.is_empty()) {
        return Ok(format!("mid:{irt}"));
    }
    // ルートメール（返信ではない）は自分の Message-ID をキーにする。
    if let Some(mid) = message_id.filter(|s| !s.is_empty()) {
        return Ok(format!("mid:{mid}"));
    }
    // ヘッダが無い: 件名正規化＋相手アドレスでフォールバック（控えめ）。
    let counterparty = if matches!(folder, Some("sent") | Some("drafts")) {
        bare_addr(to_addresses)
    } else {
        bare_addr(from_address)
    }
    .unwrap_or_default();
    Ok(format!(
        "subj:{}#{}",
        normalize_subject(subject),
        counterparty
    ))
}

/// (account_id, root_key) の論理スレッドを取得、無ければ作成して id を返す。
fn find_or_create_thread(
    conn: &Connection,
    account_id: i64,
    root_key: &str,
    subject: Option<&str>,
) -> rusqlite::Result<i64> {
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM logical_threads WHERE account_id = ?1 AND root_key = ?2",
            params![account_id, root_key],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    conn.execute(
        "INSERT INTO logical_threads (account_id, root_key, auto_title) VALUES (?1, ?2, ?3)",
        params![account_id, root_key, subject],
    )?;
    Ok(conn.last_insert_rowid())
}

/// スレッドの集計（件数・未読・最終・参加者・既定タイトル）を再計算する。
/// メールが 0 件になったスレッドは削除する。
pub fn recompute_thread(conn: &Connection, thread_id: i64) -> rusqlite::Result<()> {
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM emails WHERE logical_thread_id = ?1",
        params![thread_id],
        |r| r.get(0),
    )?;
    if count == 0 {
        // ユーザーが付けたタイトルがある空スレッドは残さない（自動掃除）。
        conn.execute(
            "DELETE FROM logical_threads WHERE id = ?1",
            params![thread_id],
        )?;
        return Ok(());
    }
    let unread: i64 = conn.query_row(
        "SELECT count(*) FROM emails WHERE logical_thread_id = ?1 AND is_read = 0",
        params![thread_id],
        |r| r.get(0),
    )?;
    let last_activity: Option<String> = conn.query_row(
        "SELECT max(date) FROM emails WHERE logical_thread_id = ?1",
        params![thread_id],
        |r| r.get(0),
    )?;
    // 参加者（差出人アドレスの重複なし・先頭数件）。
    let participants: Option<String> = conn
        .query_row(
            "SELECT group_concat(a, ', ') FROM (
                SELECT DISTINCT from_address AS a FROM emails
                WHERE logical_thread_id = ?1 AND from_address IS NOT NULL
                LIMIT 8)",
            params![thread_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    // 既定タイトル＝最古メールの件名。
    let auto_title: Option<String> = conn
        .query_row(
            "SELECT subject FROM emails WHERE logical_thread_id = ?1
             ORDER BY date_ts ASC, id ASC LIMIT 1",
            params![thread_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    conn.execute(
        "UPDATE logical_threads
         SET message_count = ?2, unread_count = ?3, last_activity = ?4,
             participants = ?5, auto_title = ?6
         WHERE id = ?1",
        params![
            thread_id,
            count,
            unread,
            last_activity,
            participants,
            auto_title
        ],
    )?;
    Ok(())
}

/// assign_thread が使う 1 行ぶんの素性（束ね判定に必要な列）。
struct AssignRow {
    account_id: i64,
    message_id: Option<String>,
    in_reply_to: Option<String>,
    references_ids: Option<String>,
    subject: Option<String>,
    from_address: Option<String>,
    to_addresses: Option<String>,
    folder: Option<String>,
    assignment: String,
    current_thread: Option<i64>,
}

/// メールに論理スレッドを割り当てる（manual は動かさない）。割当先スレッド id を返す。
/// 挿入時・再構築時に呼ぶ。
pub fn assign_thread(conn: &Connection, email_id: i64) -> rusqlite::Result<Option<i64>> {
    let row: Option<AssignRow> = conn
        .query_row(
            "SELECT account_id, message_id, in_reply_to, references_ids, subject,
                    from_address, to_addresses, folder,
                    COALESCE(thread_assignment, 'auto'), logical_thread_id
             FROM emails WHERE id = ?1",
            params![email_id],
            |r| {
                Ok(AssignRow {
                    account_id: r.get(0)?,
                    message_id: r.get(1)?,
                    in_reply_to: r.get(2)?,
                    references_ids: r.get(3)?,
                    subject: r.get(4)?,
                    from_address: r.get(5)?,
                    to_addresses: r.get(6)?,
                    folder: r.get(7)?,
                    assignment: r.get(8)?,
                    current_thread: r.get(9)?,
                })
            },
        )
        .optional()?;
    let Some(AssignRow {
        account_id,
        message_id,
        in_reply_to,
        references_ids,
        subject,
        from_address,
        to_addresses,
        folder,
        assignment,
        current_thread,
    }) = row
    else {
        return Ok(None);
    };

    // 手動割当は尊重する（再解析で動かさない）。集計は表示時に算出するのでここでは何もしない。
    if assignment == "manual" {
        return Ok(current_thread);
    }

    let root_key = compute_root_key(
        conn,
        account_id,
        message_id.as_deref(),
        in_reply_to.as_deref(),
        references_ids.as_deref(),
        subject.as_deref(),
        from_address.as_deref(),
        to_addresses.as_deref(),
        folder.as_deref(),
    )?;
    let tid = find_or_create_thread(conn, account_id, &root_key, subject.as_deref())?;
    conn.execute(
        "UPDATE emails SET logical_thread_id = ?2, thread_id = ?3, thread_assignment = 'auto'
         WHERE id = ?1",
        params![email_id, tid, root_key],
    )?;
    // 集計（件数・参加者・タイトル）は保存時に再計算しない（取り込み1通ごとに数クエリ走らせると
    // 全体が重くなるため）。会話ビューは表示時にメッセージから直接算出する（recompute_thread は
    // 手動の分割/結合/再割当でのみ使い、空スレッドの掃除を担う）。
    Ok(Some(tid))
}

/// ThreadMessage をクエリ行から組み立てる。
/// 列順: 0:id 1:account_id 2:message_id 3:from_address 4:from_name 5:to_addresses
/// 6:subject 7:date 8:direction 9:clean_body 10:body_plain 11:body_html 12:body_html_z
/// 13:has_attachments 14:has_quotes 15:is_read 16:folder 17:thread_assignment 18:is_flagged
fn map_thread_message(r: &rusqlite::Row) -> rusqlite::Result<ThreadMessage> {
    let html_z: Option<Vec<u8>> = r.get(12)?;
    let body_html = match html_z {
        Some(z) => crate::services::compress::decompress_text(&z).ok(),
        None => r.get(11)?,
    };
    Ok(ThreadMessage {
        id: r.get::<_, i64>(0)? as i32,
        account_id: r.get::<_, i64>(1)? as i32,
        message_id: r.get(2)?,
        from_address: r.get(3)?,
        from_name: r.get(4)?,
        to_addresses: r.get(5)?,
        subject: r.get(6)?,
        date: r.get(7)?,
        direction: r.get(8)?,
        clean_body: r.get(9)?,
        body_plain: r.get(10)?,
        body_html,
        has_attachments: r.get::<_, i64>(13)? != 0,
        has_quotes: r.get::<_, i64>(14)? != 0,
        is_read: r.get::<_, i64>(15)? != 0,
        is_starred: r.get::<_, i64>(18)? != 0,
        folder: r.get(16)?,
        thread_assignment: r.get(17)?,
    })
}

impl Store {
    /// 指定メールが属する論理スレッドの会話（時系列）を返す。
    /// スレッド未割当のメール（旧データ）はここで遅延割当する。
    pub fn thread_view(&self, email_id: i64) -> rusqlite::Result<Option<ThreadView>> {
        let conn = self.conn.lock().unwrap();
        // 未割当なら割り当てる。
        let tid: Option<i64> = conn
            .query_row(
                "SELECT logical_thread_id FROM emails WHERE id = ?1",
                params![email_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        let tid = match tid {
            Some(t) => t,
            None => match assign_thread(&conn, email_id)? {
                Some(t) => t,
                None => return Ok(None),
            },
        };
        Self::load_thread_view(&conn, tid)
    }

    /// スレッド id から会話を組み立てる（内部）。
    /// 集計（件数・未読・参加者・既定タイトル）は保存値に頼らず、読み込んだメッセージから算出する
    /// （取り込み時の再計算コストを無くすため）。ユーザー名（title/rename フラグ）だけ保存値を使う。
    fn load_thread_view(conn: &Connection, tid: i64) -> rusqlite::Result<Option<ThreadView>> {
        let head: Option<(Option<String>, bool)> = conn
            .query_row(
                "SELECT title, is_user_renamed FROM logical_threads WHERE id = ?1",
                params![tid],
                |r| Ok((r.get(0)?, r.get::<_, i64>(1)? != 0)),
            )
            .optional()?;
        let Some((title, is_user_renamed)) = head else {
            return Ok(None);
        };
        let sql = "SELECT e.id, e.account_id, e.message_id, e.from_address, e.from_name,
                          e.to_addresses, e.subject, e.date,
                          CASE WHEN e.folder IN ('sent','drafts') THEN 'out'
                               WHEN EXISTS(SELECT 1 FROM accounts a
                                           WHERE a.id = e.account_id
                                             AND lower(a.email) = lower(e.from_address)) THEN 'out'
                               ELSE 'in' END AS direction,
                          e.clean_body, e.body_plain, e.body_html, e.body_html_z,
                          e.has_attachments,
                          (length(COALESCE(e.body_plain,'')) > length(COALESCE(e.clean_body,''))) AS has_quotes,
                          e.is_read, e.folder, COALESCE(e.thread_assignment,'auto'), e.is_flagged
                   FROM emails e
                   WHERE e.logical_thread_id = ?1
                   ORDER BY e.date_ts ASC, e.id ASC";
        let mut stmt = conn.prepare(sql)?;
        let mut messages: Vec<ThreadMessage> = stmt
            .query_map(params![tid], map_thread_message)?
            .collect::<rusqlite::Result<_>>()?;
        // 表示名が無いものは住所録から補完する（スレッドは少数なので個別照合で十分）。
        for m in messages.iter_mut() {
            if m.from_name.is_none() {
                m.from_name = contact_name_for(conn, m.from_address.as_deref())?;
            }
        }
        // ② 内容照合による引用剥がし（形式非依存。docs/THREADING.md §2 優先3）。
        // 同スレッドの「より古いメールの先頭行」をアンカーに、ヒューリスティックで取り切れなかった
        // 引用（未知の属性行＋インライン引用など）を各メールの clean_body から追加で落とす。
        // 表示専用（保存は据え置き。全文は「引用を表示」で確認できる）。
        let mut anchors: std::collections::HashSet<String> = std::collections::HashSet::new();
        for m in messages.iter_mut() {
            if !anchors.is_empty() {
                if let Some(clean) = m.clean_body.clone() {
                    let trimmed = crate::services::quotes::cut_at_known_anchor(&clean, &anchors);
                    if trimmed.len() < clean.len() {
                        m.has_quotes = true; // 追加で剥がせた＝畳んだ引用がある
                        m.clean_body = Some(trimmed);
                    }
                }
            }
            if let Some(a) = m
                .clean_body
                .as_deref()
                .and_then(crate::services::quotes::first_anchor)
            {
                anchors.insert(a);
            }
        }
        // 集計はメッセージから算出（保存値に依存しない）。
        let message_count = messages.len() as i32;
        let unread_count = messages.iter().filter(|m| !m.is_read).count() as i32;
        // 既定タイトル＝最古メール（先頭）の件名。
        let auto_title = messages.first().and_then(|m| m.subject.clone());
        // 参加者＝差出人アドレスの重複なし（先頭数件）。
        let mut seen = std::collections::HashSet::new();
        let participants: Vec<String> = messages
            .iter()
            .filter_map(|m| m.from_address.clone())
            .filter(|a| seen.insert(a.clone()))
            .take(8)
            .collect();
        let participants = if participants.is_empty() {
            None
        } else {
            Some(participants.join(", "))
        };
        let thread = ThreadSummary {
            id: tid as i32,
            title,
            auto_title,
            message_count,
            unread_count,
            participants,
            is_user_renamed,
        };
        Ok(Some(ThreadView { thread, messages }))
    }

    /// スレッドにアプリ独自タイトルを付ける（再件名）。
    pub fn thread_rename(&self, thread_id: i64, title: Option<&str>) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        let clean = title.map(str::trim).filter(|s| !s.is_empty());
        conn.execute(
            "UPDATE logical_threads SET title = ?2, is_user_renamed = ?3 WHERE id = ?1",
            params![thread_id, clean, clean.is_some() as i64],
        )?;
        Ok(())
    }

    /// メールを別スレッドへ切り出す（手動分割）。
    /// mode="this" は指定メールのみ、mode="below" は指定メール以降（同スレッド内で日時が同じか新しい）を移す。
    /// 新スレッド id を返す。
    pub fn thread_split(&self, email_id: i64, mode: &str) -> rusqlite::Result<i64> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let (account_id, old_thread, date_ts, subject): (
            i64,
            Option<i64>,
            Option<i64>,
            Option<String>,
        ) = tx.query_row(
            "SELECT account_id, logical_thread_id, date_ts, subject FROM emails WHERE id = ?1",
            params![email_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        // 新スレッド（root_key=NULL＝手動。再解析で動かさない）。
        tx.execute(
            "INSERT INTO logical_threads (account_id, root_key, auto_title) VALUES (?1, NULL, ?2)",
            params![account_id, subject],
        )?;
        let new_thread = tx.last_insert_rowid();
        // 対象メールを集める。
        let targets: Vec<i64> = if mode == "below" {
            let mut stmt = tx.prepare(
                "SELECT id FROM emails
                 WHERE logical_thread_id = ?1 AND (date_ts >= ?2 OR id = ?3)",
            )?;
            let dt = date_ts.unwrap_or(0);
            let rows = stmt
                .query_map(params![old_thread, dt, email_id], |r| r.get(0))?
                .collect::<rusqlite::Result<_>>()?;
            rows
        } else {
            vec![email_id]
        };
        {
            let mut up = tx.prepare(
                "UPDATE emails SET logical_thread_id = ?2, thread_assignment = 'manual' WHERE id = ?1",
            )?;
            for id in &targets {
                up.execute(params![id, new_thread])?;
            }
        }
        recompute_thread(&tx, new_thread)?;
        super::emails::recompute_reps_for_thread(&tx, new_thread)?;
        if let Some(t) = old_thread {
            recompute_thread(&tx, t)?;
            super::emails::recompute_reps_for_thread(&tx, t)?;
        }
        tx.commit()?;
        Ok(new_thread)
    }

    /// 2 つの論理スレッドを結合する（source を target へ。手動固定）。
    pub fn thread_merge(&self, source_thread: i64, target_thread: i64) -> rusqlite::Result<()> {
        if source_thread == target_thread {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        tx.execute(
            "UPDATE emails SET logical_thread_id = ?2, thread_assignment = 'manual'
             WHERE logical_thread_id = ?1",
            params![source_thread, target_thread],
        )?;
        recompute_thread(&tx, source_thread)?; // 空になるので削除される
        recompute_thread(&tx, target_thread)?;
        super::emails::recompute_reps_for_thread(&tx, target_thread)?;
        tx.commit()?;
        Ok(())
    }

    /// メール 1 通を指定スレッドへ付け替える（手動固定）。
    pub fn message_reassign(&self, email_id: i64, target_thread: i64) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let old: Option<i64> = tx
            .query_row(
                "SELECT logical_thread_id FROM emails WHERE id = ?1",
                params![email_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        tx.execute(
            "UPDATE emails SET logical_thread_id = ?2, thread_assignment = 'manual' WHERE id = ?1",
            params![email_id, target_thread],
        )?;
        if let Some(o) = old {
            if o != target_thread {
                recompute_thread(&tx, o)?;
                super::emails::recompute_reps_for_thread(&tx, o)?;
            }
        }
        recompute_thread(&tx, target_thread)?;
        super::emails::recompute_reps_for_thread(&tx, target_thread)?;
        tx.commit()?;
        Ok(())
    }

    /// アカウントの auto 割当を作り直す（manual は保持）。古い順に処理して親継承を効かせる。
    pub fn rebuild_threads(&self, account_id: i64) -> rusqlite::Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        rebuild_threads_conn(&tx, account_id)?;
        tx.commit()?;
        Ok(())
    }

    /// 取り込み後のローカル加工パス: まだスレッド未割当（logical_thread_id IS NULL）の auto メールに
    /// スレッドを割り当て、代表フラグを立てる。ネットワーク不要。処理件数を返す。
    /// UI 用の接続（self.conn）を長時間ロックしないよう別接続で・小分けに実行する（WAL で並行読み取り可）。
    pub fn process_pending(&self, account_id: i64) -> rusqlite::Result<usize> {
        process_pending_at(&self.path(), account_id)
    }

    /// ローカル再加工（再ダウンロード不要）: 保存済み本文から clean_body・引用・fingerprint を作り直し、
    /// スレッド割当と代表フラグを全面的に貼り直す。パーサ改良を既存メールへ反映する用途。処理件数を返す。
    /// 別接続で実行し UI をブロックしない。
    pub fn reprocess_all(&self, account_id: i64) -> rusqlite::Result<usize> {
        let mut conn = self.open_worker_conn()?;
        reprocess_all_conn(&mut conn, account_id)
    }

    /// 加工用の作業接続を開く（UI 用の self.conn とは別。WAL で並行。busy_timeout つき）。
    fn open_worker_conn(&self) -> rusqlite::Result<Connection> {
        let conn = Connection::open(self.path())?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
        Ok(conn)
    }
}

/// 取り込み後のローカル加工を db_path から実行する（同期のブロッキングスレッドから呼べる）。
/// 別接続・200件ずつのトランザクションで、UI 用接続を長時間ロックしない。
pub fn process_pending_at(
    db_path: &std::path::Path,
    account_id: i64,
) -> rusqlite::Result<usize> {
    let mut conn = Connection::open(db_path)?;
    conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA busy_timeout=5000;")?;
    let ids: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM emails
             WHERE account_id = ?1 AND logical_thread_id IS NULL
               AND COALESCE(thread_assignment,'auto') <> 'manual'
             ORDER BY date_ts ASC, id ASC",
        )?;
        let rows = stmt
            .query_map(params![account_id], |r| r.get(0))?
            .collect::<rusqlite::Result<Vec<i64>>>()?;
        rows
    };
    let total = ids.len();
    // 小さめのチャンク＋チャンク間で書き込みロックを一瞬手放す。メタ索引バックフィルで
    // 大量の未スレッド行が来ても、UI の書き込み（既読化・会話を開く際の割当）が割り込めるように
    // する（WAL の書き込みは 1 つずつ。長いトランザクションで握り続けると UI が待たされる）。
    for chunk in ids.chunks(50) {
        let tx = conn.transaction()?;
        for id in chunk {
            assign_thread(&tx, *id)?;
            super::emails::maintain_folder_rep_on_insert(&tx, *id)?;
        }
        tx.commit()?;
        // 次のチャンクまで少し待って、待機中の UI 書き込みにロックを譲る（背景処理なので遅くて可）。
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
    Ok(total)
}

/// reprocess_all の本体（接続を受け取る版。テストからも使う）。
pub fn reprocess_all_conn(conn: &mut Connection, account_id: i64) -> rusqlite::Result<usize> {
    // (1) 保存済み body_plain から clean_body / body_fingerprint / FTS / message_quotes を再生成（200件ずつ）。
    //     併せて保存済み raw_headers から reply_to を後付けし、HTML 専用メールで 1 行に潰れた
    //     body_plain は保存済み HTML から改行を復元して貼り直す（機能追加前の既存メール向け）。
    let rows: Vec<(i64, Option<String>, Option<String>, Option<Vec<u8>>)> = {
        let mut stmt = conn.prepare(
            "SELECT id, body_plain, raw_headers, body_html_z FROM emails WHERE account_id = ?1",
        )?;
        let mapped = stmt
            .query_map(params![account_id], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })?;
        mapped.collect::<rusqlite::Result<_>>()?
    };
    let count = rows.len();
    for chunk in rows.chunks(200) {
        let tx = conn.transaction()?;
        {
            let mut up =
                tx.prepare("UPDATE emails SET clean_body = ?2, body_fingerprint = ?3 WHERE id = ?1")?;
            let mut upfts = tx.prepare("UPDATE email_fts SET clean_body = ?1 WHERE rowid = ?2")?;
            let mut delq = tx.prepare("DELETE FROM message_quotes WHERE email_id = ?1")?;
            let mut insq = tx.prepare(
                "INSERT INTO message_quotes (email_id, block_order, quoted_from, quoted_at, fingerprint)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            // 抽出できたときだけ上書き（raw_headers 無し＝抽出 None のとき既存値を消さない）。
            let mut uprt =
                tx.prepare("UPDATE emails SET reply_to = COALESCE(?2, reply_to) WHERE id = ?1")?;
            let mut upbp = tx.prepare("UPDATE emails SET body_plain = ?2 WHERE id = ?1")?;
            for (id, body, headers, html_z) in chunk {
                if let Some(h) = headers {
                    let rt = crate::services::parser::reply_to_from_headers(h);
                    uprt.execute(params![id, rt])?;
                }
                // HTML 専用メールで潰れた body_plain を、保存済み HTML から改行付きで作り直す。
                let recovered: Option<String> = match (body, html_z) {
                    (Some(bp), Some(hz)) if crate::services::parser::is_flattened_plaintext(bp) => {
                        crate::services::compress::decompress_text(hz)
                            .ok()
                            .map(|html| crate::services::parser::html_to_text(&html))
                            .filter(|fixed| !fixed.trim().is_empty())
                    }
                    _ => None,
                };
                if let Some(fixed) = &recovered {
                    upbp.execute(params![id, fixed])?;
                }
                let Some(body) = recovered.as_deref().or(body.as_deref()) else {
                    continue;
                };
                let split = crate::services::quotes::split_reply(body);
                let fp = if split.clean.trim().is_empty() {
                    None
                } else {
                    Some(crate::services::quotes::fingerprint(&split.clean))
                };
                up.execute(params![id, split.clean, fp])?;
                upfts.execute(params![split.clean, id])?;
                delq.execute(params![id])?;
                for q in &split.quotes {
                    insq.execute(params![id, q.order, q.quoted_from, q.quoted_at, q.fingerprint])?;
                }
            }
        }
        tx.commit()?;
    }
    // (2) スレッド割当・代表フラグを作り直す（1 トランザクション）。
    let tx = conn.transaction()?;
    rebuild_threads_conn(&tx, account_id)?;
    tx.commit()?;
    Ok(count)
}

/// 文字列キー（Message-ID）の Union-Find。参照（In-Reply-To/References）で繋がる
/// Message-ID 群を 1 コンポーネントに束ねる（jwz 式スレッド化）。
#[derive(Default)]
struct UnionFind {
    parent: std::collections::HashMap<String, String>,
}

impl UnionFind {
    /// x の根を返す（未登録なら x 自身を根として登録）。経路圧縮つき。
    fn find(&mut self, x: &str) -> String {
        let mut root = x.to_string();
        loop {
            match self.parent.get(&root) {
                None => {
                    self.parent.insert(root.clone(), root.clone());
                    break;
                }
                Some(p) if *p == root => break,
                Some(p) => root = p.clone(),
            }
        }
        // 経路圧縮
        let mut cur = x.to_string();
        while cur != root {
            let next = self
                .parent
                .insert(cur, root.clone())
                .unwrap_or_else(|| root.clone());
            cur = next;
        }
        root
    }

    fn union(&mut self, a: &str, b: &str) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent.insert(ra, rb);
        }
    }
}

/// rebuild_threads の本体（接続を受け取る版。reprocess_all からも使う）。
fn rebuild_threads_conn(conn: &Connection, account_id: i64) -> rusqlite::Result<()> {
    // auto メールの割当を一旦外す。
    conn.execute(
        "UPDATE emails SET logical_thread_id = NULL, thread_id = NULL
         WHERE account_id = ?1 AND COALESCE(thread_assignment,'auto') <> 'manual'",
        params![account_id],
    )?;
    // メンバーが居なくなった auto スレッド（root_key 非 NULL）を掃除する。
    conn.execute(
        "DELETE FROM logical_threads
         WHERE account_id = ?1 AND root_key IS NOT NULL
           AND id NOT IN (SELECT logical_thread_id FROM emails WHERE logical_thread_id IS NOT NULL)",
        params![account_id],
    )?;
    // 参照（In-Reply-To / References）で繋がるメールを Union-Find で 1 スレッドに束ねる。
    // References の並びが非標準なクライアント（Spark/titan.email 等）でも、共有する Message-ID が
    // 1 つでもあれば同じスレッドにまとまる（同一件名内の断片化を解消。件名が変われば別スレッド）。
    struct Row {
        id: i64,
        message_id: Option<String>,
        in_reply_to: Option<String>,
        references_ids: Option<String>,
        subject: Option<String>,
        from_address: Option<String>,
        to_addresses: Option<String>,
        folder: Option<String>,
    }
    let rows: Vec<Row> = {
        let mut stmt = conn.prepare(
            "SELECT id, message_id, in_reply_to, references_ids, subject, from_address, to_addresses, folder
             FROM emails
             WHERE account_id = ?1 AND COALESCE(thread_assignment,'auto') <> 'manual'
             ORDER BY date_ts ASC, id ASC",
        )?;
        let out: Vec<Row> = stmt
            .query_map(params![account_id], |r| {
                Ok(Row {
                    id: r.get(0)?,
                    message_id: r.get(1)?,
                    in_reply_to: r.get(2)?,
                    references_ids: r.get(3)?,
                    subject: r.get(4)?,
                    from_address: r.get(5)?,
                    to_addresses: r.get(6)?,
                    folder: r.get(7)?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        out
    };

    // self_key: Message-ID があればそれ、無ければ "id:{id}"。
    let self_key = |row: &Row| -> String {
        row.message_id
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("id:{}", row.id))
    };

    let mut uf = UnionFind::default();
    for row in &rows {
        let sk = self_key(row);
        uf.find(&sk); // ノード登録
        if let Some(irt) = row
            .in_reply_to
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            uf.union(&sk, irt);
        }
        if let Some(refs) = row.references_ids.as_deref() {
            for t in refs.split_whitespace() {
                uf.union(&sk, t);
            }
        }
    }

    // コンポーネントごとの正準キー＝メンバー self_key の最小（決定的・安定）。
    let mut canon: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for row in &rows {
        let sk = self_key(row);
        let root = uf.find(&sk);
        canon
            .entry(root)
            .and_modify(|c| {
                if sk < *c {
                    *c = sk.clone();
                }
            })
            .or_insert(sk);
    }

    // 割当。ヘッダ（Message-ID/In-Reply-To/References）が全く無いメールだけ、従来どおり
    // 件名正規化＋相手アドレスでフォールバックする（控えめ）。
    for row in &rows {
        let has_headers = row
            .message_id
            .as_deref()
            .map(str::trim)
            .is_some_and(|s| !s.is_empty())
            || row
                .in_reply_to
                .as_deref()
                .map(str::trim)
                .is_some_and(|s| !s.is_empty())
            || row
                .references_ids
                .as_deref()
                .is_some_and(|s| s.split_whitespace().next().is_some());
        let root_key = if has_headers {
            let sk = self_key(row);
            let root = uf.find(&sk);
            format!("uf:{}", canon.get(&root).cloned().unwrap_or(sk))
        } else {
            let counterparty = if matches!(row.folder.as_deref(), Some("sent") | Some("drafts")) {
                bare_addr(row.to_addresses.as_deref())
            } else {
                bare_addr(row.from_address.as_deref())
            }
            .unwrap_or_default();
            format!("subj:{}#{}", normalize_subject(row.subject.as_deref()), counterparty)
        };
        let tid = find_or_create_thread(conn, account_id, &root_key, row.subject.as_deref())?;
        conn.execute(
            "UPDATE emails SET logical_thread_id = ?2, thread_id = ?3, thread_assignment = 'auto'
             WHERE id = ?1",
            params![row.id, tid, root_key],
        )?;
    }
    // 代表フラグをアカウント全体で貼り直す（各 (スレッド/単独, フォルダ) の最新のみ 1）。
    conn.execute(
        "UPDATE emails SET is_folder_rep = 0 WHERE account_id = ?1",
        params![account_id],
    )?;
    conn.execute(
        "UPDATE emails SET is_folder_rep = 1 WHERE id IN (
            SELECT id FROM (
                SELECT id, ROW_NUMBER() OVER (
                    PARTITION BY COALESCE(logical_thread_id, -id), folder
                    ORDER BY date_ts DESC, id DESC) AS rn
                FROM emails WHERE account_id = ?1
            ) WHERE rn = 1)",
        params![account_id],
    )?;
    Ok(())
}

/// 取り込み後のローカル加工（接続版・テスト用）: 未割当（NULL）の auto メールに、古い順で
/// スレッド割当＋代表フラグ更新を行う。処理件数を返す。本番は Store::process_pending（別接続・小分け）。
#[cfg(test)]
pub fn process_pending_conn(conn: &Connection, account_id: i64) -> rusqlite::Result<usize> {
    let ids: Vec<i64> = {
        let mut stmt = conn.prepare(
            "SELECT id FROM emails
             WHERE account_id = ?1 AND logical_thread_id IS NULL
               AND COALESCE(thread_assignment,'auto') <> 'manual'
             ORDER BY date_ts ASC, id ASC",
        )?;
        let rows = stmt
            .query_map(params![account_id], |r| r.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        rows
    };
    for id in &ids {
        assign_thread(conn, *id)?;
        super::emails::maintain_folder_rep_on_insert(conn, *id)?;
    }
    Ok(ids.len())
}

/// アドレスに一致する住所録の表示名（emails.rs の同名ヘルパと同等の軽量版）。
fn contact_name_for(conn: &Connection, address: Option<&str>) -> rusqlite::Result<Option<String>> {
    let Some(addr) = address.map(str::trim).filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let lower = addr.to_lowercase();
    conn.query_row(
        "SELECT display_name FROM contacts c
         WHERE c.deleted_at IS NULL
            AND (lower(c.email) = ?1
                 OR EXISTS (SELECT 1 FROM contact_emails ce
                            WHERE ce.contact_id = c.id AND lower(ce.value) = ?1))
         ORDER BY c.is_favorite DESC LIMIT 1",
        params![lower],
        |r| r.get::<_, Option<String>>(0),
    )
    .optional()
    .map(Option::flatten)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::store::{insert_email, migrations, NewEmail};
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn test_store() -> Store {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrations::run(&conn).unwrap();
        conn.execute(
            "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'me@example.com','i','s')",
            [],
        )
        .unwrap();
        Store {
            conn: Mutex::new(conn),
            path: Mutex::new(PathBuf::new()),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn mk(
        mid: &str,
        subject: &str,
        from: &str,
        folder: &str,
        ts: i64,
        irt: Option<&str>,
        refs: Option<&str>,
    ) -> NewEmail {
        NewEmail {
            account_id: 1,
            message_id: Some(mid.to_string()),
            canonical_key: mid.to_string(),
            subject: Some(subject.to_string()),
            from_address: Some(from.to_string()),
            from_name: None,
            to_addresses: None,
            to_name: None,
            reply_to: None,
            cc_addresses: None,
            date: Some(format!("2026-06-{:02}T10:00:00+09:00", ts)),
            date_ts: Some(1_767_000_000 + ts * 86400),
            body_plain: Some("body".to_string()),
            clean_body: Some("body".to_string()),
            body_html: None,
            auth_result: None,
            list_id: None,
            in_reply_to: irt.map(str::to_string),
            references_ids: refs.map(str::to_string),
            thread_index: None,
            raw_headers: None,
            quotes: vec![],
            has_attachments: false,
            is_read: true,
            uid: None,
            folder: folder.to_string(),
            attachments: vec![],
        }
    }

    /// 受信→返信→再返信が 1 スレッドに束ねられ、取得順に依存しない（新しい順取り込みでも束ねる）。
    fn seed_conversation(store: &Store) -> Vec<i64> {
        let conn = store.conn.lock().unwrap();
        // 取り込み順は「新しい順」（同期の降順）を模す: 2 → 1 → 0。
        let m2 = mk(
            "m2@x",
            "Re: 見積もりの件",
            "you@corp.com",
            "inbox",
            3,
            Some("m1@x"),
            Some("m0@x m1@x"),
        );
        let m1 = mk(
            "m1@x",
            "Re: 見積もりの件",
            "me@example.com",
            "sent",
            2,
            Some("m0@x"),
            Some("m0@x"),
        );
        let m0 = mk(
            "m0@x",
            "見積もりの件",
            "you@corp.com",
            "inbox",
            1,
            None,
            None,
        );
        let mut ids = vec![];
        for e in [m2, m1, m0] {
            match insert_email(&conn, &e).unwrap() {
                crate::services::store::InsertOutcome::Inserted(id) => ids.push(id),
                _ => panic!("expected insert"),
            }
        }
        // 取り込み後のローカル加工（スレッド割当・代表フラグ）。
        process_pending_conn(&conn, 1).unwrap();
        ids
    }

    #[test]
    fn conversation_groups_regardless_of_insert_order() {
        let store = test_store();
        let ids = seed_conversation(&store);
        // どのメールから開いても同じ 1 スレッドに 3 通が時系列で並ぶ。
        let view = store.thread_view(ids[0]).unwrap().unwrap();
        assert_eq!(view.thread.message_count, 3);
        assert_eq!(view.messages.len(), 3);
        // 時系列（古い順）: m0(相手) → m1(自分) → m2(相手)。
        assert_eq!(view.messages[0].message_id.as_deref(), Some("m0@x"));
        assert_eq!(view.messages[0].direction, "in");
        assert_eq!(view.messages[1].message_id.as_deref(), Some("m1@x"));
        assert_eq!(view.messages[1].direction, "out"); // 自分の送信＝右
        assert_eq!(view.messages[2].direction, "in");
    }

    /// References の並びが非標準（先頭が root でない・親のみ）でも、共有する Message-ID で
    /// 1 スレッドに束ねる（Spark/titan.email 等の断片化対策・union-find）。
    #[test]
    fn rebuild_union_find_groups_nonstandard_references() {
        let store = test_store();
        let ids: Vec<i64> = {
            let conn = store.conn.lock().unwrap();
            let msgs = [
                mk("m0", "件名", "you@corp.com", "inbox", 1, None, None),
                mk("m1", "Re: 件名", "me@example.com", "sent", 2, Some("m0"), Some("m0")),
                // refs 先頭が root(m0) でなく親(m1)のみ。旧ロジックは mid:m1 で別スレッドに割れる。
                mk("m2", "Re: 件名", "you@corp.com", "inbox", 3, Some("m1"), Some("m1")),
                // さらに孫。refs="m2" のみ（root も親の親も含まない）。
                mk("m3", "Re: 件名", "me@example.com", "sent", 4, Some("m2"), Some("m2")),
            ];
            msgs.iter()
                .map(|e| match insert_email(&conn, e).unwrap() {
                    crate::services::store::InsertOutcome::Inserted(id) => id,
                    _ => panic!("insert"),
                })
                .collect()
        };
        store.rebuild_threads(1).unwrap();
        let view = store.thread_view(ids[0]).unwrap().unwrap();
        assert_eq!(
            view.thread.message_count, 4,
            "非標準 References でも 4 通が 1 スレッドに束ねる"
        );
    }

    /// 参照を共有しない別件名は同じスレッドにしない（過剰結合しない）。
    #[test]
    fn rebuild_does_not_merge_unrelated_messages() {
        let store = test_store();
        let ids: Vec<i64> = {
            let conn = store.conn.lock().unwrap();
            let msgs = [
                mk("a0", "見積もりA", "you@corp.com", "inbox", 1, None, None),
                mk("b0", "見積もりB", "you@corp.com", "inbox", 2, None, None),
            ];
            msgs.iter()
                .map(|e| match insert_email(&conn, e).unwrap() {
                    crate::services::store::InsertOutcome::Inserted(id) => id,
                    _ => panic!("insert"),
                })
                .collect()
        };
        store.rebuild_threads(1).unwrap();
        assert_eq!(store.thread_view(ids[0]).unwrap().unwrap().thread.message_count, 1);
        assert_eq!(store.thread_view(ids[1]).unwrap().unwrap().thread.message_count, 1);
    }

    #[test]
    fn split_below_moves_tail_to_new_thread() {
        let store = test_store();
        let ids = seed_conversation(&store);
        // ids = [m2, m1, m0]（挿入順）。m1 以降（m1, m2）を新スレッドへ切り出す。
        let m1_id = ids[1];
        let new_tid = store.thread_split(m1_id, "below").unwrap();
        // 元スレッドには m0 のみ、新スレッドに m1・m2。
        let orig = store.thread_view(ids[2]).unwrap().unwrap(); // m0
        assert_eq!(orig.thread.message_count, 1);
        let split = store.thread_view(m1_id).unwrap().unwrap();
        assert_eq!(split.thread.id as i64, new_tid);
        assert_eq!(split.thread.message_count, 2);
        // 手動割当は固定される。
        assert_eq!(split.messages[0].thread_assignment, "manual");
    }

    /// ② 内容照合: 未知の属性行＋インライン引用（`>` なし）が残った古いメールでも、
    /// 会話ビューで同スレッドの過去メールと一致する引用を追加で剥がす。
    #[test]
    fn thread_view_content_trims_unrecognized_quote() {
        let store = test_store();
        let child_id = {
            let conn = store.conn.lock().unwrap();
            let mut parent = mk("p@x", "件名", "you@corp.com", "inbox", 1, None, None);
            parent.body_plain = Some("見積もりの件、了解しました。".into());
            parent.clean_body = Some("見積もりの件、了解しました。".into());
            let mut child = mk("c@x", "Re: 件名", "me@example.com", "sent", 2, Some("p@x"), Some("p@x"));
            // 旧データ相当: clean_body に未知属性行＋インライン引用が残っている。
            let child_clean = "承知しました。\n\n田中 が書きました:\n見積もりの件、了解しました。";
            child.body_plain = Some(format!("{child_clean}\nよろしく"));
            child.clean_body = Some(child_clean.into());
            insert_email(&conn, &parent).unwrap();
            let cid = match insert_email(&conn, &child).unwrap() {
                crate::services::store::InsertOutcome::Inserted(id) => id,
                _ => panic!("expected insert"),
            };
            process_pending_conn(&conn, 1).unwrap();
            cid
        };
        let view = store.thread_view(child_id).unwrap().unwrap();
        let child = view
            .messages
            .iter()
            .find(|m| m.message_id.as_deref() == Some("c@x"))
            .unwrap();
        assert_eq!(child.clean_body.as_deref(), Some("承知しました。"));
        assert!(child.has_quotes);
    }

    #[test]
    fn references_chain_builds_from_parent() {
        let store = test_store();
        seed_conversation(&store);
        // m1（自分の返信）宛の References は「m0 m1」。それに返信する新規は親 m1 の連鎖を積む。
        let chain = store.references_chain_for(Some("m1@x")).unwrap().unwrap();
        assert_eq!(chain, "m0@x m1@x");
        // 手元に無い親でも、その ID 単体は参照する。
        let chain2 = store
            .references_chain_for(Some("unknown@x"))
            .unwrap()
            .unwrap();
        assert_eq!(chain2, "unknown@x");
    }

    #[test]
    fn normalize_subject_strips_prefixes() {
        assert_eq!(
            normalize_subject(Some("Re: Fwd: 見積もりの件")),
            "見積もりの件"
        );
        assert_eq!(normalize_subject(Some("RE：転送: Hello")), "hello");
        assert_eq!(
            normalize_subject(Some("  お世話になります ")),
            "お世話になります"
        );
    }

    #[test]
    fn bare_addr_extracts_email() {
        assert_eq!(
            bare_addr(Some("山田 <Yamada@Example.com>")).as_deref(),
            Some("yamada@example.com")
        );
        assert_eq!(
            bare_addr(Some("a@b.com, c@d.com")).as_deref(),
            Some("a@b.com")
        );
        assert_eq!(bare_addr(Some("")), None);
    }
}
