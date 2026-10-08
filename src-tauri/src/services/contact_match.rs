//! 照合フェーズ: 取り込んだ外部連絡先を、住所録の誰と結び付けるかを決める。
//!
//! Google 連絡先の取り込み（`services::google::contacts`）は `contact_identities`（台帳）
//! 止まりで、住所録には現れない。初回同期では Google 側と住所録に同じ人が別 ID で二重に
//! 存在するため、そのまま入れると住所録が丸ごと二重になるからである（docs/CONTACTS_SYNC.md）。
//!
//! ここは**判定だけ**を担い、DB には触れない（保存は `store::contact_sync`、取得は同期エンジン）。
//! 判定の物差しは重複検出（`services::dedupe`）と**同じ**ものを使う。同じ規則で見ているので、
//! ここで決めきれずに新規として起こした連絡先は、既存の「重複整理」がそのまま拾ってくれる。
//!
//! 方針は安全側に倒す:
//!
//! - 高確信（携帯／メールの一致＋氏名一致）の候補が**ただ 1 件**のときだけ自動で紐付ける
//! - 候補が複数あるとき・確信が弱いときは**紐付けず新規として起こし**、人の判断（重複整理）に回す
//! - 1 人のローカル連絡先を 2 つの外部 ID が掴まない（送信フェーズで宛先が定まらなくなるため）

use crate::models::{ContactFields, ContactSummary};
use crate::services::dedupe::{compare, Confidence, Rec};
use std::collections::{HashMap, HashSet};

/// 台帳 1 件をどう扱うか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchDecision {
    /// 既存の連絡先に紐付ける（高確信の候補がただ 1 件）。
    Link(i64),
    /// 新規として住所録に起こす。
    Create,
}

/// 台帳 1 件ぶんの照合結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchOutcome {
    /// 台帳の外部 ID（People API の resourceName）。
    pub external_id: String,
    pub decision: MatchDecision,
    /// 自動で決めきれなかった似た相手のローカル ID。`Create` のときだけ入る。
    /// 起こしたあとに重複整理へ出る見込みの件数を数えるために持つ。
    pub rivals: Vec<i64>,
}

