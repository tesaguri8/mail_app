//! vCard (3.0/4.0) の最小パーサ。外部依存なしで iCloud / Google のエクスポートを取り込む。
//!
//! 対応: 行折り返し（先頭スペース/タブ）・`itemN.` グループ接頭辞・`\n \, \; \\` エスケープ・
//! 複数 EMAIL/TEL（type=pref を優先）・N（姓;名;…）・X-PHONETIC-*（よみ）・ADR・BDAY・NOTE・UID。
//! PHOTO やその他 X- プロパティは無視する。

/// ラベル付きの値（メール・電話）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportedValue {
    pub label: Option<String>,
    pub value: String,
    pub is_primary: bool,
}

/// ラベル付きの構造化住所。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportedAddress {
    pub label: Option<String>,
    pub postal: Option<String>,
    pub region: Option<String>,
    pub city: Option<String>,
    pub street: Option<String>,
    pub extended: Option<String>,
    pub country: Option<String>,
    pub is_primary: bool,
}

/// 取り込んだ 1 件の連絡先（DB 投入前の中間表現）。
/// flat な email/phone/address は主(primary)値（一覧・重複判定・後方互換用）、
/// all_* が全件のラベル付き値（子テーブルへ保存）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportedContact {
    pub display_name: String,
    /// 姓（構造化名）。
    pub family_name: Option<String>,
    /// 名。
    pub given_name: Option<String>,
    /// よみ（姓）。
    pub phonetic_family: Option<String>,
    /// よみ（名）。
    pub phonetic_given: Option<String>,
    pub name_kana: Option<String>,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub organization: Option<String>,
    /// 役職。
    pub org_title: Option<String>,
    /// 部署。
    pub org_department: Option<String>,
    pub address: Option<String>,
    pub birthday: Option<String>,
    pub note: Option<String>,
    /// 全メール（ラベル付き）。
    pub all_emails: Vec<ImportedValue>,
    /// 全電話（ラベル付き）。
    pub all_phones: Vec<ImportedValue>,
    /// 全住所（ラベル付き・構造化）。
    pub all_addresses: Vec<ImportedAddress>,
    /// タグ（グループ/ラベル。vCard CATEGORIES / Google Labels）。
    pub labels: Vec<String>,
    /// 'icloud' | 'google' | 'local'（PRODID から推定）。
    pub source: String,
    /// vCard UID（あれば。後日の同期の突き合わせキー）。
    pub external_id: Option<String>,
}

/// パース結果（総カード数と、連絡先として成立した件数）。
#[derive(Debug, Default)]
pub struct ParseResult {
    pub contacts: Vec<ImportedContact>,
    /// BEGIN:VCARD の総数（display も email も phone も無く捨てたものを含む）。
    pub total_cards: usize,
}

/// vCard テキスト全体をパースする。
pub fn parse(text: &str) -> ParseResult {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text); // BOM 除去
    let lines = unfold(text);

    let mut result = ParseResult::default();
    let mut card: Option<CardAcc> = None;
    for line in &lines {
        let upper = line.trim_end();
        if upper.eq_ignore_ascii_case("BEGIN:VCARD") {
            card = Some(CardAcc::default());
            result.total_cards += 1;
            continue;
        }
        if upper.eq_ignore_ascii_case("END:VCARD") {
            if let Some(acc) = card.take() {
                if let Some(c) = acc.finish() {
                    result.contacts.push(c);
                }
            }
            continue;
        }
        if let Some(acc) = card.as_mut() {
            if let Some((name, params, value)) = split_line(line) {
                acc.absorb(&name, &params, &value);
            }
        }
    }
    result
}

/// 折り返し行（先頭が空白/タブ）を直前の論理行に連結する。改行は CRLF/LF 両対応。
fn unfold(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if let Some(rest) = line.strip_prefix(' ').or_else(|| line.strip_prefix('\t')) {
            if let Some(last) = out.last_mut() {
                last.push_str(rest);
                continue;
            }
        }
        out.push(line.to_string());
    }
    out
}

/// 分解済みの行: (プロパティ名, パラメータ群, 値)。
type ParsedLine = (String, Vec<(String, String)>, String);

