//! Start-up from YAML, JSON or TOML, export of the settings in force, and reloading when the file changes.
//!
//! The keys are the same as the builder's settings, so a config entry and code say the same thing.

pub mod kinds;
mod opts;
pub mod units;

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, PoisonError, RwLock},
    thread,
    time::{Duration, SystemTime},
};

pub use opts::Opts;
use serde_json::{Map, Value};

use crate::{
    Alarm, Builder, Compat, Error, Filter, Guard, Layout, Level, OnFail, RecordBuf, Result, Slot,
    Val,
    alarm::{self, Desk, Tiers},
    builder, compat, hub,
    layout::{Json, Levels, Schema, Theme, When},
    meta,
    process::{Processor, Redact},
    slot::{Full, Live, Mode, Queue},
};

/// Config file syntax.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// YAML 1.2.
    Yaml,
    /// JSON.
    Json,
    /// TOML 1.0.
    Toml,
}

impl Format {
    /// Guesses from a file extension: `.json`, `.toml`, anything else YAML.
    #[must_use]
    pub fn of(path: &Path) -> Self {
        match path.extension().and_then(|e| e.to_str()) {
            Some("json") => Self::Json,
            Some("toml") => Self::Toml,
            _ => Self::Yaml,
        }
    }
}

fn bad(e: impl std::fmt::Display) -> Error {
    Error::Config(e.to_string())
}

/// Reads config text as a tree, without expanding variables.
///
/// # Errors
/// When the text does not parse.
pub fn read(text: &str, format: Format) -> Result<Value> {
    match format {
        Format::Yaml => serde_saphyr::from_str(text).map_err(bad),
        Format::Json => serde_json::from_str(text).map_err(bad),
        Format::Toml => toml::from_str(text).map_err(bad),
    }
}

/// Reads config text into a [`Builder`], so code can add to it before `init`.
///
/// `${NAME}` inside any string is replaced by that environment variable.
///
/// # Errors
/// On bad syntax, settings, unset variables or unknown kinds.
pub fn parse(text: &str, format: Format) -> Result<Builder> {
    let mut v = read(text, format)?;
    expand(&mut v)?;
    from_value(v)
}

/// Initialises from a file, the format taken from its extension; reloads it if it sets `watch`.
///
/// # Errors
/// On unreadable files, bad settings or unknown kinds.
pub fn load(path: impl AsRef<Path>) -> Result<Guard> {
    let path = path.as_ref();
    let b = parse(&std::fs::read_to_string(path)?, Format::of(path))?;
    let every = b.watch;
    let guard = b.init()?;
    if let Some(every) = every {
        watch(path.to_owned(), every);
    }
    Ok(guard)
}

/// Initialises from config text.
///
/// # Errors
/// On bad syntax, settings or unknown kinds.
pub fn load_str(text: &str, format: Format) -> Result<Guard> {
    parse(text, format)?.init()
}

/// Builds settings from an already parsed tree, such as the `[log]` table of another program's config.
///
/// # Errors
/// On bad settings or unknown kinds.
pub fn from_value(tree: Value) -> Result<Builder> {
    let Value::Object(map) = tree else {
        return Err(bad("the config must be a map"));
    };
    let mut cfg = Opts::new(map, "config");
    let mut out = builder();
    if let Some(app) = cfg.str("app")? {
        out = out.app(app);
    }
    if let Some(env) = cfg.str("env")? {
        out = out.env(env);
    }
    if let Some(instance) = cfg.str("instance")? {
        out = out.instance(instance);
    }
    if let Some(profile) = cfg.parse::<Compat>("compat")? {
        out = out.compat(profile);
    }
    for (name, value, colour) in levels(cfg.map("levels")?.unwrap_or_default())? {
        out = out.define(name, value, colour);
    }
    if let Some(min) = cfg.level("level")? {
        out = out.level(min);
    }
    if let Some(spec) = cfg.str("filter")? {
        spec.parse::<Filter>()
            .map_err(|e| bad(format!("filter `{spec}`: {e}")))?;
        out = out.filter(spec);
    }
    if let Some(var) = cfg.str("filter_env")? {
        out = out.filter_env(var);
    }
    let mut chain = crate::process::Chain::default();
    for item in cfg.list("process")? {
        chain.push(step(item)?);
    }
    let redact = cfg.strs("redact")?;
    if !redact.is_empty() {
        chain.push(Arc::new(Redact::keys(redact)));
    }
    out = out.chain(&chain);
    let mut named = alarms(cfg.map("alarms")?.unwrap_or_default())?;
    if let Some(default) = named.remove("default") {
        out = out.shared_alarm(default);
    }
    for entry in cfg.list("sinks")? {
        out = out.sink(slot_with(entry, &named)?);
    }
    out.named = named.into_iter().collect();
    out.watch = cfg.duration("watch")?;
    cfg.finish()?;
    Ok(out)
}

