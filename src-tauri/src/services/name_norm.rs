//! 名前・文字列の検索用正規化と、あいまい照合（編集距離）。
//!
//! 住所録などの検索で「似た漢字」「表記ゆれ」「打ち間違い」でも引けるようにするための下ごしらえ。
//! 二段構えで扱う:
//!
//! 1. **正規化照合**（[`search_fold`]）— 旧字体・異体字（例: 苅→刈、髙→高、邊/邉→辺）を代表字へ
//!    寄せ、カタカナ→ひらがな・全角→半角・小文字化・空白除去を施してから部分一致する。
//!    「同じ文字の字形違い」だけを畳むので**誤ヒットはほぼ出ない**。
//! 2. **あいまい照合**（[`fuzzy_substring_distance`]）— 上で拾えない打ち間違いや、異体字ではない
//!    紛らわしい別字（例: 斎藤／斉藤）を、編集距離で近い順に**補助的に**拾う。順位は正規化ヒットの下。

use crate::services::dedupe::fold;

/// 検索照合用に文字列を畳む。
///
/// - [`fold`] で全角 ASCII→半角・小文字化
/// - カタカナ→ひらがな（よみの表記ゆれ吸収）
/// - 異体字→代表字（[`canon_kanji`]）
/// - 空白の完全除去（「田中 太郎」と「田中太郎」を同一視）
///
/// 部分一致の両辺（検索語・被検索テキスト）に同じ変換をかけて使う。
pub fn search_fold(s: &str) -> String {
    fold(s)
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(kana_to_hira)
        .map(canon_kanji)
        .collect()
}

/// カタカナ（U+30A1〜U+30F6）を対応するひらがなへ寄せる。それ以外はそのまま。
/// 長音符（ー）や中点はカタカナブロック外なので変換しない。
fn kana_to_hira(c: char) -> char {
    match c {
        '\u{30A1}'..='\u{30F6}' => char::from_u32(c as u32 - 0x60).unwrap_or(c),
        _ => c,
    }
}

/// 異体字・旧字体を代表字（新字体・常用字）へ寄せる。
///
/// ここに載せるのは「同じ文字の字形違い」に限る（旧字体↔新字体、はしごだか等）。
/// 斎/斉 や 島/嶋 のように**別字として姓が分かれている**組は畳まず、あいまい照合側に委ねる
/// （正規化で畳むと順位付けなしに黙って混ざり、誤ヒットになるため）。
fn canon_kanji(c: char) -> char {
    match c {
        '苅' => '刈',
        '髙' => '高',
        '﨑' | '嵜' => '崎',
        '邊' | '邉' => '辺',
        '澤' => '沢',
        '濱' | '濵' => '浜',
        '齋' => '斎',
        '齊' => '斉',
        '廣' => '広',
        '德' => '徳',
        '眞' => '真',
        '淸' => '清',
        '惠' => '恵',
        '槇' => '槙',
        '桒' => '桑',
        '冨' => '富',
        '榮' => '栄',
        '龍' => '竜',
        '瀨' => '瀬',
        '曾' => '曽',
        '增' => '増',
        '圓' => '円',
        '國' => '国',
        '學' => '学',
        '條' => '条',
        '瀧' => '滝',
        '渕' | '渊' => '淵',
        '邨' => '村',
        '禮' => '礼',
        '兒' => '児',
        '賴' => '頼',
        '樂' => '楽',
        '縣' => '県',
        '壽' => '寿',
        '團' => '団',
        '亙' => '亘',
        '步' => '歩',
        '每' => '毎',
        '郞' => '郎',
        _ => c,
    }
}

