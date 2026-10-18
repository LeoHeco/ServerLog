//! Slots at run time: names, claims, start-up checks, queues, health, failure policies, alarms and loops.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail loudly"
)]

use std::{
    sync::{
        Arc, Once,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

use serlog::{
    Entry, Error, Filter, Health, Layout, Level, OnFail, Queue, Sink, SinkExt, Tiers,
    layout::Schema,
    process::Match,
    sinks::{File, Memory},
};

fn start() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let g = serlog::builder()
            .level(Level::TRACE)
            .sink(Memory::new(10).name("main").filter("off".parse().unwrap()))
            .init()
            .unwrap();
        std::mem::forget(g);
    });
}

fn only(target: &str) -> Filter {
    format!("{target}=trace").parse().unwrap()
}

#[derive(Clone, Default)]
struct Flaky {
    fail: Arc<AtomicBool>,
    seen: Arc<AtomicUsize>,
}

impl Sink for Flaky {
    fn emit(&self, _: &Entry<'_>) -> serlog::Result {
        self.seen.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            Err(Error::other("boom"))
        } else {
            Ok(())
        }
    }

    fn kind(&self) -> &'static str {
        "flaky"
    }
}

fn status(name: &str) -> serlog::Status {
    serlog::status()
        .into_iter()
        .find(|s| s.name == name)
        .unwrap()
}

#[test]
fn a_failure_is_told_once_and_its_recovery_after() {
    start();
    let alarms = Memory::new(10);
    serlog::add(alarms.clone().name("alarms1").alarm_only()).unwrap();
    let flaky = Flaky::default();
    flaky.fail.store(true, Ordering::SeqCst);
    serlog::add(
        flaky
            .clone()
            .name("flaky1")
            .filter(only("t1"))
            .probe(Duration::ZERO)
            .on_fail(OnFail::alarm(Tiers::new([["alarms1"]]))),
    )
    .unwrap();
    for _ in 0..3 {
        serlog::info!(target: "t1", "write");
    }
    serlog::flush();
    assert_eq!(flaky.seen.load(Ordering::SeqCst), 3);
    assert_eq!(status("flaky1").health, Health::Down);
    let told = alarms.records();
    assert_eq!(told.len(), 1, "{told:?}");
    assert_eq!(told[0].level(), Level::ERROR);
    assert_eq!(told[0].msg(), "sink `flaky1` failed: boom");
    assert_eq!(
        told[0].field("phase"),
        Some(&serlog::Val::Str("opened".into()))
    );

    flaky.fail.store(false, Ordering::SeqCst);
    serlog::info!(target: "t1", "write");
    serlog::flush();
    assert_eq!(status("flaky1").health, Health::Up);
    let told = alarms.records();
    assert_eq!(told.len(), 2);
    assert_eq!(told[1].level(), Level::INFO);
    assert_eq!(
        told[1].field("phase"),
        Some(&serlog::Val::Str("resolved".into()))
    );
}

#[test]
fn a_down_sink_is_skipped_until_its_probe() {
    start();
    let flaky = Flaky::default();
    flaky.fail.store(true, Ordering::SeqCst);
    serlog::add(
        flaky
            .clone()
            .name("flaky2")
            .filter(only("t2"))
            .probe(Duration::from_hours(1))
            .on_fail(OnFail::Ignore),
    )
    .unwrap();
    for _ in 0..3 {
        serlog::info!(target: "t2", "write");
    }
    assert_eq!(flaky.seen.load(Ordering::SeqCst), 1);
    let s = status("flaky2");
    assert_eq!((s.health, s.skipped), (Health::Down, 2));
    assert_eq!(s.error.as_deref(), Some("boom"));
}

#[test]
fn the_panic_policy_raises_in_the_caller_every_time() {
    start();
    let flaky = Flaky::default();
    flaky.fail.store(true, Ordering::SeqCst);
    serlog::add(
        flaky
            .clone()
            .name("flaky3")
            .filter(only("t3"))
            .on_fail(OnFail::Panic),
    )
    .unwrap();
    for _ in 0..2 {
        let r = std::panic::catch_unwind(|| serlog::error!(target: "t3", "write"));
        assert!(r.is_err());
    }
    assert_eq!(flaky.seen.load(Ordering::SeqCst), 2);
}

