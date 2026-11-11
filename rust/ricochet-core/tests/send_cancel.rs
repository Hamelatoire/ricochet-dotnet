//! Ports the cancellation cases of `RequestSenderTests.cs`: a cancelled send stops and says so, without being
//! reported as a failure. A loopback server sends its headers at once, then the body slowly.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use ricochet_core::models::ApiRequest;
use ricochet_core::send::{CANCELLED_MESSAGE, RequestSender};

/// Serves one slow body: headers at once, then a byte every 50 ms for about 10 seconds. Counts the connections.
fn slow_site() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let connections = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&connections);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            seen.fetch_add(1, Ordering::SeqCst);
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 200\r\nConnection: close\r\n\r\n");
                for _ in 0..200 {
                    if stream.write_all(b"x").and_then(|()| stream.flush()).is_err() {
                        return;
                    }
                    thread::sleep(Duration::from_millis(50));
                }
            });
        }
    });
    (format!("http://127.0.0.1:{port}/slow"), connections)
}

fn get(url: &str) -> ApiRequest {
    ApiRequest { method: "GET".into(), url: url.into(), ..ApiRequest::default() }
}

#[test]
fn a_send_cancelled_before_it_starts_sends_nothing() {
    let (url, connections) = slow_site();
    let error = RequestSender::new().send_cancellable(&get(&url), &AtomicBool::new(true)).unwrap_err();

    assert!(error.is_cancelled());
    assert_eq!(error.message(), CANCELLED_MESSAGE);
    thread::sleep(Duration::from_millis(100));
    assert_eq!(connections.load(Ordering::SeqCst), 0);
}

#[test]
fn a_cancel_during_a_slow_body_stops_the_send() {
    let (url, _) = slow_site();
    let cancel = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancel);
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(300));
        flag.store(true, Ordering::SeqCst);
    });

    let started = Instant::now();
    let error = RequestSender::new().send_cancellable(&get(&url), &cancel).unwrap_err();
    assert!(error.is_cancelled());
    assert!(started.elapsed() < Duration::from_secs(3), "stopped after {:?}", started.elapsed());
}

#[test]
fn a_failed_send_is_not_reported_as_cancelled() {
    // Nothing listens on port 9, so the connection is refused.
    let error = RequestSender::new().send(&get("http://127.0.0.1:9/")).unwrap_err();
    assert!(!error.is_cancelled());
}
