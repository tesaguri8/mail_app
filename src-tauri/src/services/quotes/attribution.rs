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
        // 日付側 / 差出人側をカンマで分ける（"On Mon, 30 Jun 2026 10:00, Taro <t@e> wrote:"）。
        // 前から見て、前半が時刻で終わる日時として読める最初のカンマで分ける（差出人の名前の
        // "Kinjo, Aya" のカンマで割らない）。読める位置が無ければ最後のカンマ。
        let at_time = inner
            .char_indices()
            .filter(|&(_, c)| c == ',')
            .map(|(i, _)| i)
            .find(|&i| {
                ends_with_time(&inner[..i]) && when::parse_quoted_time(&inner[..i]).is_some()
            });
        if let Some(idx) = at_time.or_else(|| inner.rfind(',')) {
            let at = inner[..idx].trim().trim_end_matches(',').trim().to_string();
            let from = inner[idx + 1..].trim().to_string();
            return Some((none_if_empty(&from), none_if_empty(&at)));
        }
        return Some((none_if_empty(inner.trim()), None));
    }

    // 中国語の "在 2026-10-02 12:10:00，"名前" <addr> 写道:" / "名前 <addr> 于 2026年10月20日 11:21 写道:" /
    // フランス語の "Le …, Name <addr> a écrit :"。本文の一文と紛れないよう、アドレスを含む行だけ。
    if l.contains('@') && l.ends_with("写道:") {
        return Some(chinese_attribution(l).unwrap_or((extract_angle_addr(l), None)));
    }
    if l.contains('@') && (lower.ends_with("a écrit:") || lower.ends_with("a écrit :")) {
        return Some((extract_angle_addr(l), None));
    }

    // 日時が先で差出人が続く属性行。日時は `when::parse_quoted_time` で読めるときだけ取る。
    // - Apple Mail の "Sep 9, 2026, 9:09 AM +0900, 名前 <addr>:" / iPhone の
    //   "2026/10/02 12:10、名前 <addr>のメール:"（日時と差出人を読点・カンマで区切る）
    // - Gmail 日本語版の "2026年10月2日(金) 17:55 名前 <addr>:" / "2026年10月2日 17:55:00 addr：" /
    //   "2026/10/2 12:10:00 に addr 名前:"（時刻の後に空白で差出人が続く）
    if let Some(found) = date_then_sender(l).or_else(|| time_then_sender(l)) {
        return Some(found);
    }

    // Apple Mail / iOS メール系の属性行: 行末が「名前 <addr>:」（日付＋差出人を 1 行に含む）。
    // 例: "Dec 26, 2025, 17:25 +0900, 伊佐　日和 <isa@matsudamariko.com>:"
    //     "Nov 17, 2025, 1:22 PM +0900, 名城楓 <nashiro@matsudamariko.com>:"
    // "On … wrote:" でも西暦始まりでもないため既存規則では拾えない。山括弧内アドレス＋末尾コロンを
    // 手がかりに属性行とみなす（本文行が "<addr>:" で終わることはまず無く、誤検知は小さい）。
    // 日時の読める形は上の `date_then_sender` で読む。ここに来るのは日時の読めない形。
    if ends_with_angle_addr_colon(l) {
        return Some((extract_angle_addr(l), None));
    }

    // 日本語: "2026年6月30日(月) 10:00 佐藤 <sato@example.com>:" や "…さんは（次のように）書きました:"。
    // 誤検知抑制: 「〜書きました:」で終わる本文の一文（例: 会議録を彼は書きました:）で誤って
    // 切らないよう、属性行の裏付け（送信元アドレス @ か 4 桁の西暦）を要求する。
    // メールクライアントの属性行はほぼ必ず日付か差出人アドレスを含むため、これで区別できる。
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

