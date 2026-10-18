//! The four kinds of extension a config file can name: sinks, processors, layouts and alarms.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, PoisonError, RwLock},
};

use serde_json::Value;

use super::{Opts, from_json};
use crate::{
    Alarm, Fallback, Layout, Result, Sink, Tiers,
    process::{Dedupe, Exclude, Fields, Limit, Match, Processor, Redact, Sample, Sanitise},
};

/// Builds a sink from the keys its entry has left once the slot took its own.
pub type SinkMaker = dyn Fn(&mut Opts) -> Result<Box<dyn Sink>> + Send + Sync;
/// Builds a processor from its options.
pub type StepMaker = dyn Fn(&mut Opts) -> Result<Arc<dyn Processor>> + Send + Sync;
/// Builds a layout from its options.
pub type LayoutMaker = dyn Fn(&mut Opts) -> Result<Layout> + Send + Sync;
/// Builds an alarm from its options.
pub type AlarmMaker = dyn Fn(&mut Opts) -> Result<Arc<dyn Alarm>> + Send + Sync;

#[derive(Default)]
struct Kinds {
    sinks: HashMap<String, Arc<SinkMaker>>,
    steps: HashMap<String, Arc<StepMaker>>,
    layouts: HashMap<String, Arc<LayoutMaker>>,
    alarms: HashMap<String, Arc<AlarmMaker>>,
}

static KINDS: LazyLock<RwLock<Kinds>> = LazyLock::new(|| RwLock::new(builtin()));

fn edit(f: impl FnOnce(&mut Kinds)) {
    f(&mut KINDS.write().unwrap_or_else(PoisonError::into_inner));
}

fn look<T>(f: impl FnOnce(&Kinds) -> T) -> T {
    f(&KINDS.read().unwrap_or_else(PoisonError::into_inner))
}

/// Lets config files name a new kind of sink; a later call with the same name replaces it.
pub fn sink(
    name: impl Into<String>,
    make: impl Fn(&mut Opts) -> Result<Box<dyn Sink>> + Send + Sync + 'static,
) {
    edit(|k| {
        k.sinks.insert(name.into(), Arc::new(make));
    });
}

/// Lets config files name a new kind of processor.
pub fn step(
    name: impl Into<String>,
    make: impl Fn(&mut Opts) -> Result<Arc<dyn Processor>> + Send + Sync + 'static,
) {
    edit(|k| {
        k.steps.insert(name.into(), Arc::new(make));
    });
}

/// Lets config files name a layout with `layout: name`.
pub fn layout(
    name: impl Into<String>,
    make: impl Fn(&mut Opts) -> Result<Layout> + Send + Sync + 'static,
) {
    edit(|k| {
        k.layouts.insert(name.into(), Arc::new(make));
    });
}

/// Lets config files name a new kind of alarm with `kind: name`.
pub fn alarm(
    name: impl Into<String>,
    make: impl Fn(&mut Opts) -> Result<Arc<dyn Alarm>> + Send + Sync + 'static,
) {
    edit(|k| {
        k.alarms.insert(name.into(), Arc::new(make));
    });
}

pub(crate) fn sink_maker(name: &str) -> Option<Arc<SinkMaker>> {
    look(|k| k.sinks.get(name).cloned())
}

pub(crate) fn step_maker(name: &str) -> Option<Arc<StepMaker>> {
    look(|k| k.steps.get(name).cloned())
}

pub(crate) fn layout_maker(name: &str) -> Option<Arc<LayoutMaker>> {
    look(|k| k.layouts.get(name).cloned())
}

pub(crate) fn alarm_maker(name: &str) -> Option<Arc<AlarmMaker>> {
    look(|k| k.alarms.get(name).cloned())
}

