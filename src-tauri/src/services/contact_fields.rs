//! 連絡先の中身（[`ContactFields`]）どうしの合成。DB にも外部サービスにも触れない純粋な規則。
//!
//! 合成の場面は 3 つあり、どれも「同じ値か」の物差し（メールは小文字、電話は数字、会社は
//! 正規化名）を共有する。
//!
//! - [`overlay_google`] — Google から取り込んだ内容で、紐付いた連絡先を更新する
//!   （Google が扱う項目だけを Google の正本で置き換え、Rondine 固有の印は残す）
//! - [`fill_from_import`] — ファイル取り込みで既存の連絡先を更新する（入ってきた値で埋める）
//! - [`union_merge`] — 重複整理で複数の連絡先を 1 件にまとめる（和集合）

use crate::models::{ContactAddress, ContactFields, ContactOrganization, ContactValue, HandleKind};
use crate::services::dedupe::{digits, fold, normalize_org};
use std::collections::BTreeMap;

/// よみ（姓・ミドル・名を空白でつないだもの）。どれも無ければ None。
pub fn phonetic_name(f: &ContactFields) -> Option<String> {
    let parts: Vec<&str> = [&f.phonetic_family, &f.phonetic_middle, &f.phonetic_given]
        .into_iter()
        .filter_map(|p| p.as_deref().map(str::trim).filter(|s| !s.is_empty()))
        .collect();
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// 並び替え用の名前（よみ優先。無ければ表示名）。
pub fn sort_name(f: &ContactFields) -> String {
    phonetic_name(f).unwrap_or_else(|| f.display_name.trim().to_string())
}

/// 住所の文字の集まり（全角半角・大文字小文字をそろえた英数字と漢字かなの出現数）。区切り・
/// 空白・記号は数えない。欄の分け方や語順の違いを吸収して比べるための材料。
fn address_chars(a: &ContactAddress) -> BTreeMap<char, usize> {
    fold(&address_line(a))
        .chars()
        .filter(|c| c.is_alphanumeric())
        .fold(BTreeMap::new(), |mut m, c| {
            *m.entry(c).or_insert(0) += 1;
            m
        })
}

/// `small` の文字がすべて `large` に（数も含めて）入っているか。
fn chars_within(small: &BTreeMap<char, usize>, large: &BTreeMap<char, usize>) -> bool {
    small
        .iter()
        .all(|(c, n)| large.get(c).is_some_and(|m| m >= n))
}

/// 2 つの住所が、同じ場所の書き方違いとみなせるか。片方の文字（番地の数字も含む）が
/// もう片方に全部含まれていれば同じとみなす（欄の分け方・語順・郵便番号の有無の違いを吸収）。
/// 建物名や番地が違えば別の住所。
pub fn same_address(a: &ContactAddress, b: &ContactAddress) -> bool {
    let (x, y) = (address_chars(a), address_chars(b));
    chars_within(&x, &y) || chars_within(&y, &x)
}

/// 住所を足し合わせる。同じ場所の書き方違い（[`same_address`]）は 1 つにまとめ、文字の多い
/// （情報の多い）ほうを残す。
fn merge_addresses(dst: &mut Vec<ContactAddress>, src: &[ContactAddress]) {
    for a in src {
        let chars = address_chars(a);
        match dst.iter_mut().find(|d| same_address(d, a)) {
            Some(d) => {
                if !chars_within(&chars, &address_chars(d)) {
                    *d = a.clone();
                }
            }
            None => dst.push(a.clone()),
        }
    }
}

/// 構造化住所を 1 行の文字列へ（一覧・重複判定用）。
pub fn address_line(a: &ContactAddress) -> String {
    [
        &a.postal,
        &a.region,
        &a.city,
        &a.street,
        &a.extended,
        &a.po_box,
        &a.country,
    ]
    .into_iter()
    .filter_map(|v| v.as_deref().map(str::trim).filter(|s| !s.is_empty()))
    .collect::<Vec<_>>()
    .join(" ")
}

/// 値が無いか空白だけか。
pub fn is_blank(v: &Option<String>) -> bool {
    v.as_deref().map_or(true, |s| s.trim().is_empty())
}

/// 住所が空か（見出し以外の項目がすべて空）。
pub fn address_is_empty(a: &ContactAddress) -> bool {
    address_line(a).is_empty() && is_blank(&a.country_code)
}

/// メールの同一判定キー（小文字・全角を畳む）。
fn email_key(v: &str) -> String {
    fold(v).trim().to_string()
}

/// 電話の同一判定キー（数字だけ）。
fn phone_key(v: &str) -> String {
    digits(v)
}

/// 会社の同一判定キー（組織の重複整理と同じ正規化）。
pub fn org_key(o: &ContactOrganization) -> String {
    o.name.as_deref().map(normalize_org).unwrap_or_default()
}

/// 既存の値に付いていた Rondine 固有の印（共有の印・組織カードとのつながり）を、
/// 入ってきた値のうち同じものへ引き継ぐ。
pub fn carry_local_marks(existing: &ContactFields, incoming: &mut ContactFields) {
    carry_shared(&existing.emails, &mut incoming.emails, email_key);
    carry_shared(&existing.phones, &mut incoming.phones, phone_key);
    for o in &mut incoming.organizations {
        if o.org_id.is_some() {
            continue;
        }
        let key = org_key(o);
        if key.is_empty() {
            continue;
        }
        if let Some(prev) = existing
            .organizations
            .iter()
            .find(|p| p.org_id.is_some() && org_key(p) == key)
        {
            o.org_id = prev.org_id;
        }
    }
}

fn carry_shared(existing: &[ContactValue], incoming: &mut [ContactValue], key: fn(&str) -> String) {
    for v in incoming.iter_mut() {
        let k = key(&v.value);
        if existing.iter().any(|e| e.is_shared && key(&e.value) == k) {
            v.is_shared = true;
        }
    }
}

/// Google から取り込んだ内容（`remote`）で既存の連絡先を更新した結果を返す。
///
/// Google が扱う項目（名前・ニックネーム・誕生日・メモ・お気に入り・会社・メール・電話・住所・
/// URL・日付・関係・チャット・カスタム項目）は Google の正本で置き換える（Google 側で消した
/// 項目はここでも消える）。Google に無い項目（旧姓・会社として表示・SNS のハンドル・取引先・
/// 外部画像許可）と、共有の印・組織カードとのつながりは残す。タグは呼び出し側が別に扱う。
pub fn overlay_google(existing: &ContactFields, remote: &ContactFields) -> ContactFields {
    let mut out = remote.clone();
    carry_local_marks(existing, &mut out);
    out.maiden_name = existing.maiden_name.clone();
    out.show_as_company = existing.show_as_company;
    out.is_business = existing.is_business;
    out.allow_remote_images = existing.allow_remote_images;
    out.handles.retain(|h| h.kind == HandleKind::Im);
    out.handles.extend(
        existing
            .handles
            .iter()
            .filter(|h| h.kind == HandleKind::Social)
            .cloned(),
    );
    out.tags = existing.tags.clone();
    out
}

/// ファイル取り込み（vCard / Google CSV）で既存の連絡先を更新した結果を返す。
///
/// 1 つの値の項目は入ってきた値があればそれ、無ければ既存を残す。複数値の項目は入ってきた側に
/// 1 つでもあれば置き換え、無ければ既存を残す。利用者のフラグ（お気に入り・取引先・外部画像
/// 許可）は既存のまま。タグは和集合。
pub fn fill_from_import(existing: &ContactFields, incoming: &ContactFields) -> ContactFields {
    fn pick(new: &Option<String>, old: &Option<String>) -> Option<String> {
        new.clone().or_else(|| old.clone())
    }
    fn list<T: Clone>(new: &[T], old: &[T]) -> Vec<T> {
        if new.is_empty() {
            old.to_vec()
        } else {
            new.to_vec()
        }
    }
    let mut out = ContactFields {
        display_name: if incoming.display_name.trim().is_empty() {
            existing.display_name.clone()
        } else {
            incoming.display_name.clone()
        },
        name_prefix: pick(&incoming.name_prefix, &existing.name_prefix),
        family_name: pick(&incoming.family_name, &existing.family_name),
        middle_name: pick(&incoming.middle_name, &existing.middle_name),
        given_name: pick(&incoming.given_name, &existing.given_name),
        name_suffix: pick(&incoming.name_suffix, &existing.name_suffix),
        phonetic_family: pick(&incoming.phonetic_family, &existing.phonetic_family),
        phonetic_middle: pick(&incoming.phonetic_middle, &existing.phonetic_middle),
        phonetic_given: pick(&incoming.phonetic_given, &existing.phonetic_given),
        nickname: pick(&incoming.nickname, &existing.nickname),
        maiden_name: pick(&incoming.maiden_name, &existing.maiden_name),
        birthday: pick(&incoming.birthday, &existing.birthday),
        note: pick(&incoming.note, &existing.note),
        show_as_company: existing.show_as_company || incoming.show_as_company,
        is_favorite: existing.is_favorite,
        is_business: existing.is_business,
        allow_remote_images: existing.allow_remote_images,
        organizations: list(&incoming.organizations, &existing.organizations),
        emails: list(&incoming.emails, &existing.emails),
        phones: list(&incoming.phones, &existing.phones),
        addresses: list(&incoming.addresses, &existing.addresses),
        urls: list(&incoming.urls, &existing.urls),
        dates: list(&incoming.dates, &existing.dates),
        relations: list(&incoming.relations, &existing.relations),
        handles: list(&incoming.handles, &existing.handles),
        custom_fields: list(&incoming.custom_fields, &existing.custom_fields),
        tags: existing.tags.clone(),
    };
    push_unique(&mut out.tags, &incoming.tags, |t| t.trim().to_string());
    carry_local_marks(existing, &mut out);
    out
}

/// 複数の連絡先を 1 件にまとめた結果を返す（重複整理の統合・照合での紐付け）。
///
/// 先頭（残す側）を優先し、1 つの値の項目は前から順に最初の値を採る。複数値の項目は同じ値を
/// 除いた和集合、フラグは OR。会社は正規化名が同じものを 1 つにし、組織カードのつながりを残す。
pub fn union_merge(parts: &[&ContactFields]) -> ContactFields {
    let Some((first, rest)) = parts.split_first() else {
        return ContactFields::default();
    };
    let mut out = (*first).clone();
    for p in rest {
        fill_scalar(&mut out.name_prefix, &p.name_prefix);
        fill_scalar(&mut out.family_name, &p.family_name);
        fill_scalar(&mut out.middle_name, &p.middle_name);
        fill_scalar(&mut out.given_name, &p.given_name);
        fill_scalar(&mut out.name_suffix, &p.name_suffix);
        fill_scalar(&mut out.phonetic_family, &p.phonetic_family);
        fill_scalar(&mut out.phonetic_middle, &p.phonetic_middle);
        fill_scalar(&mut out.phonetic_given, &p.phonetic_given);
        fill_scalar(&mut out.nickname, &p.nickname);
        fill_scalar(&mut out.maiden_name, &p.maiden_name);
        fill_scalar(&mut out.birthday, &p.birthday);
        fill_scalar(&mut out.note, &p.note);
        out.show_as_company |= p.show_as_company;
        out.is_favorite |= p.is_favorite;
        out.is_business |= p.is_business;
        out.allow_remote_images |= p.allow_remote_images;
        merge_orgs(&mut out.organizations, &p.organizations);
        push_unique(&mut out.emails, &p.emails, |v| email_key(&v.value));
        push_unique(&mut out.phones, &p.phones, |v| phone_key(&v.value));
        merge_addresses(&mut out.addresses, &p.addresses);
        push_unique(&mut out.urls, &p.urls, |u| u.value.trim().to_string());
        push_unique(&mut out.dates, &p.dates, |d| d.date.trim().to_string());
        push_unique(&mut out.relations, &p.relations, |r| fold(&r.name));
        push_unique(&mut out.handles, &p.handles, |h| fold(&h.value));
        push_unique(&mut out.custom_fields, &p.custom_fields, |c| c.key.clone());
        push_unique(&mut out.tags, &p.tags, |t| t.trim().to_string());
    }
    out
}

fn fill_scalar(dst: &mut Option<String>, src: &Option<String>) {
    if is_blank(dst) {
        if let Some(v) = src.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            *dst = Some(v.to_string());
        }
    }
}

