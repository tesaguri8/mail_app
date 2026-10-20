//! 引用の属性行（"On … wrote:" / "…さんは書きました:" / "名前 <addr> のメール:" / 区切り行 ほか）を
//! 見分け、差出人・日時を取り出す。

use super::{header, when};

/// 行が引用の「属性行」か判定し、そこから (from, at) を試みに取り出す。
/// 返り値 Some((from, at)) なら属性行。from/at は取れなければ None。
pub(super) fn parse_attribution(line: &str) -> Option<(Option<String>, Option<String>)> {
    let l = line.trim();
    if l.is_empty() {
        return None;
    }
    // 行末の全角コロン（"wrote："）は半角にそろえて読む。
    let owned;
    let l = match l.strip_suffix('：') {
        Some(head) => {
            owned = format!("{head}:");
            owned.as_str()
        }
        None => l,
    };

    // 区切り系（差出人/時刻は行に無いことが多い）。
    // "-----Original Message-----" / "-----元のメッセージ-----" / Outlook の下線区切り。
    if header::is_separator_text(l) {
        return Some((None, None));
    }

    // 英語: "On <date>, <who> wrote:"（複数行に割れる場合もあるが、まず 1 行で拾う）。
    let lower = l.to_lowercase();
    if lower.starts_with("on ") && lower.trim_end().ends_with("wrote:") {
        let inner = &l[3..l.len() - "wrote:".len()];
        // 最後のカンマで日付側 / 差出人側に分ける（"On Mon, 30 Jun 2026 10:00, Taro <t@e> wrote:"）。
        if let Some(idx) = inner.rfind(',') {
            let at = inner[..idx].trim().trim_end_matches(',').trim().to_string();
            let from = inner[idx + 1..].trim().to_string();
            return Some((none_if_empty(&from), none_if_empty(&at)));
        }
        return Some((none_if_empty(inner.trim()), None));
    }

    // 中国語の "… <addr> 于 2026年10月20日 11:21 写道:" / フランス語の "Le …, Name <addr> a écrit :"。
    // 本文の一文と紛れないよう、アドレスを含む行だけ。
    if l.contains('@')
        && (l.ends_with("写道:") || lower.ends_with("a écrit:") || lower.ends_with("a écrit :"))
    {
        return Some((extract_angle_addr(l), None));
    }

    // Apple Mail / iOS メール系の属性行: 行末が「名前 <addr>:」（日付＋差出人を 1 行に含む）。
    // 例: "Dec 26, 2025, 17:25 +0900, 伊佐　日和 <isa@matsudamariko.com>:"
    //     "Nov 17, 2025, 1:22 PM +0900, 名城楓 <nashiro@matsudamariko.com>:"
    // "On … wrote:" でも西暦始まりでもないため既存規則では拾えない。山括弧内アドレス＋末尾コロンを
    // 手がかりに属性行とみなす（本文行が "<addr>:" で終わることはまず無く、誤検知は小さい）。
    if ends_with_angle_addr_colon(l) {
        return Some((extract_angle_addr(l), None));
    }

    // 日本語: "2026年6月30日(月) 10:00 佐藤 <sato@example.com>:" や "…さんは（次のように）書きました:"。
    // 誤検知抑制: 「〜書きました:」で終わる本文の一文（例: 会議録を彼は書きました:）で誤って
    // 切らないよう、属性行の裏付け（送信元アドレス @ か 4 桁の西暦）を要求する。
    // メールクライアントの属性行はほぼ必ず日付か差出人アドレスを含むため、これで区別できる。
    // iPhone などの "2026/10/02 12:10、名前 <addr> のメール:"（日付が先・読点で区切る）。日時も読む。
    if let Some(found) = date_first_attribution(l) {
        return Some(found);
    }
    // "<名前> <addr> のメール:"（Thunderbird 日本語版）。
    if (l.ends_with("書きました:") || l.ends_with("wrote:") || l.ends_with("のメール:"))
        && line_has_date_or_email(l)
    {
        // "<誰か>さんは…書きました:" の "さんは" より前を差出人とみなす。無ければ <addr>。
        if let Some(pos) = l.find("さんは") {
            let from = l[..pos].trim().to_string();
            return Some((none_if_empty(&from), None));
        }
        return Some((extract_angle_addr(l), None));
    }
    // 日付始まりの日本語属性行（"2026年…日 … <addr>:"）。行頭が西暦で末尾がコロン。
    if l.ends_with(':') || l.ends_with('：') {
        let starts_year = l
            .split(['年', '/'])
            .next()
            .map(|h| h.len() >= 4 && h.chars().all(|c| c.is_ascii_digit()))
            .unwrap_or(false);
        if starts_year {
            // 末尾のアドレス <...> を差出人として拾う。
            let from = extract_angle_addr(l);
            return Some((from, None));
        }
    }

    // Outlook 系ヘッダブロック（"差出人:" / "From:" / 太字の "*From:*" で始まる）。この行から引用開始とみなす。
    if let Some(from) = header::from_value(l) {
        return Some((none_if_empty(from), None));
    }

    None
}

