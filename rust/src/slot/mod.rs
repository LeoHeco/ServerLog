//! The layer around each sink: name, filter, processors, layout, queue, retries, health and failure policy.

mod breaker;
mod lane;

use std::{
    sync::{
        Arc, Mutex, MutexGuard, PoisonError,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

pub use breaker::{Breaker, Health, Tell};

use crate::{
    Entry, Error, Filter, Level, Record, RecordBuf, Result, Sink,
    alarm::{self, Failure, Incident, OnFail, Phase},
    guard,
    layout::{Layout, Theme, When},
    process::{Chain, Draft, Processor},
    sink::Cache,
};
use lane::Lane;

/// A queue in front of a sink, drained in batches by a background task.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[must_use]
pub struct Queue {
    /// Records it holds.
    pub size: usize,
    /// Records written together.
    pub batch: usize,
    /// Longest a record waits for its batch to fill.
    pub wait: Duration,
    /// What happens to a record arriving when it is full.
    pub full: Full,
}

/// What a full queue does with a new record.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Full {
    /// Drops and counts it.
    #[default]
    Drop,
    /// Waits for room while the sink is up; drops once it is down.
    Block,
}

impl Queue {
    /// Holds `size` records, written 1000 at a time or a second after the first arrives.
    pub const fn new(size: usize) -> Self {
        Self {
            size,
            batch: 1000,
            wait: Duration::from_secs(1),
            full: Full::Drop,
        }
    }

    /// Records written together.
    pub const fn batch(mut self, n: usize) -> Self {
        self.batch = n;
        self
    }

    /// Longest a record waits for its batch to fill.
    pub const fn wait(mut self, wait: Duration) -> Self {
        self.wait = wait;
        self
    }

    /// What happens when it is full.
    pub const fn full(mut self, full: Full) -> Self {
        self.full = full;
        self
    }
}

/// Whether a slot queues: as its sink prefers, never, or behind a given queue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Sink,
    Direct,
    Queue(Queue),
}

/// A sink with the settings of the slot it goes into.
#[must_use]
pub struct Slot {
    pub(crate) sink: Box<dyn Sink>,
    pub(crate) name: Option<String>,
    pub(crate) filter: Option<Filter>,
    pub(crate) process: Chain,
    pub(crate) layout: Option<Layout>,
    pub(crate) colour: When,
    pub(crate) theme: Theme,
    pub(crate) alarm_only: bool,
    pub(crate) on_fail: OnFail,
    pub(crate) mode: Mode,
    pub(crate) retries: Option<u32>,
    pub(crate) probe: Duration,
}

impl<S: Sink + 'static> From<S> for Slot {
    fn from(sink: S) -> Self {
        Self::new(sink)
    }
}

impl Slot {
    /// Default settings for `sink`.
    pub fn new(sink: impl Sink + 'static) -> Self {
        Self::boxed(Box::new(sink))
    }

    /// Default settings for an already boxed sink.
    pub fn boxed(sink: Box<dyn Sink>) -> Self {
        Self {
            sink,
            name: None,
            filter: None,
            process: Chain::default(),
            layout: None,
            colour: When::Auto,
            theme: Theme::default(),
            alarm_only: false,
            on_fail: OnFail::default(),
            mode: Mode::Sink,
            retries: None,
            probe: Duration::from_secs(5),
        }
    }

    /// Names the slot; without one it is called after its kind and a number, such as `file-1`.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Takes only `min` and above.
    pub fn at(self, min: Level) -> Self {
        self.filter(Filter::level(min))
    }

    /// Takes only what `filter` lets through.
    pub fn filter(mut self, filter: Filter) -> Self {
        self.filter = Some(filter);
        self
    }

    /// Adds a processor that runs for this slot only.
    pub fn process(mut self, step: impl Processor + 'static) -> Self {
        self.process.push(Arc::new(step));
        self
    }

    /// Lays entries out this way instead of the sink's default.
    pub fn layout(mut self, layout: impl Into<Layout>) -> Self {
        self.layout = Some(layout.into());
        self
    }

    /// When to colour.
    pub const fn colour(mut self, when: When) -> Self {
        self.colour = when;
        self
    }

    /// Level colours.
    pub fn theme(mut self, theme: Theme) -> Self {
        self.theme = theme;
        self
    }

    /// Keeps it off the log route, for alarms only.
    pub const fn alarm_only(mut self) -> Self {
        self.alarm_only = true;
        self
    }

    /// What happens when the sink fails.
    pub fn on_fail(mut self, on_fail: OnFail) -> Self {
        self.on_fail = on_fail;
        self
    }