fn builtin() -> Kinds {
    let mut k = Kinds::default();
    let mut sink = |name: &str, f: Box<SinkMaker>| {
        k.sinks.insert(name.into(), f.into());
    };
    #[cfg(feature = "console")]
    sink("console", Box::new(console));
    #[cfg(feature = "file")]
    sink("file", Box::new(file));
    #[cfg(feature = "loki")]
    sink("loki", Box::new(loki));
    #[cfg(feature = "mail")]
    sink("mail", Box::new(mail));
    sink("memory", Box::new(memory));
    let steps: [(&str, Box<StepMaker>); 8] = [
        ("redact", Box::new(redact)),
        ("sanitise", Box::new(|_| Ok(Arc::new(Sanitise)))),
        ("fields", Box::new(fields)),
        ("match", Box::new(|o| Ok(Arc::new(matcher(o)?)))),
        ("exclude", Box::new(|o| Ok(Arc::new(Exclude(matcher(o)?))))),
        ("sample", Box::new(sample)),
        ("limit", Box::new(limit)),
        ("dedupe", Box::new(dedupe)),
    ];
    for (n, f) in steps {
        k.steps.insert(n.into(), f.into());
    }
    k.alarms.insert("tiers".into(), Arc::new(tiers));
    k
}

#[cfg(feature = "console")]
fn console(o: &mut Opts) -> Result<Box<dyn Sink>> {
    Ok(Box::new(match o.parse("stream")? {
        Some(s) => crate::sinks::Console::on(s),
        None => crate::sinks::Console::new(),
    }))
}

#[cfg(feature = "file")]
fn file(o: &mut Opts) -> Result<Box<dyn Sink>> {
    use crate::sinks::File;
    let mut f = File::new(o.need("path")?);
    if o.bool("append")? == Some(false) {
        f = f.truncate();
    }
    if let Some(p) = o.str("perms")? {
        f = f.perms(
            u32::from_str_radix(p.trim_start_matches("0o"), 8)
                .map_err(|_| o.bad(format!("bad `perms` `{p}`")))?,
        );
    }
    if o.bool("mkdir")? == Some(false) {
        f = f.no_mkdir();
    }
    if let Some(n) = o.size("max_size")? {
        f = f.max_size(n);
    }
    if let Some(r) = o.parse("rotate")? {
        f = f.rotate(r);
    }
    if let Some(n) = o.uint("keep")? {
        f = f.keep(usize::try_from(n).unwrap_or(usize::MAX));
    }
    if let Some(a) = o.duration("max_age")? {
        f = f.max_age(a);
    }
    if let Some(n) = o.size("max_total")? {
        f = f.max_total(n);
    }
    if o.bool("gzip")? == Some(true) {
        f = f.gzip();
    }
    if let Some(t) = o.str("roll_name")? {
        f = f.roll_name(t);
    }
    if o.bool("reopen")? == Some(true) {
        f = f.reopen();
    }
    if let Some(n) = o.size("buffer")? {
        let every = o
            .duration("flush_every")?
            .unwrap_or(std::time::Duration::from_secs(1));
        f = f.buffer(usize::try_from(n).unwrap_or(usize::MAX), every);
    }
    Ok(Box::new(f))
}

#[cfg(feature = "loki")]
fn loki(o: &mut Opts) -> Result<Box<dyn Sink>> {
    let mut l = crate::sinks::Loki::new(o.need("url")?);
    for (k, v) in o.pairs("labels")? {
        l = l.label(k, v);
    }
    if let Some(t) = o.duration("timeout")? {
        l = l.timeout(t);
    }
    if o.bool("connect")? == Some(true) {
        l = l.connect();
    }
    Ok(Box::new(l))
}

#[cfg(feature = "mail")]
fn mail(o: &mut Opts) -> Result<Box<dyn Sink>> {
    use crate::sinks::{Mail, Security};
    let mut m = Mail::new(o.need("host")?);
    if let Some(p) = o.uint("port")? {
        m = m.port(u16::try_from(p).map_err(|_| o.bad("`port` must be 1 to 65535"))?);
    }
    m = m.security(match o.str("security")?.as_deref() {
        None | Some("tls") => Security::Tls,
        Some("starttls") => Security::StartTls,
        Some("none") => Security::None,
        Some(s) => return Err(o.bad(format!("`security` takes tls, starttls or none, not `{s}`"))),
    });
    match (o.str("user")?, o.str("pass")?) {
        (Some(u), Some(p)) => m = m.auth(u, p),
        (Some(_), None) => return Err(o.bad("`user` needs `pass`")),
        (None, Some(_)) => return Err(o.bad("`pass` needs `user`")),
        (None, None) => {}
    }
    if let Some(f) = o.str("from")? {
        m = m.from(f);
    }
    for to in o.strs("to")? {
        m = m.to(to);
    }
    if let Some(t) = o.duration("timeout")? {
        m = m.timeout(t);
    }
    if o.bool("connect")? == Some(true) {
        m = m.connect();
    }
    if o.slot_level() {
        m = m.level(crate::Level::new(0));
    }
    Ok(Box::new(m))
}

