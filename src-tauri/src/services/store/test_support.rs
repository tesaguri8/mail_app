//! store のテスト用の小道具（連絡先の入力を短く組み立てる）。

use crate::models::{ContactFields, ContactInput, ContactOrganization, ContactValue};

/// ラベル無し・共有でない値。
pub(crate) fn value(v: &str) -> ContactValue {
    ContactValue {
        label: None,
        value: v.into(),
        is_shared: false,
    }
}

/// 名前とメールだけの新規入力。
pub(crate) fn person(name: &str, emails: &[&str]) -> ContactInput {
    ContactInput {
        id: None,
        fields: ContactFields {
            display_name: name.into(),
            emails: emails.iter().map(|e| value(e)).collect(),
            ..Default::default()
        },
    }
}

/// 名前と会社名だけの新規入力。
pub(crate) fn employee(name: &str, org: &str) -> ContactInput {
    let mut input = person(name, &[]);
    input.fields.organizations = vec![ContactOrganization {
        name: Some(org.into()),
        ..Default::default()
    }];
    input
}
