//! 新規本文（clean_body）から署名を落とす。原文 body_plain は温存する。

// 「強い」署名区切り行（曖昧さの少ないものだけ）。誤爆（本文の一部を署名と誤認）を避ける。
// - is_dash_or_box_delimiter: RFC の "-- "（末尾スペース）／実運用の "--" 単独／"□□□" だけの行
// - is_client_footer_line: 携帯/クライアント定型（"Sent from my …" / "iPhoneから送信" 等）
// ※ 記号区切り "=====…" は newsletter の本文区切りと紛れるため、ここには含めず
//   strip_signature 側で「以降が短い＝署名/フッタ」ときだけ落とす。

/// 書き手が置く署名区切り（"--" / "-- " / "□□□"）か。この下に会議・共有のリンクがあれば切らない。
fn is_dash_or_box_delimiter(line: &str) -> bool {
    let bare = line.trim();
    bare == "--"
        || line.trim_end() == "-- "
        || (bare.chars().count() >= 3 && bare.chars().all(|c| c == '□'))
}

/// 携帯・クライアントが自動で付ける定型（"Sent from my …" 等）か。下にリンクがあっても切る。
fn is_client_footer_line(bare: &str) -> bool {
    let l = bare.to_lowercase();
    l.starts_with("sent from my ")
        || l.starts_with("get outlook for")
        || bare.starts_with("iPhoneから送信")
        || bare.starts_with("Androidから送信")
}

/// 署名区切りの下にあっても本文として残すリンクのホスト（サブドメインも含め、末尾で比べる）。
///
/// 会議の招待（Zoom 等）やファイル共有のリンクは、書き手が「--」の下に貼ることがあり、
/// 署名と一緒に落とすとバブルから入口が消える（実例: 「--」の下に Zoom の招待文が続くメール）。
/// 署名に普通に入るもの（会社のホームページ・YouTube・SNS）は入れない — 入れると署名が残り続ける
/// （実測: 「--」の下の URL の大半はホームページ等。YouTube は宣伝で 41 通）。
const KEEP_LINK_HOSTS: &[&str] = &[
    // 会議
    "zoom.us",
    "zoom.com",
    "teams.microsoft.com",
    "teams.live.com",
    "meet.google.com",
    "webex.com",
    // ファイル共有
    "drive.google.com",
    "docs.google.com",
    "dropbox.com",
    "1drv.ms",
    "onedrive.live.com",
    "sharepoint.com",
    "box.com",
    "wetransfer.com",
    "we.tl",
    "gigafile.nu",
    "firestorage.jp",
    "datadeliver.net",
    // iCloud の共有リンク（www.icloud.com/iclouddrive/… 等）。アドレスの @icloud.com は URL ではないので当たらない
    "icloud.com",
];

/// 行に含まれる http(s) の URL のホスト（小文字）を順に返す。
fn url_hosts(line: &str) -> impl Iterator<Item = String> + '_ {
    let lower = line.to_ascii_lowercase();
    let starts: Vec<usize> = lower
        .match_indices("http")
        .map(|(i, _)| i)
        .filter(|&i| lower[i..].starts_with("http://") || lower[i..].starts_with("https://"))
        .collect();
    starts.into_iter().filter_map(move |i| {
        let rest = &lower[i..];
        let after = &rest[rest.find("//")? + 2..];
        let host: String = after
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-')
            .collect();
        (!host.is_empty()).then_some(host)
    })
}

/// ホストが KEEP_LINK_HOSTS のどれか（そのもの、またはそのサブドメイン）か。
fn is_keep_link_host(host: &str) -> bool {
    let host = host.trim_end_matches('.');
    KEEP_LINK_HOSTS
        .iter()
        .any(|d| host == *d || host.strip_suffix(d).is_some_and(|head| head.ends_with('.')))
}

/// 区切りより下（引用の手前まで）に、残すべきリンクがあるか。
fn has_keep_link(tail: &[&str]) -> bool {
    tail.iter()
        .any(|line| url_hosts(line).any(|h| is_keep_link_host(&h)))
}

/// 署名のまとまりとみなす最大の実質行数（区切り行・空行を除く）。これを超えるまとまりは
/// 本文の段落・ニュースレターの節とみなして隠さない（誤爆の保険）。
const MAX_SIG_LINES: usize = 12;