fn levels(map: Map<String, Value>) -> Result<Vec<(String, u8, crate::Colour)>> {
    let mut out = Vec::new();
    for (name, spec) in map {
        let (value, colour) = match spec {
            Value::Number(n) => (n.as_u64(), crate::Colour::Plain),
            Value::Object(m) => {
                let mut lo = Opts::new(m, format!("level `{name}`"));
                let value = lo.uint("value")?;
                let colour = lo.parse("colour")?.unwrap_or_default();
                lo.finish()?;
                (value, colour)
            }
            other => {
                return Err(bad(format!(
                    "level `{name}` must be a number or a map, not {other}"
                )));
            }
        };
        let value = value
            .and_then(|n| u8::try_from(n).ok())
            .ok_or_else(|| bad(format!("level `{name}` needs a value from 0 to 255")))?;
        Level::define(&name, value, colour).map_err(|e| bad(format!("level `{name}`: {e}")))?;
        out.push((name, value, colour));
    }
    Ok(out)
}

fn alarms(map: Map<String, Value>) -> Result<HashMap<String, Arc<dyn Alarm>>> {
    let mut out: HashMap<String, Arc<dyn Alarm>> = HashMap::new();
    for (name, spec) in map {
        if name == "panic" || name == "ignore" {
            return Err(bad(format!("`{name}` cannot name an alarm")));
        }
        let Value::Object(m) = spec else {
            return Err(bad(format!("alarm `{name}` must be a map")));
        };
        let mut ao = Opts::new(m, format!("alarm `{name}`"));
        let kind = ao.str("kind")?.unwrap_or_else(|| "tiers".into());
        let make = kinds::alarm_maker(&kind).ok_or_else(|| Error::Kind(kind.clone()))?;
        let alarm = make(&mut ao)?;
        ao.finish()?;
        out.insert(name, alarm);
    }
    Ok(out)
}

fn step(v: Value) -> Result<Arc<dyn Processor>> {
    let (kind, opts) = match v {
        Value::String(s) => (s, Map::new()),
        Value::Object(m) if m.len() == 1 => {
            let Some((k, v)) = m.into_iter().next() else {
                return Err(bad("empty processor"));
            };
            let opts = match v {
                Value::Object(o) => o,
                Value::Bool(true) | Value::Null => Map::new(),
                v => return Err(bad(format!("processor `{k}` takes a map, not {v}"))),
            };
            (k, opts)
        }
        v => {
            return Err(bad(format!(
                "a processor is a name or a one-key map, not {v}"
            )));
        }
    };
    let make = kinds::step_maker(&kind).ok_or_else(|| Error::Kind(kind.clone()))?;
    let mut o = Opts::new(opts, format!("processor `{kind}`"));
    let s = make(&mut o)?;
    o.finish()?;
    Ok(s)
}

