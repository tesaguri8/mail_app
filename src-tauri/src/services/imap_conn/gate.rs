//! IMAP サーバーへの接続の門（ホスト単位の失敗の後退・接続の直列化・回数の記録）。
//!
//! **きっかけ（2026-11-11 vaio）**: さくらのサーバーが vaio の回線の IP を、メールのポートだけ
//! 一時的に制限した。制限が解けたそばから、Rondine の自動同期が 30 秒ごとに（同じホストの
//! 2 アカウントぶん）張り直し、また制限がかかっていた疑いが強い。そこで、新しく接続を張るときは
//! すべてこの門を通す（[`super::connect_login_with`] と [`super::reach`]）。
//!
//! - **失敗の後退**: 接続・ログインの失敗（つながらない・時間切れ・ログインを断られた）が続いたら、
//!   そのホストへの自動の接続を 1 分 → 2 分 → 4 分 … と 2 倍ずつ空ける（上限 30 分）。
//!   成功したら戻す。同じホスト（とポート）のアカウントは、この状態を共有する。
//! - **手動はすぐ 1 回**: 手動の同期・接続の確認など（[`Urgency::Manual`]）は、後退中でも 1 回だけ試す。
//!   続けて押されたら [`GateConfig::manual_gap`] を空ける。
//! - **直列化**: 同じホストへ同時に張る（接続〜ログイン）のは 1 本まで。続けて張るときも
//!   [`GateConfig::spacing`] を空ける。張ったあとの使用中・待機中の接続は数えない
//!   （同期の接続はアカウントごとに使い回すので、張る回数そのものが少ない）。
//! - **記録**: 張るたびに、ホストごとの結果と直近 1 時間の回数を INFO でログに残す。

use std::collections::{HashMap, VecDeque};
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use super::ImapError;

/// 接続を急ぐ度合い。後退中に通すかどうかが変わる。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Urgency {
    /// 自動（定期の同期・送った直後の取り込み・設定を開いたときの到達確認など）。後退中は張らない。
    Auto,
    /// 利用者が押した・開いた（手動の同期・接続の確認・本文や添付の取得・下書きの保存など）。
    /// 後退中でも 1 回は試す（続けて来たら間を空ける）。
    Manual,
}

/// 門の決まり（時間と本数）。本番は [`GateConfig::DEFAULT`]。
#[derive(Debug, Clone, Copy)]
pub struct GateConfig {
    /// 1 回目の失敗のあとに空ける時間。続けて失敗するたびに 2 倍にする。
    pub base: Duration,
    /// 空ける時間の上限。
    pub max: Duration,
    /// 後退中の手動の試行どうしの間隔（連打で叩き続けないため）。
    pub manual_gap: Duration,
    /// 同じホストへ続けて張るときの間隔。
    pub spacing: Duration,
    /// 先に張っている接続を待つ上限。過ぎたら [`ImapError::Busy`]。
    pub wait_limit: Duration,
}

impl GateConfig {
    /// 本番の決まり（1 分から倍々・上限 30 分・手動は 15 秒おき・続けて張るのは 1 秒おき）。
    pub const DEFAULT: Self = Self {
        base: Duration::from_secs(60),
        max: Duration::from_secs(30 * 60),
        manual_gap: Duration::from_secs(15),
        spacing: Duration::from_secs(1),
        // 接続の上限（20 秒）＋ログインまでの読み書きの上限（60 秒）より少し長く。
        wait_limit: Duration::from_secs(90),
    };
}

/// いまの時刻（試験で時計を進めるために差し替えられる）。
pub trait Clock: Send + Sync {
    /// いまの時刻。
    fn now(&self) -> Instant;
}

/// 本物の時計。
struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// 後退の様子（画面の「次の試行まで N 分」と、手前で断るときの文言に使う）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backoff {
    /// 次の自動の接続までの残り。
    pub retry_in: Duration,
    /// 続けて失敗した回数。
    pub failures: u32,
    /// 最後の失敗がログインを断られたものか（画面で「ログインできない」と分ける）。
    pub login_refused: bool,
    /// 最後の失敗の文言。
    pub last_error: String,
}

/// ホストごとの接続の回数（ログと試験の計測用）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostStats {
    /// 張ろうとした回数（門を通った回数）。
    pub attempts: u64,
    /// 成功した回数。
    pub succeeded: u64,
    /// 失敗した回数。
    pub failed: u64,
    /// 後退中で断った回数（張っていない）。
    pub refused: u64,
}