/// 差出人の後ろに付く決まり文句（"のメール" / "さんは書きました" / "wrote" 等）を外す。
fn strip_sender_suffix(who: &str) -> &str {
    [
        "のメール",
        "さんは書きました",
        "さんが書きました",
        "が書きました",
        "書きました",
        "wrote",
    ]
    .iter()
    .fold(who.trim(), |w, suffix| {
        w.strip_suffix(suffix).unwrap_or(w).trim_end()
    })
}

/// 日時の後に読点・カンマで差出人が続く属性行（Apple Mail の "Sep 9, 2026, 9:09 AM +0900, 名前 <addr>:" /
/// iPhone の "2026/10/02 12:10、名前 <addr>のメール:"）を (差出人, 日時) に読む。
/// 区切りは前から順に試し、前半が日時として読めて、後半にアドレスがある最初の位置で分ける
/// （"Sep 9" / "Sep 9, 2026" はまだ日時として読めないので、時刻まで含んだ位置で分かれる）。
/// `l` は行末のコロンを半角にそろえたもの。
fn date_then_sender(l: &str) -> Option<(Option<String>, Option<String>)> {
    let head = l.strip_suffix(':')?.trim_end();
    if !head.contains('@') {
        return None;
    }
    head.char_indices()
        .filter(|(_, c)| matches!(c, ',' | '、' | '，'))
        .find_map(|(i, c)| {
            let (at, who) = (head[..i].trim(), head[i + c.len_utf8()..].trim());
            (who.contains('@') && ends_with_time(at) && when::parse_quoted_time(at).is_some())
                .then_some((at, who))
        })
        .map(|(at, who)| (none_if_empty(strip_sender_suffix(who)), none_if_empty(at)))
}

/// 時刻の後ろに続いてよい字句（午前午後・時差・時間帯の略号）か。
fn is_time_suffix(tok: &str) -> bool {
    let t = tok.trim_matches(['(', ')', '（', '）']);
    let offset = t
        .strip_prefix(['+', '-'])
        .is_some_and(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit() || c == ':'));
    offset
        || matches!(
            t.to_ascii_lowercase().as_str(),
            "am" | "pm" | "a.m." | "p.m." | "jst" | "gmt" | "utc"
        )
}

/// 日時の文字列が、時刻・午前午後・時差の字句で終わるか（"… 12:50 +0900" / "… 9:09 AM"）。
/// 差出人の名前の途中（"… 12:50 +0900 Kinjo, Aya" の "Kinjo"）で日時を切らないための確かめ。
fn ends_with_time(at: &str) -> bool {
    at.split_whitespace()
        .next_back()
        .is_some_and(|tok| tok.contains(':') || is_time_suffix(tok))
}

/// 時刻の字句か（"17:55" / "12:10:00" / "午後5:55"）。
fn is_clock_token(tok: &str) -> bool {
    let t = tok.trim_start_matches("午前").trim_start_matches("午後");
    t.contains(':')
        && t.split(':')
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
}

/// 時刻の後に空白で差出人が続く属性行（Gmail 日本語版の "2026年10月2日(金) 17:55 名前 <addr>:" /
/// "2026年10月2日 17:55:00 addr:" / "2026/10/2 12:10:00 に addr 名前:" / Android 版 Gmail の
/// "火, 20 10月 2026、11:21 名前 <addr>:"）を (差出人, 日時) に読む。
/// 日時は最初の時刻の字句（と、続く午前午後・時差）まで。日時として読めて、残りにアドレスがあるときだけ。
/// 曜日から始まる形（[`starts_with_weekday`]）は、アドレスが無くても名前を差出人として読む。
fn time_then_sender(l: &str) -> Option<(Option<String>, Option<String>)> {
    let head = l.strip_suffix(':')?.trim_end();
    if !head.contains('@') && !starts_with_weekday(head) {
        return None;
    }
    let mut end: Option<usize> = None;
    // Android 版 Gmail は日付と時刻の間を読点で区切る（"2026、11:21"）。
    for tok in head
        .split(|c: char| c.is_whitespace() || c == '、')
        .filter(|t| !t.is_empty())
    {
        // 字句の終わりの位置（`head` の中のバイト位置）。
        let tok_end = tok.as_ptr() as usize - head.as_ptr() as usize + tok.len();
        match end {
            None if is_clock_token(tok) => end = Some(tok_end),
            Some(_) if is_time_suffix(tok) => end = Some(tok_end),
            Some(_) => break,
            None => {}
        }
    }
    let end = end?;
    let at = head[..end].trim();
    when::parse_quoted_time(at)?;
    let who = head[end..].trim();
    let who = who.strip_prefix("に ").unwrap_or(who);
    (who.contains('@') || starts_with_weekday(head))
        .then(|| (none_if_empty(strip_sender_suffix(who)), none_if_empty(at)))
        .filter(|(from, _)| from.is_some())
}