/// 罫線に使う記号のうち、3 個並べば罫線とみなすもの（全角・罫線素片・飾りの記号）。
/// 本文の中でこれだけを 3 個以上並べた行は、まず区切りか飾りの線になる。
const RULE_CHARS: &[char] = &[
    '─', '━', '═', '―', '┄', '┅', '┈', '┉', '＝', '＊', '＿', '－', 'ー', '～', '〜', '■', '□',
    '◆', '◇', '●', '○', '★', '☆',
];

/// 罫線に使う記号のうち、本文の書き方（Markdown の見出しの下線 "===" / "---"、区切りの "***"、
/// 間を置く「・・・」）とも紛れるもの。5 個以上並んだときだけ罫線とみなす。
const WEAK_RULE_CHARS: &[char] = &['-', '=', '_', '*', '~', '・', '･'];

/// 飾りの罫線の中ほどに入っていても、罫線とみなさない語（転送・元のメッセージの区切りは引用の扱い）。
const NOT_RULE_WORDS: &[&str] = &[
    "message",
    "forward",
    "original",
    "メッセージ",
    "転送",
    "引用",
];

/// 記号の並び（空白は無視）が罫線の長さに届いているか。行全体に使えば「記号だけの罫線の行か」
/// （"━━━━" / "────" / "-----" / "＊＊＊" / "ーーー" / "=====" 等）。全角・罫線素片を含めば 3 個、
/// 紛れやすい半角の記号だけなら 5 個。並びが空・罫線の記号以外を含めば false。
fn is_rule_run(run: &str) -> bool {
    let syms: Vec<char> = run.chars().filter(|c| !c.is_whitespace()).collect();
    if syms.is_empty()
        || !syms
            .iter()
            .all(|c| RULE_CHARS.contains(c) || WEAK_RULE_CHARS.contains(c))
    {
        return false;
    }
    let need = if syms.iter().any(|c| RULE_CHARS.contains(c)) {
        3
    } else {
        5
    };
    syms.len() >= need
}

/// 両端を罫線で挟んだ飾りの行か（"━━━ 株式会社サンプル ━━━"）。署名の頭や尻によく使われる。
/// 中ほどは短い文字だけ（40 文字まで・URL を含まない）。転送の区切りの語を含むものは除く。
fn is_decorated_rule_line(bare: &str) -> bool {
    let is_sym = |c: char| RULE_CHARS.contains(&c) || WEAK_RULE_CHARS.contains(&c);
    let head_len: usize = bare
        .chars()
        .take_while(|&c| is_sym(c) || c.is_whitespace())
        .map(char::len_utf8)
        .sum();
    let tail_len: usize = bare
        .chars()
        .rev()
        .take_while(|&c| is_sym(c) || c.is_whitespace())
        .map(char::len_utf8)
        .sum();
    if head_len + tail_len >= bare.len() {
        return false;
    }
    let middle = bare[head_len..bare.len() - tail_len].trim();
    let lower = middle.to_lowercase();
    is_rule_run(&bare[..head_len])
        && is_rule_run(&bare[bare.len() - tail_len..])
        && middle.chars().count() <= 40
        && !lower.contains("://")
        && !NOT_RULE_WORDS.iter().any(|w| lower.contains(w))
}

/// 署名の区切りの候補 1 つ。`col` は行の中で署名が始まる位置（行全体なら 0。行内の "□□□" 用）。
/// `strong` は書き手が署名の印として置く区切り（"--" / "-- " / "□□□"）。
#[derive(Debug, Clone, Copy)]
struct Delimiter {
    line: usize,
    col: usize,
    strong: bool,
}

/// 行全体が区切りか（"--" / "-- " / "□□□" だけの行 / 罫線 / 飾りの罫線）。
fn is_whole_line_delimiter(line: &str) -> bool {
    let bare = line.trim();
    is_dash_or_box_delimiter(line) || is_rule_run(bare) || is_decorated_rule_line(bare)
}

