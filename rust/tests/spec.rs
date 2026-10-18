//! The shared vectors in `spec/cases`, which every serlog package must pass.

#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "test helpers fail loudly"
)]

use std::{
    collections::HashSet,
    sync::{Mutex, Once},
    time::{Duration, Instant, UNIX_EPOCH},
};

use serde_json::Value;
use serlog::{
    __private::{Breaker, Tell},
    Alarm, ErrorBuf, Field, Filter, Incident, Layout, Level, Phase, Record, RecordBuf, Tiers,
    ToValue, Val,
    alarm::{Desk, Outcome, sound},
    layout::Schema,
    sinks::Memory,
};

fn cases(name: &str) -> Value {
    let path = format!("{}/../spec/cases/{name}.json", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn start() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        let m = &cases("render")["meta"];
        let guard = serlog::builder()
            .app(m["app"].as_str().unwrap())
            .env(m["env"].as_str().unwrap())
            .instance(m["instance"].as_str().unwrap())
            .sink(Memory::new(1))
            .init()
            .unwrap();
        std::mem::forget(guard);
    });
}

fn level(v: &Value) -> Level {
    v.as_str().unwrap().parse().unwrap()
}

fn val(v: &Value) -> Val {
    match v {
        Value::Null => Val::Null,
        Value::Bool(b) => Val::Bool(*b),
        Value::Number(n) => n
            .as_i64()
            .map_or_else(|| Val::F64(n.as_f64().unwrap()), Val::I64),
        Value::String(s) => Val::Str(s.clone()),
        Value::Array(a) => Val::List(a.iter().map(val).collect()),
        Value::Object(o) if o.contains_key("$error") => {
            let msgs = o["$error"].as_array().unwrap();
            let err = msgs
                .iter()
                .skip(1)
                .fold(ErrorBuf::new(msgs[0].as_str().unwrap()), |e, m| {
                    e.because(ErrorBuf::new(m.as_str().unwrap()))
                });
            Val::Error(err)
        }
        Value::Object(o) => Val::Map(o.iter().map(|(k, v)| (k.clone(), val(v))).collect()),
    }
}

/// Calls `f` with the record a case describes.
fn with_record(r: &Value, f: impl FnOnce(&Record<'_>)) {
    let owned: Vec<(String, Val)> = r["fields"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| (p[0].as_str().unwrap().to_owned(), val(&p[1])))
        .collect();
    let fields: Vec<Field<'_>> = owned
        .iter()
        .map(|(k, v)| (k.as_str(), v.to_value()))
        .collect();
    let msg = r["msg"].as_str().unwrap();
    let line = r["line"].as_u64().map(|n| u32::try_from(n).unwrap());
    f(&Record::new(
        level(&r["level"]),
        r["target"].as_str().unwrap(),
        format_args!("{msg}"),
    )
    .with(&fields)
    .stamped(UNIX_EPOCH + Duration::from_secs(r["time"].as_u64().unwrap()))
    .located(r["file"].as_str(), line));
}

fn render(layout: &Layout, rec: &Record<'_>) -> String {
    let mut s = String::new();
    layout.render(&mut s, rec, None);
    s
}

#[test]
fn renders_every_case() {
    start();
    for case in cases("render")["cases"].as_array().unwrap() {
        with_record(&case["record"], |rec| {
            for p in case["patterns"].as_array().unwrap() {
                let pattern = p["pattern"].as_str().unwrap();
                assert_eq!(
                    render(&Layout::pattern(pattern).unwrap(), rec),
                    p["expect"],
                    "{pattern}"
                );
            }
            for p in case["presets"].as_array().unwrap() {
                let name = p["name"].as_str().unwrap();
                let got = render(&Layout::pattern(name).unwrap(), rec);
                match (p["expect"].as_str(), p["tail"].as_str()) {
                    (Some(want), _) => assert_eq!(got, want, "{name}"),
                    (None, Some(tail)) => assert!(got.ends_with(tail), "{name}: {got}"),
                    _ => panic!("{name}: no expectation"),
                }
            }
            if let Some(want) = case["logfmt"].as_str() {
                assert_eq!(render(&Layout::Logfmt, rec), want);
            }
            if let Some(tail) = case["pretty"].as_str() {
                let got = render(&Layout::Pretty, rec);
                assert!(got.ends_with(tail), "pretty: {got}");
            }
            assert_eq!(render(&Layout::json(Schema::Serlog), rec), case["json"]);
            for s in case["schemas"].as_array().unwrap() {
                let schema: Schema = s["schema"].as_str().unwrap().parse().unwrap();
                let got: Value = serde_json::from_str(&render(&Layout::json(schema), rec)).unwrap();
                let keys: Vec<&str> = got
                    .as_object()
                    .unwrap()
                    .keys()
                    .map(String::as_str)
                    .collect();
                let want: Vec<&str> = s["keys"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|k| k.as_str().unwrap())
                    .collect();
                assert_eq!(keys, want, "{schema:?}");
                for (k, v) in s["has"].as_object().unwrap() {
                    assert_eq!(&got[k], v, "{schema:?} {k}");
                }
            }
        });
    }
}

#[test]
fn filters_every_case() {
    for f in cases("render")["filters"].as_array().unwrap() {
        let spec = f["spec"].as_str().unwrap();
        let filter: Filter = spec.parse().unwrap();
        for (key, want) in [("allow", true), ("deny", false)] {
            for pair in f[key].as_array().unwrap() {
                let (l, t) = (level(&pair[0]), pair[1].as_str().unwrap());
                assert_eq!(filter.allows(l, t), want, "`{spec}` {l:?} {t}");
            }
            for m in f["messages"][key].as_array().into_iter().flatten() {
                assert_eq!(filter.admits(m.as_str().unwrap()), want, "`{spec}` {m}");
            }
        }
    }
}

#[test]
fn health_machine_follows_every_case() {
    for case in cases("fail")["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let secs = |v: &Value| Duration::from_secs_f64(v.as_f64().unwrap());
        let t0 = Instant::now();
        let mut b = Breaker::new(secs(&case["probe"]), t0);
        let tell = match case["tell"].as_str().unwrap() {
            "report" => Tell::Report,
            "silent" => Tell::Silent,
            _ => Tell::Folded,
        };
        for (i, step) in case["steps"].as_array().unwrap().iter().enumerate() {
            let now = t0 + secs(&step["at"]);
            let tried = b.admit(now);
            assert_eq!(tried, step["tried"].as_bool().unwrap(), "{name} step {i}");
            let phase = tried
                .then(|| match step["result"].as_str().unwrap() {
                    "ok" => b.pass(),
                    _ => b.fault(now, "boom".into(), secs(&case["remind"]), tell),
                })
                .flatten();
            assert_eq!(b.health().as_str(), step["health"], "{name} step {i}");
            assert_eq!(
                phase.map(Phase::as_str),
                step["phase"].as_str(),
                "{name} step {i}"
            );
        }
        assert_eq!(b.skipped(), case["skipped"].as_u64().unwrap(), "{name}");
    }
}

struct Script<'a> {
    case: &'a Value,
    tried: Mutex<Vec<String>>,
}

