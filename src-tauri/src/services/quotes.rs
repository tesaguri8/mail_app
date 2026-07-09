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
    let clean = strip_signature(&lines[..body_end].join("\n").trim().to_string());

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
        || l.contains("元のメッセージ")
        || l.contains("転送メッセージ")
        || l.starts_with("差出人:")
        || l.starts_with("差出人：")
        || lower.starts_with("from:")
        || l.starts_with("送信者:")
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
}
