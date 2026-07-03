//! プレーン本文から「新しく書かれた部分（clean_body）」と「引用ブロック」を分離する。
//! docs/THREADING.md §7（引用・署名の分離）の最小実装。regex 依存を避け手書きで判定する。
//!
//! 方針:
//! - トップポスト（本文の下に引用が続く）を主対象に、最初の引用開始位置で本文と引用を切る。
//! - 引用開始の目印: 属性行（"On … wrote:" / "…が書きました:" / "-----元のメッセージ-----" ほか）、
//!   または連続する `>` 引用行。属性行があれば、その直前までを本文とする。
//! - 署名（`-- ` 区切り／"Sent from my iPhone" 等）は本文からも引用からも落とす。
//! - 100% は狙わない。取りこぼしは手動分割/引用表示で救済する（THREADING.md §4）。

/// 1 つの引用ブロック（属性行から差出人・時刻を、本文からフィンガープリントを取る）。
pub struct QuoteBlock {
    pub order: i64,
    /// 引用元の差出人（属性行から抽出。"名前 <addr>" や addr 単体。取れなければ None）。
    pub quoted_from: Option<String>,
    /// 引用元の送信時刻（属性行から抽出。生テキストのまま。取れなければ None）。
    pub quoted_at: Option<String>,
    /// 引用本文の正規化ハッシュ（同じ履歴を引いているかの照合キー）。
    pub fingerprint: String,
}

/// 分離結果。
pub struct Split {
    /// 引用・署名を除いた新規本文。
    pub clean: String,
    /// 引用ブロック（現状は末尾のまとまりを 1 ブロックとして返す）。
    pub quotes: Vec<QuoteBlock>,
}

