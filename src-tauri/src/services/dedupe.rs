//! 連絡先の重複検出（標準的な record linkage）。
//!
//! 手法は教科書的な決定的リンケージ: 正規化 → ブロッキング（メール/携帯/氏名でバケット化）→
//! 項目比較（完全一致・トークン集合・Jaro–Winkler）→ Union-Find で連結成分にまとめる。
//! 強い証拠（携帯・メール）から確信度を付け、確信度順でグループを返す。実際の統合は UI 側で
//! ユーザーが「残す1件」を選んで行う（自動融合はしない）。外部依存なしの自前実装。

use crate::models::{ContactSummary, DuplicateGroup};
use std::collections::HashMap;

/// 連絡先群を重複候補グループに束ねる（2件以上のみ、確信度順）。
pub fn group(contacts: &[ContactSummary]) -> Vec<DuplicateGroup> {
    let recs: Vec<Rec> = contacts.iter().map(Rec::from_contact).collect();
    let n = recs.len();

    // ブロッキング: 同じキーを持つ index 同士だけを比較候補にする（総当たり回避）。
    let mut by_email: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut by_mobile: HashMap<&str, Vec<usize>> = HashMap::new();
    let mut by_name: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, r) in recs.iter().enumerate() {
        if let Some(e) = &r.email {
            by_email.entry(e).or_default().push(i);
        }
        if let Some(m) = &r.mobile {
            by_mobile.entry(m).or_default().push(i);
        }
        if !r.name_norm.is_empty() {
            by_name.entry(&r.name_norm).or_default().push(i);
        }
    }

    // 候補ペア（無向、重複排除）を集める。
    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for block in by_email
        .values()
        .chain(by_mobile.values())
        .chain(by_name.values())
    {
        for a in 0..block.len() {
            for b in (a + 1)..block.len() {
                let (i, j) = (block[a], block[b]);
                pairs.push(if i < j { (i, j) } else { (j, i) });
            }
        }
    }
    pairs.sort_unstable();
    pairs.dedup();

    // 判定 → 確信度の閾値ごとに別の Union-Find へ張る。
    // uf_all: 全エッジ（候補クラスタ＝再現率）
    // uf_med: High+Medium のみ / uf_high: High のみ
    // グループの確信度は「その閾値のエッジだけで全員が繋がるか」で決める（ボトルネック確信度）。
    // これで「強い対＋弱い橋」で家族が高確信クラスタに紛れ込むのを防ぐ。
    let mut uf_all = UnionFind::new(n);
    let mut uf_med = UnionFind::new(n);
    let mut uf_high = UnionFind::new(n);
    for (i, j) in pairs {
        if let Some(c) = check(&recs[i], &recs[j]) {
            uf_all.union(i, j);
            if c >= Conf::Medium {
                uf_med.union(i, j);
            }
            if c == Conf::High {
                uf_high.union(i, j);
            }
        }
    }

    // 候補クラスタ（uf_all）ごとに連絡先を集める。
    let mut comps: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        comps.entry(uf_all.find(i)).or_default().push(i);
    }

    let mut groups: Vec<DuplicateGroup> = comps
        .into_values()
        .filter(|m| m.len() > 1)
        .map(|members| {
            // ボトルネック確信度: 全員が High だけで一体なら High、次に Medium、無ければ Low。
            let hroot = uf_high.find(members[0]);
            let all_high = members.iter().all(|&m| uf_high.find(m) == hroot);
            let conf = if all_high {
                Conf::High
            } else {
                let mroot = uf_med.find(members[0]);
                if members.iter().all(|&m| uf_med.find(m) == mroot) {
                    Conf::Medium
                } else {
                    Conf::Low
                }
            };
            DuplicateGroup {
                label: contacts[members[0]].display_name.clone(),
                confidence: conf.as_str().to_string(),
                contacts: members.iter().map(|&i| contacts[i].clone()).collect(),
            }
        })
        .collect();

    // 確信度の高い順 → 件数の多い順 → 見出し名順。
    groups.sort_by(|a, b| {
        conf_rank(&b.confidence)
            .cmp(&conf_rank(&a.confidence))
            .then_with(|| b.contacts.len().cmp(&a.contacts.len()))
            .then_with(|| a.label.cmp(&b.label))
    });
    groups
}

