//! Google アカウント 1 件ぶんの同期（「今すぐ同期」と自動同期の本体）。
//!
//! カレンダーと連絡先を 1 回でまとめて同期し、続けて未照合の連絡先を住所録へ反映する
//! （docs/CONTACTS_SYNC.md §2）。以前は「今すぐ同期」「連絡先を取り込む」「住所録へ反映」を
//! 別々に押していたが、分けている意味が無い（利用者の判断 2026-10-09）ので一本化した。
//!
//! - カレンダーと連絡先は独立に進める。片方が失敗しても、もう片方は続け、失敗は結果に添える
//! - 連絡先は push → pull → 照合の適用（[`Store::apply_contact_matches`]。規則は今までどおり:
//!   高確信は既存へつなぐ、それ以外は新規として起こし、迷ったものは重複整理の候補になる）
//! - 解除中のアカウントの扱い・アクセストークンの取得は呼び出し側（commands 層）

use super::calendar::sync as calendar_sync;
use super::contacts::sync as contacts_sync;
use super::SCOPE_CONTACTS;
use crate::models::{GoogleAccount, GoogleSyncResult};
use crate::services::store::Store;

/// 同期する範囲。
#[derive(Debug, Clone, Copy)]
pub struct SyncScope {
    /// 連絡先も同期するか（同期をオンにしているアカウントでは、自動同期も毎回 true で呼ぶ）。
    pub contacts: bool,
}

/// 連絡先を同期してよいアカウントか（有効にしていて、許可にも連絡先が含まれる）。
///
/// 許可が無いまま呼ぶと People API が 403 を返すので、先に外す（同意画面で連絡先だけ
/// 外されることがある）。
fn contacts_allowed(account: &GoogleAccount, scopes: Option<&str>) -> bool {
    account.sync_contacts && scopes.is_some_and(|g| g.split(' ').any(|s| s == SCOPE_CONTACTS))
}

/// アカウント 1 件を同期する。カレンダー（有効なら）→ 連絡先（範囲に含め、有効で許可が
/// あれば）の順。
///
/// 失敗は種類ごとに結果へ入れて返す（カレンダーが失敗しても連絡先は進める）。
pub async fn sync_account(
    store: &Store,
    access_token: &str,
    account: &GoogleAccount,
    scope: SyncScope,
) -> GoogleSyncResult {
    let account_id = i64::from(account.id);
    let mut out = GoogleSyncResult::default();

    if account.sync_calendar {
        match calendar_sync::sync_account(store, access_token, account_id).await {
            Ok(r) => out.calendar = Some(r),
            Err(e) => out.calendar_error = Some(e),
        }
    }

    let scopes = store.google_account_scopes(account_id).ok().flatten();
    if scope.contacts && contacts_allowed(account, scopes.as_deref()) {
        match contacts_sync::sync_account(store, access_token, account_id).await {
            Ok(r) => {
                out.contacts = Some(r);
                // 取り込んだ分を続けて住所録へ反映する。反映の失敗は取り込みの結果を消さない。
                match store.apply_contact_matches(account_id) {
                    Ok(m) => out.matched = Some(m),
                    Err(e) => {
                        out.contacts_error = Some(format!("住所録への反映に失敗しました: {e}"))
                    }
                }
            }
            Err(e) => out.contacts_error = Some(e.to_string()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(sync_contacts: bool) -> GoogleAccount {
        GoogleAccount {
            id: 1,
            email: "a@gmail.com".into(),
            sync_calendar: true,
            sync_contacts,
            push_new_contacts: false,
            last_calendar_sync_at: None,
            last_contacts_sync_at: None,
            disconnected_at: None,
            calendar_granted: true,
            contacts_granted: sync_contacts,
        }
    }

    #[test]
    fn contacts_need_both_the_flag_and_the_scope() {
        let both = format!("openid email {SCOPE_CONTACTS}");
        assert!(contacts_allowed(&account(true), Some(&both)));
        assert!(
            !contacts_allowed(&account(false), Some(&both)),
            "無効にしている"
        );
        assert!(
            !contacts_allowed(&account(true), Some("openid email")),
            "許可に連絡先が無い"
        );
        assert!(!contacts_allowed(&account(true), None), "許可が未記録");
    }
}
