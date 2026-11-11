use super::*;
use std::sync::Arc;

use super::super::ImapTimeouts;

/// 進めるまで止まっている時計。
#[derive(Clone)]
struct FakeClock(Arc<Mutex<Instant>>);

impl FakeClock {
    fn new() -> Self {
        Self(Arc::new(Mutex::new(Instant::now())))
    }
    fn advance(&self, d: Duration) {
        *self.0.lock().unwrap() += d;
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Instant {
        *self.0.lock().unwrap()
    }
}

/// 時計を進めて試す門（続けて張るときの間隔は 0。並びの試験は本物の時計で別に見る）。
fn gate() -> (HostGate, FakeClock) {
    let clock = FakeClock::new();
    let config = GateConfig {
        spacing: Duration::ZERO,
        ..GateConfig::DEFAULT
    };
    (HostGate::new(config, Box::new(clock.clone())), clock)
}

const HOST: &str = "imap.example.invalid";
const PORT: u16 = 993;

fn unreachable() -> Result<(), ImapError> {
    Err(ImapError::Connect(std::io::Error::from(
        std::io::ErrorKind::ConnectionRefused,
    )))
}

fn fail(g: &HostGate, urgency: Urgency) {
    g.enter(HOST, PORT, urgency)
        .expect("通るはず")
        .finish(&unreachable());
}

#[test]
fn failures_double_the_wait_up_to_thirty_minutes() {
    let (g, clock) = gate();
    let mut waits = Vec::new();
    for _ in 0..8 {
        fail(&g, Urgency::Auto);
        let b = g.backoff(HOST, PORT).expect("後退中");
        waits.push(b.retry_in.as_secs() / 60);
        // 待つ間の自動の接続は張らずに断る。
        clock.advance(b.retry_in - Duration::from_secs(1));
        assert!(matches!(
            g.enter(HOST, PORT, Urgency::Auto),
            Err(ImapError::Backoff(_))
        ));
        clock.advance(Duration::from_secs(1));
    }
    assert_eq!(waits, [1, 2, 4, 8, 16, 30, 30, 30]);
}

#[test]
fn a_success_clears_the_backoff() {
    let (g, clock) = gate();
    fail(&g, Urgency::Auto);
    fail(&g, Urgency::Manual);
    assert_eq!(g.backoff(HOST, PORT).map(|b| b.failures), Some(2));
    clock.advance(Duration::from_secs(3600));
    g.enter(HOST, PORT, Urgency::Auto)
        .unwrap()
        .finish(&Ok::<(), ImapError>(()));
    assert_eq!(g.backoff(HOST, PORT), None);
    // 次の失敗はまた 1 分から。
    fail(&g, Urgency::Auto);
    assert_eq!(g.backoff(HOST, PORT).unwrap().retry_in.as_secs(), 60);
}

#[test]
fn accounts_on_the_same_host_share_the_backoff() {
    let (g, _clock) = gate();
    fail(&g, Urgency::Auto);
    // 大小が違っても同じホスト。ポートが違えば別のサーバー。
    assert!(matches!(
        g.enter("IMAP.Example.Invalid", PORT, Urgency::Auto),
        Err(ImapError::Backoff(_))
    ));
    assert!(g.enter(HOST, 143, Urgency::Auto).is_ok());
    assert!(g.enter("imap.other.invalid", PORT, Urgency::Auto).is_ok());
}

#[test]
fn manual_tries_once_during_backoff_then_waits_between_presses() {
    let (g, clock) = gate();
    fail(&g, Urgency::Auto);
    // 後退中でも、手動は 1 回すぐ試せる。
    let t = g.enter(HOST, PORT, Urgency::Manual).expect("手動は通る");
    drop(t);
    // 続けて押しても、間を空けるまでは断る。
    clock.advance(Duration::from_secs(5));
    assert!(matches!(
        g.enter(HOST, PORT, Urgency::Manual),
        Err(ImapError::Backoff(_))
    ));
    clock.advance(GateConfig::DEFAULT.manual_gap);
    assert!(g.enter(HOST, PORT, Urgency::Manual).is_ok());
}

#[test]
fn a_login_refusal_is_remembered_for_the_status() {
    let (g, _clock) = gate();
    g.enter(HOST, PORT, Urgency::Auto)
        .unwrap()
        .finish(&Err::<(), _>(ImapError::Login(imap::Error::No(
            "[AUTHENTICATIONFAILED]".into(),
        ))));
    let b = g.backoff(HOST, PORT).unwrap();
    assert!(b.login_refused);
    assert!(b.last_error.contains("ログイン"), "{}", b.last_error);
}

#[test]
fn a_reach_success_does_not_clear_the_backoff() {
    let (g, clock) = gate();
    fail(&g, Urgency::Auto);
    clock.advance(Duration::from_secs(61));
    g.enter(HOST, PORT, Urgency::Auto)
        .unwrap()
        .finish_reach(&Ok(()));
    // 後退の時間は過ぎているが、失敗の連続は残る（次に失敗したら 2 分）。
    fail(&g, Urgency::Auto);
    assert_eq!(g.backoff(HOST, PORT).unwrap().retry_in.as_secs(), 120);
}

#[test]
fn connections_to_one_host_are_made_one_at_a_time() {
    // 本物の時計。2 本目は、1 本目が張り終えるまで待ち、さらに間隔を空けてから張る。
    let config = GateConfig {
        spacing: Duration::from_millis(100),
        ..GateConfig::DEFAULT
    };
    let g = Arc::new(HostGate::new(config, Box::new(SystemClock)));
    let first = g.enter(HOST, PORT, Urgency::Auto).unwrap();
    let started = Instant::now();
    let g2 = Arc::clone(&g);
    let second = std::thread::spawn(move || {
        let t = g2.enter(HOST, PORT, Urgency::Auto).map(|_| Instant::now());
        t.expect("順番が来る")
    });
    std::thread::sleep(Duration::from_millis(300));
    let released = Instant::now();
    first.finish(&Ok::<(), ImapError>(()));
    let entered = second.join().unwrap();
    assert!(entered >= released, "1 本目が終わる前に張った");
    assert!(entered - started >= Duration::from_millis(300));
    // 別のホストは待たない。
    let other = g.enter("imap.other.invalid", PORT, Urgency::Auto);
    assert!(other.is_ok());
}

#[test]
fn waiting_too_long_for_the_turn_gives_up() {
    let config = GateConfig {
        wait_limit: Duration::from_millis(150),
        ..GateConfig::DEFAULT
    };
    let g = HostGate::new(config, Box::new(SystemClock));
    let _first = g.enter(HOST, PORT, Urgency::Auto).unwrap();
    let started = Instant::now();
    assert!(matches!(
        g.enter(HOST, PORT, Urgency::Manual),
        Err(ImapError::Busy)
    ));
    assert!(started.elapsed() >= Duration::from_millis(150));
}

/// 1 時間の自動同期を、時計を進めて再現する（`hooks/useAutoSync.ts` の巡回: 30 秒おき・アカウントを
/// 順に・巡回中に来た依頼は終わり次第すぐ回し直す）。同じホストの 2 アカウントがつながらず、
/// 別のホストの 1 アカウントは成功する（全滅ではないので、画面側の 5 分の休みは効かなかった）。
/// つながらない接続は、本物の接続を閉じたポートへ張って失敗させ、時計は 20 秒（接続の上限）進める。
/// 戻り値は、つながらないホストへ張ろうとした回数。
fn simulate_hour(gated: bool) -> u64 {
    let (g, clock) = gate();
    let closed_port = {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let timeouts = ImapTimeouts {
        connect: Duration::from_secs(2),
        io: Duration::from_secs(2),
        total: None,
    };
    let start = clock.now();
    let mut attempts = 0u64;
    let elapsed = |c: &FakeClock| c.now() - start;
    let mut round_start = Duration::ZERO;
    while round_start < HOUR {
        for _account in 0..2 {
            let r = if gated {
                super::super::connect_login_via(
                    &g,
                    "127.0.0.1",
                    closed_port,
                    "u@example.invalid",
                    "x",
                    timeouts,
                    Urgency::Auto,
                )
                .map(|_| ())
            } else {
                attempts += 1;
                super::super::connect_client("127.0.0.1", closed_port, timeouts).map(|_| ())
            };
            match r {
                // 張らずに断った（後退中）: 時間はかからない。
                Err(ImapError::Backoff(_)) => {}
                Err(_) => clock.advance(Duration::from_secs(20)),
                Ok(()) => panic!("閉じたポートにつながった"),
            }
        }
        // 別のホストのアカウント（成功する）。
        clock.advance(Duration::from_secs(1));
        let took = elapsed(&clock) - round_start;
        let next = if took >= Duration::from_secs(30) {
            elapsed(&clock)
        } else {
            round_start + Duration::from_secs(30)
        };
        clock.advance(next - elapsed(&clock));
        round_start = next;
    }
    if gated {
        g.stats("127.0.0.1", closed_port).attempts
    } else {
        attempts
    }
}

#[test]
fn an_unreachable_host_is_tried_a_few_times_an_hour_not_every_round() {
    let before = simulate_hour(false);
    let after = simulate_hour(true);
    println!(
        "つながらないホストへ 1 時間に張ろうとした回数: 門なし {before} 回 → 門あり {after} 回"
    );
    assert!(before >= 150, "{before}");
    // 0・1・3・7・15・31 分（間は 1・2・4・8・16 分）＝ 6 回。
    assert_eq!(after, 6);
}

/// 試験のあいだ、ログの行を貯めておく記録係（試験の実行体に 1 つだけ据える）。
struct Capture(Mutex<Vec<(String, log::Level, String)>>);

impl log::Log for Capture {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, r: &log::Record) {
        self.0
            .lock()
            .unwrap()
            .push((r.target().to_owned(), r.level(), r.args().to_string()));
    }
    fn flush(&self) {}
}

fn captured() -> &'static Capture {
    static CAPTURE: std::sync::OnceLock<&'static Capture> = std::sync::OnceLock::new();
    CAPTURE.get_or_init(|| {
        let c: &'static Capture = Box::leak(Box::new(Capture(Mutex::new(Vec::new()))));
        log::set_logger(c).expect("この試験の実行体で、ほかに記録係を据えていない");
        log::set_max_level(log::LevelFilter::Trace);
        c
    })
}