    /// Runs the sink behind `queue`.
    pub const fn queue(mut self, queue: Queue) -> Self {
        self.mode = Mode::Queue(queue);
        self
    }

    /// Writes in the caller, even for a sink that queues by default.
    pub const fn direct(mut self) -> Self {
        self.mode = Mode::Direct;
        self
    }

    /// Tries a failed write this many more times.
    pub const fn retries(mut self, n: u32) -> Self {
        self.retries = Some(n);
        self
    }

    /// First wait before probing a failed sink; doubles on each failed probe, up to five minutes.
    pub const fn probe(mut self, first: Duration) -> Self {
        self.probe = first;
        self
    }

    /// The name given, if any.
    #[must_use]
    pub fn named(&self) -> Option<&str> {
        self.name.as_deref()
    }
}

/// Slot settings on any sink, so `File::new(path).name("app").at(Level::WARN)` reads as one line.
pub trait SinkExt: Sink + Sized + 'static {
    /// See [`Slot::name`].
    fn name(self, name: impl Into<String>) -> Slot {
        Slot::new(self).name(name)
    }

    /// See [`Slot::at`].
    fn at(self, min: Level) -> Slot {
        Slot::new(self).at(min)
    }

    /// See [`Slot::filter`].
    fn filter(self, filter: Filter) -> Slot {
        Slot::new(self).filter(filter)
    }

    /// See [`Slot::process`].
    fn process(self, step: impl Processor + 'static) -> Slot {
        Slot::new(self).process(step)
    }

    /// See [`Slot::layout`].
    fn layout(self, layout: impl Into<Layout>) -> Slot {
        Slot::new(self).layout(layout)
    }

    /// See [`Slot::colour`].
    fn colour(self, when: When) -> Slot {
        Slot::new(self).colour(when)
    }

    /// See [`Slot::alarm_only`].
    fn alarm_only(self) -> Slot {
        Slot::new(self).alarm_only()
    }

    /// See [`Slot::on_fail`].
    fn on_fail(self, on_fail: OnFail) -> Slot {
        Slot::new(self).on_fail(on_fail)
    }

    /// See [`Slot::queue`].
    fn queue(self, queue: Queue) -> Slot {
        Slot::new(self).queue(queue)
    }

    /// See [`Slot::direct`].
    fn direct(self) -> Slot {
        Slot::new(self).direct()
    }

    /// See [`Slot::retries`].
    fn retries(self, n: u32) -> Slot {
        Slot::new(self).retries(n)
    }

    /// See [`Slot::probe`].
    fn probe(self, first: Duration) -> Slot {
        Slot::new(self).probe(first)
    }
}

impl<S: Sink + 'static> SinkExt for S {}

/// A slot's state, for monitoring.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Status {
    /// The slot's name.
    pub name: String,
    /// The sink's kind.
    pub kind: &'static str,
    /// Whether it is working.
    pub health: Health,
    /// Records skipped while it was down.
    pub skipped: u64,
    /// Records its full queue dropped.
    pub dropped: u64,
    /// Records its own code made, dropped to avoid loops.
    pub suppressed: u64,
    /// When the current health began.
    pub since: SystemTime,
    /// The latest error, while it is not up.
    pub error: Option<String>,
}

/// What both the caller and a background task need: the sink, its look, its policy and its health.
pub(crate) struct Core {
    pub(crate) name: String,
    pub(crate) kind: &'static str,
    pub(crate) sink: Box<dyn Sink>,
    pub(crate) layout: Layout,
    pub(crate) theme: Option<Theme>,
    pub(crate) on_fail: OnFail,
    retries: u32,
    breaker: Mutex<Breaker>,
    up: AtomicBool,
    pub(crate) dropped: AtomicU64,
    pub(crate) suppressed: AtomicU64,
}

impl Core {
    fn breaker(&self) -> MutexGuard<'_, Breaker> {
        self.breaker.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn up(&self) -> bool {
        self.up.load(Ordering::Acquire)
    }

    pub(crate) const fn tell(&self) -> Tell {
        match self.on_fail {
            OnFail::Alarm(_) => Tell::Report,
            _ => Tell::Silent,
        }
    }

    fn remind(&self) -> Duration {
        match &self.on_fail {
            OnFail::Alarm(a) => a.remind(),
            _ => Duration::MAX,
        }
    }

