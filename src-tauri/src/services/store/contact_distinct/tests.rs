use super::*;
use crate::services::store::test_support::person;

fn store() -> Store {
    Store::open_in_memory_for_test()
}

fn add(s: &Store, name: &str, email: &str) -> i64 {
    s.upsert_contact(&person(name, &[email])).unwrap().id as i64
}

fn uid(s: &Store, id: i64) -> String {
    s.get_contact(id).unwrap().uid
}

/// 組の中の連絡先 ID（並びはそろえる）。
fn groups(s: &Store) -> Vec<Vec<i64>> {
    let mut out: Vec<Vec<i64>> = s
        .find_duplicate_groups()
        .unwrap()
        .into_iter()
        .map(|g| {
            let mut ids: Vec<i64> = g.contacts.iter().map(|c| i64::from(c.id)).collect();
            ids.sort();
            ids
        })
        .collect();
    out.sort();
    out
}

/// uid はどの作成経路でも振られ、UUID v4 の形（小文字・ハイフン区切り・版 4・変種 8〜b）で、
/// 人ごとに違う。
#[test]
fn every_new_contact_gets_a_v4_uid() {
    let s = store();
    let a = uid(&s, add(&s, "山田", "y@x.jp"));
    let b = uid(&s, add(&s, "田中", "t@x.jp"));
    // 直書き（取り込み等の別経路の代わり）でも振られる。
    s.conn
        .lock()
        .unwrap()
        .execute("INSERT INTO contacts (display_name) VALUES ('直書き')", [])
        .unwrap();
    let c: String = s
        .conn
        .lock()
        .unwrap()
        .query_row(
            "SELECT uid FROM contacts WHERE display_name = '直書き'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    for u in [&a, &b, &c] {
        let parts: Vec<&str> = u.split('-').collect();
        assert_eq!(
            parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
            vec![8, 4, 4, 4, 12],
            "{u}"
        );
        assert!(u
            .chars()
            .all(|ch| ch == '-' || ch.is_ascii_digit() || ('a'..='f').contains(&ch)));
        assert!(parts[2].starts_with('4'), "版 4: {u}");
        assert!("89ab".contains(&parts[3][..1]), "変種: {u}");
    }
    assert!(a != b && b != c && a != c);
}

/// 「別人」を押した組は、開き直しても出ない。取り消すとまた出る。
#[test]
fn marked_distinct_groups_stay_away_until_undone() {
    let s = store();
    let a = add(&s, "田中太郎", "taro@x.jp");
    let b = add(&s, "田中太郎", "taro@x.jp");
    assert_eq!(groups(&s), vec![vec![a, b]]);

    assert_eq!(s.mark_contacts_distinct(&[a, b]).unwrap(), 1);
    assert!(groups(&s).is_empty());
    assert_eq!(s.distinct_pairs().unwrap().len(), 1);

    assert_eq!(
        s.unmark_contacts_distinct(b, a).unwrap(),
        1,
        "向きは問わない"
    );
    assert_eq!(groups(&s), vec![vec![a, b]]);
}

/// 3 人の組で 1 人だけ別人なら、残りで組を作り直す。
#[test]
fn a_partly_distinct_group_is_rebuilt_from_the_rest() {
    let s = store();
    let a = add(&s, "佐藤一", "s@x.jp");
    let b = add(&s, "佐藤一", "s@x.jp");
    let c = add(&s, "佐藤一", "s@x.jp");
    s.mark_contacts_distinct(&[a, c]).unwrap();
    s.mark_contacts_distinct(&[b, c]).unwrap();
    assert_eq!(groups(&s), vec![vec![a, b]]);
}

/// 統合でチェックを外した人は、統合後の 1 人と別人になり、統合した組は戻ってこない。
#[test]
fn excluded_members_do_not_come_back_after_a_merge() {
    let s = store();
    let a = add(&s, "鈴木花子", "h@x.jp");
    let b = add(&s, "鈴木花子", "h@x.jp");
    let c = add(&s, "鈴木花子", "h@x.jp");
    let keep_uid = uid(&s, a);
    s.merge_contacts(a, &[b], &[c]).unwrap();
    assert!(groups(&s).is_empty(), "残った 2 人の組は出ない");
    assert_eq!(uid(&s, a), keep_uid, "残る側の uid を使う");
    // まとめての統合も従う（名前・メール・電話が同じでも別人ならまとめない）。
    assert!(s.sure_merge_preview().unwrap().groups.is_empty());
}

/// 統合で消える人の「別人」の記録は残る人へ付け替える。消えた人の記録は残らない。
#[test]
fn merging_carries_distinct_marks_to_the_survivor() {
    let s = store();
    let keep = add(&s, "高橋", "k@x.jp");
    let gone = add(&s, "高橋", "k@x.jp");
    let other = add(&s, "高橋", "k@x.jp");
    s.mark_contacts_distinct(&[gone, other]).unwrap();
    s.merge_contacts(keep, &[gone], &[]).unwrap();
    let pairs = s.distinct_pairs().unwrap();
    assert_eq!(pairs.len(), 1);
    let ids = [i64::from(pairs[0].a_id), i64::from(pairs[0].b_id)];
    assert!(ids.contains(&keep) && ids.contains(&other));
    assert!(groups(&s).is_empty());
}

/// 連絡先を消したら（ゴミ箱を空にする等）対も片付く。
#[test]
fn deleting_a_contact_removes_its_pairs() {
    let s = store();
    let a = add(&s, "伊藤", "i@x.jp");
    let b = add(&s, "伊藤", "i@x.jp");
    s.mark_contacts_distinct(&[a, b]).unwrap();
    let conn = s.conn.lock().unwrap();
    conn.execute("DELETE FROM contacts WHERE id = ?1", [b])
        .unwrap();
    let left: i64 = conn
        .query_row("SELECT count(*) FROM contact_distinct_pairs", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(left, 0);
}
