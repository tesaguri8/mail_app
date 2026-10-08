//! 添付ファイル名の復号と整形（DB 非依存の文字列処理）。
//!
//! メールのファイル名は Content-Disposition の `filename` か Content-Type の `name` に入り、
//! 非 ASCII・長い名前は RFC2231（`filename*0*=utf-8''%E3%…` の継続/拡張形）か
//! RFC2047（`=?UTF-8?B?…?=` のエンコードワード）でエンコードされている。
//! IMAP の `BODYSTRUCTURE` から取り出した値はどちらも「生のまま」なので、そのまま保存すると
//! 元ファイル名と違う名前（`utf-8''%E5%A0%B1…` 等）や、継続形の先頭片だけの尻切れ
//! （拡張子が落ちる）になる。
//!
//! 復号規則は自前で持たず、合成した MIME ヘッダを `mail_parser` に通して任せる
//! （ISO-2022-JP 等の文字コード変換を含め、復号の単一ソースを 1 つに保つため）。

use mail_parser::{MessageParser, MimeHeaders};

/// パラメータ列（`(key, value)`。`BODYSTRUCTURE` 由来で未復号）から表示用ファイル名を復号する。
///
/// * `params` - Content-Disposition か Content-Type のパラメータ列
/// * `key` - 基底のパラメータ名（`"filename"` か `"name"`）
///
/// 戻り値は復号済みのファイル名。該当パラメータが無い・空・復号できない場合は `None`。
pub fn decode_params(params: &[(&str, &str)], key: &str) -> Option<String> {
    decode_line(&param_line(params, key)?)
}

/// 既に DB へ保存されてしまった未復号のファイル名 1 本を復号する（起動時の修復用）。
///
/// `utf-8''%E3%…`（RFC2231 拡張形）と `=?UTF-8?B?…?=`（RFC2047）の両方を受ける。
/// 継続形の先頭片だけが保存された残骸（閉じ記号なし）は読める分だけ復号するので、末尾は
/// 欠けたままになる（完全に戻すにはサーバーから取り直すしかない）。
/// 復号できない・変化しない場合は `None`（＝直せないので触らない）。
pub fn decode_stored(name: &str) -> Option<String> {
    if name.contains("''") {
        return decode_line(&format!("filename*={name}")).filter(|d| d != name);
    }
    let value = join_encoded_words(name);
    // 尻切れのエンコードワード（`=?…` で始まるのに `?=` で閉じていない＝継続形の先頭片だけが
    // 保存されたもの）は、閉じ記号を補って読める分だけ拾う。完全には戻らないが生値よりは近い。
    let value = if value.contains("=?") && !value.contains("?=") {
        format!("{value}?=")
    } else {
        value
    };
    decode_line(&format!("filename={}", quote(&value))).filter(|decoded| decoded != name)
}

/// 未復号のまま保存された疑いがあるファイル名か（修復対象の判定）。
/// RFC2047 のエンコードワードの断片（`=?`）か、RFC2231 拡張形の `charset''` を含むもの。
pub fn looks_undecoded(name: &str) -> bool {
    name.contains("=?") || name.contains("''")
}

/// ファイル名が無いときの仮名（`attachment-3` 等。拡張子は [`ensure_extension`] が補う）。
pub fn placeholder(index: usize) -> String {
    format!("attachment-{}", index + 1)
}

/// [`placeholder`] が作った仮名か（拡張子が補われていてもよい）。
pub fn is_placeholder(name: &str) -> bool {
    let stem = name.split_once('.').map_or(name, |(s, _)| s);
    stem.strip_prefix("attachment-")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

/// 拡張子の無いファイル名に Content-Type 由来の拡張子を補う（`attachment-1` → `attachment-1.html`）。
/// 既に拡張子がある名前、型が分からない `application/octet-stream` 等はそのまま返す。
pub fn ensure_extension(name: &str, content_type: Option<&str>) -> String {
    if has_extension(name) {
        return name.to_string();
    }
    match content_type.and_then(extension_for) {
        Some(ext) => format!("{name}.{ext}"),
        None => name.to_string(),
    }
}

/// 末尾に拡張子らしいもの（先頭以外のドット＋英数字 1〜8 文字）があるか。
fn has_extension(name: &str) -> bool {
    match name.rfind('.') {
        Some(0) | None => false,
        Some(dot) => {
            let ext = &name[dot + 1..];
            (1..=8).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric())
        }
    }
}

