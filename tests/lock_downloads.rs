//! TL-11. The blocking client applies its timeout to each read of the body, so a slow but steady download finishes
//! and only a quiet connection times out (the audit's "120 s total" reading did not hold; DECISIONS.md D3). What is
//! missing is resume: an interrupted download starts again from byte 0.
mod support;
use arcade_tools_core::release::{checksum, Arch, Channel, Os};
use arcade_tools_core::source::Source;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Serves `body` in 16 pieces `gap` apart. With `cut`, the first request stops halfway and drops the connection.
/// Records each request's headers.
fn trickle(body: Vec<u8>, gap: Duration, cut: bool) -> (String, Arc<Mutex<Vec<Vec<String>>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/asset", listener.local_addr().unwrap());
    let seen: Arc<Mutex<Vec<Vec<String>>>> = Default::default();
    let log = seen.clone();
    std::thread::spawn(move || {
        for (n, mut stream) in listener.incoming().flatten().enumerate() {
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut headers = Vec::new();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
                headers.push(line.trim().to_ascii_lowercase());
            }
            log.lock().unwrap().push(headers);
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            for (i, piece) in body.chunks(body.len().div_ceil(16)).enumerate() {
                if cut && n == 0 && i == 8 {
                    break;
                }
                std::thread::sleep(gap);
                if stream
                    .write_all(piece)
                    .and_then(|_| stream.flush())
                    .is_err()
                {
                    break;
                }
            }
        }
    });
    (url, seen)
}

fn published(server: &support::FakeRelease, body: &[u8]) {
    server.publish(
        "arcade.look",
        "0.4.0",
        "stable",
        &[("Look_amd64.AppImage", body)],
        "nsis",
    );
}

#[test]
fn a_slow_steady_download_outlasting_the_timeout_completes() {
    let server = support::FakeRelease::new();
    let body = vec![42u8; 64 * 1024];
    published(&server, &body);
    let source = Source::test_server_with_timeout(&server.url, Duration::from_secs(1)).unwrap();
    let mut checked = source
        .check("arcade.look", Channel::Stable, Os::Linux, Arch::X64)
        .unwrap();
    checked.url = trickle(body, Duration::from_millis(200), false).0;
    let file = source
        .download(
            &checked,
            server.root.path(),
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap();
    assert_eq!(checksum(file.path()).unwrap(), checked.asset.sha256);
}

#[test]
fn a_stalled_download_times_out() {
    let server = support::FakeRelease::new();
    let body = vec![42u8; 64 * 1024];
    published(&server, &body);
    let source = Source::test_server_with_timeout(&server.url, Duration::from_millis(500)).unwrap();
    let mut checked = source
        .check("arcade.look", Channel::Stable, Os::Linux, Arch::X64)
        .unwrap();
    checked.url = trickle(body, Duration::from_millis(1500), false).0;
    let error = source
        .download(
            &checked,
            server.root.path(),
            &AtomicBool::new(false),
            &mut |_, _| {},
        )
        .unwrap_err();
    assert_eq!(error.code, "network");
}

#[test]
#[should_panic(expected = "KNOWN BUG TL-11")]
fn an_interrupted_download_resumes_where_it_stopped() {
    let server = support::FakeRelease::new();
    let body = vec![42u8; 64 * 1024];
    published(&server, &body);
    let source = Source::test_server(&server.url).unwrap();
    let mut checked = source
        .check("arcade.look", Channel::Stable, Os::Linux, Arch::X64)
        .unwrap();
    let (url, seen) = trickle(body, Duration::from_millis(10), true);
    checked.url = url;
    let cache = server.root.path().join("downloads");
    let cancel = AtomicBool::new(false);
    assert!(source
        .download(&checked, &cache, &cancel, &mut |_, _| {})
        .is_err());
    let _ = source.download(&checked, &cache, &cancel, &mut |_, _| {});
    let requests = seen.lock().unwrap().clone();
    assert!(
        requests
            .get(1)
            .is_some_and(|h| h.iter().any(|l| l.starts_with("range: bytes=32768-"))),
        "KNOWN BUG TL-11: the retry asked for the whole file again: {requests:?}"
    );
}