impl Desk for Script<'_> {
    fn logs(&self) -> Vec<String> {
        self.case["logs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect()
    }

    fn resting(&self, name: &str) -> bool {
        self.case["down"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == name)
    }

    fn deliver(&self, name: &str, _: &RecordBuf) -> Result<(), String> {
        self.tried.lock().unwrap().push(name.to_owned());
        match self.case["results"][name].as_str() {
            Some("ok") => Ok(()),
            _ => Err("refused".into()),
        }
    }
}

/// Splits the order sinks were tried in back into tiers, using the tiers the case gives.
fn by_tier(case: &Value, tried: &[String]) -> Vec<HashSet<String>> {
    let mut left: Vec<String> = tried.to_vec();
    let mut out = Vec::new();
    let logs: Vec<String> = case["logs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    for tier in case["tiers"].as_array().unwrap() {
        let members: HashSet<String> = tier
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|n| match n.as_str().unwrap() {
                "*" => logs.clone(),
                n => vec![n.to_owned()],
            })
            .collect();
        let here: HashSet<String> = left
            .iter()
            .filter(|n| members.contains(*n))
            .cloned()
            .collect();
        left.retain(|n| !here.contains(n));
        if !here.is_empty() {
            out.push(here);
        }
    }
    out
}

#[test]
fn alarm_walk_follows_every_case() {
    for case in cases("alarm")["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let tiers: Vec<Vec<String>> = case["tiers"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                t.as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_str().unwrap().to_owned())
                    .collect()
            })
            .collect();
        let alarm: &dyn Alarm = &Tiers::new(tiers);
        let desk = Script {
            case,
            tried: Mutex::new(Vec::new()),
        };
        let mut incident = Incident::new(
            case["failing"].as_str().unwrap(),
            "custom",
            Phase::Opened,
            "boom",
        );
        let outcome = sound(alarm, &mut incident, &desk);
        let want = if case["outcome"] == "heard" {
            Outcome::Heard
        } else {
            Outcome::Unheard
        };
        assert_eq!(outcome, want, "{name}");
        let tried = desk.tried.lock().unwrap().clone();
        let want: Vec<HashSet<String>> = case["tried"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| {
                t.as_array()
                    .unwrap()
                    .iter()
                    .map(|n| n.as_str().unwrap().to_owned())
                    .collect()
            })
            .collect();
        assert_eq!(by_tier(case, &tried), want, "{name}");
        let failures: Vec<&str> = incident.failures.iter().map(|f| f.sink.as_str()).collect();
        let want: Vec<&str> = case["failures"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        assert_eq!(failures, want, "{name}");
    }
}