/// 1 行が区切りの候補なら、署名が始まる位置（行内の列）と強さを返す。
fn delimiter_of(line: &str) -> Option<(usize, bool)> {
    if is_dash_or_box_delimiter(line) {
        Some((0, true))
    } else if is_whole_line_delimiter(line) {
        Some((0, false))
    } else {
        // 行内の "□□□"（改行が失われた Apple Mail 系）。手前の本文は残す。
        line.find("□□□").map(|pos| (pos, true))
    }
}

/// 区切り `d` から下（まとまり）の行。行内の区切りなら、その行は区切りから後ろだけ。
fn block_lines<'a>(lines: &[&'a str], d: Delimiter) -> Vec<&'a str> {
    std::iter::once(&lines[d.line][d.col..])
        .chain(lines[d.line + 1..].iter().copied())
        .collect()
}

/// まとまりの実質の行数（空行と、行全体が区切りの行は数えない）。
/// 行内の "□□□" の後ろに文字が続く（"□□□伊佐日和…"）なら、その行も 1 行と数える。
fn substantive_lines(lines: &[&str], d: Delimiter) -> usize {
    let first = &lines[d.line][d.col..];
    let first_counts = !is_whole_line_delimiter(first)
        && !first.trim_start().trim_start_matches('□').trim().is_empty();
    usize::from(first_counts)
        + lines[d.line + 1..]
            .iter()
            .filter(|l| !l.trim().is_empty() && !is_whole_line_delimiter(l))
            .count()
}

/// 区切り `d` から下を隠してよいか（誤爆の保険）。実質の行が多すぎる・残すべきリンクがある・
/// 隠すと本文が空になる、のどれかなら隠さない。
fn hideable(lines: &[&str], d: Delimiter) -> bool {
    substantive_lines(lines, d) <= MAX_SIG_LINES
        && !has_keep_link(&block_lines(lines, d))
        && !head_of(lines, d).trim().is_empty()
}

/// 区切り `d` より上（残す本文）。
fn head_of(lines: &[&str], d: Delimiter) -> String {
    let mut s = lines[..d.line].join("\n");
    let head = lines[d.line][..d.col].trim_end();
    if !head.is_empty() {
        if !s.is_empty() {
            s.push('\n');
        }
        s.push_str(head);
    }
    s
}

/// 署名らしさの印のうち、そのまま含まれていればよい語（全角・日本語・記号）。
/// 印の無いまとまり（通知メールの本文の段落・お客様の声・注意書きだけ）を署名と取り違えないため、
/// 隠すのは連絡先や組織名の印があるまとまりだけにする（実測: 印の無いまとまりに誤爆が集まっていた）。
const SIGNATURE_MARKERS: &[&str] = &[
    "ＴＥＬ",
    "電話",
    "ＦＡＸ",
    "〒",
    "http://",
    "https://",
    "www.",
    "株式会社",
    "有限会社",
    "合同会社",
    "(株)",
    "（株）",
    "携帯",
    "住所",
    "Inc.",
    "Co., Ltd",
    "Co.,Ltd",
    "LLC",
];

/// 署名らしさの印のうち、英字の語として現れたときだけ数えるもの（大文字小文字は問わない）。
/// "hotel" の "tel" や "faxed" のような語の一部では数えない。
const SIGNATURE_WORDS: &[&str] = &["tel", "fax", "mobile", "email", "e-mail"];

/// 行に `word` が英字の語として含まれるか（前後が英字でない）。
fn has_ascii_word(lower: &str, word: &str) -> bool {
    lower.match_indices(word).any(|(i, _)| {
        let before = lower[..i].chars().next_back();
        let after = lower[i + word.len()..].chars().next();
        !before.is_some_and(|c| c.is_ascii_alphabetic())
            && !after.is_some_and(|c| c.is_ascii_alphabetic())
    })
}

/// 行にメールアドレス（x@y.z）が含まれるか。
fn has_email_address(line: &str) -> bool {
    line.split(|c: char| c.is_whitespace() || "<>()（）:：,;".contains(c))
        .filter_map(|tok| tok.split_once('@'))
        .any(|(local, domain)| {
            !local.is_empty()
                && domain
                    .split_once('.')
                    .is_some_and(|(h, t)| !h.is_empty() && !t.is_empty())
        })
}

