//! 添付ファイル名の DB 反映（復号済みの名前へ寄せる処理）。
//!
//! 名前の復号・整形そのものは [`crate::services::attachname`]（DB 非依存）が持つ。ここは
//! 「先に入った仮名を、後から分かった本名へ差し替える」「過去に未復号のまま保存された名前を
//! 一度だけ直す」という、DB への当て方だけを担う。
//!
//! 仮名が入る経路は 2 つある（docs/SYNC.md）:
//! - メタ先行（Pass1.5）: BODYSTRUCTURE だけで行を作るため、名前は復号できても
//!   継続形の尻切れ等が残りうる。本名は Pass2 が各パートの MIME ヘッダから取り直す
//! - 過去分のメタ索引（backfill）: 本文を落とさないので Pass2 が無く、BODYSTRUCTURE の値のまま

use crate::services::attachname;
use rusqlite::{params, Connection, OptionalExtension};

/// 復号し直しの版。名前の直し方を変えたら上げる（次回起動で全行を見直す）。
const REPAIR_VERSION: &str = "1";
/// 実施済みの版を控える設定キー（app_settings）。
const KEY_REPAIRED: &str = "attachment_names.repaired_version";

/// 新しく得た添付メタで、既存行のファイル名を差し替える（part_index で対応付け）。
///
/// 差し替えるのは「今の名前が未復号の生値か仮名で、新しい名前がそれより良い」ときだけ。
/// 行は消さずに UPDATE するので、添付 id（取得済み本体・下書きの引用元）への参照は切れない。
///
/// 戻り値は 1 件以上書き換えたか。
pub fn refresh_names(
    conn: &Connection,
    email_id: i64,
    atts: &[super::emails::NewAttachment],
) -> rusqlite::Result<bool> {
    if atts.is_empty() {
        return Ok(false);
    }
    let mut updated = false;
    for a in atts {
        let stored: Option<String> = conn
            .query_row(
                "SELECT filename FROM attachments WHERE email_id = ?1 AND part_index = ?2",
                params![email_id, a.part_index],
                |r| r.get(0),
            )
            .optional()?;
        let Some(stored) = stored else { continue };
        if !is_better(&stored, &a.filename) {
            continue;
        }
        conn.execute(
            "UPDATE attachments SET filename = ?1 WHERE email_id = ?2 AND part_index = ?3",
            params![a.filename, email_id, a.part_index],
        )?;
        updated = true;
    }
    Ok(updated)
}

/// `new` は `stored` より良い名前か。
/// 「今が未復号か仮名」かつ「新しい方は未復号でない」ときだけ良いとみなす
/// （逆向きの上書き＝正しい名前を生値や仮名で潰すことを防ぐ）。
fn is_better(stored: &str, new: &str) -> bool {
    if stored == new || new.trim().is_empty() || attachname::looks_undecoded(new) {
        return false;
    }
    let stale = attachname::looks_undecoded(stored) || attachname::is_placeholder(stored);
    stale && !attachname::is_placeholder(new)
}

/// 過去に未復号のまま保存された名前を一度だけ直す（`Store::open` から呼ぶ）。
///
/// 通信は不要で、保存済みの文字列だけで直せる分（`=?UTF-8?B?…?=` / `utf-8''%E5…`）を
/// 復号し、拡張子が無ければ Content-Type から補う。継続形の先頭片だけが保存されていて
/// 元に戻せないものは触らない（サーバーから取り直すまで直らない）。
///
/// 戻り値は直した件数。
pub fn repair_stored(conn: &Connection) -> rusqlite::Result<usize> {
    let done: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key = ?1",
            params![KEY_REPAIRED],
            |r| r.get(0),
        )
        .optional()?;
    if done.as_deref() == Some(REPAIR_VERSION) {
        return Ok(0);
    }
    let rows: Vec<(i64, String, Option<String>)> = {
        let mut stmt = conn.prepare("SELECT id, filename, content_type FROM attachments")?;
        let mapped = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        mapped.collect::<rusqlite::Result<_>>()?
    };
    let mut fixed = 0;
    for (id, filename, content_type) in rows {
        let repaired = repair_one(&filename, content_type.as_deref());
        if repaired == filename {
            continue;
        }
        conn.execute(
            "UPDATE attachments SET filename = ?1 WHERE id = ?2",
            params![repaired, id],
        )?;
        fixed += 1;
    }
    conn.execute(
        "INSERT INTO app_settings (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = ?2",
        params![KEY_REPAIRED, REPAIR_VERSION],
    )?;
    Ok(fixed)
}

