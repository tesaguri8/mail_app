//! トークン化（docs/SPAM.md §3 / §7.1）。
//! 言語非依存シグナル（URL / 送信元ドメイン）を土台に、本文は
//! 「日本語＝文字 N-gram／ラテン系＝単語分割」のハイブリッドで語を出す。
//! 純ロジック（DB 非依存）でユニットテストしやすくする。
//!
//! 段階1では正規化を「小文字化＋空白圧縮」に留める（依存を増やさない）。
//! NFKC・全角半角統一（§3.1）と日本語形態素（Lindera。§3.3 段階2）は後続で追加する。

/// 保存済みメールの素性からトークン列を作る。
/// 名前空間付き（`from:` / `url:` / `w:` / `ng:`）で衝突を防ぐ（§4.1）。
/// 重複は呼び出し側（学習・スコア）で dedup する。
pub fn tokenize(from_address: Option<&str>, subject: Option<&str>, body: &str) -> Vec<String> {
    let mut toks = Vec::new();

    // (1) 言語非依存シグナル：最優先（本文の分割精度に依存しない）。
    if let Some(d) = from_address.and_then(addr_domain) {
        toks.push(format!("from:{d}"));
    }
    for d in extract_url_domains(body) {
        toks.push(format!("url:{d}"));
    }

    // (2) 本文＋件名：正規化 → 言語ざっくり判定 → 分割。
    let mut text = String::new();
    if let Some(s) = subject {
        text.push_str(s);
        text.push('\n');
    }
    text.push_str(body);
    let norm = normalize(&text);

    if is_cjk_dominant(&norm) {
        for g in char_ngrams(&norm, 2, 3) {
            toks.push(format!("ng:{g}"));
        }
    } else {
        for w in split_words(&norm) {
            toks.push(format!("w:{w}"));
        }
    }
    toks
}

/// メールアドレスの登録ドメイン部（最後の `@` 以降）を小文字で返す。
fn addr_domain(addr: &str) -> Option<String> {
    let (_, domain) = addr.rsplit_once('@')?;
    let d = domain.trim().trim_end_matches('>').to_ascii_lowercase();
    if d.is_empty() {
        None
    } else {
        Some(d)
    }
}

/// 本文中の http(s) URL からホスト名を抽出する（パス・クエリは捨てる）。
/// 依存を増やさないため素朴にスキャンする（段階1）。
fn extract_url_domains(text: &str) -> Vec<String> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut search_from = 0;
    while let Some(rel) = lower[search_from..].find("http") {
        let start = search_from + rel;
        let rest = &lower[start..];
        let after = if let Some(a) = rest.strip_prefix("https://") {
            a
        } else if let Some(a) = rest.strip_prefix("http://") {
            a
        } else {
            search_from = start + 4;
            continue;
        };
        let host: String = after
            .chars()
            .take_while(|c| {
                !matches!(
                    c,
                    '/' | '?' | '#' | ':' | '"' | '\'' | '<' | '>' | ')' | ']'
                ) && !c.is_whitespace()
            })
            .collect();
        if !host.is_empty() {
            out.push(host);
        }
        search_from = start + 4;
    }
    out
}

/// 段階1の正規化: 小文字化＋連続空白の圧縮。
fn normalize(s: &str) -> String {
    s.to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// 空白以外の文字に占める CJK（かな・漢字・全角）の割合が 5 割以上か。
fn is_cjk_dominant(s: &str) -> bool {
    let mut cjk = 0usize;
    let mut total = 0usize;
    for c in s.chars() {
        if c.is_whitespace() {
            continue;
        }
        total += 1;
        if is_cjk_char(c) {
            cjk += 1;
        }
    }
    total > 0 && cjk * 2 >= total
}

fn is_cjk_char(c: char) -> bool {
    matches!(c as u32,
        0x3040..=0x30FF |  // ひらがな・カタカナ
        0x3400..=0x4DBF |  // CJK 拡張 A
        0x4E00..=0x9FFF |  // CJK 統合漢字
        0xFF00..=0xFFEF    // 全角英数・記号
    )
}

/// 文字 N-gram（min..=max）。空白を除いた文字列に対して作る。
fn char_ngrams(s: &str, min: usize, max: usize) -> Vec<String> {
    let chars: Vec<char> = s.chars().filter(|c| !c.is_whitespace()).collect();
    let mut out = Vec::new();
    for n in min..=max {
        if chars.len() < n {
            continue;
        }
        for w in chars.windows(n) {
            out.push(w.iter().collect::<String>());
        }
    }
    out
}

/// ラテン系の単語分割（英数以外を区切りに、2 文字以上の語だけ採用）。
fn split_words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2)
        .map(|w| w.to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_from_domain_and_url() {
        let toks = tokenize(
            Some("Sender <promo@example.com>"),
            None,
            "Visit https://spam.example.net/win now",
        );
        assert!(toks.iter().any(|t| t == "from:example.com"));
        assert!(toks.iter().any(|t| t == "url:spam.example.net"));
    }

    #[test]
    fn japanese_uses_char_ngrams() {
        let toks = tokenize(None, Some("無料"), "当選しました");
        // 2-gram "無料" が生成される（件名も本文と同じ名前空間で語になる）。
        assert!(toks.iter().any(|t| t == "ng:無料"));
        assert!(toks.iter().all(|t| !t.starts_with("w:")));
    }

    #[test]
    fn latin_uses_word_split() {
        let toks = tokenize(None, None, "Free money now");
        assert!(toks.iter().any(|t| t == "w:free"));
        assert!(toks.iter().any(|t| t == "w:money"));
        // N-gram は使わない。
        assert!(toks.iter().all(|t| !t.starts_with("ng:")));
    }
}