/// まとまりに署名らしさの印（連絡先・組織名）があるか。
fn has_signature_marker(block: &[&str]) -> bool {
    block.iter().any(|line| {
        let lower = line.to_lowercase();
        SIGNATURE_MARKERS.iter().any(|m| line.contains(m))
            || SIGNATURE_WORDS.iter().any(|w| has_ascii_word(&lower, w))
            || has_email_address(line)
    })
}

/// 署名として隠すまとまりの始まり（無ければ None）。純関数。
/// 区切りを下から順に見ていく:
/// - 実質の行が MAX_SIG_LINES を超えたら、そこで打ち切る（それより上は本文）。
/// - 隠せない（残すべきリンクがある・本文が空になる）なら、1 つ上へ。
/// - まとまりに署名らしさの印があるか、強い区切り（"--" / "-- " / "□□□"。書き手が置く署名の印）なら、
///   そこから下を隠す。どちらでもなければ 1 つ上へ
///   （閉じの罫線や、署名の下に付いた注意書き・© の行も、上の署名と一緒に隠れる）。
/// - 決まった区切りのすぐ上の区切りが強い区切り（"--" 等）で、そこから下でも隠せるなら、そこまで広げる
///   （"--" の下に罫線で囲んだ署名を置く書き方）。
///
/// 途中の区切りは本文の見出し・段落の区切りとして残る。
fn signature_start(lines: &[&str]) -> Option<Delimiter> {
    let delims: Vec<Delimiter> = lines
        .iter()
        .enumerate()
        .filter_map(|(line, l)| {
            delimiter_of(l).map(|(col, strong)| Delimiter { line, col, strong })
        })
        .collect();
    let found = delims
        .iter()
        .enumerate()
        .rev()
        .take_while(|(_, d)| substantive_lines(lines, **d) <= MAX_SIG_LINES)
        .find(|(_, d)| {
            hideable(lines, **d) && (d.strong || has_signature_marker(&block_lines(lines, **d)))
        })
        .map(|(i, d)| (i, *d));
    let (i, start) = found?;
    let widened = i
        .checked_sub(1)
        .map(|j| delims[j])
        .filter(|d| d.strong && !start.strong && hideable(lines, *d));
    Some(widened.unwrap_or(start))
}