/// 日付が先で読点の後に差出人が続く属性行（iPhone の "2026/10/02 12:10、名前 <addr> のメール:"）を
/// (差出人, 日時) に読む。行頭が日時として読めて、読点の後ろにアドレスがあるときだけ
/// （無ければ、従来の「西暦で始まりコロンで終わる行」として読む）。`l` は行末のコロンを半角にそろえたもの。
fn date_first_attribution(l: &str) -> Option<(Option<String>, Option<String>)> {
    let head = l.strip_suffix(':')?;
    let (at, who) = head.split_once(['、', '，'])?;
    when::parse_quoted_time(at.trim())?;
    if !who.contains('@') {
        return None;
    }
    let who = ["のメール", "が書きました", "書きました", "wrote"]
        .iter()
        .fold(who.trim(), |w, suffix| {
            w.strip_suffix(suffix).unwrap_or(w).trim_end()
        });
    Some((none_if_empty(who), none_if_empty(at)))
}

/// 属性行の裏付け（送信元アドレス @ か 4 桁連続の西暦）を含むか。
/// 本文中の「〜書きました:」のような一文を属性行と誤認しないための足切りに使う。
pub(super) fn line_has_date_or_email(l: &str) -> bool {
    if l.contains('@') {
        return true;
    }
    // 4 桁連続の数字（西暦らしさ）。
    let mut run = 0u32;
    for c in l.chars() {
        if c.is_ascii_digit() {
            run += 1;
            if run >= 4 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

/// 行末が「… <addr@dom>:」（山括弧メールアドレス＋末尾コロン）か。
/// Apple Mail 系の属性行（"…, 名前 <addr>:"）を、日付・"wrote:" に依存せず拾うための判定。
fn ends_with_angle_addr_colon(l: &str) -> bool {
    let Some(head) = l.trim_end().strip_suffix([':', '：']) else {
        return false;
    };
    let head = head.trim_end();
    if !head.ends_with('>') {
        return false;
    }
    // 直近の "<...>" にメールアドレス（@）が入っているか。
    match head.rfind('<') {
        Some(open) => head[open..].contains('@'),
        None => false,
    }
}

/// "<addr>" を含む行から山括弧内のアドレスを返す。
fn extract_angle_addr(s: &str) -> Option<String> {
    let start = s.find('<')?;
    let end = s[start..].find('>')? + start;
    none_if_empty(s[start + 1..end].trim())
}

fn none_if_empty(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// 属性行“らしさ”のゆるい判定（内容照合で引用と確認済みの文脈で、直前の属性行を巻き込むため）。
/// 単独では日付/アドレスの裏付けを要求しない（誤検知は content 一致側が担保する）。
pub(super) fn looks_like_attribution(line: &str) -> bool {
    let l = line.trim();
    let lower = l.to_lowercase();
    l.ends_with("書きました:")
        || l.ends_with("書きました：")
        || lower.ends_with("wrote:")
        || ends_with_angle_addr_colon(l)
        || l.ends_with("のメール:")
        || header::is_separator_text(l)
        || header::is_from_line(l)
}
