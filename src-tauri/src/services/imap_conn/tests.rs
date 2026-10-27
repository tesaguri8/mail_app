use super::*;

/// 受け付けるだけで何も返さないサーバーに向けると、上限で必ず時間切れとして戻る。
#[test]
fn a_silent_server_times_out_instead_of_hanging() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    // 受け付けた接続を、クライアントが戻るまで握ったまま黙る
    // （閉じると「切断」になり、時間切れの経路を通らない）。
    let (done, wait) = std::sync::mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let (conn, _) = listener.accept().unwrap();
        let _ = wait.recv_timeout(std::time::Duration::from_secs(10));
        drop(conn);
    });
    let timeouts = ImapTimeouts {
        connect: std::time::Duration::from_secs(2),
        io: std::time::Duration::from_millis(300),
        total: None,
    };
    let started = std::time::Instant::now();
    let r = connect_login_with("127.0.0.1", port, "u@example.invalid", "x", timeouts);
    let took = started.elapsed();
    done.send(()).unwrap();
    server.join().unwrap();
    assert!(matches!(r, Err(ImapError::Timeout)), "{:?}", r.err());
    assert!(took < std::time::Duration::from_secs(2), "{took:?}");
}

/// 挨拶・ログイン・取得の途中の時間切れ（読み込みの上限）も時間切れとして扱う。
#[test]
fn an_io_timeout_inside_imap_is_a_timeout() {
    for kind in [std::io::ErrorKind::WouldBlock, std::io::ErrorKind::TimedOut] {
        let e = ImapError::from(imap::Error::Io(std::io::Error::from(kind)));
        assert!(matches!(e, ImapError::Timeout), "{kind:?}");
    }
    let e = ImapError::from(imap::Error::ConnectionLost);
    assert!(!matches!(e, ImapError::Timeout));
}

/// 接続をすぐ閉じるサーバーは時間切れではない（別の失敗として返す）。
#[test]
fn a_closed_connection_is_not_a_timeout() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || drop(listener.accept().unwrap()));
    let r = connect_login_with(
        "127.0.0.1",
        port,
        "u@example.invalid",
        "x",
        ImapTimeouts {
            connect: std::time::Duration::from_secs(2),
            io: std::time::Duration::from_secs(2),
            total: None,
        },
    );
    assert!(r.is_err());
    assert!(!matches!(r, Err(ImapError::Timeout)));
    server.join().unwrap();
}

/// 少しずつ送り続ける相手でも、全体の締め切りで必ず終わる（1 回ごとの上限には掛からない）。
#[test]
fn a_trickling_peer_stops_at_the_deadline() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (done, wait) = std::sync::mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        // 50ms ごとに 1 バイト。止めの合図が来るまで送り続ける。
        while wait.try_recv().is_err() {
            if conn.write_all(b"x").is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let tcp = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let mut s = GuardedStream::new(
        tcp,
        Duration::from_secs(1),
        Some(Instant::now() + Duration::from_millis(400)),
    );
    let started = Instant::now();
    let mut buf = [0u8; 16];
    let mut got = 0;
    let err = loop {
        match s.read(&mut buf) {
            Ok(n) => got += n,
            Err(e) => break e,
        }
    };
    let took = started.elapsed();
    done.send(()).unwrap();
    server.join().unwrap();
    assert!(got > 0, "少しは届いている");
    assert!(is_timeout(&err), "{err:?}");
    assert!(took < Duration::from_secs(1), "{took:?}");
}

/// TLS の握手の途中で少しずつしか来ない相手（レコードの見出しだけ送って中身を垂らす）でも、
/// 締め切りで時間切れとして戻る。
#[test]
fn a_trickling_tls_handshake_times_out_at_the_deadline() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (done, wait) = std::sync::mpsc::channel::<()>();
    let server = std::thread::spawn(move || {
        let (mut conn, _) = listener.accept().unwrap();
        // 握手のレコード（長さ 16384）の見出しだけ送り、中身を 50ms ごとに 1 バイトずつ垂らす。
        let _ = conn.write_all(&[0x16, 0x03, 0x03, 0x40, 0x00]);
        while wait.try_recv().is_err() {
            if conn.write_all(&[0]).is_err() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });
    let timeouts = ImapTimeouts {
        connect: Duration::from_secs(2),
        io: Duration::from_secs(2),
        total: Some(Duration::from_millis(500)),
    };
    let started = Instant::now();
    let r = connect_client("127.0.0.1", port, timeouts);
    let took = started.elapsed();
    done.send(()).unwrap();
    server.join().unwrap();
    assert!(matches!(r, Err(ImapError::Timeout)), "{:?}", r.err());
    assert!(took < Duration::from_secs(2), "{took:?}");
}

/// ログインを断られた（NO / BAD）ときはログインの失敗、時間切れ・切断はそのまま。
#[test]
fn a_refused_login_is_a_login_error() {
    for e in [
        imap::Error::No("[AUTHENTICATIONFAILED] Invalid credentials".into()),
        imap::Error::Bad("LOGIN failed".into()),
    ] {
        assert!(matches!(login_error(e), ImapError::Login(_)));
    }
    let timed_out = imap::Error::Io(std::io::Error::from(std::io::ErrorKind::TimedOut));
    assert!(matches!(login_error(timed_out), ImapError::Timeout));
    assert!(matches!(
        login_error(imap::Error::ConnectionLost),
        ImapError::Imap(imap::Error::ConnectionLost)
    ));
}

/// 待ち受けていないポートへの接続は、接続の失敗（「サーバーにつながらない」に分ける側）。
#[test]
fn a_refused_connection_is_a_connect_error() {
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };
    let r = connect_login_with(
        "127.0.0.1",
        port,
        "u@example.invalid",
        "x",
        ImapTimeouts {
            connect: Duration::from_secs(2),
            io: Duration::from_secs(2),
            total: None,
        },
    );
    assert!(matches!(r, Err(ImapError::Connect(_))), "{:?}", r.err());
}