fn layout_of(o: &mut Opts) -> Result<Option<Layout>> {
    let named = o.str("layout")?;
    let format = o.str("format")?;
    let pattern = o.str("pattern")?;
    let schema = o.parse::<Schema>("schema")?;
    let keys = o.pairs("keys")?;
    let levels = o.parse::<Levels>("levels")?;
    let nest = o.str("nest")?;
    let jsonish = schema.is_some() || !keys.is_empty() || levels.is_some() || nest.is_some();
    if let Some(n) = named {
        let make = kinds::layout_maker(&n).ok_or_else(|| Error::Kind(n.clone()))?;
        return make(&mut Opts::new(Map::new(), format!("layout `{n}`"))).map(Some);
    }
    let json = |schema: Option<Schema>| {
        let mut j = Json::new(schema.unwrap_or_default());
        for (k, v) in &keys {
            j = j.rename(k, v);
        }
        if let Some(l) = levels {
            j = j.levels(l);
        }
        if let Some(n) = &nest {
            j = j.nest(n);
        }
        Layout::from(j)
    };
    Ok(match (format.as_deref(), pattern) {
        (Some("json"), None) => Some(json(schema)),
        (None, None) if jsonish => Some(json(schema)),
        (Some("logfmt"), None) => Some(Layout::Logfmt),
        (Some("pretty"), None) => Some(Layout::Pretty),
        (None | Some("text"), Some(p)) => Some(Layout::pattern(&p).map_err(bad)?),
        (None | Some("text"), None) => None,
        (Some(f), Some(_)) => {
            return Err(o.bad(format!("`pattern` goes with `format: text`, not `{f}`")));
        }
        (Some(f), None) => {
            return Err(o.bad(format!(
                "`format` takes text, logfmt, json or pretty, not `{f}`"
            )));
        }
    })
}

/// The sink a config entry describes, with its slot settings, for adding at run time with [`crate::add`].
/// `${NAME}` is expanded; `on_fail` may be `panic`, `ignore` or `default`.
///
/// # Errors
/// On bad settings or an unknown kind.
pub fn slot(mut entry: Value) -> Result<Slot> {
    expand(&mut entry)?;
    slot_with(entry, &HashMap::new())
}

fn slot_with(v: Value, named: &HashMap<String, Arc<dyn Alarm>>) -> Result<Slot> {
    let Value::Object(mut map) = v else {
        return Err(bad("each sink must be a map"));
    };
    let Some(Value::String(kind)) = map.remove("kind") else {
        return Err(bad("each sink needs a `kind`"));
    };
    let mut o = Opts::new(map, format!("{kind} sink"));
    let name = o.str("name")?;
    let level = o.level("level")?;
    let filter = o.str("filter")?;
    let filter = match (level, filter) {
        (Some(_), Some(_)) => return Err(o.bad("give `level` or `filter`, not both")),
        (Some(l), None) => Some(Filter::level(l)),
        (None, Some(f)) => Some(
            f.parse::<Filter>()
                .map_err(|e| o.bad(format!("filter `{f}`: {e}")))?,
        ),
        (None, None) => None,
    };
    o.slot_level = filter.is_some();
    let steps = o
        .list("process")?
        .into_iter()
        .map(step)
        .collect::<Result<Vec<_>>>()?;
    let layout = layout_of(&mut o)?;
    let colour = o.parse::<When>("colour")?;
    let mut theme = Theme::default();
    for (l, c) in o.pairs("colours")? {
        let level: Level = l
            .parse()
            .map_err(|e| o.bad(format!("`colours.{l}`: {e}")))?;
        theme = theme.set(
            level,
            c.parse()
                .map_err(|e| o.bad(format!("`colours.{l}`: {e}")))?,
        );
    }
    let alarm_only = o.bool("alarm_only")?.unwrap_or(false);
    let on_fail =
        match o.str("on_fail")?.as_deref() {
            None | Some("default") => OnFail::default(),
            Some("panic") => OnFail::Panic,
            Some("ignore") => OnFail::Ignore,
            Some(a) => OnFail::Alarm(named.get(a).cloned().ok_or_else(|| {
                o.bad(format!("`on_fail` names `{a}`, which is not in `alarms`"))
            })?),
        };
    let shape = Shape {
        size: o.uint("queue")?,
        batch: o.uint("batch")?,
        wait: o.duration("wait")?,
        full: match o.str("full")?.as_deref() {
            None => None,
            Some("drop") => Some(Full::Drop),
            Some("block") => Some(Full::Block),
            Some(f) => return Err(o.bad(format!("`full` takes drop or block, not `{f}`"))),
        },
    };
    let retries = o.uint("retries")?;
    let probe = o.duration("probe")?;
    let make = kinds::sink_maker(&kind).ok_or_else(|| Error::Kind(kind.clone()))?;
    let sink = make(&mut o)?;
    o.finish()?;
    let mode = shape.mode(sink.queued());
    let mut s = Slot::boxed(sink).on_fail(on_fail).theme(theme);
    if let Some(n) = name {
        s = s.name(n);
    }
    if let Some(f) = filter {
        s = s.filter(f);
    }
    for st in steps {
        s.process.push(st);
    }
    if let Some(l) = layout {
        s = s.layout(l);
    }
    if let Some(c) = colour {
        s = s.colour(c);
    }
    if alarm_only {
        s = s.alarm_only();
    }
    match mode {
        Mode::Queue(q) => s = s.queue(q),
        Mode::Direct => s = s.direct(),
        Mode::Sink => {}
    }
    if let Some(n) = retries {
        s = s.retries(u32::try_from(n).unwrap_or(u32::MAX));
    }
    if let Some(p) = probe {
        s = s.probe(p);
    }
    Ok(s)
}