/// 2 レコードの一致確信度。強い独立キーが2つ揃えば High。
fn check(a: &Rec, b: &Rec) -> Option<Conf> {
    let shared_mobile = opt_eq(&a.mobile, &b.mobile);
    let shared_email = opt_eq(&a.email, &b.email);
    let name_ok = name_similar(a, b);

    // High は「氏名も一致」する場合に限る（家族が携帯/メールを共有する誤検出を避ける）。
    if shared_mobile && name_ok {
        return Some(Conf::High);
    }
    if shared_email && name_ok {
        return Some(Conf::High);
    }
    if shared_mobile && shared_email {
        // 強キー2つ独立一致だが氏名が違う: 同一人物（別表記）か、連絡先を共有する家族か
        // 判別できないため候補（要確認）に留める。
        return Some(Conf::Medium);
    }
    if !a.name_norm.is_empty() && a.name_norm == b.name_norm {
        let org_match = !a.org.is_empty() && a.org == b.org;
        let pref_match = !a.pref.is_empty() && a.pref == b.pref;
        if org_match || pref_match {
            return Some(Conf::Medium); // 同名＋（組織 or 県）
        }
        return Some(Conf::Low); // 同名のみ（同姓同名の別人があり得る＝要確認）
    }
    None
}

/// 氏名が十分近いか（正規化一致・トークン集合一致・かな一致・Jaro–Winkler）。
fn name_similar(a: &Rec, b: &Rec) -> bool {
    if a.name_norm.is_empty() || b.name_norm.is_empty() {
        return false;
    }
    if a.name_norm == b.name_norm {
        return true;
    }
    if !a.name_tokens.is_empty() && a.name_tokens == b.name_tokens {
        return true; // 語順違い（姓名の入れ替え・空白差）
    }
    if let (Some(ka), Some(kb)) = (&a.kana, &b.kana) {
        if ka == kb {
            return true;
        }
    }
    jaro_winkler(&a.name_norm, &b.name_norm) >= 0.92
}

fn opt_eq(a: &Option<String>, b: &Option<String>) -> bool {
    matches!((a, b), (Some(x), Some(y)) if x == y)
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Conf {
    Low,
    Medium,
    High,
}
impl Conf {
    fn as_str(self) -> &'static str {
        match self {
            Conf::High => "high",
            Conf::Medium => "medium",
            Conf::Low => "low",
        }
    }
}
fn conf_rank(s: &str) -> u8 {
    match s {
        "high" => 3,
        "medium" => 2,
        _ => 1,
    }
}

/// 正規化済みの比較用レコード。
struct Rec {
    name_norm: String,
    name_tokens: Vec<String>,
    kana: Option<String>,
    email: Option<String>,
    mobile: Option<String>,
    org: String,
    pref: String,
}

impl Rec {
    fn from_contact(c: &ContactSummary) -> Self {
        let name_norm = fold_remove_ws(&c.display_name);
        let name_tokens = tokens(&c.display_name);
        let kana = c
            .name_kana
            .as_deref()
            .map(fold_remove_ws)
            .filter(|s| !s.is_empty());
        let email = c
            .email
            .as_deref()
            .map(|e| fold(e).trim().to_string())
            .filter(|s| !s.is_empty());
        let mobile = c.phone.as_deref().and_then(mobile_number);
        let org = normalize_org(c.organization.as_deref().unwrap_or(""));
        let pref = prefecture(c.address.as_deref().unwrap_or(""));
        Rec {
            name_norm,
            name_tokens,
            kana,
            email,
            mobile,
            org,
            pref,
        }
    }
}

