//! vCard の書き出し（[`super::parse`] と対）。Google 連絡先・iPhone・Outlook に読ませる。
//!
//! - 既定は 3.0（読める相手が多い）。4.0 も選べる。文字は UTF-8・改行は CRLF・75 オクテットで折り返す
//! - 見出し: 自宅/職場/携帯/FAX/ポケベルは TYPE、それ以外（代表・記念日・配偶者・カスタム名）は
//!   iCloud と同じく `itemN.` グループ＋`X-ABLabel`（既知の語は `_$!<Main>!$_` 形式）
//! - 2 つ目以降の会社は `itemN.ORG`/`itemN.TITLE`、住所の国コードは `itemN.X-ABADR`、
//!   カスタム項目は `itemN.X-RONDINE-CUSTOM`（キーは `X-ABLabel`）
//! - 年なしの日付は 3.0 では iCloud の `X-APPLE-OMIT-YEAR=1604`、4.0 の誕生日は `--MMDD`
//! - Rondine 固有の印（お気に入り・取引先・外部画像・共有の代表値）と写真は書き出さない

use crate::models::{
    ContactAddress, ContactFields, ContactHandle, ContactOrganization, HandleKind, VcardVersion,
};
use crate::services::contact_labels::google_type_for;

/// 1 行の最大オクテット数（CRLF を除く。RFC 6350 §3.2 / RFC 2426 §2.6）。
const FOLD_AT: usize = 75;

/// iCloud が年なしの日付に使う年（`X-APPLE-OMIT-YEAR`）。
const OMIT_YEAR: &str = "1604";

/// 連絡先の並びを 1 つの vCard テキストにする。`prodid` は書き出したアプリ（PRODID）。
pub fn generate(contacts: &[ContactFields], version: VcardVersion, prodid: &str) -> String {
    contacts
        .iter()
        .map(|c| Card::new(version).write(c, prodid))
        .collect()
}

/// 1 枚のカードを組み立てる。
struct Card {
    version: VcardVersion,
    out: String,
    /// 次の `itemN.` の番号。
    next_item: usize,
}

/// 見出しをどう書くか。
enum LabelForm {
    /// 書かない（無ラベル）。
    None,
    /// TYPE パラメータ（自宅→HOME など）。
    Type(&'static str),
    /// `itemN.X-ABLabel`（既知の語は Apple 形式、カスタム名はそのまま）。
    Grouped(String),
}

impl Card {
    fn new(version: VcardVersion) -> Self {
        Self {
            version,
            out: String::new(),
            next_item: 1,
        }
    }

    fn write(mut self, c: &ContactFields, prodid: &str) -> String {
        self.line("BEGIN:VCARD");
        self.line(match self.version {
            VcardVersion::V3 => "VERSION:3.0",
            VcardVersion::V4 => "VERSION:4.0",
        });
        self.line(&format!("PRODID:{prodid}"));
        self.line(&format!("FN:{}", text(&c.display_name)));
        self.line(&format!(
            "N:{}",
            structured(&[
                &c.family_name,
                &c.given_name,
                &c.middle_name,
                &c.name_prefix,
                &c.name_suffix,
            ])
        ));
        self.opt("NICKNAME", &c.nickname);
        self.opt("X-MAIDENNAME", &c.maiden_name);
        self.opt("X-PHONETIC-LAST-NAME", &c.phonetic_family);
        self.opt("X-PHONETIC-MIDDLE-NAME", &c.phonetic_middle);
        self.opt("X-PHONETIC-FIRST-NAME", &c.phonetic_given);
        self.organizations(&c.organizations);
        if c.show_as_company {
            self.line("X-ABShowAs:COMPANY");
        }
        for (i, e) in c.emails.iter().enumerate() {
            let internet = matches!(self.version, VcardVersion::V3).then_some("INTERNET");
            self.labeled(
                "EMAIL",
                e.label.as_deref(),
                internet,
                i == 0,
                &text(&e.value),
            );
        }
        for (i, p) in c.phones.iter().enumerate() {
            self.labeled("TEL", p.label.as_deref(), None, i == 0, &text(&p.value));
        }
        for (i, a) in c.addresses.iter().enumerate() {
            self.address(a, i == 0);
        }
        for u in &c.urls {
            self.labeled("URL", u.label.as_deref(), None, false, &text(&u.value));
        }
        if let Some(b) = c.birthday.as_deref().filter(|b| !b.trim().is_empty()) {
            self.birthday(b.trim());
        }
        for d in &c.dates {
            self.date("X-ABDATE", d.label.as_deref(), d.date.trim());
        }
        for r in &c.relations {
            self.labeled(
                "X-ABRELATEDNAMES",
                r.label.as_deref(),
                None,
                false,
                &text(&r.name),
            );
        }
        for h in &c.handles {
            self.handle(h);
        }
        for f in &c.custom_fields {
            let item = self.item();
            self.line(&format!("{item}.X-RONDINE-CUSTOM:{}", text(&f.value)));
            self.line(&format!("{item}.X-ABLabel:{}", text(&f.key)));
        }
        self.opt("NOTE", &c.note);
        if !c.tags.is_empty() {
            let tags: Vec<String> = c.tags.iter().map(|t| text(t)).collect();
            self.line(&format!("CATEGORIES:{}", tags.join(",")));
        }
        self.line("END:VCARD");
        self.out
    }

