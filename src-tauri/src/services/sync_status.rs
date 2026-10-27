//! アカウントごとのメールの同期の最後の結果（つながった／失敗した＋失敗の種類＋時刻）を持つ。
//!
//! アカウント選択の接続状況の印に使う（"sync:status" イベントで画面に知らせる）。アプリの中だけで
//! 持ち、再起動すると「まだ同期していない」に戻る（再起動の直後は同期がすぐ走るので、DB には残さない）。
//! 同期の失敗はここでログにも書く（それまで画面に返すだけで、ログに何も残っていなかった。2026-10-28 vaio）。

use std::collections::HashMap;
use std::sync::RwLock;

use crate::models::{AccountSyncStatus, SyncErrorKind, SyncPhase};
use crate::services::imap_conn::ImapError;
use crate::services::imap_sync::SyncError;

/// 同期の失敗を、画面で分ける種類にする。
pub fn classify(e: &SyncError) -> SyncErrorKind {
    match e {
        SyncError::Connect(e) => classify_connect(e),
        SyncError::Other(_) => SyncErrorKind::Other,
    }
}

/// 接続・ログインの失敗を、画面で分ける種類にする。
pub fn classify_connect(e: &ImapError) -> SyncErrorKind {
    match e {
        ImapError::Timeout | ImapError::Resolve(_) | ImapError::Connect(_) => {
            SyncErrorKind::Unreachable
        }
        ImapError::Login(_) => SyncErrorKind::Auth,
        ImapError::Tls(_) | ImapError::Imap(_) | ImapError::Other(_) => SyncErrorKind::Other,
    }
}

/// ログ・ツールチップに出す文言の上限（文字数）。長いサーバーの応答で埋めない。
const MESSAGE_CHARS: usize = 200;

/// アカウントごとの同期の最後の結果。読み（画面からの問い合わせ）が多いので RwLock。
#[derive(Default)]
pub struct SyncStatusBoard(RwLock<HashMap<i64, AccountSyncStatus>>);

impl SyncStatusBoard {
    /// 書き換えて、書き換えたあとの状態を返す。毒されたロックは中身をそのまま使う
    /// （状態の表示だけなので、途中で落ちた書き込みがあっても続けてよい）。
    fn update(&self, account_id: i64, f: impl FnOnce(&mut AccountSyncStatus)) -> AccountSyncStatus {
        let mut map = self
            .0
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let status = map.entry(account_id).or_insert_with(|| AccountSyncStatus {
            account_id: i32::try_from(account_id).unwrap_or(i32::MAX),
            phase: SyncPhase::Never,
            error_kind: None,
            error: None,
            last_success_at: None,
            last_attempt_at: None,
        });
        f(status);
        status.clone()
    }

    /// 同期を始めた。前の失敗の種類と最後の成功の時刻は残す（同期中でも、なぜ前に失敗したかを見せる）。
    pub fn begin(&self, account_id: i64, now: &str) -> AccountSyncStatus {
        self.update(account_id, |s| {
            s.phase = SyncPhase::Syncing;
            s.last_attempt_at = Some(now.to_string());
        })
    }

    /// 同期が成功した。
    pub fn succeed(&self, account_id: i64, now: &str) -> AccountSyncStatus {
        self.update(account_id, |s| {
            s.phase = SyncPhase::Ok;
            s.error_kind = None;
            s.error = None;
            s.last_success_at = Some(now.to_string());
            s.last_attempt_at = Some(now.to_string());
        })
    }

    /// 同期が失敗した。`host` はログに出す（アドレス・パスワードは出さない）。
    pub fn fail(
        &self,
        account_id: i64,
        host: &str,
        kind: SyncErrorKind,
        message: &str,
        now: &str,
    ) -> AccountSyncStatus {
        let message: String = message.chars().take(MESSAGE_CHARS).collect();
        log::warn!("メールの同期に失敗（アカウント {account_id}・{host}・{kind:?}）: {message}");
        self.update(account_id, |s| {
            s.phase = SyncPhase::Failed;
            s.error_kind = Some(kind);
            s.error = Some(message);
            s.last_attempt_at = Some(now.to_string());
        })
    }

    /// 同期の結果を記録する（成功なら [`Self::succeed`]、失敗なら種類を分けて [`Self::fail`]）。
    pub fn finish<T>(
        &self,
        account_id: i64,
        host: &str,
        result: &Result<T, SyncError>,
        now: &str,
    ) -> AccountSyncStatus {
        match result {
            Ok(_) => self.succeed(account_id, now),
            Err(e) => self.fail(account_id, host, classify(e), &e.to_string(), now),
        }
    }

    /// 接続・ログインの確かめ（設定の接続の点のクリック。実際に LOGIN する）の結果を記録する。
    /// 成功は同期の成功と同じに扱う（つながってログインできた）。
    pub fn finish_check(
        &self,
        account_id: i64,
        host: &str,
        result: &Result<(), ImapError>,
        now: &str,
    ) -> AccountSyncStatus {
        match result {
            Ok(()) => self.succeed(account_id, now),
            Err(e) => self.fail(account_id, host, classify_connect(e), &e.to_string(), now),
        }
    }

    /// 軽い到達確認（TCP だけ。設定を初めて開いたとき）の結果を記録する。同期・ログインの結果が
    /// まだ無いアカウントだけ（実際の結果を、軽い確認で上書きしない）。記録しなければ None。
    /// つながったときは「つながる（まだ同期していない）」で、最後の成功の時刻は入れない。
    pub fn record_reach(
        &self,
        account_id: i64,
        host: &str,
        result: &Result<(), ImapError>,
        now: &str,
    ) -> Option<AccountSyncStatus> {
        let known = self
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(&account_id)
            .is_some_and(|s| s.phase != SyncPhase::Never);
        if known {
            return None;
        }
        Some(match result {
            Ok(()) => self.update(account_id, |s| {
                s.phase = SyncPhase::Ok;
                s.error_kind = None;
                s.error = None;
                s.last_attempt_at = Some(now.to_string());
            }),
            Err(e) => self.fail(account_id, host, classify_connect(e), &e.to_string(), now),
        })
    }

