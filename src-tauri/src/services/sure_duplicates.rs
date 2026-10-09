//! 確実な重複（まとめて統合してよい組）の基準。DB にも People API にも触らない。
//!
//! 重複の整理の「確実な重複をまとめて統合」（docs/CONTACTS_SYNC.md §3-5）の判定だけを受け持つ。
//! 一致の条件はどれか 1 つではなく**全部**:
//!
//! - 正規化した表示名が同じ（全角半角・大文字小文字・空白をそろえる。空の名前は対象外）
//! - メールの集合が同じ（大文字小文字をそろえる）
//! - 電話の集合が同じ（数字だけで比べる。書式の違いは同じとみなす）
//! - メールか電話のどちらかが 1 つ以上ある（名前だけが同じ別人をまとめない）
//!
//! さらに、和集合にすると困る欄が食い違う組は除く（誕生日・メモ・氏名の各欄・よみ・
//! 呼び名・旧姓・住所）。片方だけが持っている値は食い違いとはみなさない（和集合で埋まる）。
//! 複数持てる欄（組織・URL・記念日・関係・SNS・カスタム・タグ）は和集合で困らないので見ない。

use crate::models::{ContactAddress, ContactFields, ContactSummary};
use crate::services::contact_fields::{address_line, same_address};
use crate::services::dedupe::{digits, fold, fold_remove_ws};
use crate::services::distinct::DistinctPairs;
use std::collections::{BTreeMap, BTreeSet};

/// まとめて統合する 1 組。`keep` に残し、`drops` を消す（既存の統合と同じ）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SureGroup {
    pub keep: i64,
    pub drops: Vec<i64>,
}

/// 一致の鍵（表示名・メールの集合・電話の集合）。
type Key = (String, BTreeSet<String>, BTreeSet<String>);

/// 鍵を作る。名前が空、またはメールも電話も無ければ None（対象外）。
fn key(f: &ContactFields) -> Option<Key> {
    let name = fold_remove_ws(&f.display_name);
    let emails: BTreeSet<String> = f
        .emails
        .iter()
        .map(|e| fold(&e.value).trim().to_string())
        .filter(|e| !e.is_empty())
        .collect();
    let phones: BTreeSet<String> = f
        .phones
        .iter()
        .map(|p| digits(&p.value))
        .filter(|p| !p.is_empty())
        .collect();
    (!name.is_empty() && !(emails.is_empty() && phones.is_empty()))
        .then_some((name, emails, phones))
}

/// 和集合にすると困る 1 つきりの欄（比べる前に空白などをそろえる）。
fn scalars(f: &ContactFields) -> [Option<String>; 12] {
    [
        &f.birthday,
        &f.note,
        &f.name_prefix,
        &f.family_name,
        &f.middle_name,
        &f.given_name,
        &f.name_suffix,
        &f.phonetic_family,
        &f.phonetic_middle,
        &f.phonetic_given,
        &f.nickname,
        &f.maiden_name,
    ]
    .map(|v| v.as_deref().map(fold_remove_ws).filter(|s| !s.is_empty()))
}

/// 2 人の住所が食い違うか。どちらも住所を持っていて、片方の住所がすべてもう片方のどれかと
/// 同じ場所（[`same_address`]。書き方の違いは同じとみなす）でもなく、その逆でもないとき。
fn addresses_differ(a: &ContactFields, b: &ContactFields) -> bool {
    // 欄が空の住所（ラベルだけ）は持っていないのと同じ。
    let filled = |f: &ContactFields| -> Vec<ContactAddress> {
        f.addresses
            .iter()
            .filter(|x| !address_line(x).trim().is_empty())
            .cloned()
            .collect()
    };
    let (a, b) = (filled(a), filled(b));
    let covered = |x: &[ContactAddress], y: &[ContactAddress]| {
        x.iter().all(|p| y.iter().any(|q| same_address(p, q)))
    };
    !a.is_empty() && !b.is_empty() && !covered(&a, &b) && !covered(&b, &a)
}

/// 組の中で、和集合にすると困る欄が食い違っているか。1 つきりの欄は、値を持つ人どうしで
/// 2 通り以上あれば食い違い。住所は書き方の違いを同じとみなしたうえで比べる。
fn conflicts(members: &[&ContactSummary]) -> bool {
    let fields: Vec<[Option<String>; 12]> = members.iter().map(|m| scalars(&m.fields)).collect();
    let scalar_differs = (0..12).any(|i| {
        fields
            .iter()
            .filter_map(|f| f[i].as_ref())
            .collect::<BTreeSet<_>>()
            .len()
            > 1
    });
    scalar_differs
        || members.iter().enumerate().any(|(i, a)| {
            members[i + 1..]
                .iter()
                .any(|b| addresses_differ(&a.fields, &b.fields))
        })
}