    /// 1 行を折り返して足す。
    fn line(&mut self, s: &str) {
        fold_into(&mut self.out, s);
    }

    /// 値があれば `NAME:値`。
    fn opt(&mut self, name: &str, v: &Option<String>) {
        if let Some(v) = v.as_deref().filter(|v| !v.trim().is_empty()) {
            self.line(&format!("{name}:{}", text(v)));
        }
    }

    /// 次の `itemN`。
    fn item(&mut self) -> String {
        let n = self.next_item;
        self.next_item += 1;
        format!("item{n}")
    }

    /// 先頭の会社は素の ORG/TITLE、2 つ目以降は `itemN.` でまとめる。
    fn organizations(&mut self, orgs: &[ContactOrganization]) {
        for (i, o) in orgs.iter().enumerate() {
            let prefix = if i == 0 {
                String::new()
            } else {
                format!("{}.", self.item())
            };
            if o.name.is_some() || o.department.is_some() {
                self.line(&format!(
                    "{prefix}ORG:{}",
                    structured(&[&o.name, &o.department])
                ));
            }
            if let Some(t) = o.title.as_deref().filter(|t| !t.trim().is_empty()) {
                self.line(&format!("{prefix}TITLE:{}", text(t)));
            }
            if let Some(p) = o.phonetic_name.as_deref().filter(|p| !p.trim().is_empty()) {
                self.line(&format!("{prefix}X-PHONETIC-ORG:{}", text(p)));
            }
        }
    }

    /// 見出し付きの値。`extra_type` は常に付ける TYPE（メールの INTERNET）、`pref` は主値の印。
    fn labeled(
        &mut self,
        name: &str,
        label: Option<&str>,
        extra_type: Option<&str>,
        pref: bool,
        value: &str,
    ) {
        let form = label_form(label);
        let mut types: Vec<&str> = extra_type.into_iter().collect();
        if let LabelForm::Type(t) = form {
            types.push(t);
        }
        let params = self.params(&types, pref);
        match form {
            LabelForm::Grouped(l) => {
                let item = self.item();
                self.line(&format!("{item}.{name}{params}:{value}"));
                self.line(&format!("{item}.X-ABLabel:{}", text(&l)));
            }
            _ => self.line(&format!("{name}{params}:{value}")),
        }
    }

    /// TYPE と主値の印をパラメータにする（3.0 は TYPE=...,PREF、4.0 は小文字の TYPE と PREF=1）。
    fn params(&self, types: &[&str], pref: bool) -> String {
        let mut out = String::new();
        let mut types: Vec<String> = types.iter().map(|t| t.to_string()).collect();
        match self.version {
            VcardVersion::V3 => {
                if pref {
                    types.push("PREF".into());
                }
            }
            VcardVersion::V4 => {
                types.iter_mut().for_each(|t| *t = t.to_ascii_lowercase());
                if pref {
                    out.push_str(";PREF=1");
                }
            }
        }
        if !types.is_empty() {
            out.insert_str(0, &format!(";TYPE={}", types.join(",")));
        }
        out
    }

    /// ADR（私書箱;拡張;番地;市区町村;都道府県;郵便番号;国）。国コードがあれば `X-ABADR` を添える。
    fn address(&mut self, a: &ContactAddress, pref: bool) {
        let value = structured(&[
            &a.po_box,
            &a.extended,
            &a.street,
            &a.city,
            &a.region,
            &a.postal,
            &a.country,
        ]);
        let form = label_form(a.label.as_deref());
        let types: Vec<&str> = match form {
            LabelForm::Type(t) => vec![t],
            _ => Vec::new(),
        };
        let params = self.params(&types, pref);
        let code = a.country_code.as_deref().filter(|c| !c.trim().is_empty());
        if matches!(form, LabelForm::Grouped(_)) || code.is_some() {
            let item = self.item();
            self.line(&format!("{item}.ADR{params}:{value}"));
            if let LabelForm::Grouped(l) = form {
                self.line(&format!("{item}.X-ABLabel:{}", text(&l)));
            }
            if let Some(code) = code {
                self.line(&format!(
                    "{item}.X-ABADR:{}",
                    code.trim().to_ascii_lowercase()
                ));
            }
        } else {
            self.line(&format!("ADR{params}:{value}"));
        }
    }