/// MIME 型から代表的な拡張子を引く（分からなければ `None`）。
fn extension_for(content_type: &str) -> Option<&'static str> {
    let ct = content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
        .to_ascii_lowercase();
    let ext = match ct.as_str() {
        "application/pdf" => "pdf",
        "application/msword" => "doc",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => "docx",
        "application/vnd.ms-excel" => "xls",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => "xlsx",
        "application/vnd.ms-powerpoint" => "ppt",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation" => "pptx",
        "application/rtf" | "text/rtf" => "rtf",
        "application/zip" | "application/x-zip-compressed" => "zip",
        "application/json" => "json",
        "application/xml" | "text/xml" => "xml",
        "application/postscript" => "ps",
        "text/plain" => "txt",
        "text/html" => "html",
        "text/csv" => "csv",
        "text/calendar" => "ics",
        "text/vcard" | "text/x-vcard" => "vcf",
        "image/jpeg" | "image/jpg" | "image/pjpeg" => "jpg",
        "image/png" => "png",
        "image/gif" => "gif",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/tiff" => "tif",
        "image/heic" => "heic",
        "image/heif" => "heif",
        "image/svg+xml" => "svg",
        "audio/mpeg" => "mp3",
        "audio/mp4" | "audio/x-m4a" => "m4a",
        "audio/wav" | "audio/x-wav" => "wav",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "message/rfc822" => "eml",
        _ => return None,
    };
    Some(ext)
}

