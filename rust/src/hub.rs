//! The hub: global filter, context, global processors, the slot table and dispatch.
//!
//! Like most loggers it starts at `info` and prints to standard output until it is configured.

use std::{
    collections::{HashMap, HashSet},
    fmt,
    sync::{
        Arc, LazyLock, Mutex, PoisonError, RwLock, RwLockReadGuard,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::{Duration, Instant},
};

use crate::{
    Error, Field, Filter, Level, Record, Result, Slot, Status, ToValue,
    alarm::{self, Desk},
    guard,
    process::{Chain, Draft, Processor, Redact},
    scope,
    sink::Cache,
    slot::{Core, Live},
};

const OFF: u8 = u8::MAX;

static FLOOR: AtomicU8 = AtomicU8::new(Level::INFO.value());
static FILTER: RwLock<Filter> = RwLock::new(Filter::level(Level::INFO));
static PROCESS: LazyLock<RwLock<Arc<Chain>>> = LazyLock::new(RwLock::default);
static TABLE: LazyLock<RwLock<Table>> = LazyLock::new(RwLock::default);
static EDIT: Mutex<()> = Mutex::new(());
static CONFIGURED: AtomicBool = AtomicBool::new(false);

#[cfg(feature = "console")]
static FALLBACK: LazyLock<Live> =
    LazyLock::new(|| Live::new(Slot::new(crate::sinks::Console::new()), "console".into()));

#[cfg(feature = "console")]
static STDERR: LazyLock<Live> = LazyLock::new(|| {
    Live::new(
        Slot::new(crate::sinks::Console::stderr()).on_fail(crate::OnFail::Ignore),
        "stderr".into(),
    )
});

/// The slots, swapped whole on every change so dispatch never holds a lock while a sink runs.
type Table = Arc<Vec<Arc<Live>>>;

fn table() -> Table {
    read(&TABLE).clone()
}

/// Builds the next table from the current one; changes are serialised, readers never wait for them.
fn change<R>(f: impl FnOnce(&mut Vec<Arc<Live>>) -> R) -> R {
    let _one = EDIT.lock().unwrap_or_else(PoisonError::into_inner);
    let mut next: Vec<Arc<Live>> = table().iter().cloned().collect();
    let out = f(&mut next);
    *write(&TABLE) = Arc::new(next);
    out
}

fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(PoisonError::into_inner)
}

fn write<T>(lock: &RwLock<T>) -> std::sync::RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(PoisonError::into_inner)
}

/// Whether a record at `level` could pass the filter for some target; the macros check this first.
#[inline]
pub fn enabled(level: Level) -> bool {
    level.value() >= FLOOR.load(Ordering::Relaxed)
}

/// The global filter now in force.
#[must_use]
pub fn filter() -> Filter {
    read(&FILTER).clone()
}

/// Replaces the global filter; takes effect at once.
pub fn set_filter(filter: Filter) {
    FLOOR.store(filter.floor().map_or(OFF, Level::value), Ordering::Relaxed);
    *write(&FILTER) = filter;
}

/// Lets through `min` and above for every target.
pub fn set_level(min: Level) {
    set_filter(Filter::level(min));
}

/// Replaces the global processor chain.
pub fn set_process(chain: Chain) {
    *write(&PROCESS) = Arc::new(chain);
}

/// Adds a step to the end of the global processor chain.
pub fn process(step: impl Processor + 'static) {
    let mut lock = write(&PROCESS);
    let mut chain = Chain::clone(&lock);
    chain.push(Arc::new(step));
    *lock = Arc::new(chain);
}

/// Masks fields with these names, wherever they appear.
pub fn redact(keys: impl IntoIterator<Item = impl Into<String>>) {
    process(Redact::keys(keys));
}

#[cfg(feature = "config")]
pub(crate) fn chain() -> Chain {
    Chain::clone(&read(&PROCESS))
}