    /// 誕生日。年なし（`--MM-DD`）は 3.0 なら iCloud 形式、4.0 なら `--MMDD`。
    fn birthday(&mut self, date: &str) {
        match (date.strip_prefix("--"), self.version) {
            (Some(md), VcardVersion::V4) => self.line(&format!("BDAY:--{}", md.replace('-', ""))),
            (Some(md), VcardVersion::V3) => self.line(&format!(
                "BDAY;X-APPLE-OMIT-YEAR={OMIT_YEAR}:{OMIT_YEAR}-{md}"
            )),
            (None, _) => self.line(&format!("BDAY:{date}")),
        }
    }

    /// 記念日などの日付（`X-ABDATE`）。年なしは iCloud 形式。
    fn date(&mut self, name: &str, label: Option<&str>, date: &str) {
        if date.is_empty() {
            return;
        }
        let (params, value) = match date.strip_prefix("--") {
            Some(md) => (
                format!(";X-APPLE-OMIT-YEAR={OMIT_YEAR}"),
                format!("{OMIT_YEAR}-{md}"),
            ),
            None => (String::new(), date.to_string()),
        };
        match label_form(label) {
            LabelForm::None => self.line(&format!("{name}{params}:{value}")),
            LabelForm::Type(t) => self.line(&format!("{name};TYPE={t}{params}:{value}")),
            LabelForm::Grouped(l) => {
                let item = self.item();
                self.line(&format!("{item}.{name}{params}:{value}"));
                self.line(&format!("{item}.X-ABLabel:{}", text(&l)));
            }
        }
    }

    /// チャット（IMPP）と SNS（X-SOCIALPROFILE）。見出しはいつも `X-ABLabel`
    /// （TYPE はサービス名に使うため）。
    fn handle(&mut self, h: &ContactHandle) {
        let service = h
            .service
            .as_deref()
            .map(param_value)
            .filter(|s| !s.is_empty());
        let line = match h.kind {
            HandleKind::Im => match &service {
                Some(s) => format!("IMPP;X-SERVICE-TYPE={s}:x-apple:{}", text(&h.value)),
                None => format!("IMPP:{}", text(&h.value)),
            },
            HandleKind::Social => match &service {
                Some(s) => format!("X-SOCIALPROFILE;TYPE={s}:{}", text(&h.value)),
                None => format!("X-SOCIALPROFILE:{}", text(&h.value)),
            },
        };
        match h.label.as_deref().map(str::trim).filter(|l| !l.is_empty()) {
            Some(l) => {
                let item = self.item();
                self.line(&format!("{item}.{line}"));
                self.line(&format!("{item}.X-ABLabel:{}", text(&apple_label(l))));
            }
            None => self.line(&line),
        }
    }
}

/// 見出し → 書き方。TYPE で表せる語はそれで、ほかは `X-ABLabel`。
fn label_form(label: Option<&str>) -> LabelForm {
    let Some(l) = label.map(str::trim).filter(|l| !l.is_empty()) else {
        return LabelForm::None;
    };
    match l {
        "自宅" => LabelForm::Type("HOME"),
        "職場" => LabelForm::Type("WORK"),
        "携帯" => LabelForm::Type("CELL"),
        "FAX" => LabelForm::Type("FAX"),
        "ポケベル" => LabelForm::Type("PAGER"),
        _ => LabelForm::Grouped(apple_label(l)),
    }
}

/// 既知の語は iCloud の `_$!<Main>!$_` 形式（iPhone が自国語で表示する）、カスタム名はそのまま。
fn apple_label(label: &str) -> String {
    match google_type_for(Some(label)) {
        Some(t) if t != label => {
            let mut chars = t.chars();
            let cap: String = chars
                .next()
                .map(|f| f.to_ascii_uppercase().to_string() + chars.as_str())
                .unwrap_or_default();
            format!("_$!<{cap}>!$_")
        }
        _ => label.to_string(),
    }
}

/// テキスト値のエスケープ（`\` `,` `;` 改行）。
fn text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.trim().chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            ',' => out.push_str("\\,"),
            ';' => out.push_str("\\;"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// 構造化値（N・ADR・ORG）: 各要素をエスケープして `;` でつなぐ。
fn structured(parts: &[&Option<String>]) -> String {
    parts
        .iter()
        .map(|p| p.as_deref().map(text).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(";")
}

/// パラメータ値に入れられない文字（`:` `;` `,` `"`・改行）を除く。
fn param_value(s: &str) -> String {
    s.trim()
        .chars()
        .filter(|c| !matches!(c, ':' | ';' | ',' | '"' | '\n' | '\r'))
        .collect()
}

/// 75 オクテットごとに折り返して（続きの行は空白で始める）、CRLF で終える。
/// UTF-8 の文字の途中では切らない。
fn fold_into(out: &mut String, line: &str) {
    let mut width = 0;
    for ch in line.chars() {
        let len = ch.len_utf8();
        if width + len > FOLD_AT {
            out.push_str("\r\n ");
            width = 1;
        }
        out.push(ch);
        width += len;
    }
    out.push_str("\r\n");
}

#[cfg(test)]
mod tests;