fn memory(o: &mut Opts) -> Result<Box<dyn Sink>> {
    let n = o.uint("size")?.unwrap_or(1000);
    Ok(Box::new(crate::sinks::Memory::new(
        usize::try_from(n).unwrap_or(usize::MAX),
    )))
}

fn redact(o: &mut Opts) -> Result<Arc<dyn Processor>> {
    let mut r = Redact::keys(o.strs("keys")?);
    for p in o.strs("paths")? {
        r = r.path(&p);
    }
    let patterns = o.strs("patterns")?;
    #[cfg(feature = "regex")]
    for p in patterns {
        r = r.pattern(&p).map_err(|e| o.bad(e))?;
    }
    #[cfg(not(feature = "regex"))]
    if let Some(p) = patterns.first() {
        return Err(o.bad(format!("pattern `{p}` needs the `regex` feature")));
    }
    if let Some(m) = o.str("mask")? {
        r = r.mask(m);
    }
    Ok(Arc::new(r))
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "every maker has the same signature"
)]
fn fields(o: &mut Opts) -> Result<Arc<dyn Processor>> {
    let keys: Vec<String> = o.keys();
    let mut f = Fields::new();
    for k in keys {
        if let Some(v) = o.take(&k) {
            f = f.with(k, from_json(&v));
        }
    }
    Ok(Arc::new(f))
}

fn matcher(o: &mut Opts) -> Result<Match> {
    let mut m = Match::new();
    for t in o.strs("tags")? {
        m = m.tag(t);
    }
    for (k, v) in o.pairs("fields")? {
        m = m.field(k, v);
    }
    if let Some(l) = o.level("level")? {
        m = m.level(l);
    }
    Ok(m)
}

fn sample(o: &mut Opts) -> Result<Arc<dyn Processor>> {
    let first = o.uint("first")?.unwrap_or(100);
    let every = o.uint("every")?.unwrap_or(100);
    let per = o
        .duration("per")?
        .unwrap_or(std::time::Duration::from_secs(1));
    Ok(Arc::new(Sample::new(first, every, per)))
}

fn limit(o: &mut Opts) -> Result<Arc<dyn Processor>> {
    let rate = o.num("rate")?.ok_or_else(|| o.bad("`rate` is required"))?;
    let burst = o.num("burst")?.unwrap_or(rate);
    Ok(Arc::new(Limit::new(rate, burst)))
}

fn dedupe(o: &mut Opts) -> Result<Arc<dyn Processor>> {
    let within = o
        .duration("within")?
        .unwrap_or(std::time::Duration::from_secs(1));
    Ok(Arc::new(Dedupe::new(within)))
}

fn tiers(o: &mut Opts) -> Result<Arc<dyn Alarm>> {
    let mut tiers: Vec<Vec<String>> = Vec::new();
    for t in o.list("tiers")? {
        match t {
            Value::String(s) => tiers.push(vec![s]),
            Value::Array(a) => tiers.push(
                a.into_iter()
                    .map(|v| match v {
                        Value::String(s) => Ok(s),
                        v => Err(o.bad(format!("tier members must be sink names, not {v}"))),
                    })
                    .collect::<Result<_>>()?,
            ),
            v => return Err(o.bad(format!("a tier must be a list of sink names, not {v}"))),
        }
    }
    if tiers.is_empty() || tiers.iter().any(Vec::is_empty) {
        return Err(o.bad("`tiers` needs at least one non-empty tier"));
    }
    let mut a = Tiers::new(tiers);
    match o.str("on_fail")?.as_deref() {
        None | Some("ignore") => {}
        Some("panic") => a = a.fallback(Fallback::Panic),
        Some(s) => {
            return Err(o.bad(format!(
                "an alarm's `on_fail` is panic or ignore, not `{s}`"
            )));
        }
    }
    if let Some(r) = o.duration("remind")? {
        a = a.remind(r);
    }
    Ok(Arc::new(a))
}