    pub(crate) fn entry<'e>(&'e self, rec: Record<'e>, cache: Option<&'e Cache>) -> Entry<'e> {
        Entry::with(rec, &self.layout, self.theme.as_ref(), cache)
    }

    /// Tries `f` up to the retry count, pausing 1, 2, 4… seconds between tries when `pause` is set.
    fn attempt(self: &Arc<Self>, pause: bool, f: impl Fn(&dyn Sink) -> Result) -> Result {
        let _inside = guard::enter(self);
        let mut n = 0;
        loop {
            match f(&*self.sink) {
                Err(e) if e.is_transient() && n < self.retries => {
                    if pause {
                        thread::sleep(Duration::from_secs(1 << n.min(5)));
                    }
                    n += 1;
                }
                r => return r,
            }
        }
    }

    /// Writes in the caller's thread.
    #[expect(clippy::panic, reason = "the `panic` policy asks for exactly this")]
    pub(crate) fn write(self: &Arc<Self>, entry: &Entry<'_>) {
        if matches!(self.on_fail, OnFail::Panic) {
            if let Err(e) = self.attempt(false, |s| s.emit(entry)) {
                let _quiet = guard::enter(self);
                panic!("serlog: sink `{}` failed: {e}", self.name);
            }
            return;
        }
        if !self.up() && !self.breaker().admit(Instant::now()) {
            return;
        }
        match self.attempt(false, |s| s.emit(entry)) {
            Ok(()) => self.pass(),
            Err(e) => self.fault(&e, self.tell()),
        }
    }

    /// Writes a batch from the slot's queue, waiting out failures until it goes through or is refused.
    pub(crate) fn drain(self: &Arc<Self>, buf: &mut Vec<RecordBuf>, patient: bool) {
        while !buf.is_empty() {
            if !self.up() {
                if !patient {
                    break;
                }
                let wait = self.breaker().wait(Instant::now());
                thread::sleep(wait);
                self.breaker().admit(Instant::now());
            }
            let probing = !self.up();
            let result = {
                let entries: Vec<Entry<'_>> = buf
                    .iter()
                    .map(|b| self.entry(b.as_record(), None))
                    .collect();
                self.attempt(!probing, |s| s.emit_batch(&entries))
            };
            match result {
                Ok(()) => {
                    self.pass();
                    buf.clear();
                }
                Err(e) if matches!(self.on_fail, OnFail::Panic) => {
                    alarm::crash(&format!("sink `{}` failed: {e}", self.name));
                }
                Err(e) => {
                    self.fault(&e, self.tell());
                    if !e.is_transient() {
                        self.dropped.fetch_add(buf.len() as u64, Ordering::Relaxed);
                        buf.clear();
                    }
                }
            }
        }
        self.dropped.fetch_add(buf.len() as u64, Ordering::Relaxed);
        buf.clear();
    }

    /// Writes an alarm record straight to the sink; failures ride in the incident being delivered.
    pub(crate) fn deliver(self: &Arc<Self>, rec: &RecordBuf) -> std::result::Result<(), String> {
        if !self.up() && !self.breaker().admit(Instant::now()) {
            return Err("down".into());
        }
        let result = {
            let _inside = guard::enter(self);
            let entry = self.entry(rec.as_record(), None);
            self.sink
                .emit_batch(std::slice::from_ref(&entry))
                .and_then(|()| self.sink.flush())
        };
        match result {
            Ok(()) => {
                self.pass();
                Ok(())
            }
            Err(e) => {
                self.fault(&e, Tell::Folded);
                Err(e.to_string())
            }
        }
    }

    pub(crate) fn resting(&self) -> bool {
        !self.up() && self.breaker().resting(Instant::now())
    }

    pub(crate) fn pass(&self) {
        if self.up() {
            return;
        }
        let mut b = self.breaker();
        let phase = b.pass();
        self.up.store(true, Ordering::Release);
        if let Some(p) = phase {
            self.notify(p, &b);
        }
    }

    pub(crate) fn fault(&self, e: &Error, tell: Tell) {
        let mut b = self.breaker();
        let phase = b.fault(Instant::now(), e.to_string(), self.remind(), tell);
        self.up.store(false, Ordering::Release);
        if let Some(p) = phase {
            self.notify(p, &b);
        }
    }

    fn notify(&self, phase: Phase, b: &Breaker) {
        let OnFail::Alarm(alarm) = &self.on_fail else {
            return;
        };
        let error = b.error().unwrap_or_default().to_owned();
        let mut incident = Incident::new(&self.name, self.kind, phase, &error);
        incident.skipped = b.skipped_now();
        incident.dropped = self.dropped.load(Ordering::Relaxed);
        incident.since = b.since();
        incident.failures = if phase == Phase::Resolved {
            Vec::new()
        } else {
            vec![Failure {
                sink: self.name.clone(),
                error,
                time: SystemTime::now(),
            }]
        };
        alarm::raise(alarm.clone(), incident);
    }

    pub(crate) fn status(&self) -> Status {
        let b = self.breaker();
        Status {
            name: self.name.clone(),
            kind: self.kind,
            health: b.health(),
            skipped: b.skipped(),
            dropped: self.dropped.load(Ordering::Relaxed),
            suppressed: self.suppressed.load(Ordering::Relaxed),
            since: b.since(),
            error: b.error().map(str::to_owned),
        }
    }

    #[cfg(feature = "config")]
    pub(crate) const fn retries(&self) -> u32 {
        self.retries
    }
}

