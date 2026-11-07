//! 送った直後に、送信済み（Sent）だけを同期して、自分の返信・転送を開いている会話へ出す
//! （docs/SYNC.md §5-3）。
//!
//! 送信は SMTP の受理で完了とし、控えの保存（APPEND）と取り込みは裏で行う。ここは「いつ・どれだけ
//! 待ってから Sent を取りに行くか」の決めを持つ。同期そのものは既存の仕組み（アカウントごとの排他・
//! 接続の使い回し・"sync:status"）にそのまま乗せる（commands.rs の `refresh_sent_after_send`）。

use std::time::Duration;

/// サーバーが送信時に控えを自分で保存する（Gmail など。APPEND しない）ときの待ち。
/// 保存が SMTP の受理より少し遅れることがあり、すぐに取りに行くと間に合わない。
pub const SERVER_SAVE_WAIT: Duration = Duration::from_secs(4);

/// 同じアカウントの同期が走っているとき、終わるのを待つ上限。これを過ぎたら諦める
/// （走っていた同期か、次の定期同期が Sent も取り込む）。
pub const BUSY_WAIT_MAX: Duration = Duration::from_secs(60);

/// 同期が終わったかを見る間隔。
pub const BUSY_POLL: Duration = Duration::from_millis(500);

/// Sent を取りに行く前の待ち。自分で APPEND したなら、APPEND の完了を待ってから呼ぶので待たない。
pub fn wait_before_sync(server_saves_copy: bool) -> Duration {
    if server_saves_copy {
        SERVER_SAVE_WAIT
    } else {
        Duration::ZERO
    }
}

/// `busy` が false になるまで `poll` ごとに見て待つ。`max` 以内に空けば true、空かなければ false。
/// 同じアカウントの同期を重ねない（さくらなどは続けてログインすると遮断されるため、別の接続を張らない）。
pub async fn wait_until_free(busy: impl Fn() -> bool, max: Duration, poll: Duration) -> bool {
    let mut waited = Duration::ZERO;
    while busy() {
        if waited >= max {
            return false;
        }
        tokio::time::sleep(poll).await;
        waited += poll;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// 自分で APPEND したら待たない。サーバーが保存する側（Gmail など）は少し待つ。
    #[test]
    fn waits_only_when_the_server_saves_the_copy() {
        assert_eq!(wait_before_sync(false), Duration::ZERO);
        assert_eq!(wait_before_sync(true), SERVER_SAVE_WAIT);
    }

    /// 走っている同期が終われば、その時点で進む。
    #[test]
    fn proceeds_once_the_running_sync_ends() {
        let polls = Cell::new(0);
        let busy = || {
            polls.set(polls.get() + 1);
            polls.get() <= 3
        };
        let free = tauri::async_runtime::block_on(wait_until_free(
            busy,
            Duration::from_millis(50),
            Duration::from_millis(1),
        ));
        assert!(free);
        assert_eq!(polls.get(), 4);
    }

    /// 上限まで空かなければ諦める（同期を重ねない）。
    #[test]
    fn gives_up_when_the_sync_keeps_running() {
        let free = tauri::async_runtime::block_on(wait_until_free(
            || true,
            Duration::from_millis(5),
            Duration::from_millis(1),
        ));
        assert!(!free);
    }

    /// 空いていれば待たずに進む。
    #[test]
    fn proceeds_at_once_when_idle() {
        let free = tauri::async_runtime::block_on(wait_until_free(
            || false,
            Duration::ZERO,
            Duration::from_secs(1),
        ));
        assert!(free);
    }
}