    /// すべてのアカウントの状態（アカウント id の順）。
    pub fn all(&self) -> Vec<AccountSyncStatus> {
        let map = self
            .0
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut out: Vec<AccountSyncStatus> = map.values().cloned().collect();
        out.sort_by_key(|s| s.account_id);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_connect_login_and_other_failures() {
        let refused = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "refused");
        assert_eq!(
            classify(&SyncError::Connect(ImapError::Timeout)),
            SyncErrorKind::Unreachable
        );
        assert_eq!(
            classify(&SyncError::Connect(ImapError::Resolve(
                "no such host".into()
            ))),
            SyncErrorKind::Unreachable
        );
        assert_eq!(
            classify(&SyncError::Connect(ImapError::Connect(refused))),
            SyncErrorKind::Unreachable
        );
        assert_eq!(
            classify(&SyncError::Connect(ImapError::Login(imap::Error::No(
                "[AUTHENTICATIONFAILED] Invalid credentials".into()
            )))),
            SyncErrorKind::Auth
        );
        assert_eq!(
            classify(&SyncError::Connect(ImapError::Tls(
                "bad certificate".into()
            ))),
            SyncErrorKind::Other
        );
        assert_eq!(
            classify(&SyncError::Other("database is locked".into())),
            SyncErrorKind::Other
        );
    }

    /// まだ同期していない → 同期中 → 失敗（前の成功の時刻は残る） → 同期中 → 成功（失敗の種類は消える）。
    #[test]
    fn tracks_phase_transitions() {
        let board = SyncStatusBoard::default();
        assert!(board.all().is_empty());
        let s = board.begin(1, "2026-10-28T02:00:00+09:00");
        assert_eq!(s.phase, SyncPhase::Syncing);
        let ok: Result<(), SyncError> = Ok(());
        let s = board.finish(1, "imap.invalid", &ok, "2026-10-28T02:23:00+09:00");
        assert_eq!(
            (s.phase, s.last_success_at.as_deref()),
            (SyncPhase::Ok, Some("2026-10-28T02:23:00+09:00"))
        );
        board.begin(1, "2026-10-28T02:30:00+09:00");
        let err: Result<(), SyncError> = Err(SyncError::Connect(ImapError::Timeout));
        let s = board.finish(1, "imap.invalid", &err, "2026-10-28T02:30:20+09:00");
        assert_eq!(s.phase, SyncPhase::Failed);
        assert_eq!(s.error_kind, Some(SyncErrorKind::Unreachable));
        assert_eq!(s.error.as_deref(), Some("サーバーが応答しませんでした"));
        assert_eq!(
            s.last_success_at.as_deref(),
            Some("2026-10-28T02:23:00+09:00")
        );
        // 同期中も、前の失敗の種類は残す。
        let s = board.begin(1, "2026-10-28T02:35:00+09:00");
        assert_eq!(
            (s.phase, s.error_kind),
            (SyncPhase::Syncing, Some(SyncErrorKind::Unreachable))
        );
        let s = board.succeed(1, "2026-10-28T02:35:10+09:00");
        assert_eq!(
            (s.phase, s.error_kind, s.error),
            (SyncPhase::Ok, None, None)
        );
        // ほかのアカウントとは混ざらない。
        board.begin(2, "2026-10-28T02:36:00+09:00");
        let phases: Vec<(i32, SyncPhase)> = board
            .all()
            .iter()
            .map(|s| (s.account_id, s.phase))
            .collect();
        assert_eq!(phases, [(1, SyncPhase::Ok), (2, SyncPhase::Syncing)]);
    }

    /// 軽い到達確認は、同期・ログインの結果がまだ無いときだけ記録する（実際の結果を上書きしない）。
    #[test]
    fn reach_is_recorded_only_without_a_real_result() {
        let board = SyncStatusBoard::default();
        let s = board
            .record_reach(1, "imap.invalid", &Ok(()), "now")
            .unwrap();
        assert_eq!((s.phase, s.last_success_at), (SyncPhase::Ok, None));
        // 同期が失敗したあとの到達確認（つながった）は、失敗を上書きしない。
        let err: Result<(), SyncError> = Err(SyncError::Connect(ImapError::Timeout));
        board.finish(2, "imap.invalid", &err, "now");
        assert_eq!(
            board.record_reach(2, "imap.invalid", &Ok(()), "later"),
            None
        );
        assert_eq!(board.all()[1].phase, SyncPhase::Failed);
        // ログインの確かめは、同期と同じに記録する（成功で緑に戻る）。
        let s = board.finish_check(2, "imap.invalid", &Ok(()), "later");
        assert_eq!((s.phase, s.error_kind), (SyncPhase::Ok, None));
        let refused = imap::Error::No("[AUTHENTICATIONFAILED]".into());
        let s = board.finish_check(2, "imap.invalid", &Err(ImapError::Login(refused)), "later");
        assert_eq!(s.error_kind, Some(SyncErrorKind::Auth));
    }

    /// 長いサーバーの応答は切り詰める。
    #[test]
    fn truncates_long_messages() {
        let board = SyncStatusBoard::default();
        let long = "x".repeat(1000);
        let s = board.fail(1, "imap.invalid", SyncErrorKind::Other, &long, "now");
        assert_eq!(s.error.map(|e| e.chars().count()), Some(MESSAGE_CHARS));
    }
}