/// 合成した Content-Disposition のパラメータ列を `mail_parser` に通し、復号済みの名前を得る。
fn decode_line(param_line: &str) -> Option<String> {
    let raw = format!("Content-Disposition: attachment; {param_line}\r\n\r\n");
    let msg = MessageParser::default().parse(raw.as_bytes())?;
    msg.parts
        .first()?
        .attachment_name()
        .map(str::to_string)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// `key` 系列のパラメータを集め、基底名を `filename` に揃えたパラメータ列へ組み直す。
/// RFC2231 の拡張形（`*` 付き）が 1 つでもあれば拡張形だけを使う（RFC2231 §4: 素の値より優先）。
fn param_line(params: &[(&str, &str)], key: &str) -> Option<String> {
    let mut segments: Vec<Segment> = params
        .iter()
        .filter_map(|&(name, value)| Segment::parse(name, value, key))
        .collect();
    if segments.is_empty() {
        return None;
    }
    if segments.iter().any(|s| s.extended) {
        segments.retain(|s| s.extended);
    }
    segments.sort_by_key(|s| s.index);
    let line = segments
        .iter()
        .map(Segment::render)
        .collect::<Vec<_>>()
        .join("; ");
    Some(line)
}

/// ファイル名パラメータの 1 区画（`filename` / `filename*` / `filename*0` / `filename*0*`）。
struct Segment {
    /// 基底名を `filename` に置き換えたパラメータ名。
    name: String,
    /// RFC2231 の継続番号（番号なしは 0）。
    index: u32,
    /// RFC2231 の拡張形（`charset''%XX` 形式＝引用符で包まない）か。
    extended: bool,
    value: String,
}

impl Segment {
    fn parse(name: &str, value: &str, key: &str) -> Option<Self> {
        if name.len() < key.len() || !name[..key.len()].eq_ignore_ascii_case(key) {
            return None;
        }
        // `filename` 系列は接尾辞が空か `*` 始まり。`filenamefoo` のような別名は対象外。
        let suffix = &name[key.len()..];
        if !suffix.is_empty() && !suffix.starts_with('*') {
            return None;
        }
        let digits = suffix.trim_start_matches('*').trim_end_matches('*');
        let index = if digits.is_empty() {
            0
        } else {
            digits.parse().ok()?
        };
        Some(Self {
            name: format!("filename{suffix}"),
            index,
            extended: suffix.ends_with('*'),
            value: value.to_string(),
        })
    }

    fn render(&self) -> String {
        if self.extended {
            // 拡張形の値は `charset'lang'%XX…` というトークン。引用符で包むと復号されない。
            format!("{}={}", self.name, self.value)
        } else {
            format!("{}={}", self.name, quote(&join_encoded_words(&self.value)))
        }
    }
}

/// 隣り合うエンコードワードの間の空白を詰める（RFC2047: 連続するエンコードワードの間の
/// 空白は区切りであって中身ではない）。ヘッダの折り返しがそのまま値に残っていると
/// `履歴書_仲宗根裕樹. \tpdf` のように空白が混ざるため、復号前に取り除く。
fn join_encoded_words(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(end) = rest.find("?=") {
        let (head, tail) = rest.split_at(end + 2);
        out.push_str(head);
        let trimmed = tail.trim_start();
        if trimmed.starts_with("=?") {
            rest = trimmed;
        } else {
            rest = tail;
        }
    }
    out.push_str(rest);
    out
}

/// パラメータ値を quoted-string にする（空白や `;` を含む実名を壊さないため）。
fn quote(value: &str) -> String {
    let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC2047（エンコードワード）の `name` を復号する。実データ由来。
    #[test]
    fn decodes_encoded_word_name() {
        let params = [(
            "name",
            "=?UTF-8?B?44Od44O844OI44OV44Kp44Oq44KqX+S7suWul+agueijleaouS5wZGY=?=",
        )];
        assert_eq!(
            decode_params(&params, "name").as_deref(),
            Some("ポートフォリオ_仲宗根裕樹.pdf")
        );
    }

    /// ISO-2022-JP のエンコードワードも文字コードごと復号する。
    #[test]
    fn decodes_iso_2022_jp_encoded_word() {
        let params = [("filename", "=?ISO-2022-JP?B?GyRCPEJKKjdvGyhCLnBkZg==?=")];
        assert_eq!(
            decode_params(&params, "filename").as_deref(),
            Some("実物件.pdf")
        );
    }

    /// RFC2231 拡張形（`filename*`）をパーセント復号する。実データ由来。
    #[test]
    fn decodes_rfc2231_extended() {
        let params = [(
            "filename*",
            "utf-8''240201%2DREQUIOS%5FA%E6%A1%88%5F%E5%9B%B3%E9%9D%A2.pdf",
        )];
        assert_eq!(
            decode_params(&params, "filename").as_deref(),
            Some("240201-REQUIOS_A案_図面.pdf")
        );
    }

    /// RFC2231 の継続形は全区画をつないで復号する（先頭片だけで尻切れにしない）。
    #[test]
    fn joins_rfc2231_continuations() {
        let params = [
            ("filename*0*", "utf-8''%E5%BE%B4%E4%BF%A1%E5%9B%B3"),
            ("filename*1*", "%E7%89%87.jpg"),
        ];
        assert_eq!(
            decode_params(&params, "filename").as_deref(),
            Some("徴信図片.jpg")
        );
    }

    /// 区画の順序が入れ替わって届いても番号順につなぐ。
    #[test]
    fn sorts_continuations_by_index() {
        let params = [("filename*1", "-late.pdf"), ("filename*0", "report")];
        assert_eq!(
            decode_params(&params, "filename").as_deref(),
            Some("report-late.pdf")
        );
    }

    /// 拡張形と素の値が両方あるときは拡張形を採る（素の値は ASCII 近似で欠落しがち）。
    #[test]
    fn prefers_extended_over_plain() {
        let params = [
            ("filename", "report.pdf"),
            ("filename*", "utf-8''%E5%A0%B1%E5%91%8A%E6%9B%B8.pdf"),
        ];
        assert_eq!(
            decode_params(&params, "filename").as_deref(),
            Some("報告書.pdf")
        );
    }

    /// エンコードされていない素の名前（空白入り）もそのまま通る。
    #[test]
    fn passes_plain_name_through() {
        let params = [("filename", "A 案 見積.pdf")];
        assert_eq!(
            decode_params(&params, "filename").as_deref(),
            Some("A 案 見積.pdf")
        );
    }

    /// 対象パラメータが無ければ None。
    #[test]
    fn returns_none_without_target_param() {
        let params = [("charset", "utf-8"), ("filenames", "x")];
        assert!(decode_params(&params, "filename").is_none());
    }

    /// 保存済みの生値（エンコードワード）を後から復号できる。
    #[test]
    fn decodes_stored_encoded_word() {
        assert_eq!(
            decode_stored("=?ISO-2022-JP?B?GyRCPEJKKjdvGyhCYS5wZGY=?=").as_deref(),
            Some("実物件a.pdf")
        );
    }

    /// 保存済みの生値（RFC2231 拡張形）も後から復号できる。
    #[test]
    fn decodes_stored_rfc2231() {
        assert_eq!(
            decode_stored("utf-8''1%E9%9A%8E%E5%9F%BA%E6%9C%AC%E5%B9%B3%E9%9D%A2%E5%9B%B3.dwg")
                .as_deref(),
            Some("1階基本平面図.dwg")
        );
    }

    /// 折り返しでつながった複数エンコードワードも 1 つの名前に戻す。
    #[test]
    fn decodes_stored_folded_encoded_words() {
        assert_eq!(
            decode_stored("=?UTF-8?B?5bGl5q205pu4X+S7suWul+agueijleaouS4=?= \t=?UTF-8?B?cGRm?=")
                .as_deref(),
            Some("履歴書_仲宗根裕樹.pdf")
        );
    }

    /// 閉じ記号が落ちた尻切れのエンコードワード（RFC2231 継続形の先頭片だけが保存された残骸）は、
    /// 読める分だけ復号する。末尾は元に戻らないが、生値のまま見せるよりは元名に近い。
    #[test]
    fn decodes_stored_truncated_encoded_word() {
        assert_eq!(
            decode_stored("=?UTF-8?B?5b6u5L+h5Zu+54mHXzIwMjUxMDI0MDk0OTI5XzI2XzE0MS5qcG")
                .as_deref(),
            Some("微信图片_20251024094929_26_141.j")
        );
    }

    /// 復号済みの名前は触らない（None ＝ 変更なし）。
    #[test]
    fn stored_plain_name_is_unchanged() {
        assert!(decode_stored("報告書.pdf").is_none());
    }

    #[test]
    fn detects_undecoded_names() {
        assert!(looks_undecoded("=?UTF-8?B?5bGl?="));
        assert!(looks_undecoded("utf-8''%E5%A0%B1.pdf"));
        assert!(!looks_undecoded("報告書.pdf"));
        assert!(!looks_undecoded("50%off.pdf"));
    }

    #[test]
    fn fills_missing_extension_from_content_type() {
        assert_eq!(
            ensure_extension("attachment-1", Some("text/html")),
            "attachment-1.html"
        );
        assert_eq!(
            ensure_extension("見積", Some("application/pdf")),
            "見積.pdf"
        );
        // 既に拡張子があれば触らない。
        assert_eq!(
            ensure_extension("見積.pdf", Some("application/pdf")),
            "見積.pdf"
        );
        // 型が分からなければ補えない。
        assert_eq!(
            ensure_extension("open", Some("application/octet-stream")),
            "open"
        );
        // 先頭のドットは拡張子ではない。
        assert_eq!(
            ensure_extension(".gitignore", Some("text/plain")),
            ".gitignore.txt"
        );
        // 末尾が英数字のドット区切り（`v1.2` 等）は拡張子と見分けられないので触らない
        // （誤って `.pdf` を足すより、そのまま残す方が安全）。
        assert_eq!(
            ensure_extension("見積 2025.10.1", Some("application/pdf")),
            "見積 2025.10.1"
        );
    }

    #[test]
    fn recognizes_placeholder_names() {
        assert_eq!(placeholder(2), "attachment-3");
        assert!(is_placeholder("attachment-3"));
        assert!(is_placeholder("attachment-3.pdf"));
        assert!(!is_placeholder("attachment-a.pdf"));
        assert!(!is_placeholder("報告書.pdf"));
    }
}