/// A registered slot.
pub(crate) struct Live {
    pub(crate) core: Arc<Core>,
    pub(crate) filter: Option<Filter>,
    pub(crate) process: Chain,
    pub(crate) alarm_only: bool,
    pub(crate) claim: Option<String>,
    #[cfg_attr(
        not(feature = "config"),
        expect(dead_code, reason = "read by `config::dump`")
    )]
    pub(crate) queue: Option<Queue>,
    #[cfg_attr(
        not(feature = "config"),
        expect(dead_code, reason = "read by `config::dump`")
    )]
    pub(crate) probe: Duration,
    #[cfg_attr(
        not(feature = "config"),
        expect(dead_code, reason = "read by `config::dump`")
    )]
    pub(crate) given: Given,
    lane: Option<Lane>,
}

/// Settings given rather than defaulted, so an export writes only those.
#[derive(Clone, Debug, Default)]
#[cfg_attr(
    not(feature = "config"),
    expect(dead_code, reason = "read by `config::dump`")
)]
pub(crate) struct Given {
    pub(crate) queue: bool,
    pub(crate) retries: bool,
    pub(crate) layout: Option<Layout>,
    pub(crate) colour: When,
    pub(crate) theme: Theme,
}

impl Live {
    pub(crate) fn new(slot: Slot, name: String) -> Self {
        let queue = match slot.mode {
            Mode::Sink => slot.sink.queued(),
            Mode::Direct => None,
            Mode::Queue(q) => Some(q),
        };
        let retries = slot.retries.unwrap_or(if queue.is_some() { 3 } else { 0 });
        let claim = slot.sink.claim();
        let theme = slot
            .colour
            .resolve(slot.sink.tty())
            .then(|| slot.theme.clone());
        let layout = slot
            .layout
            .clone()
            .unwrap_or_else(|| slot.sink.default_layout());
        let core = Arc::new(Core {
            kind: slot.sink.kind(),
            name,
            sink: slot.sink,
            layout,
            theme,
            on_fail: slot.on_fail,
            retries,
            breaker: Mutex::new(Breaker::new(slot.probe, Instant::now())),
            up: AtomicBool::new(true),
            dropped: AtomicU64::new(0),
            suppressed: AtomicU64::new(0),
        });
        Self {
            lane: queue.map(|q| Lane::new(q, core.clone())),
            given: Given {
                queue: slot.mode != Mode::Sink,
                retries: slot.retries.is_some(),
                layout: slot.layout,
                colour: slot.colour,
                theme: slot.theme,
            },
            core,
            filter: slot.filter,
            process: slot.process,
            alarm_only: slot.alarm_only,
            claim,
            queue,
            probe: slot.probe,
        }
    }

    pub(crate) fn name(&self) -> &str {
        &self.core.name
    }

    pub(crate) fn emit(&self, rec: &Record<'_>, cache: &Cache) {
        if self
            .filter
            .as_ref()
            .is_some_and(|f| !f.allows(rec.level(), rec.target()))
            || !self.core.sink.enabled(rec.level(), rec.target())
        {
            return;
        }
        if self.process.is_empty() {
            return self.send(*rec, Some(cache), None);
        }
        let mut draft = Draft::new(*rec);
        let kept = {
            let _quiet = guard::quiet();
            self.process.run(&mut draft)
        };
        if !kept {
            return;
        }
        match draft.into_buf() {
            Some(buf) => self.send(buf.as_record(), None, Some(&buf)),
            None => self.send(*rec, Some(cache), None),
        }
    }

    fn send(&self, rec: Record<'_>, cache: Option<&Cache>, buf: Option<&RecordBuf>) {
        match &self.lane {
            Some(lane) => lane.push(buf.cloned().unwrap_or_else(|| rec.to_buf())),
            None => self.core.write(&self.core.entry(rec, cache)),
        }
    }

    pub(crate) fn flush(&self, deadline: Instant) {
        if let Some(lane) = &self.lane {
            lane.flush(deadline);
        } else if self.core.up()
            && let Err(e) = self.core.sink.flush()
        {
            self.core.fault(&e, self.core.tell());
        }
    }
}