/// `NAME;PARAM=V;PARAM:value` を (name, params, value) に分解する。
/// name の `itemN.` グループ接頭辞は落とし、大文字化して返す。value は最初の `:` 以降。
fn split_line(line: &str) -> Option<ParsedLine> {
    let colon = line.find(':')?;
    let (head, value) = line.split_at(colon);
    let value = &value[1..];

    let mut segs = split_unescaped(head, ';');
    if segs.is_empty() {
        return None;
    }
    let mut name = segs.remove(0);
    if let Some(dot) = name.find('.') {
        name = name[dot + 1..].to_string(); // グループ接頭辞を除去
    }
    let name = name.trim().to_ascii_uppercase();

    let params = segs
        .into_iter()
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (k.trim().to_ascii_uppercase(), v.trim().to_string()),
            None => (String::new(), p.trim().to_string()),
        })
        .collect();

    Some((name, params, value.to_string()))
}

/// バックスラッシュを尊重して `delim` で分割（各要素は未アンエスケープのまま返す）。
fn split_unescaped(s: &str, delim: char) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut escaped = false;
    for ch in s.chars() {
        if escaped {
            cur.push('\\');
            cur.push(ch);
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == delim {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    if escaped {
        cur.push('\\');
    }
    out.push(cur);
    out
}

/// vCard のエスケープ（`\n \, \; \\`）を解除する。
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('n') | Some('N') => out.push('\n'),
                Some(other) => out.push(other), // \, \; \\ など
                None => out.push('\\'),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// type=pref（または pref 指定）を持つか。
fn is_pref(params: &[(String, String)]) -> bool {
    params
        .iter()
        .any(|(k, v)| v.eq_ignore_ascii_case("pref") || k.eq_ignore_ascii_case("pref"))
}

/// カード組み立て中の中間状態。
#[derive(Default)]
struct CardAcc {
    fn_: String,
    n: Option<Vec<String>>,
    kana_last: Option<String>,
    kana_first: Option<String>,
    /// (value, label, is_pref)
    emails: Vec<(String, Option<String>, bool)>,
    tels: Vec<(String, Option<String>, bool)>,
    addresses: Vec<(ImportedAddress, bool)>,
    org: Option<String>,
    org_department: Option<String>,
    org_title: Option<String>,
    birthday: Option<String>,
    note: Option<String>,
    uid: Option<String>,
    prodid: Option<String>,
    labels: Vec<String>,
}

impl CardAcc {
    fn absorb(&mut self, name: &str, params: &[(String, String)], raw_value: &str) {
        let value = unescape(raw_value);
        match name {
            "FN" => self.fn_ = value.trim().to_string(),
            "N" => {
                self.n = Some(
                    split_unescaped(raw_value, ';')
                        .iter()
                        .map(|p| unescape(p).trim().to_string())
                        .collect(),
                )
            }
            "X-PHONETIC-LAST-NAME" => self.kana_last = non_empty(value),
            "X-PHONETIC-FIRST-NAME" => self.kana_first = non_empty(value),
            "EMAIL" => {
                if let Some(v) = non_empty(value) {
                    self.emails.push((v, type_label(params), is_pref(params)));
                }
            }
            "TEL" => {
                if let Some(v) = non_empty(value) {
                    self.tels.push((v, type_label(params), is_pref(params)));
                }
            }
            "TITLE" if self.org_title.is_none() => self.org_title = non_empty(value),
            "ORG" => {
                // 1つ目=会社名、2つ目=部署。
                let parts: Vec<String> = split_unescaped(raw_value, ';')
                    .into_iter()
                    .map(|p| unescape(&p).trim().to_string())
                    .collect();
                if self.org.is_none() {
                    self.org = parts.iter().find(|s| !s.is_empty()).cloned();
                }
                if self.org_department.is_none() {
                    self.org_department = parts.get(1).filter(|s| !s.is_empty()).cloned();
                }
            }
            "ADR" => {
                // 構造化: PO;拡張;番地;市区町村;都道府県;郵便番号;国。
                let p = split_unescaped(raw_value, ';')
                    .iter()
                    .map(|s| unescape(s).trim().to_string())
                    .collect::<Vec<_>>();
                let get = |i: usize| p.get(i).cloned().filter(|s| !s.is_empty());
                let addr = ImportedAddress {
                    label: type_label(params),
                    extended: get(1),
                    street: get(2),
                    city: get(3),
                    region: get(4),
                    postal: get(5),
                    country: get(6),
                    is_primary: false,
                };
                // いずれかの要素が非空なら採用。
                if addr.street.is_some()
                    || addr.city.is_some()
                    || addr.region.is_some()
                    || addr.postal.is_some()
                    || addr.extended.is_some()
                    || addr.country.is_some()
                {
                    self.addresses.push((addr, is_pref(params)));
                }
            }
            "BDAY" => {
                // 日付部分のみ（時刻や VALUE=date は落とす）。
                let d = value
                    .split(['T', ' '])
                    .next()
                    .unwrap_or("")
                    .trim()
                    .to_string();
                self.birthday = non_empty(d);
            }
            "NOTE" => self.note = non_empty(value),
            "CATEGORIES" => {
                for c in value.split(',') {
                    if let Some(c) = non_empty(c.to_string()) {
                        if !self.labels.contains(&c) {
                            self.labels.push(c);
                        }
                    }
                }
            }
            "UID" => self.uid = non_empty(value),
            "PRODID" => self.prodid = non_empty(value),
            _ => {}
        }
    }

    fn finish(self) -> Option<ImportedContact> {
        // pref を先頭にして全メール/全電話をラベル付きで整える。
        let all_emails = order_values(&self.emails);
        let all_phones = order_values(&self.tels);
        let email = all_emails.first().map(|v| v.value.clone());
        let phone = all_phones.first().map(|v| v.value.clone());

        // 住所も pref 先頭で並べ、先頭を主住所（flat）に整形。
        let mut all_addresses: Vec<ImportedAddress> = Vec::new();
        for (a, pref) in self.addresses {
            let mut a = a;
            a.is_primary = false;
            if pref {
                all_addresses.insert(0, a);
            } else {
                all_addresses.push(a);
            }
        }
        if let Some(a) = all_addresses.first_mut() {
            a.is_primary = true;
        }
        let address = all_addresses.first().map(format_address);

        // よみは並び替え・読み上げのため常に空白区切りで連結。
        let name_kana = match (self.kana_last.as_deref(), self.kana_first.as_deref()) {
            (Some(l), Some(f)) => Some(format!("{l} {f}")),
            (Some(l), None) => Some(l.to_string()),
            (None, Some(f)) => Some(f.to_string()),
            (None, None) => None,
        };

        // 表示名: FN → N（姓+名）→ 組織 → メール → 電話。全部無ければ捨てる。
        let display_name = non_empty(self.fn_.clone())
            .or_else(|| name_from_components(self.n.as_deref()))
            .or_else(|| self.org.clone())
            .or_else(|| email.clone())
            .or_else(|| phone.clone())?;

        let source = detect_source(self.prodid.as_deref());

        // 構造化名: N の 1つ目=姓、2つ目=名。よみは X-PHONETIC-*。
        let (family_name, given_name) = match &self.n {
            Some(n) => (
                non_empty(n.first().cloned().unwrap_or_default()),
                non_empty(n.get(1).cloned().unwrap_or_default()),
            ),
            None => (None, None),
        };

        Some(ImportedContact {
            display_name,
            family_name,
            given_name,
            phonetic_family: self.kana_last,
            phonetic_given: self.kana_first,
            name_kana,
            email,
            phone,
            organization: self.org,
            org_title: self.org_title,
            org_department: self.org_department,
            address,
            birthday: self.birthday,
            note: self.note,
            all_emails,
            all_phones,
            all_addresses,
            labels: self.labels,
            source,
            external_id: self.uid,
        })
    }
}

/// (value, label, is_pref) の列を pref 先頭・重複排除の ImportedValue 列にする。
fn order_values(items: &[(String, Option<String>, bool)]) -> Vec<ImportedValue> {
    let mut out: Vec<ImportedValue> = Vec::new();
    for (v, label, pref) in items {
        if out.iter().any(|x| x.value.eq_ignore_ascii_case(v)) {
            continue;
        }
        let iv = ImportedValue {
            label: label.clone(),
            value: v.clone(),
            is_primary: false,
        };
        if *pref {
            out.insert(0, iv);
        } else {
            out.push(iv);
        }
    }
    if let Some(first) = out.first_mut() {
        first.is_primary = true;
    }
    out
}

/// 構造化住所を1行の文字列へ（flat 保存・一覧用）。
pub fn format_address(a: &ImportedAddress) -> String {
    [
        a.postal.as_deref(),
        a.region.as_deref(),
        a.city.as_deref(),
        a.street.as_deref(),
        a.extended.as_deref(),
        a.country.as_deref(),
    ]
    .into_iter()
    .flatten()
    .filter(|s| !s.is_empty())
    .collect::<Vec<_>>()
    .join(" ")
}

/// TYPE パラメータから見出しラベルを作る（HOME→自宅 等。INTERNET/PREF/VOICE は無視）。
fn type_label(params: &[(String, String)]) -> Option<String> {
    for (k, v) in params {
        if k != "TYPE" {
            continue;
        }
        match v.to_ascii_uppercase().as_str() {
            "INTERNET" | "PREF" | "VOICE" => continue,
            "HOME" => return Some("自宅".to_string()),
            "WORK" => return Some("職場".to_string()),
            "CELL" | "IPHONE" | "MOBILE" => return Some("携帯".to_string()),
            "FAX" => return Some("FAX".to_string()),
            "MAIN" => return Some("代表".to_string()),
            other => return Some(other.to_string()),
        }
    }
    None
}

fn non_empty(s: String) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// N の構成要素（姓;名;ミドル;敬称;接尾辞）から表示名を作る。
fn name_from_components(n: Option<&[String]>) -> Option<String> {
    let n = n?;
    let last = n.first().map(String::as_str).unwrap_or("");
    let first = n.get(1).map(String::as_str).unwrap_or("");
    join_name(
        non_empty(last.to_string()).as_deref(),
        non_empty(first.to_string()).as_deref(),
    )
}

/// 姓と名を結合。両方 CJK なら詰めて（例: 石川かおり）、そうでなければ空白区切り。
fn join_name(last: Option<&str>, first: Option<&str>) -> Option<String> {
    match (last, first) {
        (Some(l), Some(f)) => {
            if is_cjk(l) && is_cjk(f) {
                Some(format!("{l}{f}"))
            } else {
                Some(format!("{l} {f}"))
            }
        }
        (Some(l), None) => Some(l.to_string()),
        (None, Some(f)) => Some(f.to_string()),
        (None, None) => None,
    }
}

/// 文字列が ASCII を含まない（＝概ね CJK/かな）かどうか。
fn is_cjk(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| !c.is_ascii())
}

/// PRODID からエクスポート元を推定する。
fn detect_source(prodid: Option<&str>) -> String {
    match prodid {
        Some(p) if p.contains("Apple") => "icloud".to_string(),
        Some(p) if p.to_ascii_lowercase().contains("google") => "google".to_string(),
        _ => "local".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_icloud_company_card() {
        let vcf = "BEGIN:VCARD\nVERSION:3.0\nFN:\nN:;;;;\nPRODID:-//Apple Inc.//Mac OS X 10.12.6//EN\nORG:アークデータ研究所;\nNOTE:ASCAL\nTEL:05037543196\nX-ABShowAs:COMPANY\nEND:VCARD\n";
        let r = parse(vcf);
        assert_eq!(r.total_cards, 1);
        let c = &r.contacts[0];
        assert_eq!(c.display_name, "アークデータ研究所"); // FN/N 空 → ORG
        assert_eq!(c.organization.as_deref(), Some("アークデータ研究所"));
        assert_eq!(c.phone.as_deref(), Some("05037543196"));
        assert_eq!(c.source, "icloud");
    }

    #[test]
    fn parses_name_kana_emails_pref_and_address() {
        let vcf = "BEGIN:VCARD\nVERSION:3.0\nN:愛川翼;;;;\nFN:愛川翼\nX-PHONETIC-LAST-NAME:アイカワ\nORG:有限会社愛建工業;\nTITLE:専務取締役\nEMAIL;type=INTERNET;type=pref:rabbit@key.ocn.ne.jp\nEMAIL:second@example.com\nTEL;type=pref:0997-52-4187\nTEL:090-7929-9937\nADR;type=pref:;;;;鹿児島県奄美市名瀬佐大熊町17-10AKビル2F;8940005;\nUID:ABC-123\nEND:VCARD\n";
        let c = &parse(vcf).contacts[0];
        assert_eq!(c.display_name, "愛川翼");
        assert_eq!(c.family_name.as_deref(), Some("愛川翼")); // N の1つ目
        assert_eq!(c.phonetic_family.as_deref(), Some("アイカワ"));
        assert_eq!(c.name_kana.as_deref(), Some("アイカワ"));
        assert_eq!(c.email.as_deref(), Some("rabbit@key.ocn.ne.jp"));
        // メールは全件保持（主＋追加）。
        assert_eq!(c.all_emails.len(), 2);
        assert_eq!(c.all_emails[0].value, "rabbit@key.ocn.ne.jp");
        assert!(c.all_emails[0].is_primary);
        assert_eq!(c.all_emails[1].value, "second@example.com");
        // 電話も全件（pref を主に）。
        assert_eq!(c.phone.as_deref(), Some("0997-52-4187"));
        assert_eq!(c.all_phones.len(), 2);
        // 組織の役職・部署。
        assert_eq!(c.org_title.as_deref(), Some("専務取締役"));
        // 住所は構造化（都道府県・郵便番号）。
        assert_eq!(c.all_addresses.len(), 1);
        assert_eq!(
            c.all_addresses[0].region.as_deref(),
            Some("鹿児島県奄美市名瀬佐大熊町17-10AKビル2F")
        );
        assert_eq!(c.all_addresses[0].postal.as_deref(), Some("8940005"));
        assert_eq!(c.external_id.as_deref(), Some("ABC-123"));
    }

    #[test]
    fn builds_display_from_n_and_joins_kana() {
        let vcf = "BEGIN:VCARD\nVERSION:3.0\nFN:\nN:石川;かおり;;;\nX-PHONETIC-LAST-NAME:イシカワ\nX-PHONETIC-FIRST-NAME:カオリ\nEMAIL:a@b.jp\nEMAIL:c@d.jp\nEND:VCARD\n";
        let c = &parse(vcf).contacts[0];
        assert_eq!(c.display_name, "石川かおり"); // CJK は詰める
        assert_eq!(c.name_kana.as_deref(), Some("イシカワ カオリ"));
        assert_eq!(c.email.as_deref(), Some("a@b.jp"));
    }

    #[test]
    fn unfolds_and_unescapes_note() {
        // \n はエスケープ改行、行頭スペースは折り返し（語中でも詰めて連結する）。
        let vcf = "BEGIN:VCARD\nVERSION:3.0\nFN:x\nNOTE:first line\\nlong word continu\n es here\nEND:VCARD\n";
        let c = &parse(vcf).contacts[0];
        let note = c.note.as_deref().unwrap();
        assert_eq!(note, "first line\nlong word continues here");
    }

    #[test]
    fn bday_strips_time_and_western_name_spaced() {
        let vcf =
            "BEGIN:VCARD\nVERSION:3.0\nN:Smith;John;;;\nBDAY;VALUE=date:1987-10-06\nEND:VCARD\n";
        let c = &parse(vcf).contacts[0];
        assert_eq!(c.display_name, "Smith John");
        assert_eq!(c.birthday.as_deref(), Some("1987-10-06"));
    }

    #[test]
    fn card_without_any_identity_is_skipped() {
        let vcf = "BEGIN:VCARD\nVERSION:3.0\nFN:\nN:;;;;\nNOTE:x\nEND:VCARD\n";
        let r = parse(vcf);
        assert_eq!(r.total_cards, 1);
        assert_eq!(r.contacts.len(), 0);
    }
}
