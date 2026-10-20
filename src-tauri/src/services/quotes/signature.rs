//! 新規本文（clean_body）から署名を落とす。原文 body_plain は温存する。

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
pub(super) fn strip_signature(clean: &str) -> String {
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

    #[test]
    fn short_equals_run_is_not_a_signature() {
        // 短い "===" は誤爆回避のため署名扱いしない（本文中の区切りを守る）。
        let body = "見出し\n===\n本文の続き";
        assert_eq!(strip_signature(body), body);
    }
}
