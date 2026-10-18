//! Config files: every key, custom kinds, errors, export in three formats, and reloading.

#![allow(clippy::unwrap_used, reason = "test helpers fail loudly")]

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::Duration,
};

use serlog::{
    Entry, Error, Level, Sink,
    config::{self, Format, Opts},
    sinks::Memory,
};

static CAPTURED: LazyLock<Mutex<HashMap<String, Memory>>> = LazyLock::new(Mutex::default);

/// A memory sink a test can find again by its `slot` key.
struct Captured(String, Memory);

impl Sink for Captured {
    fn emit(&self, entry: &Entry<'_>) -> serlog::Result {
        self.1.emit(entry)
    }

    fn kind(&self) -> &'static str {
        "captured"
    }

    fn options(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        let mut m = serde_json::Map::new();
        m.insert("slot".into(), self.0.clone().into());
        Some(m)
    }
}

fn captured(slot: &str) -> Memory {
    CAPTURED.lock().unwrap()[slot].clone()
}

fn register() {
    config::kinds::sink("captured", |o: &mut Opts| {
        let slot = o.need("slot")?;
        let m = Memory::new(100);
        CAPTURED.lock().unwrap().insert(slot.clone(), m.clone());
        Ok(Box::new(Captured(slot, m)))
    });
}

const CONFIG: &str = r#"
app: demo
level: trace
watch: 1s
levels:
  notice: { value: 35, colour: blue }
process:
  - fields: { region: cn }
redact: [pass]
alarms:
  ops:
    tiers: [[oncall], [stderr]]
    remind: 30m
sinks:
  - kind: captured
    slot: all
    name: all
    format: json
  - kind: captured
    slot: notices
    level: notice
    pattern: "{level}:{msg}"
  - kind: captured
    slot: oncall
    name: oncall
    alarm_only: true
  - kind: file
    name: app
    path: ${SERLOG_TEST_DIR}/app.log
    max_size: 1MiB
    keep: 3
    on_fail: ops
    queue: 100
    wait: 50ms
"#;

#[test]
fn loads_dumps_and_reloads() {
    register();
    let dir = tempfile::tempdir().unwrap();
    // SAFETY: set before any other thread in this test reads the environment.
    unsafe { std::env::set_var("SERLOG_TEST_DIR", dir.path()) };
    let path = dir.path().join("serlog.yaml");
    std::fs::write(&path, CONFIG).unwrap();
    let _g = config::load(&path).unwrap();

    serlog::info!(pass = "secret", "hello");
    serlog::log!(Level::new(35), "notable");
    serlog::flush();
    let all = captured("all").lines();
    assert!(
        all.iter().any(|l| l.contains(r#""msg":"hello""#)
            && l.contains(r#""region":"cn""#)
            && l.contains(r#""pass":"***""#)),
        "{all:?}"
    );
    assert_eq!(captured("notices").lines(), ["NOTICE:notable"]);
    assert!(
        std::fs::read_to_string(dir.path().join("app.log"))
            .unwrap()
            .contains("hello")
    );
    let names: Vec<String> = serlog::status().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["all", "captured-1", "oncall", "app"]);

    let yaml = config::dump(Format::Yaml).unwrap();
    for want in [
        "notice",
        "region",
        "redact",
        "ops",
        "remind: 30m",
        "max_size: 1MiB",
        "on_fail: ops",
        "queue: 100",
    ] {
        assert!(yaml.contains(want), "{want} missing from\n{yaml}");
    }
    let from_yaml = config::read(&yaml, Format::Yaml).unwrap();
    let from_json = config::read(&config::dump(Format::Json).unwrap(), Format::Json).unwrap();
    let from_toml = config::read(&config::dump(Format::Toml).unwrap(), Format::Toml).unwrap();
    assert_eq!(from_yaml, from_json);
    assert_eq!(from_json, from_toml);
    drop(config::parse(&yaml, Format::Yaml).unwrap());

    std::fs::write(
        &path,
        "level: info\nsinks:\n  - kind: captured\n    slot: after\n    name: after\n",
    )
    .unwrap();
    std::thread::sleep(Duration::from_millis(2500));
    serlog::info!("reloaded");
    serlog::flush();
    assert!(
        captured("after")
            .lines()
            .iter()
            .any(|l| l.ends_with("reloaded"))
    );
    let names: Vec<String> = serlog::status().into_iter().map(|s| s.name).collect();
    assert_eq!(names, ["after"]);
}

#[test]
fn reads_the_same_tree_from_every_format() {
    let yaml = config::read(
        "filter: info\nsinks:\n  - kind: console\n    stream: stderr\n",
        Format::Yaml,
    )
    .unwrap();
    let json = config::read(
        r#"{"filter":"info","sinks":[{"kind":"console","stream":"stderr"}]}"#,
        Format::Json,
    )
    .unwrap();
    let toml = config::read(
        "filter = \"info\"\n[[sinks]]\nkind = \"console\"\nstream = \"stderr\"\n",
        Format::Toml,
    )
    .unwrap();
    assert_eq!(yaml, json);
    assert_eq!(json, toml);
}

#[test]
fn a_config_entry_is_a_slot() {
    let entry = config::read(
        r#"{"kind":"memory","size":5,"name":"entry","queue":0}"#,
        Format::Json,
    )
    .unwrap();
    assert!(config::slot(entry).is_ok());
    let bad = config::read(r#"{"kind":"memory","colr":"never"}"#, Format::Json).unwrap();
    assert!(
        config::slot(bad)
            .err()
            .unwrap()
            .to_string()
            .contains("colr")
    );
}

#[test]
fn reports_mistakes_by_name() {
    let err = |text: &str| config::parse(text, Format::Yaml).err().unwrap().to_string();
    assert!(err("sinks:\n  - kind: console\n    colr: never\n").contains("unknown key `colr`"));
    assert!(err("bogus: 1\n").contains("unknown key `bogus`"));
    assert!(matches!(
        config::parse("sinks:\n  - kind: kafka\n", Format::Yaml),
        Err(Error::Kind(_))
    ));
    assert!(
        err("sinks:\n  - kind: console\n    level: info\n    filter: info\n").contains("not both")
    );
    assert!(err("sinks:\n  - kind: console\n    on_fail: pager\n").contains("not in `alarms`"));
    assert!(
        err("sinks:\n  - kind: file\n    path: ${SERLOG_SURELY_UNSET}\n")
            .contains("SERLOG_SURELY_UNSET")
    );
    assert!(err("sinks:\n  - kind: console\n    format: xml\n").contains("format"));
    assert!(err("alarms:\n  panic:\n    tiers: [[stderr]]\n").contains("cannot name"));
    assert!(err("process:\n  - shout: {}\n").contains("shout"));
}
