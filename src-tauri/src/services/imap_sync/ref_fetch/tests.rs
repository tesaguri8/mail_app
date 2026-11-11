use super::*;

fn listed(name: &str, attrs: &[&str]) -> Listed {
    Listed {
        name: name.to_string(),
        attrs: attrs.iter().map(|a| a.to_string()).collect(),
    }
}

fn order(list: &[Listed]) -> Vec<(&str, &str)> {
    // 借用の都合で、並びを名前とタグの組にして比べる。
    let o = search_order(list);
    o.iter()
        .map(|(n, t)| {
            let name = list
                .iter()
                .find(|l| &l.name == n)
                .map(|l| l.name.as_str())
                .unwrap_or("");
            (name, *t)
        })
        .collect()
}

#[test]
fn gmail_searches_all_mail_first_then_sent_inbox_and_skips_the_rest() {
    let list = vec![
        listed("INBOX", &[]),
        listed("[Gmail]", &["\\Noselect"]),
        listed("[Gmail]/All Mail", &["\\All"]),
        listed("[Gmail]/Drafts", &["\\Drafts"]),
        listed("[Gmail]/Important", &["\\Important"]),
        listed("[Gmail]/Sent Mail", &["\\Sent"]),
        listed("[Gmail]/Spam", &["\\Junk"]),
        listed("[Gmail]/Starred", &["\\Flagged"]),
        listed("[Gmail]/Trash", &["\\Trash"]),
        listed("Work", &[]),
    ];
    assert_eq!(
        order(&list),
        [
            ("[Gmail]/All Mail", "archive"),
            ("[Gmail]/Sent Mail", "sent"),
            ("INBOX", "inbox"),
            ("Work", "archive"),
        ]
    );
}

#[test]
fn a_server_without_special_use_is_searched_sent_inbox_then_others() {
    // さくら等: 特殊用途の属性が無く、名前で決まる。迷惑メール・ゴミ箱・下書きは名前で外す。
    let list = vec![
        listed("INBOX", &[]),
        listed("INBOX.Archive", &[]),
        listed("INBOX.Drafts", &[]),
        listed("INBOX.Sent", &[]),
        listed("INBOX.Trash", &[]),
        listed("INBOX.迷惑メール", &[]),
    ];
    assert_eq!(
        order(&list),
        [
            ("INBOX.Sent", "sent"),
            ("INBOX", "inbox"),
            ("INBOX.Archive", "archive")
        ]
    );
}

#[test]
fn a_second_sent_like_folder_is_searched_as_archive() {
    // 同期が送信済みとして扱うのは 1 つだけ。もう 1 つを sent のタグで入れると、後の操作が
    // 別のフォルダの UID を指してしまうので、archive として入れる。
    let list = vec![
        listed("INBOX", &[]),
        listed("Sent", &["\\Sent"]),
        listed("Sent Messages", &[]),
    ];
    assert_eq!(
        order(&list),
        [
            ("Sent", "sent"),
            ("INBOX", "inbox"),
            ("Sent Messages", "archive")
        ]
    );
}

#[test]
fn the_search_query_ors_the_ids_and_unsafe_ids_are_not_searched() {
    assert_eq!(search_query(&[]), None);
    assert_eq!(
        search_query(&["a@x"]).as_deref(),
        Some("HEADER Message-ID \"<a@x>\"")
    );
    assert_eq!(
        search_query(&["a@x", "b@x", "c@x"]).as_deref(),
        Some(
            "OR HEADER Message-ID \"<a@x>\" OR HEADER Message-ID \"<b@x>\" HEADER Message-ID \"<c@x>\""
        )
    );
    assert!(searchable("CAF=abc.123@mail.example.invalid"));
    assert!(!searchable("a\"b@x"));
    assert!(!searchable("a\\b@x"));
    assert!(!searchable("a b@x"));
    assert!(!searchable("あ@x"));
    assert!(!searchable(""));
}

#[test]
fn a_second_search_for_the_same_thread_waits_until_the_first_ends() {
    let first = InFlight::try_begin(424_242).expect("最初は始められる");
    assert!(InFlight::try_begin(424_242).is_none());
    // 別の会話は止めない。
    assert!(InFlight::try_begin(424_243).is_some());
    drop(first);
    assert!(InFlight::try_begin(424_242).is_some());
}