/// The queue keys of a sink entry.
struct Shape {
    size: Option<u64>,
    batch: Option<u64>,
    wait: Option<Duration>,
    full: Option<Full>,
}

impl Shape {
    /// The sink's own choice when no key was given, direct for `queue: 0`, otherwise a queue built on the sink's own.
    fn mode(&self, own: Option<Queue>) -> Mode {
        if self.size.is_none() && self.batch.is_none() && self.wait.is_none() && self.full.is_none()
        {
            return Mode::Sink;
        }
        if self.size == Some(0) {
            return Mode::Direct;
        }
        let n = |v: u64| usize::try_from(v).unwrap_or(usize::MAX);
        let mut q = self
            .size
            .map_or_else(|| own.unwrap_or(Queue::new(1024)), |s| Queue::new(n(s)));
        if let Some(b) = self.batch {
            q = q.batch(n(b));
        }
        if let Some(w) = self.wait {
            q = q.wait(w);
        }
        if let Some(f) = self.full {
            q = q.full(f);
        }
        Mode::Queue(q)
    }
}

fn expand(v: &mut Value) -> Result {
    match v {
        Value::String(s) if s.contains("${") => {
            let mut out = String::with_capacity(s.len());
            let mut rest = s.as_str();
            while let Some(i) = rest.find("${") {
                let end = rest[i..].find('}').ok_or_else(|| bad("unclosed `${`"))? + i;
                let name = &rest[i + 2..end];
                out.push_str(&rest[..i]);
                out.push_str(
                    &std::env::var(name)
                        .map_err(|_| bad(format!("environment variable {name} is not set")))?,
                );
                rest = &rest[end + 1..];
            }
            out.push_str(rest);
            *s = out;
        }
        Value::Array(a) => a.iter_mut().try_for_each(expand)?,
        Value::Object(o) => o.values_mut().try_for_each(expand)?,
        _ => {}
    }
    Ok(())
}

/// A field value as JSON.
pub(crate) fn to_json(v: &Val) -> Value {
    match v {
        Val::Null => Value::Null,
        Val::Bool(b) => (*b).into(),
        Val::I64(n) => (*n).into(),
        Val::U64(n) => (*n).into(),
        Val::F64(n) => (*n).into(),
        Val::Str(s) => s.clone().into(),
        Val::Error(e) => e.to_string().into(),
        Val::List(l) => l.iter().map(to_json).collect(),
        Val::Map(m) => m
            .iter()
            .map(|(k, v)| (k.clone(), to_json(v)))
            .collect::<Map<_, _>>()
            .into(),
    }
}

/// A JSON value as a field value.
pub(crate) fn from_json(v: &Value) -> Val {
    match v {
        Value::Null => Val::Null,
        Value::Bool(b) => Val::Bool(*b),
        Value::Number(n) => n
            .as_i64()
            .map(Val::I64)
            .or_else(|| n.as_u64().map(Val::U64))
            .unwrap_or_else(|| Val::F64(n.as_f64().unwrap_or_default())),
        Value::String(s) => Val::Str(s.clone()),
        Value::Array(a) => Val::List(a.iter().map(from_json).collect()),
        Value::Object(m) => Val::Map(m.iter().map(|(k, v)| (k.clone(), from_json(v))).collect()),
    }
}

static NAMED: RwLock<Vec<(String, Arc<dyn Alarm>)>> = RwLock::new(Vec::new());

pub(crate) fn name_alarms(named: Vec<(String, Arc<dyn Alarm>)>) {
    *NAMED.write().unwrap_or_else(PoisonError::into_inner) = named;
}

