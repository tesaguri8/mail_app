//! プレーン本文から「新しく書かれた部分（clean_body）」と「引用ブロック」を分離する。
//! docs/THREADING.md §7（引用・署名の分離）の最小実装。regex 依存を避け手書きで判定する。
//!
//! 方針:
//! - トップポスト（本文の下に引用が続く）を主対象に、最初の引用開始位置で本文と引用を切る。
//! - 引用開始の目印: 属性行（"On … wrote:" / "…が書きました:" / "-----元のメッセージ-----" ほか）、
//!   または連続する `>` 引用行。属性行があれば、その直前までを本文とする。
//! - 署名（`-- ` 区切り／"Sent from my iPhone" 等）は本文からも引用からも落とす。
//! - 100% は狙わない。取りこぼしは手動分割/引用表示で救済する（THREADING.md §4）。

pub mod header;
pub mod when;

/// 1 つの引用ブロック（属性行から差出人・時刻を、本文からフィンガープリントを取る）。
pub struct QuoteBlock {
    pub order: i64,
    /// 引用元の差出人（属性行から抽出。"名前 <addr>" や addr 単体。取れなければ None）。
    pub quoted_from: Option<String>,
    /// 引用元の送信時刻（属性行から抽出。生テキストのまま。取れなければ None）。
    pub quoted_at: Option<String>,
    /// 引用元の件名（Outlook 等の見出しにあれば。表示用で、保存はしない）。
    pub subject: Option<String>,
    /// 見出しを除いた引用の本文（表示用で、保存はしない）。
    pub body: String,
    /// 見出しも含めた、このブロックの全文（1 通として読めなかったとき、直前の枠に残す。表示用）。
    pub text: String,
    /// 引用本文の正規化ハッシュ（同じ履歴を引いているかの照合キー）。
    pub fingerprint: String,
}

/// 分離結果。
pub struct Split {
    /// 引用・署名を除いた新規本文。
    pub clean: String,
    /// 引用ブロック。末尾のまとまりを 1 ブロックとし、Outlook 等の「元のメッセージ」が
    /// 連なっていれば 1 通ずつに分ける（新しい順）。
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
        || lower.contains("forwarded message")
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

