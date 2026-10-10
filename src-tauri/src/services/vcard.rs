//! vCard (3.0/4.0) の最小パーサ。外部依存なしで iCloud / Google のエクスポートを取り込む。
//!
//! 対応: 行折り返し（先頭スペース/タブ）・`itemN.` グループ（iCloud の X-ABLabel / X-ABADR を
//! 同じグループの値へ結び付ける）・`\n \, \; \\` エスケープ・複数値（type=pref を先頭に）。
//!
//! 読む項目（docs/CONTACT_MODEL.md §1 の iCloud 列）: FN・N（姓;名;ミドル;敬称;接尾辞）・
//! NICKNAME・X-MAIDENNAME・X-PHONETIC-*-NAME・ORG・TITLE・X-PHONETIC-ORG・X-ABShowAs・EMAIL・
//! TEL・ADR（私書箱つき）＋X-ABADR・URL・BDAY・X-ABDATE／ANNIVERSARY・X-ABRELATEDNAMES・
//! IMPP・X-SOCIALPROFILE・NOTE・CATEGORIES。Rondine が書き出した `itemN.ORG`（2 つ目以降の会社）
//! と `itemN.X-RONDINE-CUSTOM`（カスタム項目）、UUID の形の `UID`（[`ContactUid`]）も読む。PHOTO やその他の X- プロパティは無視する。
//!
//! 書き出しは [`write`]（取り込み→書き出し→取り込みで中身が戻る）。
//!
//! 取り込みの出どころ（PRODID）は見ない。ファイル取り込みはどのサービスにもつながらない
//! （Rondine の連絡先として入る）。

use crate::models::{
    ContactAddress, ContactCustomField, ContactDate, ContactFields, ContactHandle,
    ContactOrganization, ContactRelation, ContactUrl, ContactValue, HandleKind,
};
use crate::services::contact_labels::label_from_term;
use crate::services::contact_uid::ContactUid;
use std::collections::HashMap;

mod write;
pub use write::generate;

/// vCard 1 枚ぶんの連絡先（取り込みの結果・書き出しの入力）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VcardContact {
    /// UID（Rondine が振った UUID の形のときだけ。ほかのアプリの独自 ID は None）。
    pub uid: Option<ContactUid>,
    pub fields: ContactFields,
}

impl From<ContactFields> for VcardContact {
    fn from(fields: ContactFields) -> Self {
        Self { uid: None, fields }
    }
}

/// パース結果（総カード数と、連絡先として成立したもの）。
#[derive(Debug, Default)]
pub struct ParseResult {
    pub contacts: Vec<VcardContact>,
    /// BEGIN:VCARD の総数（名前もメールも電話も無く捨てたものを含む）。
    pub total_cards: usize,
}