#[derive(Default)]
struct HostState {
    failures: u32,
    retry_at: Option<Instant>,
    login_refused: bool,
    last_error: String,
    last_manual: Option<Instant>,
    connecting: bool,
    last_start: Option<Instant>,
    /// 直近 1 時間に張ろうとした時刻。
    recent: VecDeque<Instant>,
    stats: HostStats,
}

const HOUR: Duration = Duration::from_secs(3600);

impl HostState {
    fn backoff(&self, now: Instant) -> Option<Backoff> {
        let at = self.retry_at.filter(|at| *at > now)?;
        Some(Backoff {
            retry_in: at - now,
            failures: self.failures,
            login_refused: self.login_refused,
            last_error: self.last_error.clone(),
        })
    }

    fn recent_count(&mut self, now: Instant) -> usize {
        while self
            .recent
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= HOUR)
        {
            self.recent.pop_front();
        }
        self.recent.len()
    }
}

/// ホスト単位の接続の門。アプリ全体で 1 つ（[`HostGate::global`]）。
pub struct HostGate {
    config: GateConfig,
    clock: Box<dyn Clock>,
    hosts: Mutex<HashMap<String, HostState>>,
    freed: Condvar,
}

/// 門の鍵（ホスト名は大小を区別しない。ポートが違えば別のサーバーとして扱う）。
fn key(host: &str, port: u16) -> String {
    format!("{}:{port}", host.trim().to_ascii_lowercase())
}

/// ログに出す時間の短い表記（「90 秒」「4 分」）。
fn human(d: Duration) -> String {
    let s = d.as_secs();
    if s < 120 {
        format!("{s} 秒")
    } else {
        format!("{} 分", s.div_ceil(60))
    }
}

impl HostGate {
    /// 決まりと時計を指定して作る（試験用。本番は [`HostGate::global`]）。
    pub fn new(config: GateConfig, clock: Box<dyn Clock>) -> Self {
        Self {
            config,
            clock,
            hosts: Mutex::new(HashMap::new()),
            freed: Condvar::new(),
        }
    }