/// 行が引用の「属性行」か判定し、そこから (from, at) を試みに取り出す。
/// 返り値 Some((from, at)) なら属性行。from/at は取れなければ None。
fn parse_attribution(line: &str) -> Option<(Option<String>, Option<String>)> {
    let l = line.trim();
    if l.is_empty() {
        return None;
    }

    // 区切り系（差出人/時刻は行に無いことが多い）。
    // "-----Original Message-----" / "-----元のメッセージ-----" / Outlook の下線区切り。
    let lower = l.to_lowercase();
    if lower.contains("original message")
        || l.contains("元のメッセージ")
        || l.contains("転送メッセージ")
        || l.starts_with("________________")
    {
        return Some((None, None));
    }

    // 英語: "On <date>, <who> wrote:"（複数行に割れる場合もあるが、まず 1 行で拾う）。
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

    // 日本語: "2026年6月30日(月) 10:00 佐藤 <sato@example.com>:" や "…さんは（次のように）書きました:"。
    // 誤検知抑制: 「〜書きました:」で終わる本文の一文（例: 会議録を彼は書きました:）で誤って
    // 切らないよう、属性行の裏付け（送信元アドレス @ か 4 桁の西暦）を要求する。
    // メールクライアントの属性行はほぼ必ず日付か差出人アドレスを含むため、これで区別できる。
    if (l.ends_with("書きました:") || l.ends_with("書きました：") || l.ends_with("wrote:"))
        && line_has_date_or_email(l)
    {
        // "<誰か>さんは…書きました:" の "さんは" より前を差出人とみなす。
        if let Some(pos) = l.find("さんは") {
            let from = l[..pos].trim().to_string();
            return Some((none_if_empty(&from), None));
        }
        return Some((None, None));
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

    // Outlook 系ヘッダブロック（"差出人:" / "From:" で始まる）。この行から引用開始とみなす。
    if l.starts_with("差出人:")
        || l.starts_with("差出人：")
        || lower.starts_with("from:")
        || l.starts_with("送信者:")
    {
        let from = l
            .split_once([':', '：'])
            .map(|(_, rest)| rest.trim().to_string());
        return Some((from.and_then(|s| none_if_empty(&s)), None));
    }

    None
}

/// 属性行の裏付け（送信元アドレス @ か 4 桁連続の西暦）を含むか。
/// 本文中の「〜書きました:」のような一文を属性行と誤認しないための足切りに使う。
fn line_has_date_or_email(l: &str) -> bool {
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

/// 署名開始行か（`--` 単独、または携帯署名）。
fn is_signature_start(line: &str) -> bool {
    let t = line.trim_end();
    // RFC の署名区切りは "-- "（末尾スペース）。実運用では "--" 単独も多い。
    if t == "-- " || t == "--" || t.trim() == "--" {
        return true;
    }
    let l = line.trim().to_lowercase();
    l.starts_with("sent from my ")
        || l.starts_with("get outlook for")
        || line.trim().starts_with("iPhoneから送信")
        || line.trim().starts_with("Androidから送信")
}

/// 本文（プレーン）を新規部分と引用に分離する。
pub fn split_reply(plain: &str) -> Split {
    let lines: Vec<&str> = plain.lines().collect();

    // 1) 引用開始行を探す: 最初の属性行、または最初の `>` 引用行の直前の空行境界。
    let mut quote_start: Option<usize> = None;
    let mut attribution: Option<(Option<String>, Option<String>)> = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some((from, at)) = parse_attribution(line) {
            quote_start = Some(i);
            attribution = Some((from, at));
            break;
        }
        if line.trim_start().starts_with('>') {
            // 属性行なしでいきなり `>` 引用が来るケース。ここから引用扱い。
            quote_start = Some(i);
            break;
        }
    }

    // 2) 署名開始行を探す（引用より手前にあれば本文はそこまで）。
    let mut sig_start: Option<usize> = None;
    let scan_end = quote_start.unwrap_or(lines.len());
    for (i, line) in lines.iter().enumerate().take(scan_end) {
        if is_signature_start(line) {
            sig_start = Some(i);
            break;
        }
    }

    // 本文の終端 = 引用開始 と 署名開始 の早い方。
    let body_end = match (quote_start, sig_start) {
        (Some(q), Some(s)) => q.min(s),
        (Some(q), None) => q,
        (None, Some(s)) => s,
        (None, None) => lines.len(),
    };

    let clean = lines[..body_end].join("\n").trim().to_string();

    // 3) 引用ブロック（引用開始〜末尾）をまとめて 1 ブロックにする。
    let mut quotes = Vec::new();
    if let Some(qs) = quote_start {
        // フィンガープリントは引用本文（`>` と空白を正規化）から作る。
        let quoted_text: String = lines[qs..]
            .iter()
            .map(|l| l.trim_start().trim_start_matches('>').trim())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let (from, at) = attribution.unwrap_or((None, None));
        quotes.push(QuoteBlock {
            order: 0,
            quoted_from: from,
            quoted_at: at,
            fingerprint: fingerprint(&quoted_text),
        });
    }

    Split { clean, quotes }
}

/// 後方互換の薄いラッパ（旧 strip_quotes 相当）。新規本文だけ欲しいとき用。
pub fn clean_body(plain: &str) -> String {
    split_reply(plain).clean
}

/// 正規化テキストの安定ハッシュ（FNV-1a 64bit → 16 桁 hex）。外部依存を足さない軽量版。
pub fn fingerprint(text: &str) -> String {
    // 空白・改行を 1 つに畳んで正規化してからハッシュする。
    let norm: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in norm.as_bytes() {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_english_top_post_and_signature() {
        let body = "Thanks, that works for me.\n\n--\nJohn\n\nOn Mon, 30 Jun 2026 10:00:00, Taro <taro@example.com> wrote:\n> Are you free next week?\n> Tuesday afternoon?";
        let s = split_reply(body);
        assert_eq!(s.clean, "Thanks, that works for me.");
        assert_eq!(s.quotes.len(), 1);
        assert_eq!(
            s.quotes[0].quoted_from.as_deref(),
            Some("Taro <taro@example.com>")
        );
        assert!(s.quotes[0].quoted_at.is_some());
    }

    #[test]
    fn strips_japanese_attribution() {
        let body = "承知しました。火曜の午後でお願いします。\n\n2026年6月30日(月) 10:00 佐藤 <sato@example.com>:\n> 来週の打ち合わせは可能ですか？";
        let s = split_reply(body);
        assert_eq!(s.clean, "承知しました。火曜の午後でお願いします。");
        assert_eq!(s.quotes.len(), 1);
        assert_eq!(s.quotes[0].quoted_from.as_deref(), Some("sato@example.com"));
    }

    #[test]
    fn strips_japanese_wrote_line() {
        let body =
            "了解です。\n\n山田太郎さんは2026年6月30日 10:00に書きました:\n> よろしくお願いします";
        let s = split_reply(body);
        assert_eq!(s.clean, "了解です。");
        assert_eq!(s.quotes.len(), 1);
        assert_eq!(s.quotes[0].quoted_from.as_deref(), Some("山田太郎"));
    }

    #[test]
    fn no_quote_keeps_whole_body() {
        let body = "初めまして。お世話になります。\n\nよろしくお願いします。";
        let s = split_reply(body);
        assert_eq!(s.clean, body.trim());
        assert!(s.quotes.is_empty());
    }

    #[test]
    fn bare_gt_quote_without_attribution() {
        let body = "はい、大丈夫です。\n> 明日は来られますか？";
        let s = split_reply(body);
        assert_eq!(s.clean, "はい、大丈夫です。");
        assert_eq!(s.quotes.len(), 1);
    }

    #[test]
    fn splits_rondine_own_reply_format() {
        // Rondine が生成する返信（compose.quoteHeader: "{{date}} に {{from}} さんが書きました:"）を
        // 入れ子で重ねたもの。新規部分（再テスト4）だけが残るべき。
        let body = "再テスト4\n\n\
2026/7/2 0:24:21 に suematsu@sng-design.com さんが書きました:\n\
> 再テスト3\n\
> \n\
> 2026/7/1 20:36:04 に suematsu@sng-design.com さんが書きました:\n\
> > 再テスト2\n\
> > \n\
> > 2026/7/1 18:09:32 に suematsu@sng-design.com さんが書きました:\n\
> > > 再テスト\n\
> > > \n\
> > > 2026-07-01T08:55:35Z に suematsu@sng-design.com さんが書きました:\n\
> > > > test";
        let s = split_reply(body);
        assert_eq!(s.clean, "再テスト4", "clean was: {:?}", s.clean);
    }

    #[test]
    fn body_sentence_ending_with_wrote_is_not_a_boundary() {
        // 本文中の「〜書きました:」（日付もアドレスも無い）は属性行と誤認しない。
        let body = "議事録をまとめて共有すると彼は書きました:\nよろしくお願いします。";
        let s = split_reply(body);
        assert_eq!(s.clean, body.trim());
        assert!(s.quotes.is_empty());
    }

    #[test]
    fn attribution_with_date_or_email_still_splits() {
        // 日付/アドレスの裏付けがあれば従来どおり属性行として切る。
        let with_email =
            "本文です。\n\nsato@example.com さんが書きました:\n> 以前のメール";
        assert_eq!(split_reply(with_email).clean, "本文です。");
        let with_date = "本文です。\n\n2026/07/03 10:00 に 佐藤 さんが書きました:\n> 以前のメール";
        assert_eq!(split_reply(with_date).clean, "本文です。");
    }

    #[test]
    fn fingerprint_is_whitespace_stable() {
        assert_eq!(fingerprint("hello   world"), fingerprint("hello world\n"));
        assert_ne!(fingerprint("a"), fingerprint("b"));
    }
}
