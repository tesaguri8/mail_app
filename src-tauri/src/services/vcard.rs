//! vCard (3.0/4.0) の最小パーサ。外部依存なしで iCloud / Google のエクスポートを取り込む。
//!
//! 対応: 行折り返し（先頭スペース/タブ）・`itemN.` グループ接頭辞・`\n \, \; \\` エスケープ・
//! 複数 EMAIL/TEL（type=pref を優先）・N（姓;名;…）・X-PHONETIC-*（よみ）・ADR・BDAY・NOTE・UID。
//! PHOTO やその他 X- プロパティは無視する。

/// 取り込んだ 1 件の連絡先（DB 投入前の中間表現）。
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ImportedContact {
    pub display_name: String,
    pub name_kana: Option<String>,
    pub email: Option<String>,
    /// 追加アドレス（JSON 配列文字列。主アドレス以外が 1 件以上あるときのみ）。
    pub emails_json: Option<String>,
    pub phone: Option<String>,
    pub organization: Option<String>,
    pub address: Option<String>,
    pub birthday: Option<String>,
    pub note: Option<String>,
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
    /// (value, is_pref)
    emails: Vec<(String, bool)>,
    tels: Vec<(String, bool)>,
    org: Option<String>,
    address: Option<(String, bool)>,
    birthday: Option<String>,
    note: Option<String>,
    uid: Option<String>,
    prodid: Option<String>,
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
                    self.emails.push((v, is_pref(params)));
                }
            }
            "TEL" => {
                if let Some(v) = non_empty(value) {
                    self.tels.push((v, is_pref(params)));
                }
            }
            "ORG" => {
                // 先頭コンポーネント（会社名）を採用。
                let first = split_unescaped(raw_value, ';')
                    .into_iter()
                    .map(|p| unescape(&p).trim().to_string())
                    .find(|s| !s.is_empty());
                if self.org.is_none() {
                    self.org = first;
                }
            }
            "ADR" => {
                // 空でない構成要素を vCard 並びで連結。pref を優先採用。
                let joined = split_unescaped(raw_value, ';')
                    .iter()
                    .map(|p| unescape(p).trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                if let Some(addr) = non_empty(joined) {
                    let pref = is_pref(params);
                    if self.address.is_none() || (pref && !self.address.as_ref().unwrap().1) {
                        self.address = Some((addr, pref));
                    }
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
            "UID" => self.uid = non_empty(value),
            "PRODID" => self.prodid = non_empty(value),
            _ => {}
        }
    }

    fn finish(self) -> Option<ImportedContact> {
        // 主/追加メールを pref 優先で並べ替え。
        let mut emails: Vec<String> = Vec::new();
        for (v, pref) in &self.emails {
            if *pref {
                emails.insert(0, v.clone());
            } else {
                emails.push(v.clone());
            }
        }
        emails.dedup();
        let email = emails.first().cloned();
        let emails_json = if emails.len() > 1 {
            serde_json::to_string(&emails[1..]).ok()
        } else {
            None
        };

        let phone = {
            let pref = self.tels.iter().find(|(_, p)| *p).map(|(v, _)| v.clone());
            pref.or_else(|| self.tels.first().map(|(v, _)| v.clone()))
        };

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

        Some(ImportedContact {
            display_name,
            name_kana,
            email,
            emails_json,
            phone,
            organization: self.org,
            address: self.address.map(|(a, _)| a),
            birthday: self.birthday,
            note: self.note,
            source,
            external_id: self.uid,
        })
    }
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
        assert_eq!(c.name_kana.as_deref(), Some("アイカワ"));
        assert_eq!(c.email.as_deref(), Some("rabbit@key.ocn.ne.jp"));
        assert_eq!(c.emails_json.as_deref(), Some("[\"second@example.com\"]"));
        assert_eq!(c.phone.as_deref(), Some("0997-52-4187")); // pref 優先
        assert_eq!(
            c.address.as_deref(),
            Some("鹿児島県奄美市名瀬佐大熊町17-10AKビル2F 8940005")
        );
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
