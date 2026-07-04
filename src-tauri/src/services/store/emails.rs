use super::Store;
use crate::models::{AttachmentSummary, MailDetail, MailSummary, ThreadListItem};
use rusqlite::{params, Connection, OptionalExtension};

/// メール挿入用（内部）。
pub struct NewEmail {
    pub account_id: i64,
    pub message_id: Option<String>,
    pub canonical_key: String,
    pub subject: Option<String>,
    pub from_address: Option<String>,
    /// 差出人の表示名（ヘッダ From の名前部。無ければ None）。
    pub from_name: Option<String>,
    pub to_addresses: Option<String>,
    /// 宛先（先頭）の表示名（ヘッダ To の名前部。無ければ None）。
    pub to_name: Option<String>,
    /// Cc の全アドレス（"名前 <addr>, ..." の表示用文字列。無ければ None）。
    pub cc_addresses: Option<String>,
    pub date: Option<String>,
    /// 並び替え用の epoch 秒（date の UTC 換算）。インデックスで新しい順に引くのに使う。
    pub date_ts: Option<i64>,
    pub body_plain: Option<String>,
    pub clean_body: Option<String>,
    pub body_html: Option<String>,
    /// Authentication-Results 生テキスト（SPF/DKIM/DMARC。docs/SPAM.md §7.7）。
    pub auth_result: Option<String>,
    /// List-Id 生テキスト（メルマガ/ML 判定。docs/SPAM.md §7.7）。
    pub list_id: Option<String>,
    /// In-Reply-To（返信元 Message-ID。山括弧なし）。docs/THREADING.md §2。
    pub in_reply_to: Option<String>,
    /// References（祖先 Message-ID の連鎖。空白区切り・山括弧なし・古い順）。
    pub references_ids: Option<String>,
    /// Thread-Index（Outlook/Exchange の会話ツリー）。
    pub thread_index: Option<String>,
    /// ヘッダ部の生テキスト（解析やり直し用）。
    pub raw_headers: Option<String>,
    /// 引用ブロック（属性行から from+時刻、本文から fingerprint）。message_quotes へ保存。
    pub quotes: Vec<NewQuote>,
    pub has_attachments: bool,
    /// サーバー上の既読状態（IMAP \Seen フラグ）。未読数をサーバーと一致させる。
    pub is_read: bool,
    /// メッセージの IMAP UID（添付のオンデマンド再取得用）。
    pub uid: Option<i64>,
    /// 保存先フォルダ（'inbox' | 'sent' | 'drafts' | 'trash' | 'spam'）。
    pub folder: String,
    /// 添付メタ（本体は未取得。ダウンロード時に再取得）。
    pub attachments: Vec<NewAttachment>,
}

/// フォルダをまたいで同じ Message-ID が存在する（自分宛て＝受信＋送信済 等）ため、
/// 重複排除キー(canonical_key)は非 inbox フォルダでは "folder:key" と接頭辞を付けて
/// フォルダごとに別レコードとして扱う（UNIQUE(account_id, canonical_key) を活かしたまま）。
fn folder_key(folder: &str, canonical_key: &str) -> String {
    if folder == "inbox" {
        canonical_key.to_string()
    } else {
        format!("{folder}:{canonical_key}")
    }
}

/// 引用ブロック挿入用（内部）。message_quotes に対応。docs/THREADING.md §7。
pub struct NewQuote {
    pub order: i64,
    pub quoted_from: Option<String>,
    pub quoted_at: Option<String>,
    pub fingerprint: String,
}

/// 添付メタ挿入用（内部）。
pub struct NewAttachment {
    pub part_index: i64,
    pub filename: String,
    pub content_type: Option<String>,
    pub size: i64,
    pub kind: &'static str,
    pub content_id: Option<String>,
}

/// オンデマンド再取得に必要な情報（添付＋親メール）。
pub struct AttachmentFetchInfo {
    pub account_id: i64,
    /// 親メールの IMAP UID（None なら再取得不可＝要再同期）。
    pub email_uid: Option<i64>,
    pub part_index: i64,
    pub filename: String,
    /// 取得済みの保存先（未取得なら None）。
    pub file_path: Option<String>,
}

/// 添付メタ（本体は file_path NULL = 未取得）を一括挿入する。
fn insert_attachments(
    conn: &Connection,
    email_id: i64,
    atts: &[NewAttachment],
) -> rusqlite::Result<()> {
    if atts.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare(
        "INSERT INTO attachments (email_id, filename, content_type, size, part_index, kind, content_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
    )?;
    for a in atts {
        stmt.execute(params![
            email_id,
            a.filename,
            a.content_type,
            a.size,
            a.part_index,
            a.kind,
            a.content_id
        ])?;
    }
    Ok(())
}

/// メール挿入の結果。
pub enum InsertOutcome {
    /// 新規挿入した（新しい email id）。
    Inserted(i64),
    /// 既存メールに uid/添付メタを埋め戻した。
    Backfilled,
    /// 既存メールで変更なし。
    Unchanged,
}

/// 接続を直接受け取る挿入（同期スレッドの別接続から使うため）。
/// 新規なら挿入して Inserted を返す。重複（account_id, canonical_key）の場合は
/// 新規挿入はしないが、機能追加前に取り込んだ古いメールでも添付が使えるよう、
/// uid と添付メタが未設定なら埋め戻して（バックフィル）Backfilled を返す。
pub fn insert_email(conn: &Connection, e: &NewEmail) -> rusqlite::Result<InsertOutcome> {
    // 表示専用の HTML 本文は zstd 圧縮して BLOB 列へ（TEXT の body_html は使わない）。
    let body_html_z = e
        .body_html
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(crate::services::compress::compress_text);
    // フォルダごとに別レコードにするため canonical_key はフォルダ接頭辞付きで保存する。
    let key = folder_key(&e.folder, &e.canonical_key);
    // 新規本文の fingerprint（引用照合・重複ヒントの手掛かり）。
    let body_fingerprint = e
        .clean_body
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .map(crate::services::quotes::fingerprint);
    let changed = conn.execute(
        "INSERT OR IGNORE INTO emails
           (account_id, message_id, canonical_key, subject, from_address, from_name, to_addresses, to_name, cc_addresses, date, date_ts, has_attachments, body_plain, clean_body, body_html_z, uid, auth_result, list_id, folder, is_read, in_reply_to, references_ids, thread_index, raw_headers, body_fingerprint)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20, ?21, ?22, ?23, ?24, ?25)",
        params![
            e.account_id,
            e.message_id,
            key,
            e.subject,
            e.from_address,
            e.from_name,
            e.to_addresses,
            e.to_name,
            e.cc_addresses,
            e.date,
            e.date_ts,
            e.has_attachments as i64,
            e.body_plain,
            e.clean_body,
            body_html_z,
            e.uid,
            e.auth_result,
            e.list_id,
            e.folder,
            e.is_read as i64,
            e.in_reply_to,
            e.references_ids,
            e.thread_index,
            e.raw_headers,
            body_fingerprint,
        ],
    )?;
    if changed == 0 {
        // 既存メール: uid / 添付メタ / スレッド用ヘッダを埋め戻す（再同期での後付け）。
        let did = backfill_existing(conn, e)?;
        return Ok(if did {
            InsertOutcome::Backfilled
        } else {
            InsertOutcome::Unchanged
        });
    }
    let id = conn.last_insert_rowid();
    // FTS5（rowid = emails.id）
    conn.execute(
        "INSERT INTO email_fts(rowid, subject, from_address, clean_body) VALUES (?1, ?2, ?3, ?4)",
        params![id, e.subject, e.from_address, e.clean_body],
    )?;
    insert_attachments(conn, id, &e.attachments)?;
    insert_quotes(conn, id, &e.quotes)?;
    // 取り込み（download＋保存）はここまで。スレッド割当・代表フラグ（＝他メール参照が要る
    // クロス処理）は接続を閉じた後のローカル加工パス process_pending で行う（docs/THREADING.md §5）。
    // ここで logical_thread_id は NULL のまま＝一覧では 1 通ずつ即表示され、加工後に束ねられる。
    Ok(InsertOutcome::Inserted(id))
}

