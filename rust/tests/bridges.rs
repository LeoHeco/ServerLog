//! Records arriving through the `log` crate and through `tracing`, spans included.

#![allow(clippy::unwrap_used, reason = "test helpers fail loudly")]

use serlog::{Layer, Layout, Level, Val, sinks::Memory};
use tracing_subscriber::layer::SubscriberExt;

#[test]
fn both_ecosystems_reach_the_sinks() {
    let mem = Memory::new(20);
    let _g = serlog::builder()
        .level(Level::TRACE)
        .sink(serlog::SinkExt::layout(
            mem.clone(),
            Layout::pattern("tracing").unwrap(),
        ))
        .init()
        .unwrap();
    serlog::Bridge::install().unwrap();
    log::warn!(target: "legacy", "from log {}", 1);
    let rec = mem.find(|r| r.msg() == "from log 1").pop().unwrap();
    assert_eq!(
        (rec.level(), rec.as_record().target()),
        (Level::WARN, "legacy")
    );

    let subscriber = tracing_subscriber::registry().with(Layer);
    tracing::subscriber::with_default(subscriber, || {
        let outer = tracing::info_span!("req", id = 7);
        let _o = outer.enter();
        let inner = tracing::debug_span!("db");
        let _i = inner.enter();
        tracing::info!(rows = 3_u64, ok = true, "query done");
        serlog::set_level(Level::INFO);
        tracing::debug!("hidden");
        serlog::set_level(Level::TRACE);
        tracing::debug!("shown");
    });
    assert_eq!(mem.find(|r| r.msg() == "hidden").len(), 0);
    assert_eq!(mem.find(|r| r.msg() == "shown").len(), 1);
    let rec = mem.find(|r| r.msg() == "query done").pop().unwrap();
    assert_eq!(rec.field("rows"), Some(&Val::U64(3)));
    assert_eq!(rec.field("ok"), Some(&Val::Bool(true)));
    assert_eq!(rec.as_record().span_chain(), Some("req{id=7}:db"));
    let line = mem
        .lines()
        .into_iter()
        .find(|l| l.contains("query done"))
        .unwrap();
    assert!(
        line.ends_with(" INFO req{id=7}:db: bridges: query done rows=3 ok=true"),
        "{line}"
    );
}
