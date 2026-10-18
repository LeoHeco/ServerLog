//! A batch of records delivered as one message, checked against a stand-in SMTP server.

#![allow(clippy::unwrap_used, reason = "test helpers fail loudly")]

use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

use serlog::{
    Entry, Layout, Level, Record, Sink,
    sinks::{Mail, Security},
};

/// Accepts one session and hands back the message data.
fn serve() -> (u16, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut r = BufReader::new(stream.try_clone().unwrap());
        let mut w = &stream;
        w.write_all(b"220 test\r\n").unwrap();
        let mut data = String::new();
        let mut line = String::new();
        while r.read_line(&mut line).unwrap() > 0 {
            let cmd = line.to_ascii_uppercase();
            let reply: &[u8] = if cmd.starts_with("EHLO") {
                b"250-test\r\n250 OK\r\n"
            } else if cmd.starts_with("DATA") {
                w.write_all(b"354 go on\r\n").unwrap();
                let mut l = String::new();
                while r.read_line(&mut l).unwrap() > 0 && l != ".\r\n" {
                    data.push_str(&l);
                    l.clear();
                }
                b"250 queued\r\n"
            } else if cmd.starts_with("QUIT") {
                w.write_all(b"221 bye\r\n").unwrap();
                break;
            } else {
                b"250 OK\r\n"
            };
            w.write_all(reply).unwrap();
            line.clear();
        }
        tx.send(data).unwrap();
    });
    (port, rx)
}

#[test]
fn sends_one_message_per_batch() {
    let (port, got) = serve();
    let mail = Mail::new("127.0.0.1")
        .port(port)
        .security(Security::None)
        .from("bot@example.org")
        .to("ops@example.org");
    mail.ready().unwrap();
    assert!(!mail.enabled(Level::WARN, "x") && mail.enabled(Level::ERROR, "x"));
    let layout = Layout::pattern("{level} {msg}").unwrap();
    let entries = [
        Entry::new(
            Record::new(Level::ERROR, "sync", format_args!("disk <full>")),
            &layout,
        ),
        Entry::new(
            Record::new(Level::FATAL, "sync", format_args!("mirror stalled")),
            &layout,
        ),
    ];
    mail.emit_batch(&entries).unwrap();
    let data = got.recv_timeout(Duration::from_secs(20)).unwrap();
    assert!(
        data.contains("Subject: [") && data.contains("2 FATAL records"),
        "{data}"
    );
    assert!(
        data.contains("ERROR disk <full>") && data.contains("disk &lt;full&gt;"),
        "{data}"
    );
    assert!(data.contains("mirror stalled"));
}

#[test]
fn refuses_to_start_without_addresses() {
    assert!(
        Mail::new("smtp.example.org")
            .to("ops@example.org")
            .ready()
            .is_err()
    );
    assert!(
        Mail::new("smtp.example.org")
            .from("bot@example.org")
            .ready()
            .is_err()
    );
    assert!(
        Mail::new("smtp.example.org")
            .auth("bot@example.org", "")
            .to("ops@example.org")
            .ready()
            .is_err()
    );
}

#[test]
fn template_matches_spec() {
    let spec = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../spec/mail.html");
    if let Ok(s) = std::fs::read_to_string(spec) {
        assert_eq!(s, include_str!("../assets/mail.html"));
    }
}