/// 行が曜日とカンマで始まるか（"火, 20 10月 2026、11:21 …" / "Tue, Oct 20, 2026 …"）。
/// 曜日の字句は和文 1 字か英字 3 文字（"火" / "Tue"）。
fn starts_with_weekday(head: &str) -> bool {
    head.split_once([',', '，']).is_some_and(|(w, _)| {
        let w = w.trim();
        matches!(w, "月" | "火" | "水" | "木" | "金" | "土" | "日")
            || matches!(
                w.to_ascii_lowercase().as_str(),
                "mon" | "tue" | "wed" | "thu" | "fri" | "sat" | "sun"
            )
    })
}

/// 中国語の属性行（"在 2026-10-02 12:10:00，"名前" <addr> 写道:" / "名前 <addr> 于 2026年10月20日 11:21 写道:"）を
/// (差出人, 日時) に読む。日時が読めなければ None（呼び出し側は差出人だけを取る）。
fn chinese_attribution(l: &str) -> Option<(Option<String>, Option<String>)> {
    let head = l.strip_suffix("写道:")?.trim();
    let (at, who) = match head.strip_prefix('在') {
        Some(rest) => rest.split_once(['，', ','])?,
        None => {
            let (who, at) = head.rsplit_once('于')?;
            (at, who)
        }
    };
    let at = at.trim();
    when::parse_quoted_time(at)?;
    Some((none_if_empty(who.trim()), none_if_empty(at)))
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
        || header::is_rule_line(l)
        || header::is_from_line(l)
}

/// 属性行の後ろ半分らしい行か（"<addr>:" / "<addr>のメール:" / 山括弧の途中で割れた "addr>:"）。
fn is_attribution_tail(line: &str) -> bool {
    let l = line.trim();
    !l.starts_with('>')
        && [">:", ">：", ">のメール:", ">のメール："]
            .iter()
            .any(|end| l.ends_with(end))
}

/// 属性行が改行で 2 行に割れた形（"Oct 2, 2026, 9:05 PM +0900, 名前" の次の行が "<addr>:" /
/// "…、名前 <" の次の行が "hanako@example.com>:"）を、つないで読む。
/// `line` が後ろ半分らしく、つないだ行にアドレスがあり、つないだほうが 1 行だけより多く読めるとき（日時が取れる・1 行では
/// 属性行と分からない）だけ Some。山括弧の途中で割れた形（`line` に "<" が無い）は空白を入れずにつなぐ。
pub(super) fn wrapped_attribution(
    prev: &str,
    line: &str,
) -> Option<(Option<String>, Option<String>)> {
    let p = prev.trim();
    if !is_attribution_tail(line) || p.is_empty() || p.starts_with('>') || header::is_header_line(p)
    {
        return None;
    }
    let l = line.trim();
    let joined = if l.contains('<') {
        format!("{p} {l}")
    } else {
        format!("{p}{l}")
    };
    if !joined.contains('@') {
        return None;
    }
    let (from, at) = parse_attribution(&joined)?;
    let better = match parse_attribution(l) {
        None => from.is_some(),
        Some((_, single_at)) => at.is_some() && single_at.is_none(),
    };
    better.then_some((from, at))
}