/// Writes the settings in force as config text that loads back to the same behaviour.
///
/// Passwords come out as `***`.
///
/// # Errors
/// When a custom sink, processor, layout or alarm cannot describe itself.
pub fn dump(format: Format) -> Result<String> {
    let m = meta();
    let mut top = Map::new();
    for (k, v) in [("app", &m.app), ("env", &m.env), ("instance", &m.instance)] {
        if !v.is_empty() {
            top.insert(k.into(), v.clone().into());
        }
    }
    if compat() != Compat::Serlog {
        top.insert("compat".into(), compat().name().into());
    }
    let defined = Level::defined();
    if !defined.is_empty() {
        let levels: Map<String, Value> = defined
            .into_iter()
            .map(|(name, value, colour)| {
                let v = if colour == crate::Colour::Plain {
                    value.into()
                } else {
                    serde_json::json!({ "value": value, "colour": colour.name() })
                };
                (name.to_owned(), v)
            })
            .collect();
        top.insert("levels".into(), levels.into());
    }
    top.insert("filter".into(), hub::filter().to_string().into());
    let steps = steps_json(hub::chain().steps(), "the global chain")?;
    if !steps.is_empty() {
        top.insert("process".into(), steps.into());
    }
    let mut alarms = Map::new();
    let named = NAMED.read().unwrap_or_else(PoisonError::into_inner).clone();
    let default = alarm::current();
    let plain = Tiers::default().options();
    let current = default
        .options()
        .ok_or_else(|| bad("the default alarm cannot be exported"))?;
    if Some(&current) != plain.as_ref() {
        alarms.insert("default".into(), current.into());
    }
    let mut extra: Named = Vec::new();
    let sinks = hub::with_table(|t| {
        t.iter()
            .map(|l| sink_json(l, &named, &mut extra))
            .collect::<Result<Vec<_>>>()
    })?;
    for (name, a) in named.iter().chain(&extra) {
        let o = a
            .options()
            .ok_or_else(|| bad(format!("alarm `{name}` cannot be exported")))?;
        alarms.insert(name.clone(), o.into());
    }
    top.insert("sinks".into(), sinks.into());
    if !alarms.is_empty() {
        top.insert("alarms".into(), alarms.into());
    }
    let v = Value::Object(top);
    match format {
        Format::Yaml => serde_saphyr::to_string(&v).map_err(bad),
        Format::Json => serde_json::to_string_pretty(&v).map_err(bad),
        Format::Toml => toml::to_string(&v).map_err(bad),
    }
}

fn steps_json(steps: &[Arc<dyn Processor>], whose: &str) -> Result<Vec<Value>> {
    steps
        .iter()
        .map(|s| {
            s.options().ok_or_else(|| {
                bad(format!(
                    "a `{}` processor in {whose} cannot be exported",
                    s.kind()
                ))
            })
        })
        .collect()
}

type Named = Vec<(String, Arc<dyn Alarm>)>;

fn sink_json(live: &Live, named: &Named, extra: &mut Named) -> Result<Value> {
    let core = &live.core;
    let mut out = Map::new();
    out.insert("kind".into(), core.kind.into());
    out.insert("name".into(), core.name.clone().into());
    let own = core
        .sink
        .options()
        .ok_or_else(|| bad(format!("sink `{}` cannot be exported", core.name)))?;
    out.extend(own);
    if let Some(f) = &live.filter {
        out.insert("filter".into(), f.to_string().into());
    }
    let steps = steps_json(live.process.steps(), &format!("sink `{}`", core.name))?;
    if !steps.is_empty() {
        out.insert("process".into(), steps.into());
    }
    if let Some(layout) = &live.given.layout {
        layout_json(layout, &mut out).ok_or_else(|| {
            bad(format!(
                "the layout of sink `{}` cannot be exported",
                core.name
            ))
        })?;
    }
    if live.given.colour != When::Auto {
        out.insert("colour".into(), live.given.colour.name().into());
    }
    let overrides = live.given.theme.overrides();
    if !overrides.is_empty() {
        let theme: Map<String, Value> = overrides
            .iter()
            .map(|(level, colour)| (level.name().to_owned(), colour.name().into()))
            .collect();
        out.insert("colours".into(), theme.into());
    }
    if live.alarm_only {
        out.insert("alarm_only".into(), true.into());
    }
    if let Some(policy) = on_fail_json(&core.on_fail, named, extra) {
        out.insert("on_fail".into(), policy.into());
    }
    if live.given.queue {
        queue_json(live.queue, &mut out);
    }
    if live.given.retries {
        out.insert("retries".into(), core.retries().into());
    }
    if live.probe != Duration::from_secs(5) {
        out.insert("probe".into(), units::show(live.probe).into());
    }
    Ok(out.into())
}