#[test]
fn names_claims_and_checks_are_enforced() {
    start();
    let a = serlog::add(Memory::new(1).filter("off".parse().unwrap())).unwrap();
    let b = serlog::add(Memory::new(1).filter("off".parse().unwrap())).unwrap();
    assert!(
        a.starts_with("memory-") && b.starts_with("memory-") && a != b,
        "{a} {b}"
    );
    assert!(matches!(
        serlog::add(Memory::new(1).name("main")),
        Err(Error::Name(_))
    ));
    assert!(matches!(
        serlog::add(Memory::new(1).name("*")),
        Err(Error::Name(_))
    ));

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one.log");
    serlog::add(File::new(&path).name("one").filter("off".parse().unwrap())).unwrap();
    assert!(matches!(
        serlog::add(File::new(&path).name("two")),
        Err(Error::Claimed(_))
    ));
    assert!(matches!(
        serlog::add(File::new(dir.path()).name("three")),
        Err(Error::NotReady { .. })
    ));
    assert!(serlog::remove("one"));
    assert!(!serlog::remove("one"));
}

#[test]
fn removing_a_slot_writes_its_queue_and_closes_its_sink() {
    start();
    let mem = Memory::new(10);
    serlog::add(
        mem.clone()
            .name("leaving")
            .filter(only("t11"))
            .queue(Queue::new(10).wait(Duration::from_secs(3600))),
    )
    .unwrap();
    for _ in 0..3 {
        serlog::info!(target: "t11", "queued");
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("gone.log");
    serlog::add(File::new(&path).name("gone").filter("off".parse().unwrap())).unwrap();
    assert!(serlog::remove("leaving") && serlog::remove("gone"));
    assert_eq!(mem.len(), 3);
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn a_queued_slot_writes_in_the_background() {
    start();
    let mem = Memory::new(100);
    serlog::add(
        mem.clone()
            .name("queued")
            .filter(only("t5"))
            .queue(Queue::new(100).wait(Duration::from_millis(20))),
    )
    .unwrap();
    for i in 0..5 {
        serlog::info!(target: "t5", n = i, "queued");
    }
    serlog::flush();
    assert_eq!(mem.len(), 5);
}

#[test]
fn slots_lay_out_and_process_on_their_own() {
    start();
    let json = Memory::new(10);
    let text = Memory::new(10);
    serlog::add(
        json.clone()
            .name("json")
            .filter(only("t6"))
            .layout(Layout::json(Schema::Serlog)),
    )
    .unwrap();
    serlog::add(
        text.clone()
            .name("text")
            .filter(only("t6"))
            .layout(Layout::pattern("{level}:{msg}").unwrap())
            .process(Match::new().tag("keep")),
    )
    .unwrap();
    serlog::info!(target: "t6", tags: ["keep"], "kept");
    serlog::info!(target: "t6", "dropped by text");
    assert_eq!(json.len(), 2);
    assert!(json.lines()[0].starts_with("{\"time\":"));
    assert_eq!(text.lines(), ["INFO:kept"]);
}

struct Loud(Arc<AtomicUsize>);

impl Sink for Loud {
    fn emit(&self, _: &Entry<'_>) -> serlog::Result {
        self.0.fetch_add(1, Ordering::SeqCst);
        serlog::info!(target: "t7", "from inside the sink");
        Ok(())
    }
}

#[test]
fn records_made_inside_a_sink_are_suppressed() {
    start();
    let seen = Arc::new(AtomicUsize::new(0));
    serlog::add(Loud(seen.clone()).name("loud").filter(only("t7"))).unwrap();
    serlog::info!(target: "t7", "outside");
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    assert_eq!(status("loud").suppressed, 1);
}

#[test]
fn panics_are_logged_once_caught() {
    start();
    let mem = Memory::new(10);
    serlog::add(mem.clone().name("panics").filter(only("panic"))).unwrap();
    serlog::catch_panics();
    let r = std::thread::Builder::new()
        .name("worker".into())
        .spawn(|| panic!("worker gave up"))
        .unwrap()
        .join();
    assert!(r.is_err());
    let rec = mem.find(|r| r.msg() == "worker gave up").pop().unwrap();
    assert_eq!(rec.level(), Level::FATAL);
    assert_eq!(
        rec.field("thread"),
        Some(&serlog::Val::Str("worker".into()))
    );
    assert_eq!(rec.as_record().file(), Some(file!()));
}