/// vCard テキスト全体をパースする。
pub fn parse(text: &str) -> ParseResult {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text); // BOM 除去
    let mut result = ParseResult::default();
    let mut card: Option<CardAcc> = None;
    for line in &unfold(text) {
        let trimmed = line.trim_end();
        if trimmed.eq_ignore_ascii_case("BEGIN:VCARD") {
            card = Some(CardAcc::default());
            result.total_cards += 1;
        } else if trimmed.eq_ignore_ascii_case("END:VCARD") {
            if let Some(c) = card.take().and_then(CardAcc::finish) {
                result.contacts.push(c);
            }
        } else if let (Some(acc), Some(parsed)) = (card.as_mut(), split_line(line)) {
            acc.absorb(&parsed);
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

/// 分解済みの 1 行。
struct Line {
    /// `itemN.` グループ（小文字）。無ければ None。
    group: Option<String>,
    /// プロパティ名（大文字）。
    name: String,
    /// パラメータ（キーは大文字。`TEL;CELL:` のような裸の値はキーが空）。
    params: Vec<(String, String)>,
    /// 値（未アンエスケープ）。
    raw: String,
}

impl Line {
    /// アンエスケープ・trim した値。空なら None。
    fn value(&self) -> Option<String> {
        non_empty(&unescape(&self.raw))
    }

    /// `;` 区切りの構造化値の各要素（アンエスケープ・trim 済み）。
    fn parts(&self) -> Vec<String> {
        split_unescaped(&self.raw, ';')
            .iter()
            .map(|p| unescape(p).trim().to_string())
            .collect()
    }

    fn param(&self, key: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.trim_matches('"'))
    }

    /// type=pref（または pref 指定）を持つか。
    fn is_pref(&self) -> bool {
        self.params.iter().any(|(k, v)| {
            k == "PREF" || v.split(',').any(|t| t.trim().eq_ignore_ascii_case("pref"))
        })
    }

    /// TYPE パラメータから見出しを作る（HOME→自宅 等。INTERNET/PREF/VOICE は無視）。
    fn type_label(&self) -> Option<String> {
        self.params
            .iter()
            .filter(|(k, _)| k == "TYPE" || k.is_empty())
            .flat_map(|(_, v)| v.split(','))
            .find_map(label_from_term)
    }
}

/// `GROUP.NAME;PARAM=V;PARAM:value` を分解する。
fn split_line(line: &str) -> Option<Line> {
    let colon = line.find(':')?;
    let (head, value) = line.split_at(colon);
    let mut segs = split_unescaped(head, ';').into_iter();
    let first = segs.next()?;
    let (group, name) = match first.split_once('.') {
        Some((g, n)) => (Some(g.trim().to_ascii_lowercase()), n),
        None => (None, first.as_str()),
    };
    let params = segs
        .map(|p| match p.split_once('=') {
            Some((k, v)) => (k.trim().to_ascii_uppercase(), v.trim().to_string()),
            None => (String::new(), p.trim().to_string()),
        })
        .collect();
    Some(Line {
        group,
        name: name.trim().to_ascii_uppercase(),
        params,
        raw: value[1..].to_string(),
    })
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

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// グループに結び付けて溜める値（後で X-ABLabel を当てる）。
struct Grouped<T> {
    item: T,
    group: Option<String>,
    type_label: Option<String>,
    pref: bool,
}

/// IMPP / X-SOCIALPROFILE の中身（種類・サービス名・値）。
type HandleParts = (HandleKind, Option<String>, String);

/// カード組み立て中の中間状態。
#[derive(Default)]
struct CardAcc {
    uid: Option<ContactUid>,
    fn_: Option<String>,
    n: Vec<String>,
    nickname: Option<String>,
    maiden: Option<String>,
    /// よみ（姓・ミドル・名）。
    phonetic: [Option<String>; 3],
    org: Option<ContactOrganization>,
    /// 2 つ目以降の会社（`itemN.ORG` / `itemN.TITLE`。グループ → 会社、出てきた順）。
    extra_orgs: Vec<(String, ContactOrganization)>,
    show_as_company: bool,
    emails: Vec<Grouped<String>>,
    tels: Vec<Grouped<String>>,
    addresses: Vec<Grouped<ContactAddress>>,
    urls: Vec<Grouped<String>>,
    dates: Vec<Grouped<String>>,
    relations: Vec<Grouped<String>>,
    handles: Vec<Grouped<HandleParts>>,
    /// カスタム項目（`itemN.X-RONDINE-CUSTOM`。キーは同じグループの X-ABLabel）。
    custom: Vec<Grouped<String>>,
    birthday: Option<String>,
    note: Option<String>,
    categories: Vec<String>,
    /// グループ → X-ABLabel。
    group_labels: HashMap<String, String>,
    /// グループ → X-ABADR（国コード）。
    group_country: HashMap<String, String>,
}

impl CardAcc {
    fn absorb(&mut self, l: &Line) {
        match l.name.as_str() {
            "UID" => self.uid = l.value().as_deref().and_then(ContactUid::parse),
            "FN" => self.fn_ = l.value(),
            "N" => self.n = l.parts(),
            "NICKNAME" => self.nickname = l.value().and_then(|v| v.split(',').find_map(non_empty)),
            "X-MAIDENNAME" => self.maiden = l.value(),
            "X-PHONETIC-LAST-NAME" => self.phonetic[0] = l.value(),
            "X-PHONETIC-MIDDLE-NAME" => self.phonetic[1] = l.value(),
            "X-PHONETIC-FIRST-NAME" => self.phonetic[2] = l.value(),
            "ORG" | "TITLE" | "X-PHONETIC-ORG" => self.absorb_org(l),
            "X-ABSHOWAS" => {
                self.show_as_company = l.value().is_some_and(|v| v.eq_ignore_ascii_case("COMPANY"))
            }
            "EMAIL" => push_value(&mut self.emails, l, l.value()),
            "TEL" => push_value(&mut self.tels, l, l.value()),
            "URL" => push_value(&mut self.urls, l, l.value()),
            "ADR" => push_value(&mut self.addresses, l, address(l)),
            "BDAY" => self.birthday = vcard_date(l),
            "X-ABDATE" => push_value(&mut self.dates, l, vcard_date(l)),
            "ANNIVERSARY" => {
                push_value(&mut self.dates, l, vcard_date(l));
                if let Some(d) = self.dates.last_mut().filter(|d| d.type_label.is_none()) {
                    d.type_label = label_from_term("anniversary");
                }
            }
            "X-ABRELATEDNAMES" => push_value(&mut self.relations, l, l.value()),
            "IMPP" => push_value(&mut self.handles, l, impp(l)),
            "X-SOCIALPROFILE" => {
                if let Some(item) = social(l) {
                    // TYPE はサービス名（twitter など）なので、見出しにはしない。
                    self.handles.push(Grouped {
                        item,
                        group: l.group.clone(),
                        type_label: None,
                        pref: l.is_pref(),
                    });
                }
            }
            "X-RONDINE-CUSTOM" => push_value(&mut self.custom, l, l.value()),
            "X-ABLABEL" => {
                if let (Some(g), Some(v)) = (l.group.clone(), l.value()) {
                    self.group_labels.insert(g, v);
                }
            }
            "X-ABADR" => {
                if let (Some(g), Some(v)) = (l.group.clone(), l.value()) {
                    self.group_country.insert(g, v.to_ascii_uppercase());
                }
            }
            "NOTE" => self.note = l.value(),
            "CATEGORIES" => {
                for c in l
                    .value()
                    .unwrap_or_default()
                    .split(',')
                    .filter_map(non_empty)
                {
                    if !self.categories.contains(&c) {
                        self.categories.push(c);
                    }
                }
            }
            _ => {}
        }
    }

    /// ORG（会社名;部署）・TITLE（役職）・X-PHONETIC-ORG（会社名のよみ）。グループなしは最初の
    /// 値を採って主の会社に、`itemN.` 付きは同じグループごとに 2 つ目以降の会社にする。
    fn absorb_org(&mut self, l: &Line) {
        let org = match &l.group {
            Some(g) => {
                let i = match self.extra_orgs.iter().position(|(grp, _)| grp == g) {
                    Some(i) => i,
                    None => {
                        self.extra_orgs
                            .push((g.clone(), ContactOrganization::default()));
                        self.extra_orgs.len() - 1
                    }
                };
                &mut self.extra_orgs[i].1
            }
            None => self.org.get_or_insert_with(ContactOrganization::default),
        };
        match l.name.as_str() {
            "ORG" => {
                let parts = l.parts();
                if org.name.is_none() {
                    org.name = parts.first().and_then(|s| non_empty(s));
                }
                if org.department.is_none() {
                    org.department = parts.get(1).and_then(|s| non_empty(s));
                }
            }
            "TITLE" if org.title.is_none() => org.title = l.value(),
            "X-PHONETIC-ORG" => org.phonetic_name = l.value(),
            _ => {}
        }
    }

    /// グループの X-ABLabel を優先し、無ければ TYPE から作った見出し。
    fn label_of<T>(&self, g: &Grouped<T>) -> Option<String> {
        match g.group.as_ref().and_then(|grp| self.group_labels.get(grp)) {
            Some(l) => label_from_term(l),
            None => g.type_label.clone(),
        }
    }

    /// pref を先頭に並べ、見出しを当てた (見出し, 値) の列にする。
    fn ordered<'a, T>(&self, items: &'a [Grouped<T>]) -> Vec<(Option<String>, &'a T)> {
        let (pref, rest): (Vec<_>, Vec<_>) = items.iter().partition(|g| g.pref);
        pref.into_iter()
            .chain(rest)
            .map(|g| (self.label_of(g), &g.item))
            .collect()
    }

    fn addresses(&self) -> Vec<ContactAddress> {
        let (pref, rest): (Vec<_>, Vec<_>) = self.addresses.iter().partition(|g| g.pref);
        pref.into_iter()
            .chain(rest)
            .map(|g| {
                let mut a = g.item.clone();
                a.label = self.label_of(g);
                a.country_code = g
                    .group
                    .as_ref()
                    .and_then(|grp| self.group_country.get(grp))
                    .cloned();
                a
            })
            .collect()
    }

    fn finish(self) -> Option<VcardContact> {
        let uid = self.uid.clone();
        self.into_fields()
            .map(|fields| VcardContact { uid, fields })
    }

    fn into_fields(self) -> Option<ContactFields> {
        let emails = dedup_values(&self.ordered(&self.emails));
        let phones = dedup_values(&self.ordered(&self.tels));
        let n = |i: usize| self.n.get(i).and_then(|s| non_empty(s));
        let (family, given) = (n(0), n(1));
        let org = self
            .org
            .clone()
            .filter(|o| o != &ContactOrganization::default());
        let organizations: Vec<ContactOrganization> = org
            .clone()
            .into_iter()
            .chain(self.extra_orgs.iter().map(|(_, o)| o.clone()))
            .filter(|o| o != &ContactOrganization::default())
            .collect();
        let custom_fields: Vec<ContactCustomField> = self
            .custom
            .iter()
            .filter_map(|g| {
                let key = g
                    .group
                    .as_ref()
                    .and_then(|grp| self.group_labels.get(grp))?;
                Some(ContactCustomField {
                    key: key.clone(),
                    value: g.item.clone(),
                })
            })
            .collect();

        // 表示名: FN → N（姓+名）→ 会社 → メール → 電話。全部無ければ捨てる。
        let display_name = self
            .fn_
            .clone()
            .or_else(|| join_name(family.as_deref(), given.as_deref()))
            .or_else(|| org.as_ref().and_then(|o| o.name.clone()))
            .or_else(|| emails.first().map(|e| e.value.clone()))
            .or_else(|| phones.first().map(|p| p.value.clone()))?;

        Some(ContactFields {
            display_name,
            family_name: family,
            given_name: given,
            middle_name: n(2),
            name_prefix: n(3),
            name_suffix: n(4),
            phonetic_family: self.phonetic[0].clone(),
            phonetic_middle: self.phonetic[1].clone(),
            phonetic_given: self.phonetic[2].clone(),
            nickname: self.nickname.clone(),
            maiden_name: self.maiden.clone(),
            birthday: self.birthday.clone(),
            note: self.note.clone(),
            show_as_company: self.show_as_company,
            organizations,
            custom_fields,
            emails,
            phones,
            addresses: self.addresses(),
            urls: self
                .ordered(&self.urls)
                .into_iter()
                .map(|(label, value)| ContactUrl {
                    label,
                    value: value.clone(),
                })
                .collect(),
            dates: self
                .ordered(&self.dates)
                .into_iter()
                .map(|(label, date)| ContactDate {
                    label,
                    date: date.clone(),
                })
                .collect(),
            relations: self
                .ordered(&self.relations)
                .into_iter()
                .map(|(label, name)| ContactRelation {
                    label,
                    name: name.clone(),
                })
                .collect(),
            handles: self
                .ordered(&self.handles)
                .into_iter()
                .map(|(label, (kind, service, value))| ContactHandle {
                    kind: *kind,
                    service: service.clone(),
                    value: value.clone(),
                    label,
                })
                .collect(),
            tags: self.categories.clone(),
            ..Default::default()
        })
    }
}

/// 値があれば溜める。
fn push_value<T>(dst: &mut Vec<Grouped<T>>, l: &Line, item: Option<T>) {
    if let Some(item) = item {
        dst.push(Grouped {
            item,
            group: l.group.clone(),
            type_label: l.type_label(),
            pref: l.is_pref(),
        });
    }
}

/// 同じ値（大文字小文字違いを含む）を除いたメール・電話の列。
fn dedup_values(items: &[(Option<String>, &String)]) -> Vec<ContactValue> {
    let mut out: Vec<ContactValue> = Vec::new();
    for (label, value) in items {
        if !out.iter().any(|x| x.value.eq_ignore_ascii_case(value)) {
            out.push(ContactValue {
                label: label.clone(),
                value: (*value).clone(),
                is_shared: false,
            });
        }
    }
    out
}

/// ADR（私書箱;拡張;番地;市区町村;都道府県;郵便番号;国）。全要素が空なら None。
fn address(l: &Line) -> Option<ContactAddress> {
    let p = l.parts();
    let get = |i: usize| p.get(i).and_then(|s| non_empty(s));
    let a = ContactAddress {
        po_box: get(0),
        extended: get(1),
        street: get(2),
        city: get(3),
        region: get(4),
        postal: get(5),
        country: get(6),
        ..Default::default()
    };
    (a != ContactAddress::default()).then_some(a)
}

/// 日付（BDAY / X-ABDATE / ANNIVERSARY）を `YYYY-MM-DD` / 年なし `--MM-DD` にそろえる。
/// iCloud は年なしを `X-APPLE-OMIT-YEAR=1604` ＋ `1604-MM-DD` で表す。時刻は落とす。
fn vcard_date(l: &Line) -> Option<String> {
    let v = l.value()?;
    let d = v.split(['T', ' ']).next().unwrap_or("").trim();
    let compact: String = d.chars().filter(|c| *c != '-').collect();
    let all_digits = |s: &str| s.chars().all(|c| c.is_ascii_digit());
    let normalized = match d.strip_prefix("--") {
        // 4.0 の年なし（`--MMDD`）は `--MM-DD` にそろえる。
        Some(md) if md.len() == 4 && all_digits(md) => format!("--{}-{}", &md[..2], &md[2..]),
        Some(_) => d.to_string(),
        None if compact.len() == 8 && all_digits(&compact) => {
            format!("{}-{}-{}", &compact[..4], &compact[4..6], &compact[6..])
        }
        None => d.to_string(),
    };
    match l
        .param("X-APPLE-OMIT-YEAR")
        .and_then(|y| normalized.strip_prefix(&format!("{y}-")))
    {
        Some(md) => Some(format!("--{md}")),
        None => non_empty(&normalized),
    }
}

/// IMPP（`skype:user` など）。サービス名は X-SERVICE-TYPE、無ければ URI のスキーム。
fn impp(l: &Line) -> Option<HandleParts> {
    let v = l.value()?;
    let (scheme, user) = match v.split_once(':') {
        Some((s, u)) => (non_empty(s), u.trim_start_matches('/').to_string()),
        None => (None, v.clone()),
    };
    let service = l.param("X-SERVICE-TYPE").and_then(non_empty).or(scheme);
    non_empty(&user).map(|u| (HandleKind::Im, service, u))
}

/// X-SOCIALPROFILE（type=twitter;x-user=foo:URL）。値は x-user、無ければ URL。
fn social(l: &Line) -> Option<HandleParts> {
    let service = l.param("TYPE").and_then(non_empty);
    let value = l
        .param("X-USER")
        .and_then(non_empty)
        .or_else(|| l.value())?;
    Some((HandleKind::Social, service, value))
}

/// 姓と名を結合。両方 CJK なら詰めて（例: 石川かおり）、そうでなければ空白区切り。
fn join_name(last: Option<&str>, first: Option<&str>) -> Option<String> {
    match (last, first) {
        (Some(l), Some(f)) if is_cjk(l) && is_cjk(f) => Some(format!("{l}{f}")),
        (Some(l), Some(f)) => Some(format!("{l} {f}")),
        (Some(l), None) => Some(l.to_string()),
        (None, Some(f)) => Some(f.to_string()),
        (None, None) => None,
    }
}

/// 文字列が ASCII を含まない（＝概ね CJK/かな）かどうか。
fn is_cjk(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| !c.is_ascii())
}

#[cfg(test)]
mod tests;