    /// アプリ全体で共有する門（本物の時計・[`GateConfig::DEFAULT`]）。
    pub fn global() -> &'static HostGate {
        static GATE: OnceLock<HostGate> = OnceLock::new();
        GATE.get_or_init(|| HostGate::new(GateConfig::DEFAULT, Box::new(SystemClock)))
    }

    /// 毒されたロックも中身をそのまま使う（数と時刻だけなので、続けてよい）。
    fn lock(&self) -> MutexGuard<'_, HashMap<String, HostState>> {
        self.hosts.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// そのホストがいま後退中なら、その様子（後退していなければ None）。状態は変えない。
    pub fn backoff(&self, host: &str, port: u16) -> Option<Backoff> {
        let now = self.clock.now();
        self.lock().get(&key(host, port))?.backoff(now)
    }

    /// そのホストの接続の回数。
    pub fn stats(&self, host: &str, port: u16) -> HostStats {
        self.lock()
            .get(&key(host, port))
            .map(|s| s.stats)
            .unwrap_or_default()
    }

    /// 張ってよいかを確かめ、よければ順番を取る（同じホストで先に張っているものがあれば待つ）。
    /// 張り終えたら、返した [`Ticket`] の `finish` に結果を渡す。
    ///
    /// # Errors
    /// 後退中（自動・または手動の連打）なら [`ImapError::Backoff`]。先の接続を
    /// [`GateConfig::wait_limit`] 待っても空かなければ [`ImapError::Busy`]。
    pub fn enter(&self, host: &str, port: u16, urgency: Urgency) -> Result<Ticket<'_>, ImapError> {
        let k = key(host, port);
        let waited_since = Instant::now();
        let mut hosts = self.lock();
        loop {
            let now = self.clock.now();
            let state = hosts.entry(k.clone()).or_default();
            if let Some(b) = state.backoff(now) {
                let manual_ok = urgency == Urgency::Manual
                    && state.last_manual.map_or(true, |t| {
                        now.saturating_duration_since(t) >= self.config.manual_gap
                    });
                if !manual_ok {
                    state.stats.refused += 1;
                    return Err(ImapError::Backoff(b.retry_in));
                }
            }
            // 先に張っているものが終わるのを待つ。続けて張るときは間を空ける。
            let gap = state
                .last_start
                .map(|t| {
                    self.config
                        .spacing
                        .saturating_sub(now.saturating_duration_since(t))
                })
                .unwrap_or_default();
            if !state.connecting && gap.is_zero() {
                state.connecting = true;
                state.last_start = Some(now);
                if urgency == Urgency::Manual && state.retry_at.is_some_and(|at| at > now) {
                    state.last_manual = Some(now);
                }
                state.recent.push_back(now);
                state.stats.attempts += 1;
                return Ok(Ticket {
                    gate: self,
                    key: k,
                    urgency,
                    started: Instant::now(),
                    open: true,
                });
            }
            let left = self
                .config
                .wait_limit
                .saturating_sub(waited_since.elapsed());
            if left.is_zero() {
                return Err(ImapError::Busy);
            }
            let wait = if state.connecting {
                left
            } else {
                gap.min(left)
            };
            hosts = self
                .freed
                .wait_timeout(hosts, wait)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// 張り終えた（または諦めた）。`outcome` が None なら結果を記録せず順番だけ返す。
    fn leave(&self, ticket: &Ticket<'_>, outcome: Option<Outcome<'_>>) {
        let now = self.clock.now();
        let mut hosts = self.lock();
        let state = hosts.entry(ticket.key.clone()).or_default();
        state.connecting = false;
        let elapsed = ticket.started.elapsed().as_millis();
        let urgency = ticket.urgency;
        match outcome {
            None => {}
            Some(Outcome::Ok) => {
                state.stats.succeeded += 1;
                let recovered = state.failures > 0;
                state.failures = 0;
                state.retry_at = None;
                state.login_refused = false;
                state.last_error.clear();
                let recent = state.recent_count(now);
                log::info!(
                    "IMAP 接続 {}: 成功（{urgency:?}・{elapsed}ms）{}直近 1 時間 {recent} 回",
                    ticket.key,
                    if recovered { "後退を解除・" } else { "" },
                );
            }
            Some(Outcome::Reached) => {
                // TCP だけの到達確認の成功。ログインできるかは分からないので、後退は解かない。
                state.stats.succeeded += 1;
                let recent = state.recent_count(now);
                log::info!(
                    "IMAP 到達確認 {}: つながる（{elapsed}ms）直近 1 時間 {recent} 回",
                    ticket.key
                );
            }
            Some(Outcome::Failed(e)) => {
                state.stats.failed += 1;
                state.failures = state.failures.saturating_add(1);
                let factor = 1u32
                    .checked_shl(state.failures.saturating_sub(1).min(16))
                    .unwrap_or(u32::MAX);
                let delay = self
                    .config
                    .base
                    .checked_mul(factor)
                    .unwrap_or(self.config.max)
                    .min(self.config.max);
                state.retry_at = Some(now + delay);
                state.login_refused = matches!(e, ImapError::Login(_));
                state.last_error = e.to_string().chars().take(200).collect();
                let recent = state.recent_count(now);
                log::info!(
                    "IMAP 接続 {}: 失敗（{urgency:?}・{elapsed}ms）連続 {} 回 → 自動の接続は {} 空ける・直近 1 時間 {recent} 回: {}",
                    ticket.key,
                    state.failures,
                    human(delay),
                    state.last_error
                );
            }
        }
        drop(hosts);
        self.freed.notify_all();
    }
}

/// 張った結果。
enum Outcome<'a> {
    /// 接続・ログインできた。
    Ok,
    /// TCP でつながった（ログインはしていない）。
    Reached,
    /// 失敗した。
    Failed(&'a ImapError),
}

/// 門の順番。張り終えたら [`Ticket::finish`] に結果を渡す（渡さずに落とすと順番だけ返す）。
pub struct Ticket<'a> {
    gate: &'a HostGate,
    key: String,
    urgency: Urgency,
    started: Instant,
    open: bool,
}

impl Ticket<'_> {
    /// 接続・ログインの結果を記録して、順番を返す。
    pub fn finish<T>(mut self, result: &Result<T, ImapError>) {
        self.open = false;
        let outcome = match result {
            Ok(_) => Outcome::Ok,
            Err(e) => Outcome::Failed(e),
        };
        self.gate.leave(&self, Some(outcome));
    }

    /// TCP だけの到達確認の結果を記録して、順番を返す（成功しても後退は解かない）。
    pub fn finish_reach(mut self, result: &Result<(), ImapError>) {
        self.open = false;
        let outcome = match result {
            Ok(()) => Outcome::Reached,
            Err(e) => Outcome::Failed(e),
        };
        self.gate.leave(&self, Some(outcome));
    }
}

impl Drop for Ticket<'_> {
    fn drop(&mut self) {
        if self.open {
            self.gate.leave(self, None);
        }
    }
}

#[cfg(test)]
mod tests;
