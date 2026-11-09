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
/// - "--" / "□□□" / "===" の下に会議の招待・ファイル共有のリンク（KEEP_LINK_HOSTS）があれば、
///   その区切りでは切らない（区切り行も含めて本文に残す）。携帯定型は対象外（いつも切る）。
pub(super) fn strip_signature(clean: &str) -> String {
    // 署名本体とみなす最大の実質行数。これを超えて本文が続く記号区切りは「本文の区切り」とみなす。
    const MAX_SIG_LINES: usize = 10;
    let lines: Vec<&str> = clean.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        let bare = line.trim();
        // (a) 明確な署名区切り。その行以降を落とす。
        let keeps_links = || has_keep_link(&lines[i + 1..]);
        let cut = if is_dash_or_box_delimiter(line) {
            if keeps_links() {
                continue;
            }
            Some(lines[..i].join("\n"))
        } else if is_client_footer_line(bare) {
            Some(lines[..i].join("\n"))
        } else if is_symbol_rule_line(bare) {
            // (b) 記号区切り: 以降が短いとき（末尾の署名/フッタ）だけ落とす。長ければ本文の区切り。
            let tail = lines[i + 1..].iter().filter(|l| !l.trim().is_empty()).count();
            if tail <= MAX_SIG_LINES && !keeps_links() {
                Some(lines[..i].join("\n"))
            } else {
                continue;
            }
        } else if let Some(pos) = line
            .find("□□□")
            .filter(|&pos| !has_keep_link(&[&line[pos..]]) && !keeps_links())
        {
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

    #[test]
    fn short_equals_run_is_not_a_signature() {
        // 短い "===" は誤爆回避のため署名扱いしない（本文中の区切りを守る）。
        let body = "見出し\n===\n本文の続き";
        assert_eq!(strip_signature(body), body);
    }
}