/// Registers a sink with its slot settings at run time; returns the name it got.
///
/// # Errors
/// When the name or a claimed resource is taken, or the sink fails its start-up check.
pub fn add(slot: impl Into<Slot>) -> Result<String> {
    let name = change(|t| -> Result<String> {
        let live = prepare(vec![slot.into()], t)?;
        let name = live[0].name().to_owned();
        t.extend(live.into_iter().map(Arc::new));
        Ok(name)
    })?;
    CONFIGURED.store(true, Ordering::Release);
    Ok(name)
}

/// Takes a slot out, writes what its queue holds and lets its sink go, which closes it. `false` when no slot has
/// that name.
#[must_use]
pub fn remove(name: &str) -> bool {
    let gone: Vec<_> = change(|t| {
        let (gone, kept) = t.drain(..).partition(|l| l.name() == name);
        *t = kept;
        gone
    });
    let found = !gone.is_empty();
    retire(gone);
    found
}

/// Waits for what the slots' queues hold; dropping them then ends their threads and closes their sinks.
fn retire(gone: Vec<Arc<Live>>) {
    let deadline = Instant::now() + Duration::from_secs(5);
    for l in &gone {
        l.flush(deadline);
    }
    drop(gone);
}

/// Every slot's state.
#[must_use]
pub fn status() -> Vec<Status> {
    table().iter().map(|l| l.core.status()).collect()
}

/// Waits up to five seconds for every queue and the alarm route to drain.
pub fn flush() {
    let deadline = Instant::now() + Duration::from_secs(5);
    flush_slots(Duration::from_secs(5));
    alarm::settle(deadline.saturating_duration_since(Instant::now()));
}

/// Logs every panic at `fatal`, with where it happened, before the previous panic hook runs.
pub fn catch_panics() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let msg = payload
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
            .unwrap_or("panicked");
        let at = info.location();
        let current = std::thread::current();
        let fields: [Field<'_>; 1] = [("thread", crate::Value::Str(current.name().unwrap_or("?")))];
        emit(
            &Record::new(Level::FATAL, "panic", format_args!("{msg}"))
                .located(
                    at.map(std::panic::Location::file),
                    at.map(std::panic::Location::line),
                )
                .with(&fields),
        );
        flush_slots(Duration::from_secs(1));
        previous(info);
    }));
}

pub(crate) fn flush_slots(within: Duration) {
    let deadline = Instant::now() + within;
    for l in table().iter() {
        l.flush(deadline);
    }
}

/// Sends `rec` through the filter, the context and the global processors, then to every slot on the log route.
pub fn emit(rec: &Record<'_>) {
    if guard::suppress() {
        return;
    }
    {
        let f = read(&FILTER);
        if !f.allows(rec.level(), rec.target())
            || (f.has_pattern() && !f.admits(&rec.msg().to_string()))
        {
            return;
        }
    }
    scope::with(|bound| {
        if bound.is_empty() {
            return processed(rec);
        }
        let all: Vec<Field<'_>> = bound
            .iter()
            .map(|(k, v)| (k.as_str(), v.to_value()))
            .chain(rec.fields())
            .collect();
        processed(&rec.with(&all));
    });
}

fn processed(rec: &Record<'_>) {
    let chain = read(&PROCESS).clone();
    if chain.is_empty() {
        return dispatch(rec);
    }
    let mut draft = Draft::new(*rec);
    let kept = {
        let _quiet = guard::quiet();
        chain.run(&mut draft)
    };
    if !kept {
        return;
    }
    match draft.into_buf() {
        Some(buf) => dispatch(&buf.as_record()),
        None => dispatch(rec),
    }
}

fn dispatch(rec: &Record<'_>) {
    let cache = Cache::default();
    if !CONFIGURED.load(Ordering::Acquire) {
        #[cfg(feature = "console")]
        FALLBACK.emit(rec, &cache);
        return;
    }
    for l in table().iter().filter(|l| !l.alarm_only) {
        l.emit(rec, &cache);
    }
}

