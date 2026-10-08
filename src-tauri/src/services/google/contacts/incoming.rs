//! 取り込み: Google の Person → 連絡先の中身（[`ContactFields`]）。
//!
//! vCard / Google CSV の取り込みと同じ型に落とすので、照合（`services::contact_match`）も保存
//! （`store`）も取り込み元を問わず同じ道を通る。ラベルの語彙は `services::contact_labels`。

use super::api::{GAddress, GDate, GPerson, GTypedValue};
use super::STARRED_GROUP;
use crate::models::{
    ContactAddress, ContactCustomField, ContactDate, ContactFields, ContactHandle,
    ContactOrganization, ContactRelation, ContactUrl, ContactValue, HandleKind,
};
use crate::services::contact_fields::address_is_empty;
use crate::services::contact_labels::label_from_google;
use std::collections::HashMap;

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

fn opt(s: &Option<String>) -> Option<String> {
    s.as_deref().and_then(non_empty)
}

/// 主値（primary）を先頭に並べる（安定ソート）。People API は配列の先頭を主値として扱わない
/// ことがあるので、metadata.primary を正とする。
fn primary_first<T>(mut items: Vec<(bool, T)>) -> Vec<T> {
    items.sort_by_key(|(primary, _)| !*primary);
    items.into_iter().map(|(_, v)| v).collect()
}

/// メール・電話（値が空のものは捨てる）。
fn values(src: &[GTypedValue]) -> Vec<ContactValue> {
    primary_first(
        src.iter()
            .filter_map(|v| {
                Some((
                    v.metadata.primary,
                    ContactValue {
                        label: label_from_google(
                            v.value_type.as_deref(),
                            v.formatted_type.as_deref(),
                        ),
                        value: opt(&v.value)?,
                        is_shared: false,
                    },
                ))
            })
            .collect(),
    )
}

fn addresses(src: &[GAddress]) -> Vec<ContactAddress> {
    primary_first(
        src.iter()
            .map(|a| {
                (
                    a.metadata.primary,
                    ContactAddress {
                        label: label_from_google(
                            a.value_type.as_deref(),
                            a.formatted_type.as_deref(),
                        ),
                        po_box: opt(&a.po_box),
                        postal: opt(&a.postal_code),
                        region: opt(&a.region),
                        city: opt(&a.city),
                        street: opt(&a.street_address),
                        extended: opt(&a.extended_address),
                        country: opt(&a.country),
                        country_code: opt(&a.country_code),
                    },
                )
            })
            // 見出し以外が全部空の住所は捨てる。
            .filter(|(_, a)| !address_is_empty(a))
            .collect(),
    )
}

/// 日付を住所録の表記（`YYYY-MM-DD`）へ。年が無い場合は vCard 4.0 と同じ `--MM-DD`。
fn date_text(d: &GDate) -> Option<String> {
    let (m, day) = (d.month?, d.day?);
    Some(match d.year {
        Some(y) => format!("{y:04}-{m:02}-{day:02}"),
        None => format!("--{m:02}-{day:02}"),
    })
}

fn birthday(p: &GPerson) -> Option<String> {
    let b = p.birthdays.first()?;
    b.date.as_ref().and_then(date_text).or_else(|| opt(&b.text))
}

/// Person 1 件を連絡先の中身へ。表示名も主メールも主電話も無いものは連絡先として成立しないので None。
///
/// `group_names` は連絡先グループ ID → 名前の対応（`api::list_contact_groups` 由来）。未知の ID は
/// 捨てる（システムグループは含まれない）。スター（`starred`）はお気に入りにする。
pub fn fields_from_person(
    p: &GPerson,
    group_names: &HashMap<String, String>,
) -> Option<ContactFields> {
    let name = p
        .names
        .iter()
        .find(|n| n.metadata.primary)
        .or_else(|| p.names.first());
    let n = |f: fn(&super::api::GName) -> &Option<String>| name.and_then(|n| opt(f(n)));
    let organizations: Vec<ContactOrganization> = primary_first(
        p.organizations
            .iter()
            .map(|o| {
                (
                    o.metadata.primary,
                    ContactOrganization {
                        org_id: None,
                        name: opt(&o.name),
                        phonetic_name: opt(&o.phonetic_name),
                        title: opt(&o.title),
                        department: opt(&o.department),
                    },
                )
            })
            .filter(|(_, o)| o != &ContactOrganization::default())
            .collect(),
    );
    let emails = values(&p.email_addresses);
    let phones = values(&p.phone_numbers);
    let family = n(|n| &n.family_name);
    let given = n(|n| &n.given_name);

    let display_name = n(|n| &n.display_name)
        .or_else(|| match (family.as_deref(), given.as_deref()) {
            (Some(l), Some(f)) => Some(format!("{l} {f}")),
            (l, f) => l.or(f).map(str::to_string),
        })
        // 名前が無い連絡先（会社の代表アドレスだけ等）は会社名／主値で代用する。
        .or_else(|| organizations.first().and_then(|o| o.name.clone()))
        .or_else(|| emails.first().map(|e| e.value.clone()))
        .or_else(|| phones.first().map(|p| p.value.clone()))?;

    let group_ids: Vec<&str> = p
        .memberships
        .iter()
        .filter_map(|m| {
            m.contact_group_membership
                .as_ref()?
                .contact_group_id
                .as_deref()
        })
        .collect();

    Some(ContactFields {
        display_name,
        name_prefix: n(|n| &n.honorific_prefix),
        family_name: family,
        middle_name: n(|n| &n.middle_name),
        given_name: given,
        name_suffix: n(|n| &n.honorific_suffix),
        phonetic_family: n(|n| &n.phonetic_family_name),
        phonetic_middle: n(|n| &n.phonetic_middle_name),
        phonetic_given: n(|n| &n.phonetic_given_name),
        nickname: p.nicknames.iter().find_map(|v| opt(&v.value)),
        birthday: birthday(p),
        note: p.biographies.first().and_then(|b| opt(&b.value)),
        is_favorite: group_ids.contains(&STARRED_GROUP),
        organizations,
        emails,
        phones,
        addresses: addresses(&p.addresses),
        urls: values(&p.urls)
            .into_iter()
            .map(|v| ContactUrl {
                label: v.label,
                value: v.value,
            })
            .collect(),
        dates: p
            .events
            .iter()
            .filter_map(|e| {
                Some(ContactDate {
                    label: label_from_google(e.value_type.as_deref(), e.formatted_type.as_deref()),
                    date: e.date.as_ref().and_then(date_text)?,
                })
            })
            .collect(),
        relations: p
            .relations
            .iter()
            .filter_map(|r| {
                Some(ContactRelation {
                    label: label_from_google(r.value_type.as_deref(), r.formatted_type.as_deref()),
                    name: opt(&r.person)?,
                })
            })
            .collect(),
        handles: p
            .im_clients
            .iter()
            .filter_map(|c| {
                Some(ContactHandle {
                    kind: HandleKind::Im,
                    service: opt(&c.formatted_protocol).or_else(|| opt(&c.protocol)),
                    value: opt(&c.username)?,
                    label: label_from_google(c.value_type.as_deref(), c.formatted_type.as_deref()),
                })
            })
            .collect(),
        custom_fields: p
            .user_defined
            .iter()
            .filter_map(|u| {
                Some(ContactCustomField {
                    key: opt(&u.key)?,
                    value: opt(&u.value)?,
                })
            })
            .collect(),
        tags: group_ids
            .iter()
            .filter_map(|id| group_names.get(*id).cloned())
            .collect(),
        ..Default::default()
    })
}

#[cfg(test)]
mod tests;
