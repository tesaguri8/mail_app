//! 連絡先の書き出し（vCard ファイル）。docs/IMPORT_EXPORT.md。
//!
//! 中身を DB から読み（[`Store::contacts_for_export`]）、[`vcard::generate`] で vCard にして
//! ファイルへ書く。数千件でも一度に組み立てるので、呼び出し側は spawn_blocking に載せる。

use crate::models::VcardVersion;
use crate::services::store::Store;
use crate::services::vcard;
use std::path::Path;

/// 書き出しのエラー。
#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("連絡先を読めません: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("ファイルに書けません: {0}")]
    Io(#[from] std::io::Error),
}

/// 連絡先を vCard ファイルに書き出し、書き出した人数を返す。
///
/// - `ids`: Some ならその人だけ（一覧で絞り込んでいる分）、None ならゴミ箱を除く全員
/// - `prodid`: vCard の PRODID（書き出したアプリ）
///
/// # Errors
/// DB の読み出しやファイルの書き込みに失敗したとき。
pub fn export_vcard(
    store: &Store,
    path: &Path,
    ids: Option<&[i64]>,
    version: VcardVersion,
    prodid: &str,
) -> Result<usize, ExportError> {
    let contacts = store.contacts_for_export(ids)?;
    std::fs::write(path, vcard::generate(&contacts, version, prodid))?;
    Ok(contacts.len())
}