/// 全角 ASCII/数字/空白を半角へ畳み、小文字化する。
fn fold(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\u{3000}' => ' ',                                            // 全角スペース
            '\u{FF01}'..='\u{FF5E}' => (c as u32 - 0xFEE0) as u8 as char, // 全角 ASCII → 半角
            _ => c,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// fold して全空白を除去。
fn fold_remove_ws(s: &str) -> String {
    fold(s).split_whitespace().collect()
}

/// fold して空白で分割し、ソート済みユニークなトークン集合にする。
fn tokens(s: &str) -> Vec<String> {
    let mut v: Vec<String> = fold(s).split_whitespace().map(str::to_string).collect();
    v.sort();
    v.dedup();
    v
}

/// 数字だけ抜き出す（全角数字は fold 済み前提）。
fn digits(s: &str) -> String {
    fold(s).chars().filter(|c| c.is_ascii_digit()).collect()
}

/// 日本の携帯番号なら正規化して返す（070/080/090・11桁）。それ以外（固定電話等）は None。
fn mobile_number(raw: &str) -> Option<String> {
    let mut d = digits(raw);
    // 国番号 +81 / 81 を 0 始まりへ。
    if let Some(rest) = d.strip_prefix("81") {
        if rest.len() == 10 {
            d = format!("0{rest}");
        }
    }
    if d.len() == 11 && (d.starts_with("070") || d.starts_with("080") || d.starts_with("090")) {
        Some(d)
    } else {
        None
    }
}

/// 組織名の正規化: fold・空白除去し、代表的な法人格表記を取り除く。
fn normalize_org(s: &str) -> String {
    let mut t = fold_remove_ws(s);
    for token in [
        "株式会社",
        "有限会社",
        "合同会社",
        "合資会社",
        "合名会社",
        "一般社団法人",
        "一般財団法人",
        "(株)",
        "(有)",
        "(合)",
    ] {
        t = t.replace(token, "");
    }
    t
}

/// 住所文字列の先頭から都道府県名を取り出す（無ければ空）。
fn prefecture(addr: &str) -> String {
    let a = fold(addr);
    // 「県/都/府/道」の最初の出現までを都道府県とみなす（先頭付近のみ対象）。
    let mut buf = String::new();
    for (idx, ch) in a.char_indices() {
        if idx > 12 {
            break; // 都道府県は先頭にあるはず。長すぎるなら住所形式でない
        }
        buf.push(ch);
        if matches!(ch, '県' | '都' | '府' | '道') && buf.chars().count() >= 3 {
            // 「北海道」以外は「〇〇県/都/府」。3文字以上を条件に誤検出を減らす。
            return buf.trim().to_string();
        }
    }
    String::new()
}

/// Jaro–Winkler 類似度（0.0〜1.0）。人名の表記ゆれ吸収に使う標準指標。
fn jaro_winkler(a: &str, b: &str) -> f64 {
    let j = jaro(a, b);
    if j < 0.7 {
        return j;
    }
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let prefix = a
        .iter()
        .zip(b.iter())
        .take(4)
        .take_while(|(x, y)| x == y)
        .count() as f64;
    j + prefix * 0.1 * (1.0 - j)
}

fn jaro(a: &str, b: &str) -> f64 {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (la, lb) = (a.len(), b.len());
    if la == 0 && lb == 0 {
        return 1.0;
    }
    if la == 0 || lb == 0 {
        return 0.0;
    }
    let max_dist = (la.max(lb) / 2).saturating_sub(1);
    let mut a_match = vec![false; la];
    let mut b_match = vec![false; lb];
    let mut matches = 0usize;
    for i in 0..la {
        let lo = i.saturating_sub(max_dist);
        let hi = (i + max_dist + 1).min(lb);
        for j in lo..hi {
            if !b_match[j] && a[i] == b[j] {
                a_match[i] = true;
                b_match[j] = true;
                matches += 1;
                break;
            }
        }
    }
    if matches == 0 {
        return 0.0;
    }
    // 転置数。
    let mut t = 0usize;
    let mut k = 0usize;
    for i in 0..la {
        if a_match[i] {
            while !b_match[k] {
                k += 1;
            }
            if a[i] != b[k] {
                t += 1;
            }
            k += 1;
        }
    }
    let m = matches as f64;
    let t = (t / 2) as f64;
    (m / la as f64 + m / lb as f64 + (m - t) / m) / 3.0
}

/// 素朴な Union-Find（重み付き＋経路圧縮）。
struct UnionFind {
    parent: Vec<usize>,
    rank: Vec<u8>,
}
impl UnionFind {
    fn new(n: usize) -> Self {
        UnionFind {
            parent: (0..n).collect(),
            rank: vec![0; n],
        }
    }
    fn find(&mut self, x: usize) -> usize {
        let mut root = x;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        let mut cur = x;
        while self.parent[cur] != root {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        if self.rank[ra] < self.rank[rb] {
            self.parent[ra] = rb;
        } else if self.rank[ra] > self.rank[rb] {
            self.parent[rb] = ra;
        } else {
            self.parent[rb] = ra;
            self.rank[ra] += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(
        id: i32,
        name: &str,
        email: Option<&str>,
        phone: Option<&str>,
        org: Option<&str>,
    ) -> ContactSummary {
        ContactSummary {
            id,
            display_name: name.into(),
            family_name: None,
            given_name: None,
            phonetic_family: None,
            phonetic_given: None,
            name_kana: None,
            email: email.map(str::to_string),
            phone: phone.map(str::to_string),
            organization: org.map(str::to_string),
            org_title: None,
            org_department: None,
            address: None,
            birthday: None,
            note: None,
            is_favorite: false,
            is_business: false,
            allow_remote_images: false,
            emails: Vec::new(),
            phones: Vec::new(),
            addresses: Vec::new(),
            tags: Vec::new(),
        }
    }

    #[test]
    fn mobile_normalization() {
        assert_eq!(
            mobile_number("090-7929-9937").as_deref(),
            Some("09079299937")
        );
        assert_eq!(
            mobile_number("＋８１ 90 7929 9937").as_deref(),
            Some("09079299937")
        );
        assert_eq!(mobile_number("(03) 5287-3625"), None); // 固定電話
    }

    #[test]
    fn two_strong_keys_but_different_names_is_medium_candidate() {
        // 携帯＋メールが一致でも氏名が別表記なら、同一人物か家族共有か判別不能 → Medium。
        let list = vec![
            c(1, "末松信吾", Some("s@x.jp"), Some("090-1111-2222"), None),
            c(2, "S. Suematsu", Some("s@x.jp"), Some("09011112222"), None),
        ];
        let g = group(&list);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].confidence, "medium");
        assert_eq!(g[0].contacts.len(), 2);
    }

    #[test]
    fn mobile_plus_matching_name_is_high() {
        // 携帯＋氏名（語順違い）一致は High。
        let list = vec![
            c(1, "末松 信吾", None, Some("090-1111-2222"), None),
            c(2, "信吾 末松", None, Some("09011112222"), None),
        ];
        let g = group(&list);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].confidence, "high");
    }

    #[test]
    fn shared_company_email_different_names_not_linked() {
        // 代表メール共有の別人は連結しない（適合率）。
        let list = vec![
            c(1, "田中一郎", Some("info@acme.co.jp"), None, None),
            c(2, "鈴木花子", Some("info@acme.co.jp"), None, None),
        ];
        assert!(group(&list).is_empty());
    }

    #[test]
    fn name_order_and_spacing_absorbed() {
        // 「末松 信吾」と「信吾 末松」はトークン集合一致＋同メールで High。
        let list = vec![
            c(1, "末松 信吾", Some("a@b.jp"), None, None),
            c(2, "信吾 末松", Some("a@b.jp"), None, None),
        ];
        let g = group(&list);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].confidence, "high");
    }

    #[test]
    fn same_name_only_is_low() {
        let list = vec![
            c(1, "山田太郎", None, None, None),
            c(2, "山田太郎", None, None, None),
        ];
        let g = group(&list);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].confidence, "low");
    }

    #[test]
    fn same_name_plus_org_is_medium() {
        let list = vec![
            c(1, "山田太郎", None, None, Some("株式会社テスト")),
            c(2, "山田太郎", None, None, Some("(株)テスト")),
        ];
        let g = group(&list);
        assert_eq!(g.len(), 1);
        assert_eq!(g[0].confidence, "medium");
    }
}