    // Outlook 系ヘッダブロック（"差出人:" / "From:" / 太字の "*From:*" で始まる）。この行から引用開始とみなす。
    if let Some(from) = header::from_value(l) {
        return Some((none_if_empty(from), None));
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

/// 「強い」署名区切り行か（曖昧さの少ないものだけ）。誤爆（本文の一部を署名と誤認）を避ける。
/// - RFC の署名区切り "-- "（末尾スペース）／実運用の "--" 単独
/// - "□□□" だけの見出し行（純粋な □ の連なり）
/// - 携帯/クライアント定型（"Sent from my …" / "iPhoneから送信" 等）
///
/// ※ 記号区切り "=====…" は newsletter の本文区切りと紛れるため、ここには含めず
///   strip_signature 側で「以降が短い＝署名/フッタ」ときだけ落とす。
fn is_signature_delimiter_line(line: &str) -> bool {
    let bare = line.trim();
    if bare == "--" || line.trim_end() == "-- " {
        return true;
    }
    if bare.chars().count() >= 3 && bare.chars().all(|c| c == '□') {
        return true;
    }
    let l = bare.to_lowercase();
    l.starts_with("sent from my ")
        || l.starts_with("get outlook for")
        || bare.starts_with("iPhoneから送信")
        || bare.starts_with("Androidから送信")
}

/// 記号だけの区切り線か（"=====…" / "_____…"）。10 文字以上に限定（"──" 等の box 文字は対象外）。
fn is_symbol_rule_line(bare: &str) -> bool {
    bare.chars().count() >= 10
        && (bare.chars().all(|c| c == '=') || bare.chars().all(|c| c == '_'))
}

/// clean_body（引用除去後の新規本文）から署名以降を落とす。原文 body_plain は温存し、
/// 全文表示（body_plain）ではこれまで通り署名も見られる。表示・保存の派生テキスト専用。
/// 保守的方針:
/// - 明確なマーカー（"--" / "□□□" / 携帯定型）はその行以降を落とす。
/// - 記号区切り "===" は、以降の実質行が MAX_SIG_LINES 以下（＝末尾の署名/フッタらしい）ときだけ落とす。
///   newsletter の途中に入る本文区切り（以降に本文が続く）は落とさない。
/// - 本文が空になる切り方は採用しない（署名の無いメール・誤爆時は元のまま返す）。
pub fn strip_signature(clean: &str) -> String {
    // 署名本体とみなす最大の実質行数。これを超えて本文が続く記号区切りは「本文の区切り」とみなす。
    const MAX_SIG_LINES: usize = 10;
    let lines: Vec<&str> = clean.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let bare = line.trim();
        // (a) 明確な署名区切り。その行以降を落とす。
        let cut = if is_signature_delimiter_line(line) {
            Some(lines[..i].join("\n"))
        } else if is_symbol_rule_line(bare) {
            // (b) 記号区切り: 以降が短いとき（末尾の署名/フッタ）だけ落とす。長ければ本文の区切り。
            let tail = lines[i + 1..].iter().filter(|l| !l.trim().is_empty()).count();
            if tail <= MAX_SIG_LINES {
                Some(lines[..i].join("\n"))
            } else {
                continue;
            }
        } else if let Some(pos) = line.find("□□□") {
            // (c) 行内の "□□□"（改行が失われた Apple Mail 系対策）。手前の本文までを残す。
            let head = line[..pos].trim_end();
            let mut s = lines[..i].join("\n");
            if !head.is_empty() {
                if !s.is_empty() {
                    s.push('\n');
                }
                s.push_str(head);
            }
            Some(s)
        } else {
            None
        };
        if let Some(result) = cut {
            let trimmed = result.trim_end().to_string();
            // 本文が全部消えるならこの切り方は不採用（誤爆保険）。
            if trimmed.trim().is_empty() && !clean.trim().is_empty() {
                return clean.trim_end().to_string();
            }
            return trimmed;
        }
    }
    clean.trim_end().to_string()
}

/// 本文（プレーン）を新規部分と引用に分離する。
pub fn split_reply(plain: &str) -> Split {
    let lines: Vec<&str> = plain.lines().collect();

    // 1) 引用開始行を探す。
    //    a) まず属性行（"On … wrote:" / "…が書きました:" / "-----元のメッセージ-----" 等）。
    //       これは末尾の返信引用の確実な目印なので、本文途中のインライン `>` 引用より優先する
    //       （インライン引用の後ろに書いた新規テキストを clean から落とさないため）。
    let mut quote_start: Option<usize> = None;
    let mut attribution: Option<(Option<String>, Option<String>)> = None;
    for (i, line) in lines.iter().enumerate() {
        if let Some((from, at)) = parse_attribution(line) {
            quote_start = Some(i);
            attribution = Some((from, at));
            break;
        }
    }
    //    b) 属性行が無いときだけ、最初の `>` 引用行を引用開始とする（属性行なしで下部に
    //       いきなり `>` 引用が来る返信のため。従来動作）。
    if quote_start.is_none() {
        for (i, line) in lines.iter().enumerate() {
            if line.trim_start().starts_with('>') {
                quote_start = Some(i);
                break;
            }
        }
    }

    // 2) 本文の終端＝引用開始。署名は後段の strip_signature で落とす（原文 body_plain は温存）。
    // ※ メール末尾（引用より後ろ）に付く署名は、引用ごと落ちるのでここで扱う必要はない。
    let body_end = quote_start.unwrap_or(lines.len());
    let clean = strip_signature(lines[..body_end].join("\n").trim());

    // 3) 引用ブロック（引用開始〜末尾）。「元のメッセージ」が連なっていれば 1 通ずつに分ける。
    let quotes = match quote_start {
        Some(qs) => quote_blocks(&lines[qs..], attribution),
        None => Vec::new(),
    };

    Split { clean, quotes }
}

/// 引用の領域の途中にある属性行（"On … wrote:" / "〜さんは書きました:" 等）なら、その (from, at)。
/// 区切り行と "From:" の見出しは別に扱うので含めない。`>` の付いた行（引用の中の引用）と、
/// 日付もアドレスも無い行（本文の「…と彼は書きました:」）は属性行とみなさない。
fn inner_attribution(line: &str) -> Option<(Option<String>, Option<String>)> {
    let l = line.trim();
    if l.starts_with('>')
        || header::is_separator(l)
        || header::is_from_line(l)
        || !line_has_date_or_email(l)
    {
        return None;
    }
    parse_attribution(l)
}

/// 引用の領域で、次の 1 通が始まる行か。`>` の付かない「元のメッセージ」の区切り、属性行
/// （"On … wrote:" 等）、または区切りの無い見出し（"From:" の後に "Sent:" 等が続く）。
/// 区切りの直後の "From:" は同じ 1 通。
fn starts_next_block(region: &[&str], j: usize) -> bool {
    if header::is_separator(region[j]) || inner_attribution(region[j]).is_some() {
        return true;
    }
    if !header::is_from_line(region[j]) {
        return false;
    }
    let prev = region[..j].iter().rev().find(|l| !l.trim().is_empty());
    if prev.is_some_and(|l| header::is_separator(l)) {
        return false;
    }
    // 見出しらしさの裏付け: 続く数行に日時の項目がある。
    region[j + 1..]
        .iter()
        .take(4)
        .any(|l| header::is_date_line(l))
}

/// 行の先頭の共通の字下げを外して 1 つの文字列にする（字下げされた転送の中の 1 通を、
/// 左に寄せて見せるため）。前後の空行は落とす。
fn dedent(lines: &[&str]) -> String {
    let indent = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        // 外すのは半角の空白とタブだけ（全角の空白は本文の字下げとして残す）。
        .map(|l| l.len() - l.trim_start_matches([' ', '\t']).len())
        .min()
        .unwrap_or(0);
    lines
        .iter()
        // 空白だけの行は indent より短いことがあるので空にする。
        .map(|l| {
            if l.trim().is_empty() {
                ""
            } else {
                l.get(indent..).unwrap_or(l)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim_matches('\n')
        .trim_end()
        .to_string()
}

/// 引用の領域（引用開始〜末尾）を 1 通ずつのブロックに分ける。先頭のブロックは、本文を切った
/// 属性行（`attribution`。`>` 行で切ったときは None）の差出人・時刻を、見出しから取れなかった
/// 分の代わりに使う。
fn quote_blocks(
    region: &[&str],
    attribution: Option<(Option<String>, Option<String>)>,
) -> Vec<QuoteBlock> {
    // 先頭のブロックが属性行（"On … wrote:" 等）から始まるなら、その行は見出しとして飛ばす。
    let skip_attribution_line = attribution.is_some() && !header::is_from_line(region[0]);
    // 次の 1 通の始まりは、いまの 1 通の見出しより後ろでだけ探す（項目名と値が別の行にある
    // 見出しでは "From:" の後に "To:" 等が続き、区切りの無い見出しと見分けられないため）。
    let mut starts: Vec<usize> = vec![0];
    let mut header_end = header::parse_header(region, skip_attribution_line).body_start;
    for j in 1..region.len() {
        if j < header_end || !starts_next_block(region, j) {
            continue;
        }
        starts.push(j);
        let skip = inner_attribution(region[j]).is_some();
        header_end = j + header::parse_header(&region[j..], skip).body_start;
    }
    let mut first = Some(attribution.unwrap_or((None, None)));
    starts
        .iter()
        .enumerate()
        .map(|(k, &s)| {
            let e = starts.get(k + 1).copied().unwrap_or(region.len());
            let block = &region[s..e];
            // 途中のブロックが属性行で始まるなら、差出人・時刻はその行から取り、行は見出しとして飛ばす。
            let inner = (k > 0).then(|| inner_attribution(block[0])).flatten();
            let skip_first = (k == 0 && skip_attribution_line) || inner.is_some();
            let h = header::parse_header(block, skip_first);
            let (attr_from, attr_at) = first.take().or(inner).unwrap_or((None, None));
            // フィンガープリントは引用本文（`>` と空白を正規化）から作る。
            let quoted_text: String = block
                .iter()
                .map(|l| l.trim_start().trim_start_matches('>').trim())
                .filter(|l| !l.is_empty())
                .collect::<Vec<_>>()
                .join("\n");
            QuoteBlock {
                order: k as i64,
                quoted_from: h.from.or(attr_from),
                quoted_at: h.at.or(attr_at),
                subject: h.subject,
                body: dedent(&block[h.body_start.min(block.len())..]),
                text: dedent(block),
                fingerprint: fingerprint(&quoted_text),
            }
        })
        .collect()
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

// ---- ② 内容照合による引用剥がし（docs/THREADING.md §2 優先3。形式非依存） ----

/// 行を引用照合用に正規化する（先頭の `>`（多重）と空白を除去し、内部空白を1つに畳む）。
pub fn normalize_quote_line(line: &str) -> String {
    let mut s = line.trim_start();
    while let Some(rest) = s.strip_prefix('>') {
        s = rest.trim_start();
    }
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// テキストの最初の「意味ある行」（4文字以上）を正規化して返す。引用照合のアンカーに使う。
pub fn first_anchor(text: &str) -> Option<String> {
    for line in text.lines() {
        let n = normalize_quote_line(line);
        if n.chars().count() >= 4 {
            return Some(n);
        }
    }
    None
}

/// 属性行“らしさ”のゆるい判定（内容照合で引用と確認済みの文脈で、直前の属性行を巻き込むため）。
/// 単独では日付/アドレスの裏付けを要求しない（誤検知は content 一致側が担保する）。
pub fn looks_like_attribution(line: &str) -> bool {
    let l = line.trim();
    let lower = l.to_lowercase();
    l.ends_with("書きました:")
        || l.ends_with("書きました：")
        || lower.ends_with("wrote:")
        || ends_with_angle_addr_colon(l)
        || lower.contains("original message")
        || lower.contains("forwarded message")
        || l.contains("元のメッセージ")
        || l.contains("転送メッセージ")
        || header::is_from_line(l)
}

/// clean_body を、同スレッドの過去メール由来のアンカー集合と一致する行以降で追加的に切り詰める。
/// ヒューリスティックで取り切れなかった引用（未知の属性行＋インライン引用など）を、
/// 「手元の過去メールと一致する＝引用」という形式非依存の判定で落とす。
/// 誤検知を避けるため、一致行が `>` 引用か、直前が属性行のときだけ採用する。
pub fn cut_at_known_anchor(clean: &str, anchors: &std::collections::HashSet<String>) -> String {
    let lines: Vec<&str> = clean.lines().collect();
    for i in 1..lines.len() {
        let norm = normalize_quote_line(lines[i]);
        if norm.chars().count() < 4 || !anchors.contains(&norm) {
            continue;
        }
        // 引用らしさの裏付け: その行が `>` 引用 か、直前（空行を挟んでも）が属性行。
        let quoted_marker = lines[i].trim_start().starts_with('>');
        let prev = lines[i - 1].trim();
        let prev_attr = looks_like_attribution(prev)
            || (prev.is_empty() && i >= 2 && looks_like_attribution(lines[i - 2].trim()));
        if !(quoted_marker || prev_attr) {
            continue;
        }
        // カット位置: 直前の属性行・空行も一緒に落とす。
        let mut cut = i;
        while cut > 0 {
            let p = lines[cut - 1].trim();
            if p.is_empty() || looks_like_attribution(p) {
                cut -= 1;
            } else {
                break;
            }
        }
        if cut == 0 {
            continue; // 本文が全部消える位置は採用しない
        }
        return lines[..cut].join("\n").trim().to_string();
    }
    clean.to_string()
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
    fn keeps_new_text_after_inline_quote() {
        // 本文途中にインライン引用（`>`）があり、その後にも新規テキストがある返信。
        // 末尾の属性行（"On … wrote:"）を引用開始とし、インライン引用の後の新規文は clean に残す。
        let body = "伊佐様\nお世話になっております。\n> ・4/6 (月) 午後\n> ・4/7 (火) 午前/午後\n→いづれかで大丈夫です。\n宜しくお願い致します。\n\nOn 2026/03/31 16:27, Isa <isa@example.com> wrote:\n> 末松 さま\n> お世話になっております。";
        let s = split_reply(body);
        assert!(
            s.clean.contains("→いづれかで大丈夫です。"),
            "インライン引用の後の新規文が clean に残る"
        );
        assert!(s.clean.contains("宜しくお願い致します。"));
        assert!(s.clean.contains("お世話になっております。"));
        // 末尾の返信引用（属性行以降）は clean に含まない。
        assert!(!s.clean.contains("末松 さま"));
        assert_eq!(s.quotes.len(), 1);
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
    fn strips_apple_mail_addr_colon_attribution() {
        // Apple Mail / iOS メール系: "…, 名前 <addr>:" で終わる属性行（"On…wrote:" でも西暦始まりでもない）。
        // 実データで 400 通超がこの形の属性行を clean_body に残していた回帰の防止。
        let body = "松田様、伊佐様\n\n\
本年も引き続き宜しくお願い致します。\n\n\
末松\n\n\
Dec 26, 2025, 17:25 +0900, 伊佐　日和 <isa@matsudamariko.com>:\n\n\
> 末松 様\n> お世話になっております。";
        let s = split_reply(body);
        assert_eq!(
            s.clean,
            "松田様、伊佐様\n\n本年も引き続き宜しくお願い致します。\n\n末松"
        );
        assert_eq!(s.quotes.len(), 1);
        assert_eq!(
            s.quotes[0].quoted_from.as_deref(),
            Some("isa@matsudamariko.com")
        );
    }

    #[test]
    fn addr_colon_attribution_without_gt_body() {
        // 引用本文が `>` 無し（Apple Mail の text/plain がインデントのみ）でも属性行で切れる。
        let body = "了解しました。\n\nNov 5, 2024, 3:15 PM +0900, shiradou <siradou@example.com>:\n本文の引用がそのまま続く";
        assert_eq!(split_reply(body).clean, "了解しました。");
    }

    #[test]
    fn colon_line_without_angle_email_is_not_attribution() {
        // 末尾コロンでも山括弧アドレスが無ければ属性行扱いしない（本文の見出し等を守る）。
        let body = "変更点は以下の通りです:\n- 1点目\n- 2点目";
        let s = split_reply(body);
        assert_eq!(s.clean, body.trim());
        assert!(s.quotes.is_empty());
    }

    #[test]
    fn strips_equals_delimiter_signature() {
        // "=====" 区切りの署名（Thunderbird 等の手書き署名）を落とす。
        let body = "松田様、伊佐様\n\n江島邸の件、承知しました。\n\n末松\n\n\
=========================\n\
Shingo Suematsu CEO\n\
sngDESIGN Inc.\n\
=========================\n\n\
Dec 26, 2025, 17:25 +0900, 伊佐　日和 <isa@matsudamariko.com>:\n\n\
> よろしくお願いします";
        let s = split_reply(body);
        assert_eq!(s.clean, "松田様、伊佐様\n\n江島邸の件、承知しました。\n\n末松");
    }

    #[test]
    fn strips_inline_box_marker_signature() {
        // 改行が失われた Apple Mail 系: 本文と "□□□" 署名が 1 行に潰れていても手前で切る。
        let body =
            "末松　さまお世話になっております。伊佐です。よろしくお願いいたします。□□□伊佐日和090-3794-8055合同会社松田まり子建築設計事務所";
        assert_eq!(
            strip_signature(body),
            "末松　さまお世話になっております。伊佐です。よろしくお願いいたします。"
        );
    }

    #[test]
    fn no_signature_body_is_unchanged() {
        // 署名の無いメールは 1 文字も削らない（区切りマーカーが無ければ素通り）。
        let body = "承知しました。\n明日までにお送りします。\n引き続きよろしくお願いします。";
        assert_eq!(strip_signature(body), body);
        assert_eq!(split_reply(body).clean, body);
    }

    #[test]
    fn signature_only_body_is_not_nuked() {
        // 万一「署名だけ」に見える切り方になっても、本文が空になるなら採用しない（誤爆保険）。
        let body = "□□□\n伊佐日和\n090-3794-8055";
        // 先頭が □□□ 単独行 → 手前が空。空にはせず元のまま返す。
        assert_eq!(strip_signature(body), body);
    }

    #[test]
    fn short_equals_run_is_not_a_signature() {
        // 短い "===" は誤爆回避のため署名扱いしない（本文中の区切りを守る）。
        let body = "見出し\n===\n本文の続き";
        assert_eq!(strip_signature(body), body);
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

    /// Outlook の英語の見出しで連なった「元のメッセージ」を、1 通ずつのブロックに分ける
    /// （実例の形。個人名と宛先は作り物）。
    #[test]
    fn splits_chained_outlook_original_messages() {
        let body = "鈴木様\n\nチェックバックその3を送ります。\n\n田中\n\n\
-----Original Message-----\n\
From: 田中 一郎 <tanaka@example.jp>\n\
Sent: Friday, October 2, 2026 5:55 PM\n\
To: 鈴木 次郎 <suzuki@example.jp>\n\
Subject: RE: プレカット図 チェックバック\n\
\n\
鈴木様\n修正点をまとめました。\n\
\n\
-----Original Message-----\n\
From: 高橋 花子 <takahashi@example.com>\n\
Sent: Friday, October 2, 2026 2:50 PM\n\
To: 田中 一郎 <tanaka@example.jp>; 鈴木 次郎 <suzuki@example.jp>\n\
Subject: プレカット図 チェックバック\n\
\n\
田中様、鈴木様\n図面を送ります。";
        let s = split_reply(body);
        assert_eq!(s.clean, "鈴木様\n\nチェックバックその3を送ります。\n\n田中");
        assert_eq!(s.quotes.len(), 2);
        let (a, b) = (&s.quotes[0], &s.quotes[1]);
        assert_eq!((a.order, b.order), (0, 1));
        assert_eq!(
            a.quoted_from.as_deref(),
            Some("田中 一郎 <tanaka@example.jp>")
        );
        assert_eq!(
            a.quoted_at.as_deref(),
            Some("Friday, October 2, 2026 5:55 PM")
        );
        assert_eq!(
            a.subject.as_deref(),
            Some("RE: プレカット図 チェックバック")
        );
        assert_eq!(a.body, "鈴木様\n修正点をまとめました。");
        assert_eq!(
            b.quoted_from.as_deref(),
            Some("高橋 花子 <takahashi@example.com>")
        );
        assert_eq!(
            b.quoted_at.as_deref(),
            Some("Friday, October 2, 2026 2:50 PM")
        );
        assert_eq!(b.body, "田中様、鈴木様\n図面を送ります。");
        assert_ne!(a.fingerprint, b.fingerprint);
    }

    /// 日本語の Outlook（区切り無し・差出人/送信日時）と、Gmail の転送も 1 通ずつに分ける。
    #[test]
    fn splits_japanese_outlook_header_and_gmail_forward() {
        let body = "転送します。\n\n\
---------- Forwarded message ---------\n\
From: 高橋 花子 <takahashi@example.com>\n\
Date: 2026年10月2日(金) 14:50\n\
Subject: 図面\n\
To: <tanaka@example.jp>\n\
\n\
図面です。\n\
\n\
差出人: 田中 一郎 <tanaka@example.jp>\n\
送信日時: 2026年10月1日 9:30\n\
宛先: 高橋 花子\n\
件名: 図面のお願い\n\
\n\
図面をお願いします。";
        let s = split_reply(body);
        assert_eq!(s.clean, "転送します。");
        assert_eq!(s.quotes.len(), 2);
        assert_eq!(
            s.quotes[0].quoted_at.as_deref(),
            Some("2026年10月2日(金) 14:50")
        );
        assert_eq!(s.quotes[0].body, "図面です。");
        assert_eq!(
            s.quotes[1].quoted_from.as_deref(),
            Some("田中 一郎 <tanaka@example.jp>")
        );
        assert_eq!(s.quotes[1].quoted_at.as_deref(), Some("2026年10月1日 9:30"));
        assert_eq!(s.quotes[1].body, "図面をお願いします。");
    }

    /// `>` の中に入った見出しでは分けない（引用の中の引用は、外側の 1 通の一部）。
    #[test]
    fn does_not_split_inside_gt_quotes() {
        let body = "OK\n\nOn Mon, 30 Jun 2026 10:00, Taro <taro@example.com> wrote:\n> hi\n> -----Original Message-----\n> From: Jiro <jiro@example.com>\n> Sent: Monday, June 29, 2026 9:00 AM\n> older";
        let s = split_reply(body);
        assert_eq!(s.quotes.len(), 1);
        assert!(s.quotes[0].body.starts_with("> hi"));
    }

    /// 転送と返信が重なった本文（実例の区切りの書き方のまま。個人名とアドレスは作り物）。
    /// Thunderbird の転送・太字の Outlook 見出し・"On … wrote:"・字下げした Gmail 転送と
    /// Outlook の「元のメッセージ」が、1 本の引用の中に続いている。
    fn stacked() -> String {
        [
            "皆様",
            "",
            "下記、転送します。",
            "",
            "田中",
            "",
            "-------- Forwarded Message --------",
            "Subject: \tRE: 図面の件",
            "Date: \tFri, 9 Oct 2026 15:29:01 +0900",
            "From: \t木材商事 山田花子 <hanako@mokuzai.example.jp>",
            "To: \t田中 <tanaka@example.jp>",
            "",
            "田中様",
            "10/9 の本文です。",
            "",
            "*From:*木材商事　　山田花子<hanako@mokuzai.example.jp>",
            "*Sent:* Wednesday, October 7, 2026 7:36 PM",
            "*To:* 佐藤一郎<i-sato@kensetsu.example.co.jp>",
            "*Subject:* RE: 図面の件",
            "",
            "佐藤様",
            "10/7 の本文です。",
            "",
            "*From:*佐藤一郎<i-sato@kensetsu.example.co.jp>",
            "*Sent:* Tuesday, October 6, 2026 11:28 AM",
            "*To:* 木材商事　　山田花子<hanako@mokuzai.example.jp>",
            "",
            "山田様",
            "10/6 の本文です。",
            "",
            "On 2026/10/05 6:28, 鈴木次郎wrote:",
            "        佐藤様",
            "        10/5 の本文です。",
            "",
            "        ---------- Forwarded message ---------",
            "        From: <taro@maeda.example> <mailto:taro@maeda.example>",
            "        Date: 2026年10月3日(土) 12:37",
            "        Subject: 図面",
            "        To: <jiro@example.jp>",
            "",
            "        10/3 の本文です。",
            "          字下げの続き。",
            "",
            "        -----Original Message-----",
            "        From: taro@maeda.example <taro@maeda.example> <mailto:taro@maeda.example>",
            "        Sent: Friday, October 2, 2026 5:55 PM",
            "        To: 高橋 健<12345TAKA@city.example.lg.jp>",
            "",
            "        10/2 夕方の本文です。",
            "",
            "        -----Original Message-----",
            "        From: 高橋 健<12345TAKA@city.example.lg.jp>",
            "        Sent: Friday, October 2, 2026 2:50 PM",
            "",
            "        10/2 昼の本文です。",
        ]
        .join("\n")
    }

    /// 太字の見出し（"*From:*"）・途中の "On … wrote:"・字下げした区切りのどれでも 1 通ずつに分ける。
    #[test]
    fn splits_stacked_forwards_bold_headers_and_inner_wrote() {
        let s = split_reply(&stacked());
        assert_eq!(s.clean, "皆様\n\n下記、転送します。\n\n田中");
        let got: Vec<(Option<&str>, Option<&str>)> = s
            .quotes
            .iter()
            .map(|q| (q.quoted_from.as_deref(), q.quoted_at.as_deref()))
            .collect();
        assert_eq!(
            got,
            vec![
                (
                    Some("木材商事 山田花子 <hanako@mokuzai.example.jp>"),
                    Some("Fri, 9 Oct 2026 15:29:01 +0900")
                ),
                (
                    Some("木材商事　　山田花子<hanako@mokuzai.example.jp>"),
                    Some("Wednesday, October 7, 2026 7:36 PM")
                ),
                (
                    Some("佐藤一郎<i-sato@kensetsu.example.co.jp>"),
                    Some("Tuesday, October 6, 2026 11:28 AM")
                ),
                (Some("鈴木次郎"), Some("2026/10/05 6:28")),
                (
                    Some("<taro@maeda.example> <mailto:taro@maeda.example>"),
                    Some("2026年10月3日(土) 12:37")
                ),
                (
                    Some("taro@maeda.example <taro@maeda.example> <mailto:taro@maeda.example>"),
                    Some("Friday, October 2, 2026 5:55 PM")
                ),
                (
                    Some("高橋 健<12345TAKA@city.example.lg.jp>"),
                    Some("Friday, October 2, 2026 2:50 PM")
                ),
            ]
        );
        let bodies: Vec<&str> = s.quotes.iter().map(|q| q.body.as_str()).collect();
        assert_eq!(bodies[0], "田中様\n10/9 の本文です。");
        assert_eq!(bodies[1], "佐藤様\n10/7 の本文です。");
        assert_eq!(bodies[3], "佐藤様\n10/5 の本文です。");
        // 字下げした転送の中の 1 通は左に寄せる（中の相対的な字下げは残す）。
        assert_eq!(bodies[4], "10/3 の本文です。\n  字下げの続き。");
        assert_eq!(bodies[6], "10/2 昼の本文です。");
        let orders: Vec<i64> = s.quotes.iter().map(|q| q.order).collect();
        assert_eq!(orders, (0..7).collect::<Vec<_>>());
    }

    /// 引用の途中でも、日付もアドレスも無い「…wrote:」「…書きました:」の文では分けない。
    #[test]
    fn inner_sentence_ending_with_wrote_is_not_a_boundary() {
        let body = "OK\n\n-----Original Message-----\nFrom: Taro <taro@example.com>\nSent: Friday, October 2, 2026 5:55 PM\n\nOn Monday, I wrote:\n議事録をまとめて共有すると彼は書きました:\nよろしく";
        let s = split_reply(body);
        assert_eq!(s.quotes.len(), 1);
        assert!(s.quotes[0].body.contains("On Monday, I wrote:"));
        assert!(s.quotes[0].body.contains("彼は書きました:"));
    }

    /// Outlook の見出しの間に、Thunderbird の表の形の転送（項目名と値が別の行）が挟まっても、
    /// 1 通ずつに分け、差出人・日時を読む（名前・宛先・URL は作り物）。
    #[test]
    fn splits_thunderbird_table_forward_between_outlook_headers() {
        let body = "前田様\n\n転送します。\n\n\
From: 仲村 亮 <ryo@shinko.example.jp> \r\n\
Sent: Tuesday, October 20, 2026 11:36 AM\r\n\
To: Mak <mak@example.jp>\r\n\
Subject: Fwd: 図面 その3\r\n\
\r\n\
前田様\r\n\r\n下記の通りです。\r\n\r\n新興　仲村\r\n\r\n\r\n\
-------- Forwarded Message -------- \r\n\r\n\r\n\
Subject: \r\n\r\nRE: Fwd: 図面 その3\r\n\r\n\r\n\
Date: \r\n\r\nTue, 20 Oct 2026 11:21:08 +0900\r\n\r\n\r\n\
From: \r\n\r\n木材商事 山田花子  <mailto:hanako@mokuzai.example.jp> <hanako@mokuzai.example.jp>\r\n\r\n\r\n\
To: \r\n\r\n'仲村 亮'  <mailto:ryo@shinko.example.jp> <ryo@shinko.example.jp>\r\n\r\n\r\n\r\n\
仲村様\r\n\r\n図面はこちらから取ってください。\r\nhttps://files.example.jp/d/abc123\r\n\r\n\
From: 仲村 亮 <ryo@shinko.example.jp> \r\n\
Sent: Monday, October 19, 2026 6:46 AM\r\n\
To: 木材商事 山田花子 <hanako@mokuzai.example.jp>\r\n\
Subject: Fwd: 図面 その3\r\n\
\r\n\
山田様\r\nお願いします。";
        let s = split_reply(body);
        assert_eq!(s.clean, "前田様\n\n転送します。");
        assert_eq!(s.quotes.len(), 3);
        let tb = &s.quotes[1];
        assert_eq!(
            tb.quoted_from.as_deref(),
            Some(
                "木材商事 山田花子  <mailto:hanako@mokuzai.example.jp> <hanako@mokuzai.example.jp>"
            )
        );
        assert_eq!(
            tb.quoted_at.as_deref(),
            Some("Tue, 20 Oct 2026 11:21:08 +0900")
        );
        assert_eq!(tb.subject.as_deref(), Some("RE: Fwd: 図面 その3"));
        assert!(tb.body.starts_with("仲村様"), "{:?}", tb.body);
        assert!(tb.body.contains("https://files.example.jp/d/abc123"));
        // 直前の 1 通は自分の署名で終わり、転送の中身を抱えない。
        assert!(
            s.quotes[0].body.ends_with("新興　仲村"),
            "{:?}",
            s.quotes[0].body
        );
        assert_eq!(
            s.quotes[2].quoted_at.as_deref(),
            Some("Monday, October 19, 2026 6:46 AM")
        );
    }

    /// "-------- Original Message --------" の表の形も同じに読む。
    #[test]
    fn splits_thunderbird_table_original_message() {
        let body = "了解です。\n\n-------- Original Message --------\n\nSubject:\n\n見積もり\n\n\
Date:\n\nFri, 9 Oct 2026 15:29:01 +0900\n\nFrom:\n\n鈴木 次郎 <jiro@example.com>\n\n\
To:\n\n佐藤 一郎 <ichiro@example.jp>\n\n\n佐藤様\n見積もりを送ります。";
        let s = split_reply(body);
        assert_eq!(s.clean, "了解です。");
        assert_eq!(s.quotes.len(), 1);
        let q = &s.quotes[0];
        assert_eq!(
            q.quoted_from.as_deref(),
            Some("鈴木 次郎 <jiro@example.com>")
        );
        assert_eq!(
            q.quoted_at.as_deref(),
            Some("Fri, 9 Oct 2026 15:29:01 +0900")
        );
        assert_eq!(q.body, "佐藤様\n見積もりを送ります。");
    }

    #[test]
    fn fingerprint_is_whitespace_stable() {
        assert_eq!(fingerprint("hello   world"), fingerprint("hello world\n"));
        assert_ne!(fingerprint("a"), fingerprint("b"));
    }

    #[test]
    fn content_match_trims_unrecognized_attribution() {
        use std::collections::HashSet;
        // 親メールの新規部分の先頭行がアンカー。
        let mut anchors = HashSet::new();
        anchors.insert("見積もりの件、了解しました。".to_string());
        // 子メール: 日付なしの未知属性行＋インライン引用（`>` 無し）で親の内容を引いている。
        // ヒューリスティック単体では切れないが、内容一致で引用と確認して落とす。
        let clean = "承知しました。\n\n田中 が書きました:\n見積もりの件、了解しました。\nよろしく";
        assert_eq!(cut_at_known_anchor(clean, &anchors), "承知しました。");
    }

    #[test]
    fn content_match_ignores_coincidental_line_without_quote_context() {
        use std::collections::HashSet;
        let mut anchors = HashSet::new();
        anchors.insert("よろしくお願いします".to_string());
        // 新規本文にたまたま同じ行があるが、`>` も直前の属性行も無い → 切らない（誤検知回避）。
        let clean = "本題です。\nよろしくお願いします\n続きの本文";
        assert_eq!(cut_at_known_anchor(clean, &anchors), clean);
    }

    #[test]
    fn first_anchor_skips_short_lines() {
        assert_eq!(first_anchor("はい\n\n見積もりの件について"), Some("見積もりの件について".to_string()));
        assert_eq!(first_anchor("> 引用だけ"), Some("引用だけ".to_string()));
    }

    /// 手元確認用: 実物の本文を `RONDINE_QUOTE_SAMPLE`（ファイル。先頭 3 行は DB の行情報）から読み、
    /// 枠ごとの差出人・日時・本文の最初と最後の行を出す。本物の本文はリポジトリに入れない。
    /// `RONDINE_QUOTE_SAMPLE=… cargo test dump_quote_sample -- --ignored --nocapture`
    #[test]
    #[ignore = "手元の実物の本文が要る"]
    fn dump_quote_sample() {
        let Ok(path) = std::env::var("RONDINE_QUOTE_SAMPLE") else {
            return;
        };
        let text = std::fs::read_to_string(path).unwrap();
        let body: String = text.lines().skip(3).collect::<Vec<_>>().join("\n");
        let s = split_reply(&body);
        for q in &s.quotes {
            let lines: Vec<&str> = q.body.lines().filter(|l| !l.trim().is_empty()).collect();
            println!(
                "#{} from={:?} at={:?} subject={:?} lines={}\n    first={:?}\n    last={:?}",
                q.order,
                q.quoted_from,
                q.quoted_at,
                q.subject,
                lines.len(),
                lines.first(),
                lines.last()
            );
        }
    }
}
