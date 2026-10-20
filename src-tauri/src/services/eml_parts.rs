//! 添付された転送メール（message/rfc822・.eml）の中の添付を取り出す（docs/THREADING.md §5）。
//!
//! 同期は BODYSTRUCTURE の message/rfc822 を 1 つの添付として扱い、中は辿らない（bodystructure.rs）。
//! 中の添付は、.eml 本体を取得したあとで、その場で解析して一覧にし、選ばれたものだけを書き出す。
//! 書き出し先は .eml と同じ添付フォルダ（`attachments/<添付id>/inner/<序数>/`）の下なので、
//! 添付の追い出し（storage.rs はフォルダごと消す）で一緒に消える。

use crate::models::{EmlAttachment, EmlContents};
use crate::services::attachment_fetch::sanitize_filename;
use crate::services::parser;
use mail_parser::{Message, MessageParser};
use std::path::{Path, PathBuf};

/// 転送メールの中の添付を扱うときの失敗。
#[derive(Debug, thiserror::Error)]
pub enum EmlError {
    #[error("転送メールを解析できませんでした")]
    Parse,
    #[error("転送メールの中に {0} 番目の添付がありません")]
    NoPart(usize),
    #[error("ファイルを読み書きできませんでした: {0}")]
    Io(#[from] std::io::Error),
}

/// 中の添付を書き出すフォルダの名前（.eml と同じフォルダの下）。
const INNER_DIR: &str = "inner";

fn parse(raw: &[u8]) -> Result<Message<'_>, EmlError> {
    MessageParser::default().parse(raw).ok_or(EmlError::Parse)
}

/// 差出人を "名前 <addr>"（名前が無ければアドレスだけ）にする。
fn from_label(msg: &Message<'_>) -> Option<String> {
    let addr = msg.from()?.first()?;
    match (addr.name(), addr.address()) {
        (Some(name), Some(a)) if !name.trim().is_empty() => Some(format!("{} <{a}>", name.trim())),
        (_, Some(a)) => Some(a.to_string()),
        (Some(name), None) => Some(name.trim().to_string()),
        (None, None) => None,
    }
}

/// .eml の中身（見出しと添付の一覧）を読む。
///
/// # Errors
/// メールとして解析できないとき。
pub fn contents(raw: &[u8]) -> Result<EmlContents, EmlError> {
    let msg = parse(raw)?;
    let attachments = parser::real_attachment_parts(&msg)
        .into_iter()
        .enumerate()
        .map(|(i, p)| EmlAttachment {
            index: i32::try_from(i).unwrap_or(i32::MAX),
            filename: parser::part_filename(p, i),
            content_type: parser::part_content_type(p),
            size: i32::try_from(p.contents().len()).unwrap_or(i32::MAX),
        })
        .collect();
    Ok(EmlContents {
        subject: msg.subject().map(str::to_string),
        from: from_label(&msg),
        date: msg.date().map(|d| d.to_rfc3339()),
        attachments,
    })
}

/// .eml の中の `index` 番目の添付の、ファイル名と中身を返す。
///
/// # Errors
/// 解析できないとき、その序数の添付が無いとき。
pub fn extract(raw: &[u8], index: usize) -> Result<(String, Vec<u8>), EmlError> {
    let msg = parse(raw)?;
    let part = parser::real_attachment_parts(&msg)
        .get(index)
        .copied()
        .ok_or(EmlError::NoPart(index))?;
    Ok((parser::part_filename(part, index), part.contents().to_vec()))
}

/// 手元の .eml（`eml_path`）の中の `index` 番目の添付をファイルに書き出し、そのパスを返す。
/// 書き出し済みなら、それを使う。
///
/// # Errors
/// .eml を読めないとき、解析できないとき、その序数の添付が無いとき、書き込めないとき。
pub fn extract_to_file(eml_path: &Path, index: usize) -> Result<PathBuf, EmlError> {
    let dir = eml_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(INNER_DIR)
        .join(index.to_string());
    if let Some(done) = std::fs::read_dir(&dir)
        .ok()
        .and_then(|mut it| it.find_map(|e| e.ok().map(|e| e.path()).filter(|p| p.is_file())))
    {
        return Ok(done);
    }
    let raw = std::fs::read(eml_path)?;
    let (name, bytes) = extract(&raw, index)?;
    std::fs::create_dir_all(&dir)?;
    // 途中で止まった書き込みを「書き出し済み」と取り違えないよう、フォルダの外へ書いてから移す。
    let tmp = dir.with_extension("part");
    std::fs::write(&tmp, bytes)?;
    let path = dir.join(sanitize_filename(&name));
    std::fs::rename(&tmp, &path)?;
    Ok(path)
}

#[cfg(test)]
mod tests;