/// パターン `pat`（正規化済みの文字列）を `text`（正規化済み）へ**近似部分一致**させたときの
/// 最小編集距離を返す。text 側の前後の読み飛ばしは自由（＝部分一致）、パターンの過不足は 1 コスト。
///
/// 完全な部分一致なら 0、1 文字違いなら 1 を返す。あいまい照合の近さ指標に使う。
pub fn fuzzy_substring_distance(pat: &str, text: &str) -> usize {
    let pat: Vec<char> = pat.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (m, nt) = (pat.len(), text.len());
    if m == 0 {
        return 0;
    }
    if nt == 0 {
        return m;
    }
    // dp[j] = pat[0..i] と「text の j で終わる部分列」との最小編集距離。
    // 先頭行（空パターン）は全 0＝text のどこからでも一致開始できる（＝部分一致）。
    let mut prev = vec![0usize; nt + 1];
    for i in 1..=m {
        let mut cur = vec![0usize; nt + 1];
        cur[0] = i; // パターン i 文字ぶんの削除
        for j in 1..=nt {
            let cost = usize::from(pat[i - 1] != text[j - 1]);
            cur[j] = (prev[j - 1] + cost) // 置換／一致
                .min(prev[j] + 1) // パターン側の削除
                .min(cur[j - 1] + 1); // text 側の挿入（読み飛ばし）
        }
        prev = cur;
    }
    prev.into_iter().min().unwrap_or(m)
}

/// 検索語長に応じて許容する編集距離のしきい値。短いほど厳しく。
pub fn fuzzy_threshold(query_len: usize) -> usize {
    match query_len {
        0..=1 => 0,
        2..=4 => 1,
        5..=8 => 2,
        _ => query_len / 3,
    }
}

/// 検索語 `query` に対する 1 レコードの一致度を返す。住所録・宛先候補で共通利用する。
///
/// - `contains_fields`: 正規化部分一致の対象（名前・よみ・メール・組織など）
/// - `fuzzy_fields`: あいまい照合（編集距離）の対象。打ち間違い救済したい対象に絞る（通常は名前・よみ）
///
/// 返り値:
/// - `None` — 不一致
/// - `Some(0)` — いずれかに正規化部分一致（確実。異体字・カナ/かな・全角半角・空白差を吸収）
/// - `Some(d)`（`d >= 1`）— あいまい一致（編集距離 `d`。小さいほど近い）
///
/// 各フィールドは生文字列で渡す（内部で [`search_fold`] する）。空フィールドは無視。
pub fn match_rank(query: &str, contains_fields: &[&str], fuzzy_fields: &[&str]) -> Option<usize> {
    let nq = search_fold(query);
    if nq.is_empty() {
        return None;
    }
    if contains_fields.iter().any(|f| {
        let nf = search_fold(f);
        !nf.is_empty() && nf.contains(nq.as_str())
    }) {
        return Some(0);
    }
    let qlen = nq.chars().count();
    if qlen < 2 {
        return None; // 1 文字だけのあいまい照合はノイズが多すぎるので行わない
    }
    fuzzy_fields
        .iter()
        .map(|f| fuzzy_substring_distance(nq.as_str(), search_fold(f).as_str()))
        .min()
        .filter(|&d| d <= fuzzy_threshold(qlen))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variant_kanji_folds_to_canonical() {
        // 苅→刈: 検索語と登録名が異体字違いでも同じ畳み結果になる。
        assert_eq!(search_fold("銘苅"), search_fold("銘刈"));
        assert_eq!(search_fold("髙橋"), search_fold("高橋"));
        assert_eq!(search_fold("渡邊"), search_fold("渡辺"));
        assert_eq!(search_fold("渡邉"), search_fold("渡辺"));
    }

    #[test]
    fn katakana_and_width_fold() {
        // カタカナ→ひらがな、全角英数→半角、空白除去。
        assert_eq!(search_fold("メカル"), search_fold("めかる"));
        assert_eq!(search_fold("ＡＢＣ"), "abc");
        assert_eq!(search_fold("田中 太郎"), search_fold("田中太郎"));
    }

    #[test]
    fn distinct_surnames_not_folded() {
        // 斎/斉 は別姓なので正規化では畳まない（あいまい照合に委ねる）。
        assert_ne!(search_fold("斎藤"), search_fold("斉藤"));
    }

    #[test]
    fn fuzzy_distance_basics() {
        // 完全な部分一致は 0。
        assert_eq!(fuzzy_substring_distance("たなか", "たなかたろう"), 0);
        // 1 文字違いは 1（斎藤 vs 斉藤）。
        assert_eq!(fuzzy_substring_distance("斉藤", "斎藤"), 1);
        // 全く違えば距離は大きい。
        assert!(fuzzy_substring_distance("やまだ", "すずき") >= 2);
    }
}
