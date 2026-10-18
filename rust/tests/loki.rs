//! A push request as Loki and Alloy's `loki.source.api` expect it, checked against a stand-in server.

#![allow(clippy::unwrap_used, reason = "test helpers fail loudly")]

use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

use serlog::{Entry, Error, Field, Level, Record, Sink, ToValue, sinks::Loki};

/// Answers each request with the next status and hands back its request line, headers and body.
fn serve(answers: Vec<u16>) -> (String, mpsc::Receiver<(String, String)>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/loki/api/v1/push", listener.local_addr().unwrap());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for status in answers {
            let (stream, _) = listener.accept().unwrap();
            let mut r = BufReader::new(stream.try_clone().unwrap());
            let mut head = String::new();
            let mut len = 0;
            loop {
                let mut line = String::new();
                r.read_line(&mut line).unwrap();
                if line == "\r\n" {
                    break;
                }
                if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                    len = v.trim().parse().unwrap();
                }
                head.push_str(&line);
            }
            let mut raw = vec![0; len];
            r.read_exact(&mut raw).unwrap();
            let mut body = String::new();
            if len > 0 {
                flate2::read::GzDecoder::new(&raw[..])
                    .read_to_string(&mut body)
                    .unwrap();
            }
            (&stream)
                .write_all(
                    format!(
                        "HTTP/1.1 {status} X\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                    .as_bytes(),
                )
                .unwrap();
            tx.send((head, body)).unwrap();
        }
    });
    (url, rx)
}

#[test]
fn pushes_one_stream_per_level_with_metadata() {
    let (url, got) = serve(vec![204]);
    let loki = Loki::new(url).label("team", "mirrors");
    let layout = loki.default_layout();
    let fields: [Field<'_>; 2] = [("mirror", "debian".to_value()), ("tries", 3.to_value())];
    let a = Record::new(Level::INFO, "sync", format_args!("started"))
        .at("src/a.rs", 3)
        .with(&fields);
    let b = Record::new(Level::ERROR, "sync", format_args!("failed \"rsync\""));
    let c = Record::new(Level::INFO, "sync", format_args!("retrying"));
    let entries = [
        Entry::new(a, &layout),
        Entry::new(b, &layout),
        Entry::new(c, &layout),
    ];
    loki.emit_batch(&entries).unwrap();

    let (head, body) = got.recv_timeout(Duration::from_secs(10)).unwrap();
    let head = head.to_ascii_lowercase();
    assert!(head.starts_with("post /loki/api/v1/push"), "{head}");
    assert!(
        head.contains("content-encoding: gzip") && head.contains("content-type: application/json"),
        "{head}"
    );
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    let streams = v["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 2, "{body}");
    assert_eq!(streams[0]["stream"]["level"], "info");
    assert_eq!(streams[0]["stream"]["team"], "mirrors");
    let values = streams[0]["values"].as_array().unwrap();
    assert_eq!(values.len(), 2);
    assert!(values[0][0].as_str().unwrap().parse::<u128>().unwrap() > 0);
    assert_eq!(values[0][1], "started");
    assert_eq!(values[0][2]["target"], "sync");
    assert_eq!(values[0][2]["file"], "src/a.rs:3");
    assert_eq!(values[0][2]["mirror"], "debian");
    assert_eq!(values[0][2]["tries"], "3");
    assert_eq!(streams[1]["values"][0][1], "failed \"rsync\"");
}

#[test]
fn tells_retryable_answers_from_refusals() {
    let (url, _got) = serve(vec![503, 400]);
    let loki = Loki::new(url);
    let layout = loki.default_layout();
    let rec = Record::new(Level::INFO, "t", format_args!("x"));
    let e = loki.emit(&Entry::new(rec, &layout)).unwrap_err();
    assert!(e.is_transient(), "{e}");
    let e = loki.emit(&Entry::new(rec, &layout)).unwrap_err();
    assert!(matches!(e, Error::Rejected(_)), "{e}");
}

#[test]
fn checks_the_url_and_asks_ready() {
    assert!(Loki::new("ftp://x").ready().is_err());
    let (url, got) = serve(vec![200]);
    Loki::new(url).connect().ready().unwrap();
    let (head, _) = got.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(
        head.to_ascii_lowercase().starts_with("get /ready"),
        "{head}"
    );
}