/// clean_body（引用除去後の新規本文）から署名を落とす。原文 body_plain は温存し、
/// 全文表示（body_plain）ではこれまで通り署名も見られる。表示・保存の派生テキスト専用。純関数。
/// 方針（docs/THREADING.md §7）:
/// - 携帯・クライアントの定型（"Sent from my …" / "iPhoneから送信"）は、どこにあってもその行から下を落とす。
/// - 区切りの候補（"--" / "-- " / "□□□" / 記号だけの罫線 / 両端を罫線で挟んだ飾りの行）を全部拾い、
///   下から見て、署名らしさの印（連絡先・組織名）がある末尾のまとまりを署名として落とす（signature_start）。
///   途中の区切りは本文として残す。
/// - まとまりの実質の行が MAX_SIG_LINES を超える・会議の招待やファイル共有のリンク（KEEP_LINK_HOSTS）
///   がある・落とすと本文が空になる、のどれかなら落とさない。
pub(super) fn strip_signature(clean: &str) -> String {
    let all: Vec<&str> = clean.lines().collect();
    // 携帯の定型より下は、どこにあっても落とす（定型より上に本文があるときだけ）。
    let end = all
        .iter()
        .position(|l| is_client_footer_line(l.trim()))
        .filter(|&i| all[..i].iter().any(|l| !l.trim().is_empty()))
        .unwrap_or(all.len());
    let lines = &all[..end];
    let kept = match signature_start(lines) {
        Some(d) => head_of(lines, d),
        None => lines.join("\n"),
    };
    kept.trim_end().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::quotes::split_reply;

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
        assert_eq!(
            s.clean,
            "松田様、伊佐様\n\n江島邸の件、承知しました。\n\n末松"
        );
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

    /// 「--」の下の Zoom の招待は署名ではない。区切り行も含めて本文に残す。
    #[test]
    fn keeps_zoom_invite_below_dashes() {
        let body =
            "皆さま\n\n明日の打ち合わせはオンラインで行います。\nよろしくお願いします。\n\n山田\n\
--\n\
山田 太郎 さんがあなたを予約された Zoom ミーティングに招待しています。\n\
トピック: 定例会議\n\
時間: 2026年11月10日 10:00 AM 大阪、札幌、東京\n\
Zoom ミーティングに参加する\n\
https://us02web.zoom.us/j/81234567890?pwd=abcdef.1\n\
ミーティング ID: 812 3456 7890\n\
パスコード: 123456";
        let s = split_reply(body);
        assert_eq!(s.clean, body);
        // 下に引用があっても、引用の手前までを本文に残す。
        let with_quote = format!("{body}\n\nOn Mon, Nov 9, 2026 at 9:00 AM 佐藤 <sato@example.invalid> wrote:\n> 了解です");
        assert_eq!(split_reply(&with_quote).clean, body);
    }

    /// 「-- 」の下の Google Drive の共有リンクも残す。記号区切り（===）でも同じ。
    #[test]
    fn keeps_drive_share_link_below_delimiters() {
        let dash = "資料を共有します。\n\n-- \n資料フォルダ:\nhttps://drive.google.com/drive/folders/1AbCdEf?usp=sharing\n山田";
        assert_eq!(strip_signature(dash), dash);
        let rule = "資料を共有します。\n==========\n議事録: https://docs.google.com/document/d/1xyz/edit\n山田";
        assert_eq!(strip_signature(rule), rule);
    }

    /// 会社の住所とホームページだけの普通の署名は、これまでどおり落ちる。
    #[test]
    fn plain_company_signature_is_still_stripped() {
        let body = "承知しました。\n\n--\n株式会社サンプル 山田太郎\n〒100-0001 東京都千代田区千代田1-1\nTEL 03-0000-0000\nhttps://www.sample.invalid/";
        assert_eq!(strip_signature(body), "承知しました。");
    }

    /// YouTube の URL だけの署名（宣伝）も、これまでどおり落ちる。
    #[test]
    fn youtube_only_signature_is_still_stripped() {
        let body = "了解です。\n-- \n山田太郎\n新しい動画を公開しました https://www.youtube.com/watch?v=abc https://youtu.be/abc";
        assert_eq!(strip_signature(body), "了解です。");
    }

    /// ホストは末尾で比べる（サブドメインは当たる・似た名前のドメインは当たらない）。
    #[test]
    fn keep_link_host_matches_by_suffix() {
        assert!(is_keep_link_host("us02web.zoom.us"));
        assert!(is_keep_link_host("zoom.us"));
        assert!(is_keep_link_host("www.dropbox.com"));
        assert!(!is_keep_link_host("notzoom.us"));
        assert!(!is_keep_link_host("zoom.us.example.invalid"));
        assert!(!is_keep_link_host("www.youtube.com"));
        let hosts: Vec<String> =
            url_hosts("会議 HTTPS://Meet.Google.com/abc-defg-hij と http://x.invalid").collect();
        assert_eq!(hosts, vec!["meet.google.com", "x.invalid"]);
    }

    /// 携帯の定型（Sent from my …）は、下にリンクがあっても切る（対象外）。
    #[test]
    fn client_footer_still_cuts_even_with_link() {
        let body = "了解です。\nSent from my iPhone\nhttps://zoom.us/j/1";
        assert_eq!(strip_signature(body), "了解です。");
    }

    /// 罫線（━）で上下を挟んだ日本の会社の署名は隠れる。
    #[test]
    fn hides_heavy_rule_enclosed_company_signature() {
        let body = "山田様\n\nお見積りを添付いたします。\nご確認ください。\n\n\
━━━━━━━━━━━━━━━━━━━━\n\
株式会社サンプル 営業部\n\
佐藤 一郎\n\
〒100-0001 東京都千代田区千代田1-1\n\
TEL: 03-0000-0000 / FAX: 03-0000-0001\n\
https://www.sample.invalid/\n\
━━━━━━━━━━━━━━━━━━━━";
        assert_eq!(
            strip_signature(body),
            "山田様\n\nお見積りを添付いたします。\nご確認ください。"
        );
    }

    /// 細い罫線（─）で挟んだ署名も、両端を罫線で飾った見出し（"━━ 社名 ━━"）で始まる署名も隠れる。
    #[test]
    fn hides_light_rule_and_decorated_rule_signatures() {
        let light = "承知しました。\n─────────────\n佐藤 一郎\nTEL 090-0000-0000\n─────────────";
        assert_eq!(strip_signature(light), "承知しました。");
        let decorated =
            "承知しました。\n\n━━━ 株式会社サンプル ━━━\n佐藤 一郎\nsato@sample.invalid\n━━━━━━━━━━━━━━";
        assert_eq!(strip_signature(decorated), "承知しました。");
    }

    /// 署名らしさの印: 英字の語は語として現れたときだけ数える（"hotel" の "tel" は数えない）。
    #[test]
    fn signature_markers() {
        assert!(has_signature_marker(&["TEL:03-0000-0000"]));
        assert!(has_signature_marker(&["Tel.090"]));
        assert!(has_signature_marker(&["〒100-0001"]));
        assert!(has_signature_marker(&["（株）サンプル"]));
        assert!(has_signature_marker(&["E-mail: a@b.invalid"]));
        assert!(has_signature_marker(&["連絡先 <a@b.invalid>"]));
        assert!(has_signature_marker(&["www.sample.invalid"]));
        assert!(!has_signature_marker(&[
            "We loved the hotel and the staff."
        ]));
        assert!(!has_signature_marker(&["@mention だけ", "a@b"]));
    }

    /// 「-----」の下の名前・電話・URL だけの署名は隠れる（閉じの罫線が無くても）。
    #[test]
    fn hides_ascii_dash_rule_signature() {
        let body = "ありがとうございます。\n明日伺います。\n\n-----\nTaro Yamada\n+81-90-0000-0000\nhttps://yamada.invalid";
        assert_eq!(
            strip_signature(body),
            "ありがとうございます。\n明日伺います。"
        );
    }

    /// 本文の途中の罫線（見出し・段落の区切り）は残し、末尾のまとまりだけを隠す。
    #[test]
    fn keeps_rules_in_the_middle_and_hides_only_the_last_block() {
        let head = "皆さま\n\n来週の定例の議題です。\n────────────\n■ 議題\n1. 進捗\n2. 予算\n────────────\n以上、よろしくお願いします。\n山田";
        let body = format!(
            "{head}\n\n＊＊＊＊＊＊＊＊＊＊\n山田 太郎\n株式会社サンプル\n＊＊＊＊＊＊＊＊＊＊"
        );
        assert_eq!(strip_signature(&body), head);
    }

    /// 末尾のまとまりの実質の行が 13 行以上なら、署名ではなく本文の節とみなして残す。
    #[test]
    fn keeps_a_long_last_block() {
        let lines: Vec<String> = (1..=13).map(|n| format!("{n}. 項目の説明")).collect();
        let body = format!("本日の報告です。\n━━━━━━━━━━\n{}", lines.join("\n"));
        assert_eq!(strip_signature(&body), body);
        // 印のある 12 行までなら署名として隠す。
        let twelve = format!(
            "本日の報告です。\n━━━━━━━━━━\n{}\nTEL 03-0000-0000",
            lines[..11].join("\n")
        );
        assert_eq!(strip_signature(&twelve), "本日の報告です。");
    }

    /// 罫線だけで本文が無いメールは空にしない。
    #[test]
    fn rule_only_body_is_not_emptied() {
        let body = "━━━━━━━━━━\n━━━━━━━━━━";
        assert_eq!(strip_signature(body), body);
        let sig_only = "──────────\n山田 太郎\nTEL 03-0000-0000";
        assert_eq!(strip_signature(sig_only), sig_only);
    }

    /// 「--」の下に罫線で囲んだ署名を置く書き方は、「--」から下をまとめて隠す。
    #[test]
    fn dashes_above_enclosed_signature_widen_the_block() {
        let body = "了解です。\n\n--\n━━━━━━━━━━\n山田 太郎\n株式会社サンプル\n━━━━━━━━━━";
        assert_eq!(strip_signature(body), "了解です。");
    }

    /// 「--」が途中にあっても、その下が長ければ本文として残し、末尾のまとまりだけを隠す。
    #[test]
    fn dashes_in_the_middle_are_kept_when_the_rest_is_long() {
        let middle: Vec<String> = (1..=14).map(|n| format!("本文の {n} 行目")).collect();
        let head = format!("前置き\n--\n{}", middle.join("\n"));
        let body = format!("{head}\n─────────\n山田\nTEL 03-0000-0000\n─────────");
        assert_eq!(strip_signature(&body), head);
    }

    /// 紛れやすい短い記号の並び（"---" / "***" / "・・・"）は罫線とみなさない。
    #[test]
    fn short_weak_runs_are_not_rules() {
        for body in [
            "本文\n---\n追伸です",
            "本文\n***\n追伸です",
            "えーと\n・・・\nよろしくお願いします",
        ] {
            assert_eq!(strip_signature(body), body);
        }
        // 全角の記号は 3 個で罫線（"＊＊＊" / "ーーー"）。
        assert_eq!(
            strip_signature("本文です\n＊＊＊\n山田 tel.03-0000-0000"),
            "本文です"
        );
        assert_eq!(
            strip_signature("本文です\nーーー\n山田\nyamada@example.invalid"),
            "本文です"
        );
    }

    /// 転送・元のメッセージの区切りの語を含む飾りの行は、署名の区切りにしない。
    #[test]
    fn forwarded_separator_is_not_a_signature_rule() {
        let body = "参考まで。\n---------- Forwarded message ---------\nFrom: a@example.invalid";
        assert_eq!(strip_signature(body), body);
    }

    /// 携帯の定型は、どこにあってもその行から下を落とす。定型の上の署名のまとまりも隠す。
    #[test]
    fn client_footer_cuts_anywhere_then_last_block() {
        assert_eq!(
            strip_signature("本文です\nSent from my iPhone\n━━━━━━\n山田"),
            "本文です"
        );
        assert_eq!(
            strip_signature("本文です\n━━━━━━\n山田\nTEL 03-0000-0000\niPhoneから送信"),
            "本文です"
        );
    }

    /// 末尾の罫線だけ（下に何も無い）なら、印が無いので落とさない。
    #[test]
    fn trailing_rule_alone_is_kept() {
        assert_eq!(
            strip_signature("本文です\n=========="),
            "本文です\n=========="
        );
    }

    /// 罫線で挟んだ件名と URL の下に続く本文の段落（印が無い・長い）は隠さない（通知メールの形）。
    /// 段落の実質の行が MAX_SIG_LINES を超えると、そこで打ち切って上の URL まで遡らない。
    #[test]
    fn body_paragraph_below_a_ruled_headline_is_kept() {
        let para: Vec<String> = (1..=13)
            .map(|n| format!("お知らせの本文 {n} 行目です。"))
            .collect();
        let body = format!(
            "新しいお知らせがあります。\n==========\n【お知らせ】年末年始の営業について\n\
https://notice.example.invalid/n/123\n==========\n{}",
            para.join("\n")
        );
        assert_eq!(strip_signature(&body), body);
    }

    /// 署名の下に罫線と注意書き 1 行があるときは、署名から下を全部隠す。
    #[test]
    fn hides_signature_together_with_the_notice_below_it() {
        let body = "資料をお送りします。\n\n\
──────────────\n\
株式会社サンプル 佐藤 一郎\n\
FAX: 03-0000-0001\n\
sato@sample.invalid\n\
●○●○●○●○●○\n\
※ このメールは BCC でお送りしています。";
        assert_eq!(strip_signature(body), "資料をお送りします。");
    }

    /// 印の無い 10 行の段落（お客様の声など）は、罫線の下でも隠さない。
    #[test]
    fn markerless_paragraph_below_a_rule_is_kept() {
        let voices: Vec<String> = (1..=10)
            .map(|n| format!("Customer voice {n}: lovely stay, will come again."))
            .collect();
        let body = format!(
            "Thank you for staying with us.\n★★★★★\n{}",
            voices.join("\n")
        );
        assert_eq!(strip_signature(&body), body);
    }

    #[test]
    fn short_equals_run_is_not_a_signature() {
        // 短い "===" は誤爆回避のため署名扱いしない（本文中の区切りを守る）。
        let body = "見出し\n===\n本文の続き";
        assert_eq!(strip_signature(body), body);
    }
}
