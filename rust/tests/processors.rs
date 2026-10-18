//! The built-in processors, run on single records.

#![allow(clippy::unwrap_used, reason = "test helpers fail loudly")]

use std::time::Duration;

use serlog::{
    Field, Level, Record, ToValue, Val,
    process::{Dedupe, Draft, Exclude, Fields, Limit, Match, Processor, Redact, Sample, Sanitise},
};

fn run(p: &dyn Processor, rec: Record<'_>) -> Option<serlog::RecordBuf> {
    let mut d = Draft::new(rec);
    p.process(&mut d).then(|| d.record().to_buf())
}

#[test]
fn redacts_keys_paths_and_patterns() {
    let headers = [("authorization", "Bearer x"), ("accept", "*/*")];
    let req = Val::Map(vec![(
        "headers".into(),
        Val::Map(
            headers
                .iter()
                .map(|(k, v)| ((*k).into(), Val::Str((*v).into())))
                .collect(),
        ),
    )]);
    let fields: [Field<'_>; 3] = [
        ("pass", "hunter2".to_value()),
        ("req", req.to_value()),
        ("user", "ann".to_value()),
    ];
    let r = Redact::keys(["pass"]).path("req.headers.authorization");
    #[cfg(feature = "regex")]
    let r = r.pattern(r"\d{11}").unwrap();
    let out = run(
        &r,
        Record::new(Level::INFO, "t", format_args!("call 13800138000 now")).with(&fields),
    )
    .unwrap();
    assert_eq!(out.field("pass"), Some(&Val::Str("***".into())));
    assert_eq!(out.field("user"), Some(&Val::Str("ann".into())));
    let Some(Val::Map(req)) = out.field("req") else {
        panic!()
    };
    let Val::Map(h) = &req[0].1 else { panic!() };
    assert_eq!(h[0].1, Val::Str("***".into()));
    assert_eq!(h[1].1, Val::Str("*/*".into()));
    #[cfg(feature = "regex")]
    assert_eq!(out.msg(), "call *** now");
}

#[test]
fn leaves_clean_records_uncopied() {
    let mut d = Draft::new(Record::new(Level::INFO, "t", format_args!("fine")));
    assert!(Redact::keys(["pass"]).process(&mut d) && Sanitise.process(&mut d));
    assert!(!d.edited());
}

#[test]
fn sanitises_line_breaks() {
    let fields: [Field<'_>; 1] = [("who", "a\nb".to_value())];
    let out = run(
        &Sanitise,
        Record::new(Level::INFO, "t", format_args!("x\r\ny\u{7}")).with(&fields),
    )
    .unwrap();
    assert_eq!(out.msg(), "x\\r\\ny\\u{0007}");
    assert_eq!(out.field("who"), Some(&Val::Str("a\\nb".into())));
}

#[test]
fn adds_matches_and_excludes() {
    let out = run(
        &Fields::new().with("region", "cn").with("n", 1),
        Record::new(Level::INFO, "t", format_args!("x")),
    )
    .unwrap();
    assert_eq!(out.field("region"), Some(&Val::Str("cn".into())));
    let tags = ["audit"];
    let fields: [Field<'_>; 1] = [("mirror", "debian".to_value())];
    let rec = Record::new(Level::WARN, "t", format_args!("x"))
        .tagged(&tags)
        .with(&fields);
    let m = Match::new()
        .tag("audit")
        .field("mirror", "debian")
        .level(Level::WARN);
    assert!(run(&m, rec).is_some());
    assert!(run(&Match::new().tag("other"), rec).is_none());
    assert!(run(&Exclude(Match::new().tag("audit")), rec).is_none());
    assert!(run(&Exclude(Match::new().level(Level::ERROR)), rec).is_some());
}

#[test]
fn samples_limits_and_dedupes() {
    let s = Sample::new(2, 3, Duration::from_mins(1));
    let kept: Vec<bool> = (0..8)
        .map(|_| run(&s, Record::new(Level::INFO, "t", format_args!("same"))).is_some())
        .collect();
    assert_eq!(kept, [true, true, false, false, true, false, false, true]);

    let l = Limit::new(0.0, 3.0);
    let kept = (0..5)
        .filter(|_| run(&l, Record::new(Level::INFO, "t", format_args!("x"))).is_some())
        .count();
    assert_eq!(kept, 3);

    let d = Dedupe::new(Duration::from_millis(200));
    assert!(run(&d, Record::new(Level::INFO, "t", format_args!("again"))).is_some());
    assert!(run(&d, Record::new(Level::INFO, "t", format_args!("again"))).is_none());
    assert!(run(&d, Record::new(Level::INFO, "t", format_args!("again"))).is_none());
    std::thread::sleep(Duration::from_millis(250));
    let next = run(&d, Record::new(Level::INFO, "t", format_args!("again"))).unwrap();
    assert_eq!(next.field("repeated"), Some(&Val::U64(2)));
}
