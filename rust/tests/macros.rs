//! The macros: typed fields, sigils, tags, targets, the caller and bound fields.

#![allow(clippy::unwrap_used, reason = "test helpers fail loudly")]

use std::{collections::BTreeMap, sync::OnceLock};

use serlog::{Level, Val, sinks::Memory};

fn mem() -> &'static Memory {
    static MEM: OnceLock<Memory> = OnceLock::new();
    MEM.get_or_init(|| {
        let m = Memory::new(100);
        std::mem::forget(
            serlog::builder()
                .level(Level::TRACE)
                .sink(m.clone())
                .init()
                .unwrap(),
        );
        m
    })
}

#[derive(Debug)]
#[expect(dead_code, reason = "shown only through Debug")]
struct State {
    n: u8,
}

#[test]
fn fields_keep_their_types() {
    let m = mem();
    let name = "debian";
    let state = State { n: 2 };
    let sizes = vec![1_u64, 2];
    let mut labels = BTreeMap::new();
    labels.insert("arch", "amd64");
    serlog::info!(
        target: "types",
        tries = 3,
        ratio = 0.5,
        ok = true,
        mirror = %name,
        ?state,
        sizes,
        labels,
        "synced {} files",
        42
    );
    let rec = m.find(|r| r.msg() == "synced 42 files").pop().unwrap();
    let r = rec.as_record();
    assert_eq!(r.target(), "types");
    let get = |k: &str| rec.field(k).cloned().unwrap();
    assert_eq!(get("tries"), Val::I64(3));
    assert_eq!(get("ratio"), Val::F64(0.5));
    assert_eq!(get("ok"), Val::Bool(true));
    assert_eq!(get("mirror"), Val::Str("debian".into()));
    assert_eq!(get("state"), Val::Str("State { n: 2 }".into()));
    assert_eq!(get("sizes"), Val::List(vec![Val::U64(1), Val::U64(2)]));
    assert_eq!(
        get("labels"),
        Val::Map(vec![("arch".into(), Val::Str("amd64".into()))])
    );
}

#[test]
fn records_the_caller_and_tags() {
    let m = mem();
    serlog::warn!(tags: ["audit", "db"], "tagged");
    let rec = m.find(|r| r.msg() == "tagged").pop().unwrap();
    let r = rec.as_record();
    assert_eq!(r.level(), Level::WARN);
    assert_eq!(r.target(), module_path!());
    assert_eq!(r.site().module, Some(module_path!()));
    assert_eq!(r.site().func, Some("records_the_caller_and_tags"));
    assert_eq!(r.file(), Some(file!()));
    assert_eq!(r.tags().collect::<Vec<_>>(), ["audit", "db"]);
}

#[test]
fn skips_arguments_below_the_level() {
    let m = mem();
    serlog::set_level(Level::INFO);
    let mut hit = false;
    let mut touch = || {
        hit = true;
        1
    };
    serlog::debug!(n = touch(), "quiet");
    assert!(!hit && m.find(|r| r.msg() == "quiet").is_empty());
    serlog::set_level(Level::TRACE);
    serlog::fatal!("loud");
    assert_eq!(
        m.find(|r| r.msg() == "loud").pop().unwrap().level(),
        Level::FATAL
    );
}

#[test]
fn binds_fields_for_a_scope() {
    let m = mem();
    {
        let _g = serlog::bind!(request = 7, user = %"ann");
        serlog::info!(extra = 1, "inside");
    }
    serlog::info!("outside");
    let inside = m.find(|r| r.msg() == "inside").pop().unwrap();
    let keys: Vec<&str> = inside.fields().iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["request", "user", "extra"]);
    let outside = m.find(|r| r.msg() == "outside").pop().unwrap();
    assert_eq!(outside.fields(), []);
}

#[test]
fn closures_report_their_function() {
    let m = mem();
    let run = || serlog::info!("from a closure");
    run();
    let rec = m.find(|r| r.msg() == "from a closure").pop().unwrap();
    assert_eq!(
        rec.as_record().site().func,
        Some("closures_report_their_function")
    );
}