/// Writes the keys for `layout`; `None` for a closure, which cannot be written.
fn layout_json(layout: &Layout, out: &mut Map<String, Value>) -> Option<()> {
    match layout {
        Layout::Inherit => {}
        Layout::Pattern(p) => {
            out.insert("pattern".into(), p.source().into());
        }
        Layout::Logfmt => {
            out.insert("format".into(), "logfmt".into());
        }
        Layout::Pretty => {
            out.insert("format".into(), "pretty".into());
        }
        Layout::Json(json) => {
            out.insert("format".into(), "json".into());
            if json.schema() != Schema::Serlog {
                out.insert("schema".into(), json.schema().name().into());
            }
            if !json.renamed().is_empty() {
                let keys: Map<String, Value> = json
                    .renamed()
                    .iter()
                    .map(|(from, to)| (from.clone(), to.clone().into()))
                    .collect();
                out.insert("keys".into(), keys.into());
            }
            if let Some(style) = json.level_style() {
                out.insert(
                    "levels".into(),
                    if style == Levels::Number {
                        "number"
                    } else {
                        "name"
                    }
                    .into(),
                );
            }
            if let Some(key) = json.nested() {
                out.insert("nest".into(), key.into());
            }
        }
        _ => return None,
    }
    Some(())
}

/// The `on_fail` value, naming alarms that came from code as `alarm-1`, `alarm-2`…
fn on_fail_json(policy: &OnFail, named: &Named, extra: &mut Named) -> Option<String> {
    match policy {
        OnFail::Panic => Some("panic".into()),
        OnFail::Ignore => Some("ignore".into()),
        OnFail::Alarm(a) if alarm::is_default(a) => None,
        OnFail::Alarm(a) => {
            if let Some((name, _)) = named
                .iter()
                .chain(extra.iter())
                .find(|(_, x)| Arc::ptr_eq(x, a))
            {
                return Some(name.clone());
            }
            let name = format!("alarm-{}", extra.len() + 1);
            extra.push((name.clone(), a.clone()));
            Some(name)
        }
    }
}

fn queue_json(queue: Option<Queue>, out: &mut Map<String, Value>) {
    let Some(q) = queue else {
        out.insert("queue".into(), 0.into());
        return;
    };
    out.insert("queue".into(), q.size.into());
    out.insert("batch".into(), q.batch.into());
    out.insert("wait".into(), units::show(q.wait).into());
    if q.full == Full::Block {
        out.insert("full".into(), "block".into());
    }
}

fn watch(path: PathBuf, every: Duration) {
    let stamp = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    let mut seen: Option<SystemTime> = stamp(&path);
    let _ = thread::Builder::new()
        .name("serlog-watch".into())
        .spawn(move || {
            loop {
                thread::sleep(every);
                let now = stamp(&path);
                if now == seen {
                    continue;
                }
                seen = now;
                let result = std::fs::read_to_string(&path)
                    .map_err(Error::from)
                    .and_then(|t| parse(&t, Format::of(&path)))
                    .and_then(|b| b.plan(&[]));
                match result {
                    Ok(plan) => builder::apply(plan, true),
                    Err(e) => {
                        let rec = RecordBuf::new(
                            Level::ERROR,
                            "serlog",
                            format!("reloading {} failed: {e}", path.display()),
                        );
                        let _ = hub::Board.deliver("stderr", &rec);
                    }
                }
            }
        });
}

#[cfg(test)]
mod tests {
    use super::{Format, read};

    #[test]
    fn reads_all_three_formats_alike() {
        let y = read("sinks:\n  - kind: console\n", Format::Yaml).unwrap();
        let j = read(r#"{"sinks":[{"kind":"console"}]}"#, Format::Json).unwrap();
        let t = read("[[sinks]]\nkind = \"console\"\n", Format::Toml).unwrap();
        assert_eq!(y, j);
        assert_eq!(j, t);
    }
}