#[test]
fn every_connection_leaves_an_info_line_that_the_log_file_keeps() {
    let capture = captured();
    let (g, clock) = gate();
    let host = "log-line.example.invalid";
    g.enter(host, PORT, Urgency::Auto)
        .unwrap()
        .finish(&unreachable());
    clock.advance(Duration::from_secs(61));
    g.enter(host, PORT, Urgency::Auto)
        .unwrap()
        .finish(&Ok::<(), ImapError>(()));
    g.enter(host, PORT, Urgency::Auto)
        .unwrap()
        .finish_reach(&Ok(()));

    let key = format!("{host}:{PORT}");
    let lines: Vec<_> = capture
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, _, m)| m.contains(&key))
        .cloned()
        .collect();
    assert_eq!(lines.len(), 3, "{lines:?}");
    for (n, (target, level, message)) in lines.iter().enumerate() {
        assert_eq!(*level, log::Level::Info, "{message}");
        assert!(
            message.contains(&format!("直近 1 時間 {} 回", n + 1)),
            "{message}"
        );
        // 本番のログの設定（水準・target の絞り）で、ファイルまで届く。
        assert!(
            crate::services::applog::keeps(target, *level),
            "{target} の INFO がログに残らない"
        );
    }
    assert!(lines[0].2.contains("失敗"), "{}", lines[0].2);
    assert!(lines[1].2.contains("成功"), "{}", lines[1].2);
}