/// 新規挿入したメール 1 通について、その (スレッド, フォルダ) の代表フラグを更新する（O(1)）。
/// 挿入直後は is_folder_rep=1（既定）。既存代表より新しければ既存を降格、古ければ自分を降格。
pub fn maintain_folder_rep_on_insert(conn: &Connection, email_id: i64) -> rusqlite::Result<()> {
    let row: Option<(Option<i64>, String, Option<i64>)> = conn
        .query_row(
            "SELECT logical_thread_id, folder, date_ts FROM emails WHERE id = ?1",
            params![email_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    // スレッド未割当（NULL）は各自が代表（既定 1 のまま）。
    let Some((Some(tid), folder, dts)) = row else {
        return Ok(());
    };
    let dts = dts.unwrap_or(0);
    // 自分以外の現代表（この thread+folder）。通常 1 件。
    let prev: Option<(i64, i64)> = conn
        .query_row(
            "SELECT id, COALESCE(date_ts, 0) FROM emails
             WHERE logical_thread_id = ?1 AND folder = ?2 AND is_folder_rep = 1 AND id <> ?3
             ORDER BY date_ts DESC, id DESC LIMIT 1",
            params![tid, folder, email_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((pid, pts)) = prev {
        if (dts, email_id) > (pts, pid) {
            // 自分が新しい → 旧代表を降格（自分は既定 1 のまま）。
            conn.execute(
                "UPDATE emails SET is_folder_rep = 0 WHERE id = ?1",
                params![pid],
            )?;
        } else {
            // 自分が古い → 自分を降格。
            conn.execute(
                "UPDATE emails SET is_folder_rep = 0 WHERE id = ?1",
                params![email_id],
            )?;
        }
    }
    Ok(())
}

/// 指定スレッドの各フォルダについて、代表フラグを 1 つだけ（最新）に貼り直す（堅牢版）。
/// 削除・手動の分割/結合/再割当・ヘッダ後付けの再割当など、任意の状態から正す用途。
pub fn recompute_reps_for_thread(conn: &Connection, thread_id: i64) -> rusqlite::Result<()> {
    let folders: Vec<String> = {
        let mut stmt =
            conn.prepare("SELECT DISTINCT folder FROM emails WHERE logical_thread_id = ?1")?;
        let rows = stmt.query_map(params![thread_id], |r| r.get::<_, String>(0))?;
        rows.collect::<rusqlite::Result<_>>()?
    };
    for folder in folders {
        // まず当該 (thread, folder) の代表を全て降格し、最新 1 通だけ立てる。
        conn.execute(
            "UPDATE emails SET is_folder_rep = 0
             WHERE logical_thread_id = ?1 AND folder = ?2 AND is_folder_rep = 1",
            params![thread_id, folder],
        )?;
        conn.execute(
            "UPDATE emails SET is_folder_rep = 1 WHERE id = (
                SELECT id FROM emails WHERE logical_thread_id = ?1 AND folder = ?2
                ORDER BY date_ts DESC, id DESC LIMIT 1)",
            params![thread_id, folder],
        )?;
    }
    Ok(())
}

/// 引用ブロック（message_quotes）を一括挿入する。
fn insert_quotes(conn: &Connection, email_id: i64, quotes: &[NewQuote]) -> rusqlite::Result<()> {
    if quotes.is_empty() {
        return Ok(());
    }
    let mut stmt = conn.prepare(
        "INSERT INTO message_quotes (email_id, block_order, quoted_from, quoted_at, fingerprint)
         VALUES (?1, ?2, ?3, ?4, ?5)",
    )?;
    for q in quotes {
        stmt.execute(params![
            email_id,
            q.order,
            q.quoted_from,
            q.quoted_at,
            q.fingerprint
        ])?;
    }
    Ok(())
}

/// 既存メールに uid と添付メタを埋め戻す（再同期で古いメールを後付け対応）。
/// 何か変更したら true を返す。
fn backfill_existing(conn: &Connection, e: &NewEmail) -> rusqlite::Result<bool> {
    let key = folder_key(&e.folder, &e.canonical_key);
    let id: Option<i64> = conn
        .query_row(
            "SELECT id FROM emails WHERE account_id = ?1 AND canonical_key = ?2",
            params![e.account_id, key],
            |r| r.get(0),
        )
        .optional()?;
    let Some(id) = id else { return Ok(false) };
    let mut touched = false;

    // uid が未設定なら設定する（オンデマンド再取得に必要）。
    if e.uid.is_some() {
        let n = conn.execute(
            "UPDATE emails SET uid = ?1 WHERE id = ?2 AND uid IS NULL",
            params![e.uid, id],
        )?;
        touched |= n > 0;
    }
    // サーバーで既読（\Seen）なら既読に補正する（既読→未読の巻き戻しはしない。
    // ローカルで開いた既読はサーバーへ送っていないため、逆方向は消えてしまう）。
    if e.is_read {
        let n = conn.execute(
            "UPDATE emails SET is_read = 1 WHERE id = ?1 AND is_read = 0",
            params![id],
        )?;
        touched |= n > 0;
    }
    // 表示名（ヘッダ From/To の名前部）を後付けする（未設定のときだけ）。
    if e.from_name.is_some() {
        let n = conn.execute(
            "UPDATE emails SET from_name = ?1 WHERE id = ?2 AND from_name IS NULL",
            params![e.from_name, id],
        )?;
        touched |= n > 0;
    }
    if e.to_name.is_some() {
        let n = conn.execute(
            "UPDATE emails SET to_name = ?1 WHERE id = ?2 AND to_name IS NULL",
            params![e.to_name, id],
        )?;
        touched |= n > 0;
    }
    // Cc を後付けする（この機能の追加前に取り込んだ古いメールでも、再取り込みで Cc を表示できる）。
    if e.cc_addresses.is_some() {
        let n = conn.execute(
            "UPDATE emails SET cc_addresses = ?1 WHERE id = ?2 AND cc_addresses IS NULL",
            params![e.cc_addresses, id],
        )?;
        touched |= n > 0;
    }
    // ヘッダ素性（§7.7）を後付けする（機能追加前に取り込んだ古いメール向け）。
    if e.auth_result.is_some() {
        let n = conn.execute(
            "UPDATE emails SET auth_result = ?1 WHERE id = ?2 AND auth_result IS NULL",
            params![e.auth_result, id],
        )?;
        touched |= n > 0;
    }
    if e.list_id.is_some() {
        let n = conn.execute(
            "UPDATE emails SET list_id = ?1 WHERE id = ?2 AND list_id IS NULL",
            params![e.list_id, id],
        )?;
        touched |= n > 0;
    }
    // スレッド用ヘッダ（この機能の追加前に取り込んだ古いメール向け。点検再取り込みで後付け）。
    let mut header_backfilled = false;
    for (col, val) in [
        ("in_reply_to", &e.in_reply_to),
        ("references_ids", &e.references_ids),
        ("thread_index", &e.thread_index),
        ("raw_headers", &e.raw_headers),
    ] {
        if val.is_some() {
            let sql = format!("UPDATE emails SET {col} = ?1 WHERE id = ?2 AND {col} IS NULL");
            let n = conn.execute(&sql, params![val, id])?;
            if n > 0 {
                header_backfilled = true;
                touched = true;
            }
        }
    }
    // 引用ブロックが未保存なら入れて、スレッドを割り当て直す（束ねの精度が上がる）。
    let has_quotes: i64 = conn.query_row(
        "SELECT count(*) FROM message_quotes WHERE email_id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    if has_quotes == 0 && !e.quotes.is_empty() {
        insert_quotes(conn, id, &e.quotes)?;
        touched = true;
    }
    // ヘッダを後付けできたら、auto 割当を「未処理」に戻して次のローカル加工で引き直させる
    // （ここでは再割当しない＝取り込みは保存に徹する）。manual は動かさない。
    if header_backfilled {
        let n = conn.execute(
            "UPDATE emails SET logical_thread_id = NULL, thread_id = NULL
             WHERE id = ?1 AND COALESCE(thread_assignment,'auto') <> 'manual'",
            params![id],
        )?;
        touched |= n > 0;
    }
    // clean_body（引用/署名を除いた新規部分）を新エンジンの分離結果に更新する。
    // 旧パーサ（行頭 `>` を消すだけ）で取り込んだ古いメールは、`-----Original Message-----` や
    // `On … wrote:` の引用が本文に残ったままになっている。点検再取り込みでバブル表示を正す。
    // 全文(body_plain)は据え置き（「引用を表示」用）。
    if let Some(new_clean) = e.clean_body.as_deref() {
        let stored_clean: Option<String> = conn.query_row(
            "SELECT clean_body FROM emails WHERE id = ?1",
            params![id],
            |r| r.get(0),
        )?;
        if stored_clean.as_deref() != Some(new_clean) {
            let fp = if new_clean.trim().is_empty() {
                None
            } else {
                Some(crate::services::quotes::fingerprint(new_clean))
            };
            conn.execute(
                "UPDATE emails SET clean_body = ?1, body_fingerprint = ?2 WHERE id = ?3",
                params![new_clean, fp, id],
            )?;
            // FTS（clean_body 索引）も揃える。
            conn.execute(
                "UPDATE email_fts SET clean_body = ?1 WHERE rowid = ?2",
                params![new_clean, id],
            )?;
            touched = true;
        }
    }
    // 添付行が無ければ挿入する（重複防止）。
    let existing: i64 = conn.query_row(
        "SELECT count(*) FROM attachments WHERE email_id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    if existing == 0 && !e.attachments.is_empty() {
        insert_attachments(conn, id, &e.attachments)?;
        touched = true;
    }
    Ok(touched)
}

/// ユーザー入力を FTS5 の安全なクエリ式へ変換する。
/// 各トークンを二重引用符でくくって特殊文字（AND/OR/*/"/-/(/) 等）を無害化し、
/// 末尾に `*` を付けて前方一致にする。空白区切りは FTS5 の暗黙 AND。
/// 有効なトークンが無ければ None（＝検索対象なし）。
fn build_fts_query(input: &str) -> Option<String> {
    let expr = input
        .split_whitespace()
        // FTS5 の引用文字列内では " を "" にエスケープする。
        .map(|tok| format!("\"{}\"*", tok.replace('"', "\"\"")))
        .collect::<Vec<_>>()
        .join(" ");
    if expr.is_empty() {
        None
    } else {
        Some(expr)
    }
}

/// MailSummary を組み立てる共通行マッパ（list_emails / search_emails で共有）。
/// SELECT の列順は 0:id 1:subject 2:from_address 3:date 4:is_read 5:has_attachments
/// 6:preview 7:is_flagged 8:is_bookmarked 9:tag_ids 10:has_real 11:to_addresses
/// 12:is_known 13:is_vip 14:from_name 15:to_name 16:account_id。
fn map_mail_summary(r: &rusqlite::Row) -> rusqlite::Result<MailSummary> {
    // group_concat はカンマ区切り文字列。空（タグ無し）は None。
    let tag_ids = r
        .get::<_, Option<String>>(9)?
        .map(|s| s.split(',').filter_map(|p| p.parse::<i32>().ok()).collect())
        .unwrap_or_default();
    Ok(MailSummary {
        id: r.get::<_, i64>(0)? as i32,
        account_id: r.get::<_, i64>(16)? as i32,
        subject: r.get(1)?,
        from_address: r.get(2)?,
        from_name: r.get(14)?,
        to_addresses: r.get(11)?,
        to_name: r.get(15)?,
        date: r.get(3)?,
        is_read: r.get::<_, i64>(4)? != 0,
        has_attachments: r.get::<_, i64>(5)? != 0,
        preview: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
        is_starred: r.get::<_, i64>(7)? != 0,
        is_bookmarked: r.get::<_, i64>(8)? != 0,
        tag_ids,
        has_real_attachments: r.get::<_, i64>(10)? != 0,
        is_known: r.get::<_, i64>(12)? != 0,
        is_vip: r.get::<_, i64>(13)? != 0,
        // グリーンは行取得後にまとめて算出する（グリーン集合を 1 回だけ引くため）。
        is_green: false,
    })
}

/// 取得済みの一覧行に is_green をまとめて付与する（グリーン集合を 1 回だけ引く）。
/// is_green = 差出人が住所録本人(is_known) または 差出人ドメインがグリーン集合。
fn fill_is_green(conn: &Connection, rows: &mut [MailSummary]) -> rusqlite::Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    let set = super::greendomain::green_domain_set(conn)?;
    for m in rows.iter_mut() {
        let domain_green = m
            .from_address
            .as_deref()
            .and_then(super::greendomain::domain_of)
            .is_some_and(|d| set.contains(&d));
        m.is_green = m.is_known || domain_green;
    }
    Ok(())
}

/// アドレス（素のメールアドレス）に一致する住所録の表示名を返す。
/// contacts.email（primary）と contact_emails.value を小文字で完全一致（式インデックス）で照合。
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

/// 差出人（from）が住所録の連絡先かを判定する SELECT 断片（is_known, is_vip）。
/// `from_col` は各クエリの from_address 列（list=emails.from_address / search=e.from_address）。
/// from_address は素のメールアドレスなので、小文字化の完全一致（式インデックス）で高速に照合する。
fn known_vip_cols(from_col: &str) -> String {
    format!(
        "(EXISTS (SELECT 1 FROM contacts c WHERE c.deleted_at IS NULL AND lower(c.email) = lower({from_col})) \
          OR EXISTS (SELECT 1 FROM contact_emails ce JOIN contacts c3 ON c3.id = ce.contact_id \
                     WHERE c3.deleted_at IS NULL AND lower(ce.value) = lower({from_col}))) AS is_known, \
         (EXISTS (SELECT 1 FROM contacts c WHERE c.deleted_at IS NULL AND c.is_favorite = 1 AND lower(c.email) = lower({from_col})) \
          OR EXISTS (SELECT 1 FROM contact_emails ce JOIN contacts c2 ON c2.id = ce.contact_id \
                     WHERE c2.deleted_at IS NULL AND c2.is_favorite = 1 AND lower(ce.value) = lower({from_col}))) AS is_vip"
    )
}

impl Store {
    /// フォルダのメール一覧。`account_id` が None なら全アカウント横断（「全て」表示）。
    pub fn list_emails(
        &self,
        account_id: Option<i64>,
        folder: &str,
        limit: i64,
        offset: i64,
    ) -> rusqlite::Result<Vec<MailSummary>> {
        let conn = self.conn.lock().unwrap();
        // アカウント指定の有無で WHERE を切替える（?4 はアカウント指定時のみ）。
        let acct = if account_id.is_some() {
            "account_id = ?4 AND "
        } else {
            ""
        };
        let sql = format!(
            "SELECT id, subject, from_address, date, is_read, has_attachments,
                    substr(COALESCE(clean_body, body_plain, ''), 1, 140) AS preview,
                    is_flagged, is_bookmarked,
                    (SELECT group_concat(tag_id) FROM email_tags WHERE email_id = emails.id) AS tag_ids,
                    (emails.has_attachments = 1
                     OR EXISTS(SELECT 1 FROM attachments a WHERE a.email_id = emails.id AND COALESCE(a.kind, 'attachment') <> 'inline')) AS has_real,
                    to_addresses, {known_vip}, from_name, to_name, account_id
             FROM emails WHERE {acct}folder = ?1
             ORDER BY date_ts DESC, id DESC LIMIT ?2 OFFSET ?3",
            known_vip = known_vip_cols("emails.from_address"),
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows: Vec<MailSummary> = match account_id {
            Some(a) => stmt
                .query_map(params![folder, limit, offset, a], map_mail_summary)?
                .collect::<rusqlite::Result<_>>()?,
            None => stmt
                .query_map(params![folder, limit, offset], map_mail_summary)?
                .collect::<rusqlite::Result<_>>()?,
        };
        fill_is_green(&conn, &mut rows)?;
        Ok(rows)
    }

    /// 件名・差出人・本文（FTS5 索引）を全文検索する。
    /// `account_id` が None なら全アカウント横断。指定フォルダに限定し、前方一致・複数語 AND。
    /// 空クエリ（有効トークン無し）は空配列を返す。
    pub fn search_emails(
        &self,
        account_id: Option<i64>,
        folder: &str,
        query: &str,
        limit: i64,
    ) -> rusqlite::Result<Vec<MailSummary>> {
        let Some(fts) = build_fts_query(query) else {
            return Ok(Vec::new());
        };
        let conn = self.conn.lock().unwrap();
        let acct = if account_id.is_some() {
            "e.account_id = ?4 AND "
        } else {
            ""
        };
        let sql = format!(
            "SELECT e.id, e.subject, e.from_address, e.date, e.is_read, e.has_attachments,
                    substr(COALESCE(e.clean_body, e.body_plain, ''), 1, 140) AS preview,
                    e.is_flagged, e.is_bookmarked,
                    (SELECT group_concat(tag_id) FROM email_tags WHERE email_id = e.id) AS tag_ids,
                    (e.has_attachments = 1
                     OR EXISTS(SELECT 1 FROM attachments a WHERE a.email_id = e.id AND COALESCE(a.kind, 'attachment') <> 'inline')) AS has_real,
                    e.to_addresses, {known_vip}, e.from_name, e.to_name, e.account_id
             FROM email_fts JOIN emails e ON e.id = email_fts.rowid
             WHERE email_fts MATCH ?1 AND {acct}e.folder = ?2
             ORDER BY e.date_ts DESC, e.id DESC LIMIT ?3",
            known_vip = known_vip_cols("e.from_address"),
        );
        let mut stmt = conn.prepare(&sql)?;
        let mut rows: Vec<MailSummary> = match account_id {
            Some(a) => stmt
                .query_map(params![fts, folder, limit, a], map_mail_summary)?
                .collect::<rusqlite::Result<_>>()?,
            None => stmt
                .query_map(params![fts, folder, limit], map_mail_summary)?
                .collect::<rusqlite::Result<_>>()?,
        };
        fill_is_green(&conn, &mut rows)?;
        Ok(rows)
    }

    /// スレッド単位のメール一覧（代表＝フォルダ内最新）。docs/THREADING.md §5。
    /// 論理スレッド未割当の旧データは 1 通ずつ（負値の擬似 gkey）で扱う。
    /// message_count は全フォルダ横断のスレッド総件数、unread_count は当フォルダの未読。
    pub fn list_threads(
        &self,
        account_id: Option<i64>,
        folder: &str,
        limit: i64,
        offset: i64,
    ) -> rusqlite::Result<Vec<ThreadListItem>> {
        let conn = self.conn.lock().unwrap();
        let acct = if account_id.is_some() {
            "AND r.account_id = ?4"
        } else {
            ""
        };
        // 代表フラグ（is_folder_rep=1）を部分索引で先頭 N 件だけ引く（全走査しない）。
        // 重い集計サブクエリはその N 件に対してだけ評価される（docs/THREADING.md §5）。
        let sql = format!(
            "SELECT r.id, COALESCE(r.logical_thread_id, -r.id) AS gkey, r.account_id,
                    COALESCE(lt.title, r.subject) AS subject,
                    r.from_address, r.from_name, r.to_addresses, r.to_name, r.date,
                    substr(COALESCE(r.clean_body, r.body_plain, ''), 1, 140) AS preview,
                    r.is_flagged, r.is_bookmarked,
                    (SELECT group_concat(tag_id) FROM email_tags WHERE email_id = r.id) AS tag_ids,
                    (r.has_attachments = 1
                     OR EXISTS(SELECT 1 FROM attachments a WHERE a.email_id = r.id AND COALESCE(a.kind,'attachment') <> 'inline')) AS has_real,
                    {known_vip},
                    CASE WHEN r.logical_thread_id IS NULL THEN 1
                         ELSE (SELECT count(*) FROM emails t WHERE t.logical_thread_id = r.logical_thread_id) END AS msg_count,
                    CASE WHEN r.logical_thread_id IS NULL THEN (CASE WHEN r.is_read = 0 THEN 1 ELSE 0 END)
                         ELSE (SELECT count(*) FROM emails t WHERE t.logical_thread_id = r.logical_thread_id AND t.folder = ?1 AND t.is_read = 0) END AS unread_cnt,
                    (SELECT group_concat(t.id) FROM emails t
                     WHERE t.folder = ?1
                       AND (CASE WHEN r.logical_thread_id IS NULL THEN t.id = r.id
                                 ELSE t.logical_thread_id = r.logical_thread_id END)) AS folder_ids
             FROM emails r
             LEFT JOIN logical_threads lt ON lt.id = r.logical_thread_id
             WHERE r.folder = ?1 {acct} AND r.is_folder_rep = 1
             ORDER BY r.date_ts DESC, r.id DESC
             LIMIT ?2 OFFSET ?3",
            known_vip = known_vip_cols("r.from_address"),
        );
        let mut stmt = conn.prepare(&sql)?;
        let map = |row: &rusqlite::Row| -> rusqlite::Result<ThreadListItem> {
            let parse_ids = |s: Option<String>| -> Vec<i32> {
                s.map(|s| s.split(',').filter_map(|p| p.parse::<i32>().ok()).collect())
                    .unwrap_or_default()
            };
            // 列: 16=msg_count, 17=unread_cnt, 18=folder_ids。
            let unread_count = row.get::<_, i64>(17)? as i32;
            Ok(ThreadListItem {
                id: row.get::<_, i64>(0)? as i32,
                thread_id: row.get::<_, i64>(1)? as i32,
                account_id: row.get::<_, i64>(2)? as i32,
                subject: row.get(3)?,
                from_address: row.get(4)?,
                from_name: row.get(5)?,
                to_addresses: row.get(6)?,
                to_name: row.get(7)?,
                date: row.get(8)?,
                preview: row.get::<_, Option<String>>(9)?.unwrap_or_default(),
                is_starred: row.get::<_, i64>(10)? != 0,
                is_bookmarked: row.get::<_, i64>(11)? != 0,
                tag_ids: parse_ids(row.get(12)?),
                has_real_attachments: row.get::<_, i64>(13)? != 0,
                is_known: row.get::<_, i64>(14)? != 0,
                is_vip: row.get::<_, i64>(15)? != 0,
                is_green: false,
                message_count: row.get::<_, i64>(16)? as i32,
                unread_count,
                is_read: unread_count == 0,
                email_ids: parse_ids(row.get(18)?),
            })
        };
        let mut rows: Vec<ThreadListItem> = match account_id {
            Some(a) => stmt
                .query_map(params![folder, limit, offset, a], map)?
                .collect::<rusqlite::Result<_>>()?,
            None => stmt
                .query_map(params![folder, limit, offset], map)?
                .collect::<rusqlite::Result<_>>()?,
        };
        // グリーン（本人 or 認定ドメイン）をまとめて付与する。
        let set = super::greendomain::green_domain_set(&conn)?;
        for m in rows.iter_mut() {
            let domain_green = m
                .from_address
                .as_deref()
                .and_then(super::greendomain::domain_of)
                .is_some_and(|d| set.contains(&d));
            m.is_green = m.is_known || domain_green;
        }
        Ok(rows)
    }

    /// 指定 ID 群に対し、フラグ列（is_read / is_starred / is_bookmarked）を一括更新する。
    fn set_flag(&self, column: &str, ids: &[i64], value: bool) -> rusqlite::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        let sql = format!("UPDATE emails SET {column} = ?1 WHERE id = ?2");
        {
            let mut stmt = tx.prepare(&sql)?;
            for id in ids {
                stmt.execute(params![value as i64, id])?;
            }
        }
        tx.commit()
    }

    pub fn set_emails_read(&self, ids: &[i64], read: bool) -> rusqlite::Result<()> {
        self.set_flag("is_read", ids, read)
    }

    pub fn set_emails_starred(&self, ids: &[i64], value: bool) -> rusqlite::Result<()> {
        // お気に入り（スター）は IMAP の \Flagged に対応する is_flagged を使う。
        self.set_flag("is_flagged", ids, value)
    }

    pub fn set_emails_bookmarked(&self, ids: &[i64], value: bool) -> rusqlite::Result<()> {
        self.set_flag("is_bookmarked", ids, value)
    }

    /// メールを一括削除（FTS インデックスも削除）。
    pub fn delete_emails(&self, ids: &[i64]) -> rusqlite::Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().unwrap();
        let tx = conn.transaction()?;
        // 削除で代表が消える (スレッド,フォルダ) を後で貼り直すため、影響スレッドを控える。
        let mut affected: std::collections::HashSet<i64> = std::collections::HashSet::new();
        {
            let mut thread_of = tx.prepare("SELECT logical_thread_id FROM emails WHERE id = ?1")?;
            let mut fts = tx.prepare("DELETE FROM email_fts WHERE rowid = ?1")?;
            let mut etags = tx.prepare("DELETE FROM email_tags WHERE email_id = ?1")?;
            let mut att = tx.prepare("DELETE FROM attachments WHERE email_id = ?1")?;
            let mut del = tx.prepare("DELETE FROM emails WHERE id = ?1")?;
            for id in ids {
                if let Ok(Some(t)) = thread_of
                    .query_row(params![id], |r| r.get::<_, Option<i64>>(0))
                    .optional()
                    .map(Option::flatten)
                {
                    affected.insert(t);
                }
                fts.execute(params![id])?;
                etags.execute(params![id])?;
                att.execute(params![id])?; // FK 制約のため先に添付を削除
                del.execute(params![id])?;
            }
            for t in &affected {
                recompute_reps_for_thread(&tx, *t)?;
            }
        }
        tx.commit()
    }

    /// 指定フォルダを空にする（全メールを完全削除）。`account_id` が None なら全アカウント。
    /// 削除件数を返す。ゴミ箱/迷惑メールの「空にする」で使う。
    pub fn empty_folder(&self, account_id: Option<i64>, folder: &str) -> rusqlite::Result<i32> {
        let ids: Vec<i64> = {
            let conn = self.conn.lock().unwrap();
            match account_id {
                Some(a) => {
                    let mut stmt = conn
                        .prepare("SELECT id FROM emails WHERE folder = ?1 AND account_id = ?2")?;
                    let v: Vec<i64> = stmt
                        .query_map(params![folder, a], |r| r.get(0))?
                        .collect::<rusqlite::Result<_>>()?;
                    v
                }
                None => {
                    let mut stmt = conn.prepare("SELECT id FROM emails WHERE folder = ?1")?;
                    let v: Vec<i64> = stmt
                        .query_map(params![folder], |r| r.get(0))?
                        .collect::<rusqlite::Result<_>>()?;
                    v
                }
            }
        };
        let n = ids.len() as i32;
        self.delete_emails(&ids)?;
        Ok(n)
    }

    /// 書きかけのメールをローカルの drafts フォルダへ保存/更新する（サーバー同期は別途）。
    /// `draft_id` があれば既存行を更新、無ければ新規作成する。保存した下書きの emails.id を返す。
    /// 新規時は再送・サーバー同期で使う Message-ID を採番して保存する（更新時は据え置き）。
    pub fn save_draft(&self, d: &crate::models::DraftInput) -> rusqlite::Result<i64> {
        let now = chrono::Utc::now();
        let iso = now.to_rfc3339();
        let ts = now.timestamp();
        let to = d.to.join(", ");
        let cc = d.cc.join(", ");
        let bcc = d.bcc.join(", ");
        let conn = self.conn.lock().unwrap();
        // 差出人アドレス（一覧・検索の見出し用）はアカウントから引く。
        let from: Option<String> = conn
            .query_row(
                "SELECT email FROM accounts WHERE id = ?1",
                params![d.account_id as i64],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = d.draft_id {
            let id = id as i64;
            conn.execute(
                "UPDATE emails SET subject = ?1, to_addresses = ?2, cc_addresses = ?3, \
                   bcc_addresses = ?4, body_plain = ?5, clean_body = ?5, date = ?6, date_ts = ?7, \
                   in_reply_to = ?8 \
                 WHERE id = ?9 AND folder = 'drafts'",
                params![d.subject, to, cc, bcc, d.body, iso, ts, d.in_reply_to, id],
            )?;
            conn.execute(
                "UPDATE email_fts SET subject = ?1, clean_body = ?2 WHERE rowid = ?3",
                params![d.subject, d.body, id],
            )?;
            return Ok(id);
        }
        // 新規: フォルダ接頭辞つきの一意キー（UNIQUE(account_id, canonical_key) を満たす）。
        let nanos = now.timestamp_nanos_opt().unwrap_or(ts);
        let key = format!("drafts:draft-{}-{}", d.account_id, nanos);
        // サーバー Drafts 上で自分の下書きを一意に特定するための Message-ID を採番する。
        let domain = from
            .as_deref()
            .and_then(|e| e.split('@').nth(1))
            .unwrap_or("rondine.local");
        let message_id = format!("<draft-{}-{}@{}>", d.account_id, nanos, domain);
        conn.execute(
            "INSERT INTO emails \
               (account_id, canonical_key, message_id, subject, from_address, to_addresses, cc_addresses, \
                bcc_addresses, date, date_ts, body_plain, clean_body, folder, is_read, in_reply_to) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, 'drafts', 1, ?12)",
            params![
                d.account_id as i64,
                key,
                message_id,
                d.subject,
                from,
                to,
                cc,
                bcc,
                iso,
                ts,
                d.body,
                d.in_reply_to
            ],
        )?;
        let id = conn.last_insert_rowid();
        conn.execute(
            "INSERT INTO email_fts(rowid, subject, from_address, clean_body) VALUES (?1, ?2, ?3, ?4)",
            params![id, d.subject, from, d.body],
        )?;
        Ok(id)
    }

    /// 下書きのサーバー同期に使う参照情報（account_id, Message-ID）を取得する。
    /// Message-ID 未設定（古い下書き等）なら None を返す。
    pub fn draft_remote_ref(&self, id: i64) -> rusqlite::Result<Option<(i64, String)>> {
        let conn = self.conn.lock().unwrap();
        let row: Option<(i64, Option<String>)> = conn
            .query_row(
                "SELECT account_id, message_id FROM emails WHERE id = ?1 AND folder = 'drafts'",
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        Ok(row.and_then(|(a, m)| m.map(|mid| (a, mid))))
    }

    /// 下書き 1 件を作成画面へ読み戻すための内容を取得する（drafts フォルダのみ）。
    pub fn get_draft(&self, id: i64) -> rusqlite::Result<Option<crate::models::DraftContent>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, account_id, COALESCE(to_addresses, ''), COALESCE(cc_addresses, ''), \
                    COALESCE(bcc_addresses, ''), COALESCE(subject, ''), COALESCE(body_plain, ''), \
                    in_reply_to \
             FROM emails WHERE id = ?1 AND folder = 'drafts'",
            params![id],
            |r| {
                Ok(crate::models::DraftContent {
                    id: r.get::<_, i64>(0)? as i32,
                    account_id: r.get::<_, i64>(1)? as i32,
                    to: r.get(2)?,
                    cc: r.get(3)?,
                    bcc: r.get(4)?,
                    subject: r.get(5)?,
                    body: r.get(6)?,
                    in_reply_to: r.get(7)?,
                })
            },
        )
        .optional()
    }

    /// メール本文の取得（表示用）。差出人/宛先の表示名は住所録から解決する。
    pub fn get_email(&self, id: i64) -> rusqlite::Result<Option<MailDetail>> {
        let conn = self.conn.lock().unwrap();
        let detail = conn
            .query_row(
                "SELECT id, subject, from_address, to_addresses, date, clean_body, body_plain, body_html, body_html_z, has_attachments, body_compacted, message_id, from_name, to_name, account_id, cc_addresses
                 FROM emails WHERE id = ?1",
                params![id],
                |r| {
                    // HTML 本文は圧縮列(body_html_z)を優先し展開。無ければ旧 TEXT 列を使う。
                    let html_z: Option<Vec<u8>> = r.get(8)?;
                    let body_html = match html_z {
                        Some(z) => crate::services::compress::decompress_text(&z).ok(),
                        None => r.get(7)?,
                    };
                    Ok(MailDetail {
                        id: r.get::<_, i64>(0)? as i32,
                        account_id: r.get::<_, i64>(14)? as i32,
                        message_id: r.get(11)?,
                        subject: r.get(1)?,
                        from_address: r.get(2)?,
                        from_name: r.get(12)?,
                        to_addresses: r.get(3)?,
                        to_name: r.get(13)?,
                        cc_addresses: r.get(15)?,
                        date: r.get(4)?,
                        clean_body: r.get(5)?,
                        body_plain: r.get(6)?,
                        body_html,
                        has_attachments: r.get::<_, i64>(9)? != 0,
                        body_compacted: r.get::<_, i64>(10)? != 0,
                        is_green: false,
                        is_vip: false,
                    })
                },
            )
            .optional()?;
        let Some(mut d) = detail else {
            return Ok(None);
        };
        // グリーン判定（本人 or 認定ドメイン）と VIP（お気に入り連絡先）判定。
        let green_set = super::greendomain::green_domain_set(&conn)?;
        d.is_green =
            super::greendomain::address_is_green(&conn, &green_set, d.from_address.as_deref())?;
        if let Some(from) = d.from_address.as_deref() {
            d.is_vip = super::greendomain::address_is_vip(&conn, from)?;
        }
        // ヘッダの表示名が無ければ住所録から補完する（既存メール・表示名なしメール向け）。
        if d.from_name.is_none() {
            d.from_name = contact_name_for(&conn, d.from_address.as_deref())?;
        }
        if d.to_name.is_none() {
            d.to_name = contact_name_for(&conn, d.to_addresses.as_deref())?;
        }
        Ok(Some(d))
    }

    /// 全文再取得に必要な情報（親メールの account_id と IMAP UID）。
    /// UID が None のメールは再取得不可（要再同期）。
    pub fn email_refetch_info(
        &self,
        email_id: i64,
    ) -> rusqlite::Result<Option<(i64, Option<i64>)>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT account_id, uid FROM emails WHERE id = ?1",
            params![email_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()
    }

    /// 本文を上書きして全文キャッシュを復元する（要約保存の解除）。
    /// HTML は再圧縮して body_html_z に格納し、body_compacted=0 に戻す。FTS も更新。
    pub fn update_email_body(
        &self,
        id: i64,
        body_plain: Option<&str>,
        clean_body: Option<&str>,
        body_html: Option<&str>,
    ) -> rusqlite::Result<()> {
        let body_html_z = body_html
            .filter(|s| !s.is_empty())
            .map(crate::services::compress::compress_text);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE emails
             SET body_plain = ?1, clean_body = ?2, body_html_z = ?3, body_html = NULL, body_compacted = 0
             WHERE id = ?4",
            params![body_plain, clean_body, body_html_z, id],
        )?;
        // FTS5（clean_body 索引）も更新する。
        conn.execute(
            "UPDATE email_fts SET clean_body = ?1 WHERE rowid = ?2",
            params![clean_body, id],
        )?;
        Ok(())
    }

    /// 返信送信時の References チェーン（祖先 Message-ID を空白区切り・古い順）を作る。
    /// = 親メールの References ＋ 親メールの Message-ID。親が手元に無ければ親 ID 単体。
    /// `in_reply_to`（返信元 Message-ID・山括弧なし）が None なら None（新規メール）。
    pub fn references_chain_for(
        &self,
        in_reply_to: Option<&str>,
    ) -> rusqlite::Result<Option<String>> {
        let Some(parent_mid) = in_reply_to.map(str::trim).filter(|s| !s.is_empty()) else {
            return Ok(None);
        };
        let conn = self.conn.lock().unwrap();
        let row: Option<(Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT references_ids, message_id FROM emails WHERE message_id = ?1
                 ORDER BY id LIMIT 1",
                params![parent_mid],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        let chain = match row {
            Some((refs, mid)) => {
                let mut ids: Vec<String> = refs
                    .as_deref()
                    .map(|s| s.split_whitespace().map(str::to_string).collect())
                    .unwrap_or_default();
                let parent = mid.as_deref().unwrap_or(parent_mid);
                if !ids.iter().any(|x| x == parent) {
                    ids.push(parent.to_string());
                }
                ids.join(" ")
            }
            // 親が手元に無い（別経路で開始したスレッド等）: 少なくとも親 ID を参照する。
            None => parent_mid.to_string(),
        };
        Ok(Some(chain))
    }

    /// 既読にする。
    pub fn mark_read(&self, id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE emails SET is_read = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// 旧 TEXT 列に残る body_html を一度だけ zstd 圧縮して body_html_z へ移す。
    /// 起動時に呼ぶ。処理済み（body_html IS NULL）の行は対象外なので2回目以降は no-op。
    /// 圧縮した件数を返す。
    pub fn compress_legacy_bodies(&self) -> rusqlite::Result<usize> {
        let mut conn = self.conn.lock().unwrap();
        let rows: Vec<(i64, String)> = {
            let mut stmt = conn.prepare(
                "SELECT id, body_html FROM emails
                 WHERE body_html IS NOT NULL AND body_html <> '' AND body_html_z IS NULL",
            )?;
            let mapped =
                stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
            mapped.collect::<rusqlite::Result<Vec<_>>>()?
        };
        if rows.is_empty() {
            return Ok(0);
        }
        let tx = conn.transaction()?;
        {
            let mut up =
                tx.prepare("UPDATE emails SET body_html_z = ?1, body_html = NULL WHERE id = ?2")?;
            for (id, html) in &rows {
                let z = crate::services::compress::compress_text(html);
                up.execute(params![z, id])?;
            }
        }
        tx.commit()?;
        Ok(rows.len())
    }

    fn map_attachment(r: &rusqlite::Row) -> rusqlite::Result<AttachmentSummary> {
        let file_path: Option<String> = r.get(4)?;
        Ok(AttachmentSummary {
            id: r.get::<_, i64>(0)? as i32,
            filename: r.get(1)?,
            content_type: r.get(2)?,
            size: r.get::<_, Option<i64>>(3)?.unwrap_or(0) as i32,
            is_downloaded: file_path.is_some(),
            file_path,
            kind: r.get(5)?,
            content_id: r.get(6)?,
        })
    }

    /// メールの添付メタ一覧（序数順）。
    pub fn list_attachments(&self, email_id: i64) -> rusqlite::Result<Vec<AttachmentSummary>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, filename, content_type, size, file_path, kind, content_id
             FROM attachments WHERE email_id = ?1 ORDER BY part_index",
        )?;
        let rows = stmt.query_map(params![email_id], Self::map_attachment)?;
        rows.collect()
    }

    /// 添付 1 件のメタ。
    pub fn get_attachment(&self, id: i64) -> rusqlite::Result<Option<AttachmentSummary>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, filename, content_type, size, file_path, kind, content_id FROM attachments WHERE id = ?1",
            params![id],
            Self::map_attachment,
        )
        .optional()
    }

    /// オンデマンド再取得に必要な情報を取得する。
    pub fn attachment_fetch_info(
        &self,
        attachment_id: i64,
    ) -> rusqlite::Result<Option<AttachmentFetchInfo>> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT e.account_id, e.uid, a.part_index, a.filename, a.file_path
             FROM attachments a JOIN emails e ON e.id = a.email_id
             WHERE a.id = ?1",
            params![attachment_id],
            |r| {
                Ok(AttachmentFetchInfo {
                    account_id: r.get(0)?,
                    email_uid: r.get(1)?,
                    part_index: r.get(2)?,
                    filename: r.get(3)?,
                    file_path: r.get(4)?,
                })
            },
        )
        .optional()
    }

    /// ダウンロード完了を記録（保存先・簡易チェックサム・最終アクセス時刻）。
    pub fn set_attachment_downloaded(
        &self,
        attachment_id: i64,
        path: &str,
        checksum: Option<&str>,
    ) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE attachments SET file_path = ?1, checksum = ?2, accessed_at = datetime('now') WHERE id = ?3",
            params![path, checksum, attachment_id],
        )?;
        Ok(())
    }

    /// 添付の最終アクセス時刻を更新（表示/オープン時。LRU エビクションの保護に使う）。
    pub fn touch_attachment(&self, attachment_id: i64) -> rusqlite::Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE attachments SET accessed_at = datetime('now') WHERE id = ?1",
            params![attachment_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::store::migrations;
    use rusqlite::Connection;
    use std::path::PathBuf;
    use std::sync::Mutex;

    fn test_store() -> Store {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
        migrations::run(&conn).unwrap();
        conn.execute(
            "INSERT INTO accounts (id, email, imap_host, smtp_host) VALUES (1,'a@b','i','s')",
            [],
        )
        .unwrap();
        Store {
            conn: Mutex::new(conn),
            path: Mutex::new(PathBuf::new()),
        }
    }

    fn seed(store: &Store, subject: &str, from: &str, body: &str, folder: &str, key: &str) {
        let conn = store.conn.lock().unwrap();
        let e = NewEmail {
            account_id: 1,
            message_id: None,
            canonical_key: key.to_string(),
            subject: Some(subject.to_string()),
            from_address: Some(from.to_string()),
            from_name: None,
            to_addresses: None,
            to_name: None,
            cc_addresses: None,
            date: Some("2026-01-01 00:00:00".to_string()),
            date_ts: Some(1_767_225_600),
            body_plain: Some(body.to_string()),
            clean_body: Some(body.to_string()),
            body_html: None,
            auth_result: None,
            list_id: None,
            in_reply_to: None,
            references_ids: None,
            thread_index: None,
            raw_headers: None,
            quotes: vec![],
            has_attachments: false,
            is_read: false,
            uid: None,
            folder: folder.to_string(),
            attachments: vec![],
        };
        insert_email(&conn, &e).unwrap();
    }

    /// \Seen 取り込み: 既読で挿入→既読で保存。再取り込みで未読→既読の補正はするが、
    /// 逆（既読→未読）はしない（ローカルで開いた既読を守る）。
    #[test]
    fn insert_and_backfill_respect_seen_flag() {
        let store = test_store();
        let conn = store.conn.lock().unwrap();
        let mk = |key: &str, read: bool| NewEmail {
            account_id: 1,
            message_id: None,
            canonical_key: key.to_string(),
            subject: Some("s".into()),
            from_address: None,
            from_name: None,
            to_addresses: None,
            to_name: None,
            cc_addresses: None,
            date: None,
            date_ts: None,
            body_plain: None,
            clean_body: None,
            body_html: None,
            auth_result: None,
            list_id: None,
            in_reply_to: None,
            references_ids: None,
            thread_index: None,
            raw_headers: None,
            quotes: vec![],
            has_attachments: false,
            is_read: read,
            uid: None,
            folder: "inbox".to_string(),
            attachments: vec![],
        };
        let read_of = |key: &str| -> i64 {
            conn.query_row(
                "SELECT is_read FROM emails WHERE canonical_key = ?1",
                params![key],
                |r| r.get(0),
            )
            .unwrap()
        };

        // 既読フラグ付きで挿入 → 既読で保存される。
        insert_email(&conn, &mk("k-seen", true)).unwrap();
        assert_eq!(read_of("k-seen"), 1);

        // 未読で挿入 → 再取り込みでサーバー既読なら既読へ補正。
        insert_email(&conn, &mk("k-later", false)).unwrap();
        assert_eq!(read_of("k-later"), 0);
        insert_email(&conn, &mk("k-later", true)).unwrap();
        assert_eq!(read_of("k-later"), 1);

        // ローカルで既読のものは、サーバー未読でも未読へ巻き戻さない。
        insert_email(&conn, &mk("k-seen", false)).unwrap();
        assert_eq!(read_of("k-seen"), 1);
    }

    /// 点検再取り込み（backfill）で、旧パーサ由来の clean_body が新エンジンの分離結果に更新される。
    #[test]
    fn resync_recomputes_clean_body() {
        let store = test_store();
        let conn = store.conn.lock().unwrap();
        let mk = |clean: &str| NewEmail {
            account_id: 1,
            message_id: Some("m@x".into()),
            canonical_key: "m@x".into(),
            subject: Some("s".into()),
            from_address: Some("a@b".into()),
            from_name: None,
            to_addresses: None,
            to_name: None,
            cc_addresses: None,
            date: Some("2026-01-01T00:00:00Z".into()),
            date_ts: Some(1_767_225_600),
            body_plain: Some("新規部分\n-----Original Message-----\n古い引用".into()),
            clean_body: Some(clean.into()),
            body_html: None,
            auth_result: None,
            list_id: None,
            in_reply_to: None,
            references_ids: None,
            thread_index: None,
            raw_headers: None,
            quotes: vec![],
            has_attachments: false,
            is_read: true,
            uid: None,
            folder: "inbox".into(),
            attachments: vec![],
        };
        // 旧データ相当: 引用が残ったままの clean_body で保存。
        let old_clean = "新規部分\n-----Original Message-----\n古い引用";
        assert!(matches!(
            insert_email(&conn, &mk(old_clean)).unwrap(),
            InsertOutcome::Inserted(_)
        ));
        // 再取り込み: 新エンジンの分離結果（新規部分だけ）で backfill。
        let did = matches!(
            insert_email(&conn, &mk("新規部分")).unwrap(),
            InsertOutcome::Backfilled
        );
        assert!(did);
        let stored: String = conn
            .query_row(
                "SELECT clean_body FROM emails WHERE canonical_key = 'm@x'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stored, "新規部分");
        // FTS も更新され、引用語では当たらず新規部分で当たる。
        let hit_new: i64 = conn
            .query_row(
                "SELECT count(*) FROM email_fts WHERE email_fts MATCH '新規部分'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hit_new, 1);
    }

    /// スレッド一覧: 同スレッドは 1 行に畳まれ、代表＝最新・件数・未読が正しい。
    #[test]
    fn list_threads_collapses_and_counts() {
        let store = test_store();
        let mk = |mid: &str, ts: i64, read: bool, irt: Option<&str>, refs: Option<&str>| NewEmail {
            account_id: 1,
            message_id: Some(mid.into()),
            canonical_key: mid.into(),
            subject: Some("件名".into()),
            from_address: Some("you@corp.com".into()),
            from_name: None,
            to_addresses: None,
            to_name: None,
            cc_addresses: None,
            date: Some(format!("2026-06-{:02}T10:00:00Z", ts)),
            date_ts: Some(1_767_000_000 + ts * 86400),
            body_plain: Some("body".into()),
            clean_body: Some("body".into()),
            body_html: None,
            auth_result: None,
            list_id: None,
            in_reply_to: irt.map(str::to_string),
            references_ids: refs.map(str::to_string),
            thread_index: None,
            raw_headers: None,
            quotes: vec![],
            has_attachments: false,
            is_read: read,
            uid: None,
            folder: "inbox".into(),
            attachments: vec![],
        };
        {
            let conn = store.conn.lock().unwrap();
            // スレッドA: 2 通（root + 返信）。返信は未読。
            insert_email(&conn, &mk("a0@x", 1, true, None, None)).unwrap();
            insert_email(&conn, &mk("a1@x", 2, false, Some("a0@x"), Some("a0@x"))).unwrap();
            // 単独メール B（別 root）。既読。
            insert_email(&conn, &mk("b0@x", 3, true, None, None)).unwrap();
        }
        // 取り込み後のローカル加工（スレッド割当・代表フラグ）。
        store.process_pending(1).unwrap();
        let rows = store.list_threads(Some(1), "inbox", 50, 0).unwrap();
        // 2 行（スレッドA と 単独B）。新しい順なので先頭は B（ts=3）。
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, {
            // B の代表 id を引く。
            let conn = store.conn.lock().unwrap();
            conn.query_row("SELECT id FROM emails WHERE message_id='b0@x'", [], |r| r.get::<_, i64>(0))
                .unwrap() as i32
        });
        assert_eq!(rows[0].message_count, 1);
        assert!(rows[0].is_read);
        // スレッドA: 代表＝最新(a1)・件数2・未読1。
        let a = &rows[1];
        assert_eq!(a.message_count, 2);
        assert_eq!(a.unread_count, 1);
        assert!(!a.is_read);
        assert_eq!(a.email_ids.len(), 2);
    }

    /// 取り込みと加工の分離: 取り込み直後はスレッド未束ね（各メール1行）。process_pending で束ねる。
    /// 併せて reprocess_all が保存済み本文から clean_body を作り直すことも確認する。
    #[test]
    fn ingest_defers_threading_then_process_and_reprocess() {
        let store = test_store();
        let mk = |mid: &str, ts: i64, refs: Option<&str>, clean: &str, body: &str| NewEmail {
            account_id: 1,
            message_id: Some(mid.into()),
            canonical_key: mid.into(),
            subject: Some("件名".into()),
            from_address: Some("you@corp.com".into()),
            from_name: None,
            to_addresses: None,
            to_name: None,
            cc_addresses: None,
            date: Some(format!("2026-06-{:02}T10:00:00Z", ts)),
            date_ts: Some(1_767_000_000 + ts * 86400),
            body_plain: Some(body.into()),
            clean_body: Some(clean.into()),
            body_html: None,
            auth_result: None,
            list_id: None,
            in_reply_to: refs.map(str::to_string),
            references_ids: refs.map(str::to_string),
            thread_index: None,
            raw_headers: None,
            quotes: vec![],
            has_attachments: false,
            is_read: true,
            uid: None,
            folder: "inbox".into(),
            attachments: vec![],
        };
        {
            let conn = store.conn.lock().unwrap();
            insert_email(&conn, &mk("t0@x", 1, None, "本文0", "本文0")).unwrap();
            // stale な clean_body（引用が残ったまま）で保存 → reprocess で直る想定。
            insert_email(
                &conn,
                &mk(
                    "t1@x",
                    2,
                    Some("t0@x"),
                    "新規1\n\n2026/01/01 に a@b さんが書きました:\n> 引用",
                    "新規1\n\n2026/01/01 に a@b さんが書きました:\n> 引用",
                ),
            )
            .unwrap();
        }
        // 取り込み直後: スレッド未束ね＝各メール1行。
        let before = store.list_threads(Some(1), "inbox", 50, 0).unwrap();
        assert_eq!(before.len(), 2);
        assert!(before.iter().all(|r| r.message_count == 1));

        // ローカル加工: スレッドに束ねる。
        store.process_pending(1).unwrap();
        let after = store.list_threads(Some(1), "inbox", 50, 0).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].message_count, 2);

        // ローカル再加工: 保存済み本文から clean_body を作り直す（stale を修正）。
        store.reprocess_all(1).unwrap();
        let clean: String = {
            let conn = store.conn.lock().unwrap();
            conn.query_row(
                "SELECT clean_body FROM emails WHERE message_id = 't1@x'",
                [],
                |r| r.get(0),
            )
            .unwrap()
        };
        assert_eq!(clean, "新規1");
    }

    /// 代表フラグ: 代表メールを削除したら、その (スレッド,フォルダ) の残りの最新が代表に昇格する。
    #[test]
    fn folder_rep_promotes_after_delete() {
        let store = test_store();
        let base = |mid: &str, ts: i64, refs: Option<&str>| NewEmail {
            account_id: 1,
            message_id: Some(mid.into()),
            canonical_key: mid.into(),
            subject: Some("件名".into()),
            from_address: Some("you@corp.com".into()),
            from_name: None,
            to_addresses: None,
            to_name: None,
            cc_addresses: None,
            date: Some(format!("2026-06-{:02}T10:00:00Z", ts)),
            date_ts: Some(1_767_000_000 + ts * 86400),
            body_plain: Some("body".into()),
            clean_body: Some("body".into()),
            body_html: None,
            auth_result: None,
            list_id: None,
            in_reply_to: refs.map(str::to_string),
            references_ids: refs.map(str::to_string),
            thread_index: None,
            raw_headers: None,
            quotes: vec![],
            has_attachments: false,
            is_read: true,
            uid: None,
            folder: "inbox".into(),
            attachments: vec![],
        };
        let (a0, a1) = {
            let conn = store.conn.lock().unwrap();
            let a0 = match insert_email(&conn, &base("a0@x", 1, None)).unwrap() {
                InsertOutcome::Inserted(id) => id,
                _ => panic!(),
            };
            let a1 = match insert_email(&conn, &base("a1@x", 2, Some("a0@x"))).unwrap() {
                InsertOutcome::Inserted(id) => id,
                _ => panic!(),
            };
            (a0, a1)
        };
        // 取り込み後のローカル加工でスレッド割当・代表フラグを付ける。
        store.process_pending(1).unwrap();
        // 1 スレッドに畳まれ、代表は最新の a1。
        let rows = store.list_threads(Some(1), "inbox", 50, 0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id as i64, a1);
        // 代表 a1 を削除 → a0 が代表へ昇格し、スレッドは 1 行のまま。
        store.delete_emails(&[a1]).unwrap();
        let rows = store.list_threads(Some(1), "inbox", 50, 0).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id as i64, a0);
        assert_eq!(rows[0].message_count, 1);
    }

    #[test]
    fn build_fts_query_quotes_and_prefixes() {
        assert_eq!(build_fts_query("hello"), Some("\"hello\"*".to_string()));
        assert_eq!(
            build_fts_query("foo bar"),
            Some("\"foo\"* \"bar\"*".to_string())
        );
        assert_eq!(build_fts_query("   "), None);
        // 引用符を含む入力もエスケープして構文エラーにしない。
        assert_eq!(build_fts_query("a\"b"), Some("\"a\"\"b\"*".to_string()));
    }

    #[test]
    fn search_matches_subject_body_sender_and_respects_folder() {
        let store = test_store();
        seed(
            &store,
            "Invoice March",
            "alice@corp.com",
            "payment details enclosed",
            "inbox",
            "k1",
        );
        seed(
            &store,
            "Lunch plans",
            "bob@corp.com",
            "let us meet on friday",
            "inbox",
            "k2",
        );
        seed(
            &store,
            "Old invoice",
            "alice@corp.com",
            "archived",
            "trash",
            "k3",
        );

        // 件名一致（inbox 内）。
        let r = store
            .search_emails(Some(1), "inbox", "invoice", 50)
            .unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].subject.as_deref(), Some("Invoice March"));

        // 本文の前方一致（frid → friday）。
        let r = store.search_emails(Some(1), "inbox", "frid", 50).unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].subject.as_deref(), Some("Lunch plans"));

        // 差出人一致。
        assert_eq!(
            store
                .search_emails(Some(1), "inbox", "alice", 50)
                .unwrap()
                .len(),
            1
        );

        // フォルダ限定: trash の invoice は inbox 検索に出ない。
        let r = store
            .search_emails(Some(1), "trash", "invoice", 50)
            .unwrap();
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].subject.as_deref(), Some("Old invoice"));

        // 空クエリは空。
        assert!(store
            .search_emails(Some(1), "inbox", "   ", 50)
            .unwrap()
            .is_empty());

        // 複数語は AND。
        assert_eq!(
            store
                .search_emails(Some(1), "inbox", "invoice march", 50)
                .unwrap()
                .len(),
            1
        );
        assert!(store
            .search_emails(Some(1), "inbox", "invoice lunch", 50)
            .unwrap()
            .is_empty());
    }
}