/// 情報量（残す 1 件の選び方。重複の整理の画面の `fieldCount` と同じ数え方）。
fn info_count(c: &ContactSummary) -> usize {
    let scalars = [&c.sort_name, &c.fields.birthday, &c.fields.note]
        .into_iter()
        .filter(|v| v.as_deref().is_some_and(|s| !s.trim().is_empty()))
        .count();
    scalars
        + c.fields.emails.len()
        + c.fields.phones.len()
        + c.fields.organizations.len()
        + c.fields.addresses.len()
}

/// 確実な重複の組を返す（2 件以上の組だけ）。残す 1 件は既存の統合と同じく、情報量の多いもの、
/// 同じなら先に並んでいるもの（表示名は組の中で同じなので「最多一致」は効かない）。
/// 利用者が「別人」と記録した対（`distinct`）は同じ組にしない（組を分け直す）。
pub fn sure_groups(contacts: &[ContactSummary], distinct: &DistinctPairs) -> Vec<SureGroup> {
    let mut by_key: BTreeMap<Key, Vec<&ContactSummary>> = BTreeMap::new();
    contacts
        .iter()
        .filter(|c| c.deleted_at.is_none())
        .for_each(|c| {
            if let Some(k) = key(&c.fields) {
                by_key.entry(k).or_default().push(c);
            }
        });
    let mut groups: Vec<SureGroup> = by_key
        .into_values()
        .filter(|m| m.len() > 1)
        .flat_map(|m| distinct.split(m, |c| i64::from(c.id)))
        .filter(|m| !conflicts(m))
        .map(|members| {
            // max_by_key は同点で後ろを返すので、先に並んだものを残すよう逆順から探す。
            let keep = members
                .iter()
                .rev()
                .max_by_key(|c| info_count(c))
                .map_or(0, |c| i64::from(c.id));
            SureGroup {
                keep,
                drops: members
                    .iter()
                    .map(|c| i64::from(c.id))
                    .filter(|id| *id != keep)
                    .collect(),
            }
        })
        .collect();
    groups.sort_by_key(|g| g.keep);
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ContactAddress, ContactValue};

    fn value(v: &str) -> ContactValue {
        ContactValue {
            label: None,
            value: v.into(),
            is_shared: false,
        }
    }

    fn contact(id: i32, name: &str, emails: &[&str], phones: &[&str]) -> ContactSummary {
        let mut c = ContactSummary {
            id,
            ..Default::default()
        };
        c.fields.display_name = name.into();
        c.fields.emails = emails.iter().map(|e| value(e)).collect();
        c.fields.phones = phones.iter().map(|p| value(p)).collect();
        c
    }

    #[test]
    fn same_name_emails_and_phones_make_a_group() {
        let cs = [
            contact(1, "末松 信吾", &["S@x.jp"], &["090-1111-2222"]),
            // 空白・全角・大文字小文字・電話の書式の違いは同じとみなす。
            contact(2, "末松信吾", &["s@x.jp"], &["０９０１１１１２２２２"]),
            contact(3, "末松信吾", &["s@x.jp"], &["(090) 1111 2222"]),
        ];
        assert_eq!(
            sure_groups(&cs, &DistinctPairs::default()),
            vec![SureGroup {
                keep: 1,
                drops: vec![2, 3]
            }]
        );
    }

    #[test]
    fn a_different_email_or_phone_is_not_sure() {
        let cs = [
            contact(1, "山田", &["a@x.jp"], &["0311112222"]),
            contact(2, "山田", &["b@x.jp"], &["0311112222"]),
            contact(3, "山田", &["a@x.jp"], &["0311112223"]),
            // 片方にだけメールが多い（集合が違う）。
            contact(4, "山田", &["a@x.jp", "c@x.jp"], &["0311112222"]),
        ];
        assert!(sure_groups(&cs, &DistinctPairs::default()).is_empty());
    }

    #[test]
    fn empty_names_and_name_only_matches_are_skipped() {
        let cs = [
            contact(1, "", &["a@x.jp"], &[]),
            contact(2, " ", &["a@x.jp"], &[]),
            // 名前だけが同じ（メールも電話も無い）別人はまとめない。
            contact(3, "佐藤", &[], &[]),
            contact(4, "佐藤", &[], &[]),
        ];
        assert!(sure_groups(&cs, &DistinctPairs::default()).is_empty());
    }

    #[test]
    fn conflicting_birthdays_or_addresses_are_left_out() {
        let mut a = contact(1, "鈴木", &["s@x.jp"], &[]);
        let mut b = contact(2, "鈴木", &["s@x.jp"], &[]);
        a.fields.birthday = Some("1980-01-01".into());
        b.fields.birthday = Some("1981-01-01".into());
        assert!(
            sure_groups(&[a.clone(), b.clone()], &DistinctPairs::default()).is_empty(),
            "誕生日が違う"
        );

        // 片方だけが持っている値は食い違いではない（和集合で埋まる）。
        b.fields.birthday = None;
        assert_eq!(
            sure_groups(&[a.clone(), b.clone()], &DistinctPairs::default()).len(),
            1
        );

        let addr = |city: &str| ContactAddress {
            city: Some(city.into()),
            ..Default::default()
        };
        a.fields.addresses = vec![addr("那覇市")];
        b.fields.addresses = vec![addr("名護市")];
        assert!(
            sure_groups(&[a.clone(), b.clone()], &DistinctPairs::default()).is_empty(),
            "住所が違う"
        );
        b.fields.addresses = vec![addr("那覇市")];
        assert_eq!(
            sure_groups(&[a.clone(), b.clone()], &DistinctPairs::default()).len(),
            1,
            "住所が同じならまとめる"
        );
        // 書き方の違い（欄の分け方・語順・郵便番号の有無）は同じ住所とみなす。
        a.fields.addresses = vec![ContactAddress {
            region: Some("沖縄県".into()),
            city: Some("名護市".into()),
            street: Some("1 1番 二丁目 大南".into()),
            ..Default::default()
        }];
        b.fields.addresses = vec![ContactAddress {
            postal: Some("905-0015".into()),
            street: Some("沖縄県名護市大南二丁目1番1号".into()),
            ..Default::default()
        }];
        assert_eq!(
            sure_groups(&[a.clone(), b.clone()], &DistinctPairs::default()).len(),
            1,
            "書き方違い"
        );
        // 都道府県・市区町村だけの住所は詳しい住所と同じとはみなさない（食い違いとして除く）。
        b.fields.addresses = vec![ContactAddress {
            region: Some("沖縄県".into()),
            ..Default::default()
        }];
        assert!(
            sure_groups(&[a.clone(), b.clone()], &DistinctPairs::default()).is_empty(),
            "県だけ"
        );
        // 欄が空の住所は持っていないのと同じ。
        b.fields.addresses = vec![ContactAddress {
            label: Some("自宅".into()),
            ..Default::default()
        }];
        assert_eq!(
            sure_groups(&[a.clone(), b.clone()], &DistinctPairs::default()).len(),
            1,
            "空の住所"
        );
        // 番地が違えば別の住所。
        b.fields.addresses = vec![ContactAddress {
            street: Some("沖縄県名護市大南二丁目2番1号".into()),
            ..Default::default()
        }];
        assert!(
            sure_groups(&[a, b], &DistinctPairs::default()).is_empty(),
            "番地が違う"
        );
    }

    #[test]
    fn keeps_the_richest_then_the_first() {
        let a = contact(5, "高橋", &["t@x.jp"], &[]);
        let mut b = contact(6, "高橋", &["t@x.jp"], &[]);
        b.fields.note = Some("メモ".into());
        let c = contact(7, "高橋", &["t@x.jp"], &[]);
        assert_eq!(
            sure_groups(&[a.clone(), b, c.clone()], &DistinctPairs::default())[0].keep,
            6
        );
        assert_eq!(
            sure_groups(&[a, c], &DistinctPairs::default())[0].keep,
            5,
            "同じなら先に並んだもの"
        );
    }

    #[test]
    fn deleted_contacts_are_ignored() {
        let mut b = contact(2, "伊藤", &["i@x.jp"], &[]);
        b.deleted_at = Some("2026-10-01".into());
        assert!(sure_groups(
            &[contact(1, "伊藤", &["i@x.jp"], &[]), b],
            &DistinctPairs::default()
        )
        .is_empty());
    }
}