/// `src` のうち `dst` に同じキーが無いものを後ろへ足す（空キーは足さない）。
fn push_unique<T: Clone, K: PartialEq>(dst: &mut Vec<T>, src: &[T], key: impl Fn(&T) -> K) {
    for item in src {
        let k = key(item);
        if !dst.iter().any(|d| key(d) == k) {
            dst.push(item.clone());
        }
    }
}

/// 会社をまとめる。正規化名が同じなら 1 つにし、空いている項目と組織カードのつながりを補う。
fn merge_orgs(dst: &mut Vec<ContactOrganization>, src: &[ContactOrganization]) {
    for o in src {
        let key = org_key(o);
        match dst
            .iter_mut()
            .find(|d| !key.is_empty() && org_key(d) == key)
        {
            Some(d) => {
                if d.org_id.is_none() {
                    d.org_id = o.org_id;
                }
                fill_scalar(&mut d.phonetic_name, &o.phonetic_name);
                fill_scalar(&mut d.title, &o.title);
                fill_scalar(&mut d.department, &o.department);
            }
            None => dst.push(o.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ContactHandle;

    /// 統合の和集合: 同じ住所の書き方違いは 1 つにし、情報の多いほうを残す。別の住所は両方残す。
    #[test]
    fn union_merge_folds_differently_written_addresses() {
        let addr = |postal: Option<&str>, street: &str| ContactAddress {
            postal: postal.map(str::to_string),
            street: Some(street.into()),
            ..Default::default()
        };
        let a = ContactFields {
            addresses: vec![addr(None, "沖縄県 名護市 大南 二丁目 1番 1")],
            ..Default::default()
        };
        let b = ContactFields {
            addresses: vec![
                addr(Some("905-0015"), "沖縄県名護市大南二丁目1番1号"),
                addr(None, "沖縄県那覇市泉崎1-1-1"),
            ],
            ..Default::default()
        };
        let merged = union_merge(&[&a, &b]);
        assert_eq!(
            merged.addresses,
            vec![
                addr(Some("905-0015"), "沖縄県名護市大南二丁目1番1号"),
                addr(None, "沖縄県那覇市泉崎1-1-1")
            ]
        );
    }

    fn email(v: &str, shared: bool) -> ContactValue {
        ContactValue {
            label: None,
            value: v.into(),
            is_shared: shared,
        }
    }

    fn org(name: &str, id: Option<i32>) -> ContactOrganization {
        ContactOrganization {
            org_id: id,
            name: Some(name.into()),
            ..Default::default()
        }
    }

    #[test]
    fn sort_name_prefers_the_reading() {
        let mut f = ContactFields {
            display_name: "山田 太郎".into(),
            ..Default::default()
        };
        assert_eq!(sort_name(&f), "山田 太郎");
        f.phonetic_family = Some("ヤマダ".into());
        f.phonetic_given = Some("タロウ".into());
        assert_eq!(sort_name(&f), "ヤマダ タロウ");
    }

    #[test]
    fn overlay_google_replaces_google_fields_and_keeps_local_marks() {
        let existing = ContactFields {
            display_name: "山田太郎".into(),
            nickname: Some("やまちゃん".into()),
            maiden_name: Some("佐藤".into()),
            is_business: true,
            emails: vec![email("info@x.jp", true), email("taro@x.jp", false)],
            organizations: vec![org("株式会社テスト", Some(3))],
            handles: vec![ContactHandle {
                kind: HandleKind::Social,
                service: Some("Twitter".into()),
                value: "@taro".into(),
                label: None,
            }],
            tags: vec!["自分用".into()],
            ..Default::default()
        };
        let remote = ContactFields {
            display_name: "山田 太郎".into(),
            emails: vec![email("INFO@x.jp", false)],
            organizations: vec![org("(株)テスト", None)],
            is_favorite: true,
            ..Default::default()
        };
        let out = overlay_google(&existing, &remote);
        assert_eq!(out.display_name, "山田 太郎");
        assert_eq!(out.nickname, None, "Google 側で消したニックネームは消える");
        assert_eq!(
            out.maiden_name.as_deref(),
            Some("佐藤"),
            "Google に無い項目は残す"
        );
        assert!(out.is_business && out.is_favorite);
        assert_eq!(out.emails.len(), 1);
        assert!(out.emails[0].is_shared, "共有の印は同じ値へ引き継ぐ");
        assert_eq!(
            out.organizations[0].org_id,
            Some(3),
            "組織カードのつながりは残す"
        );
        assert_eq!(
            out.handles.len(),
            1,
            "SNS のハンドルは Google に無いので残す"
        );
        assert_eq!(out.tags, vec!["自分用".to_string()]);
    }

    #[test]
    fn fill_from_import_keeps_existing_when_incoming_is_empty() {
        let existing = ContactFields {
            display_name: "山田太郎".into(),
            note: Some("メモ".into()),
            is_favorite: true,
            emails: vec![email("taro@x.jp", false)],
            tags: vec!["A".into()],
            ..Default::default()
        };
        let incoming = ContactFields {
            display_name: "山田太郎".into(),
            phones: vec![email("090-1111-2222", false)],
            tags: vec!["B".into()],
            ..Default::default()
        };
        let out = fill_from_import(&existing, &incoming);
        assert_eq!(out.note.as_deref(), Some("メモ"));
        assert!(out.is_favorite);
        assert_eq!(out.emails.len(), 1);
        assert_eq!(out.phones.len(), 1);
        assert_eq!(out.tags, vec!["A".to_string(), "B".to_string()]);
    }

    #[test]
    fn union_merge_dedups_values_and_merges_orgs() {
        let a = ContactFields {
            display_name: "田中太郎".into(),
            emails: vec![email("taro@a.jp", false)],
            organizations: vec![org("株式会社テスト", None)],
            ..Default::default()
        };
        let b = ContactFields {
            display_name: "田中 太郎".into(),
            phonetic_family: Some("タナカ".into()),
            is_business: true,
            emails: vec![email("TARO@a.jp", false), email("taro@b.jp", false)],
            organizations: vec![ContactOrganization {
                org_id: Some(9),
                name: Some("(株)テスト".into()),
                title: Some("部長".into()),
                ..Default::default()
            }],
            ..Default::default()
        };
        let out = union_merge(&[&a, &b]);
        assert_eq!(out.display_name, "田中太郎", "残す側の表示名");
        assert_eq!(out.phonetic_family.as_deref(), Some("タナカ"));
        assert!(out.is_business);
        assert_eq!(out.emails.len(), 2);
        assert_eq!(out.organizations.len(), 1);
        assert_eq!(out.organizations[0].org_id, Some(9));
        assert_eq!(out.organizations[0].title.as_deref(), Some("部長"));
    }
}