/// 保存済みの名前 1 本を直す（復号できなければ元の名前のまま拡張子だけ補う）。
fn repair_one(filename: &str, content_type: Option<&str>) -> String {
    let decoded = attachname::decode_stored(filename).unwrap_or_else(|| filename.to_string());
    attachname::ensure_extension(&decoded, content_type)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaces_undecoded_name_with_decoded_one() {
        assert!(is_better("utf-8''%E5%A0%B1%E5%91%8A.pdf", "報告.pdf"));
        assert!(is_better("=?UTF-8?B?5bGl?=", "履歴書.pdf"));
        assert!(is_better("attachment-1", "見積.pdf"));
    }

    #[test]
    fn keeps_good_name_against_worse_one() {
        // 正しい名前を生値・仮名で潰さない。
        assert!(!is_better("報告.pdf", "utf-8''%E5%A0%B1.pdf"));
        assert!(!is_better("報告.pdf", "attachment-1.pdf"));
        assert!(!is_better("attachment-1", "attachment-1.pdf"));
        assert!(!is_better("報告.pdf", "報告.pdf"));
        assert!(!is_better("報告.pdf", "   "));
    }

    #[test]
    fn repairs_encoded_and_missing_extension() {
        assert_eq!(
            repair_one(
                "=?ISO-2022-JP?B?GyRCPEJKKjdvGyhCLnBkZg==?=",
                Some("application/pdf")
            ),
            "実物件.pdf"
        );
        assert_eq!(
            repair_one("utf-8''1%E9%9A%8E.dwg", Some("application/octet-stream")),
            "1階.dwg"
        );
        assert_eq!(
            repair_one("attachment-1", Some("text/html")),
            "attachment-1.html"
        );
        // 直せないものはそのまま（尻切れの生値・型不明）。
        assert_eq!(repair_one("open", Some("application/octet-stream")), "open");
    }

    /// DB 一巡: 未復号の行だけが直り、実施済みの印が付いて 2 回目は走らない。
    #[test]
    fn repair_stored_is_one_shot() {
        let store = crate::services::store::Store::open_in_memory_for_test();
        let conn = store.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO accounts (id, email, imap_host, smtp_host)
             VALUES (1, 'me@example.com', 'imap.example.com', 'smtp.example.com')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO emails (id, account_id, canonical_key, folder) VALUES (1, 1, 'k', 'inbox')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO attachments (email_id, filename, content_type, size, part_index, kind)
             VALUES (1, ?1, 'application/pdf', 1, 0, 'attachment'),
                    (1, '報告.pdf', 'application/pdf', 1, 1, 'attachment')",
            params!["=?ISO-2022-JP?B?GyRCPEJKKjdvGyhCLnBkZg==?="],
        )
        .unwrap();

        assert_eq!(repair_stored(&conn).unwrap(), 1);
        let names: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT filename FROM attachments ORDER BY part_index")
                .unwrap();
            let rows = stmt.query_map([], |r| r.get(0)).unwrap();
            rows.collect::<rusqlite::Result<_>>().unwrap()
        };
        assert_eq!(
            names,
            vec!["実物件.pdf".to_string(), "報告.pdf".to_string()]
        );
        // 2 回目は印が付いているので走らない。
        assert_eq!(repair_stored(&conn).unwrap(), 0);
    }
}
