//! 内容照合による引用剥がし（docs/THREADING.md §2 優先3。形式非依存）。
//! 同じ会話の過去メールの本文と一致する行を、引用として落とす。

use super::attribution::looks_like_attribution;

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