/// Builds and emits a record; the macros call this.
#[expect(clippy::too_many_arguments, reason = "one call per macro expansion")]
pub fn log(
    level: Level,
    target: &str,
    module: &'static str,
    func: &'static str,
    file: &'static str,
    line: u32,
    tags: &[&str],
    fields: &[Field<'_>],
    args: fmt::Arguments<'_>,
) {
    emit(
        &Record::new(level, target, args)
            .caller(crate::Caller {
                module: Some(module),
                func: Some(func),
                file: Some(file),
                line: Some(line),
            })
            .tagged(tags)
            .with(fields),
    );
}

/// Names, checks and wraps new slots against those already registered; nothing is kept on error.
pub(crate) fn prepare(slots: Vec<Slot>, existing: &[Arc<Live>]) -> Result<Vec<Live>> {
    let mut names: HashSet<String> = existing.iter().map(|l| l.name().to_owned()).collect();
    names.insert("*".into());
    let mut claims: HashSet<String> = existing.iter().filter_map(|l| l.claim.clone()).collect();
    let mut counts: HashMap<&'static str, usize> = HashMap::new();
    for l in existing {
        *counts.entry(l.core.kind).or_default() += 1;
    }
    let mut out = Vec::with_capacity(slots.len());
    for slot in slots {
        let kind = slot.sink.kind();
        let name = match &slot.name {
            Some(n) => n.clone(),
            None => loop {
                let n = counts.entry(kind).or_default();
                *n += 1;
                let candidate = format!("{kind}-{n}");
                if !names.contains(&candidate) {
                    break candidate;
                }
            },
        };
        if !names.insert(name.clone()) {
            return Err(Error::Name(name));
        }
        if let Some(c) = slot.sink.claim()
            && !claims.insert(c.clone())
        {
            return Err(Error::Claimed(c));
        }
        slot.sink.ready().map_err(|e| Error::NotReady {
            sink: name.clone(),
            cause: Box::new(e),
        })?;
        out.push(Live::new(slot, name));
    }
    Ok(out)
}

/// Every name an alarm may route to: registered slots, `stderr` and `*`.
pub(crate) fn known(extra: &[Live]) -> HashSet<String> {
    let mut all: HashSet<String> = table().iter().map(|l| l.name().to_owned()).collect();
    all.extend(extra.iter().map(|l| l.name().to_owned()));
    all.insert("stderr".into());
    all.insert("*".into());
    all
}

/// Puts prepared slots in the table.
pub(crate) fn install(live: Vec<Live>) {
    change(|t| t.extend(live.into_iter().map(Arc::new)));
    CONFIGURED.store(true, Ordering::Release);
}

/// Swaps the whole table, for reloading a config.
pub(crate) fn replace(live: Vec<Live>) {
    let old = change(|t| std::mem::replace(t, live.into_iter().map(Arc::new).collect()));
    CONFIGURED.store(true, Ordering::Release);
    retire(old);
}

pub(crate) fn with_table<R>(f: impl FnOnce(&[Arc<Live>]) -> R) -> R {
    f(&table())
}

fn find(name: &str) -> Option<Arc<Core>> {
    if let Some(l) = table().iter().find(|l| l.name() == name) {
        return Some(l.core.clone());
    }
    #[cfg(feature = "console")]
    if name == "stderr" {
        return Some(STDERR.core.clone());
    }
    None
}

/// The registered slots, as the alarm walk sees them.
pub(crate) struct Board;

impl Desk for Board {
    fn logs(&self) -> Vec<String> {
        table()
            .iter()
            .filter(|l| !l.alarm_only)
            .map(|l| l.name().to_owned())
            .collect()
    }

    fn resting(&self, name: &str) -> bool {
        find(name).is_some_and(|c| c.resting())
    }

    fn deliver(&self, name: &str, rec: &crate::RecordBuf) -> std::result::Result<(), String> {
        find(name).map_or_else(
            || Err(format!("no sink called `{name}`")),
            |c| c.deliver(rec),
        )
    }
}