/// 未照合の台帳を住所録と突き合わせ、1 件ずつの扱いを決める。
///
/// 戻り値は `remote` と**同じ順・同じ数**（呼び出し側が位置で対応付けられる）。
///
/// - `remote`: 台帳の（外部 ID, 取り込んだ内容）。決定は与えられた順に確定する
/// - `locals`: 住所録の連絡先（共有値を除いた比較材料つき）
/// - `already_linked`: すでに別の外部 ID が掴んでいるローカル ID（紐付け先から外す）
pub fn plan(
    remote: &[(String, ContactFields)],
    locals: &[ContactSummary],
    already_linked: &HashSet<i64>,
) -> Vec<MatchOutcome> {
    let local_recs: Vec<Rec> = locals.iter().map(|c| Rec::from_fields(&c.fields)).collect();

    // ブロッキング: 同じキー（メール・携帯・氏名）を持つ相手だけを比較候補にする。
    let mut buckets: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, r) in local_recs.iter().enumerate() {
        for key in r.blocking_keys() {
            buckets.entry(key).or_default().push(i);
        }
    }

    // この実行で掴んだローカル ID（2 つの外部 ID が同じ人を掴まないようにする）。
    let mut claimed: HashSet<i64> = already_linked.clone();

    remote
        .iter()
        .map(|(external_id, contact)| {
            let rec = Rec::from_fields(contact);

            // 候補の添字を集める（同じ相手が複数キーで挙がるので重複排除）。
            let mut seen: HashSet<usize> = HashSet::new();
            let candidates: Vec<(usize, Confidence)> = rec
                .blocking_keys()
                .filter_map(|key| buckets.get(key))
                .flatten()
                .copied()
                .filter(|i| seen.insert(*i))
                .filter_map(|i| compare(&rec, &local_recs[i]).map(|c| (i, c)))
                .collect();

            // 高確信がただ 1 件で、まだ誰にも掴まれていないときだけ自動で紐付ける。
            let mut high = candidates
                .iter()
                .filter(|(_, c)| *c == Confidence::High)
                .map(|(i, _)| locals[*i].id as i64);
            let sole_high = match (high.next(), high.next()) {
                (Some(id), None) if !claimed.contains(&id) => Some(id),
                _ => None,
            };

            match sole_high {
                Some(id) => {
                    claimed.insert(id);
                    MatchOutcome {
                        external_id: external_id.clone(),
                        decision: MatchDecision::Link(id),
                        rivals: Vec::new(),
                    }
                }
                None => MatchOutcome {
                    external_id: external_id.clone(),
                    decision: MatchDecision::Create,
                    rivals: candidates.iter().map(|(i, _)| locals[*i].id as i64).collect(),
                },
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value(v: &str) -> crate::models::ContactValue {
        crate::models::ContactValue {
            label: None,
            value: v.into(),
            is_shared: false,
        }
    }

    fn fields(name: &str, email: Option<&str>, phone: Option<&str>) -> ContactFields {
        ContactFields {
            display_name: name.into(),
            emails: email.map(value).into_iter().collect(),
            phones: phone.map(value).into_iter().collect(),
            ..Default::default()
        }
    }

    fn local(id: i32, name: &str, email: Option<&str>, phone: Option<&str>) -> ContactSummary {
        ContactSummary {
            id,
            fields: fields(name, email, phone),
            ..Default::default()
        }
    }

    fn remote(
        id: &str,
        name: &str,
        email: Option<&str>,
        phone: Option<&str>,
    ) -> (String, ContactFields) {
        (id.into(), fields(name, email, phone))
    }

    #[test]
    fn sole_high_confidence_candidate_is_linked() {
        // メール＋氏名の一致は High。候補が 1 件だけなら自動で紐付ける。
        let locals = vec![local(7, "末松 信吾", Some("s@x.jp"), None)];
        let remote = vec![remote("people/c1", "末松信吾", Some("S@x.jp"), None)];
        let got = plan(&remote, &locals, &HashSet::new());
        assert_eq!(got[0].decision, MatchDecision::Link(7));
        assert!(got[0].rivals.is_empty());
    }

    #[test]
    fn no_candidate_is_created_as_new() {
        let locals = vec![local(7, "末松 信吾", Some("s@x.jp"), None)];
        let remote = vec![remote("people/c1", "山田太郎", Some("t@y.jp"), None)];
        let got = plan(&remote, &locals, &HashSet::new());
        assert_eq!(got[0].decision, MatchDecision::Create);
        assert!(got[0].rivals.is_empty(), "似た相手が居ないなら要確認にしない");
    }

    #[test]
    fn several_high_candidates_are_left_to_the_human() {
        // 同姓同名＋同じメールが 2 件。どちらへ寄せるかは機械には決められない。
        let locals = vec![
            local(1, "末松信吾", Some("s@x.jp"), None),
            local(2, "末松信吾", Some("s@x.jp"), None),
        ];
        let remote = vec![remote("people/c1", "末松信吾", Some("s@x.jp"), None)];
        let got = plan(&remote, &locals, &HashSet::new());
        assert_eq!(got[0].decision, MatchDecision::Create);
        assert_eq!(got[0].rivals, vec![1, 2], "重複整理へ回す候補として残す");
    }

    #[test]
    fn weak_candidate_is_created_but_flagged() {
        // 同名だけ（同姓同名の別人があり得る）＝ Low。紐付けず、要確認として起こす。
        let locals = vec![local(1, "山田太郎", Some("a@x.jp"), None)];
        let remote = vec![remote("people/c1", "山田太郎", Some("b@y.jp"), None)];
        let got = plan(&remote, &locals, &HashSet::new());
        assert_eq!(got[0].decision, MatchDecision::Create);
        assert_eq!(got[0].rivals, vec![1]);
    }

    #[test]
    fn a_local_contact_is_not_claimed_twice() {
        // Google 側に同じ人の重複が 2 件あっても、ローカルの 1 人を掴むのは先の 1 件だけ。
        let locals = vec![local(7, "末松信吾", Some("s@x.jp"), None)];
        let remote = vec![
            remote("people/c1", "末松信吾", Some("s@x.jp"), None),
            remote("people/c2", "末松信吾", Some("s@x.jp"), None),
        ];
        let got = plan(&remote, &locals, &HashSet::new());
        assert_eq!(got[0].decision, MatchDecision::Link(7));
        assert_eq!(got[1].decision, MatchDecision::Create);
        assert_eq!(got[1].rivals, vec![7]);
    }

    #[test]
    fn an_already_linked_local_contact_is_left_alone() {
        // すでに別の外部 ID が掴んでいる相手には寄せない（送信時に宛先が定まらなくなる）。
        let locals = vec![local(7, "末松信吾", Some("s@x.jp"), None)];
        let remote = vec![remote("people/c2", "末松信吾", Some("s@x.jp"), None)];
        let got = plan(&remote, &locals, &HashSet::from([7]));
        assert_eq!(got[0].decision, MatchDecision::Create);
    }

    #[test]
    fn mobile_and_name_match_links_across_formatting() {
        // 電話の書式ゆれ・氏名の空白ゆれは正規化して同じとみなす（判定は dedupe と同じ物差し）。
        let locals = vec![local(3, "末松 信吾", None, Some("090-1111-2222"))];
        let remote = vec![remote("people/c1", "末松信吾", None, Some("+81 90 1111 2222"))];
        let got = plan(&remote, &locals, &HashSet::new());
        assert_eq!(got[0].decision, MatchDecision::Link(3));
    }
}
