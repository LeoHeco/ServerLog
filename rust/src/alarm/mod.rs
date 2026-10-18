//! What happens when a sink fails: panic, ignore, or raise an alarm that walks tiers of other sinks.

mod sound;
mod task;

use std::{
    fmt,
    sync::{Arc, LazyLock, PoisonError, RwLock},
    time::{Duration, SystemTime},
};

use crate::{Level, RecordBuf};

pub use sound::{Desk, Outcome, sound};
pub use task::dropped;
pub(crate) use task::{crash, raise, settle};

/// What a slot does when its sink fails.
#[derive(Clone)]
pub enum OnFail {
    /// Fails loudly: panics in the caller when writing directly, ends the process from a queue.
    Panic,
    /// Counts the failure and carries on.
    Ignore,
    /// Hands an incident to this alarm.
    Alarm(Arc<dyn Alarm>),
}

impl OnFail {
    /// Raises incidents through `alarm`.
    pub fn alarm(alarm: impl Alarm + 'static) -> Self {
        Self::Alarm(Arc::new(alarm))
    }
}

impl Default for OnFail {
    /// The process-wide default alarm, looked up when an incident happens.
    fn default() -> Self {
        Self::Alarm(handle())
    }
}

impl fmt::Debug for OnFail {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Panic => "Panic",
            Self::Ignore => "Ignore",
            Self::Alarm(_) => "Alarm",
        })
    }
}

/// What an alarm does when no tier took the incident.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fallback {
    /// Ends the process.
    Panic,
    /// Drops the incident.
    #[default]
    Ignore,
}

/// Which report of an incident this is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Phase {
    /// The sink just failed.
    Opened,
    /// The sink is still failing.
    Reminder,
    /// The sink works again.
    Resolved,
}

impl Phase {
    /// The lower-case name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Opened => "opened",
            Self::Reminder => "reminder",
            Self::Resolved => "resolved",
        }
    }
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A sink's failure, from the first error to recovery.
#[derive(Clone, Debug)]
#[non_exhaustive]
pub struct Incident {
    /// The failing sink's name.
    pub sink: String,
    /// Its kind.
    pub kind: &'static str,
    /// Which report this is.
    pub phase: Phase,
    /// The latest error.
    pub error: String,
    /// Every failure so far, the sink's own first, then each alarm target that failed too.
    pub failures: Vec<Failure>,
    /// Records skipped since the failure began.
    pub skipped: u64,
    /// Records the sink's queue dropped, over its life.
    pub dropped: u64,
    /// When the failure began.
    pub since: SystemTime,
}

/// One failed write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Failure {
    /// The sink that failed.
    pub sink: String,
    /// What it said.
    pub error: String,
    /// When.
    pub time: SystemTime,
}

impl Incident {
    /// A fresh incident for `sink`.
    #[must_use]
    pub fn new(
        sink: impl Into<String>,
        kind: &'static str,
        phase: Phase,
        error: impl Into<String>,
    ) -> Self {
        let (sink, error) = (sink.into(), error.into());
        Self {
            failures: vec![Failure {
                sink: sink.clone(),
                error: error.clone(),
                time: SystemTime::now(),
            }],
            sink,
            kind,
            phase,
            error,
            skipped: 0,
            dropped: 0,
            since: SystemTime::now(),
        }
    }
}

/// Routes incidents to sinks.
///
/// Implement [`route`](Self::route) to choose the tiers; the runtime walks them, skips the failing sink
/// and any that are down, delivers within a tier in parallel and stops at the first tier where one
/// sink took it. Implementors cannot cause loops or storms.
pub trait Alarm: Send + Sync {
    /// The tiers for this incident, tried in order; `*` stands for every sink on the log route.
    fn route(&self, incident: &Incident) -> Vec<Vec<String>>;

    /// The record sent for this incident; asked again before each tier, so later tiers see earlier failures.
    fn describe(&self, incident: &Incident) -> RecordBuf {
        describe(incident)
    }

    /// What to do when no tier took it.
    fn fallback(&self) -> Fallback {
        Fallback::Ignore
    }

    /// How long a sink may stay down before it is reported again.
    fn remind(&self) -> Duration {
        Duration::from_hours(1)
    }

    /// Every sink name this alarm can route to, checked at start-up.
    fn names(&self) -> Vec<String> {
        Vec::new()
    }

    /// The config entry that rebuilds this alarm; `None` when it cannot be exported.
    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        None
    }
}

/// The built-in alarm: fixed tiers of sink names.
#[derive(Clone, Debug, PartialEq, Eq)]
#[must_use]
pub struct Tiers {
    groups: Vec<Vec<String>>,
    fallback: Fallback,
    remind: Duration,
}

impl Default for Tiers {
    /// Standard error alone, kept apart from the log route.
    fn default() -> Self {
        Self::new([["stderr"]])
    }
}

impl Tiers {
    /// Tiers of sink names, such as `[["loki"], ["oncall"], ["stderr"]]`.
    pub fn new<T, N>(tiers: impl IntoIterator<Item = T>) -> Self
    where
        T: IntoIterator<Item = N>,
        N: Into<String>,
    {
        Self {
            groups: tiers
                .into_iter()
                .map(|t| t.into_iter().map(Into::into).collect())
                .collect(),
            fallback: Fallback::Ignore,
            remind: Duration::from_hours(1),
        }
    }

    /// What happens when every tier fails.
    pub const fn fallback(mut self, fallback: Fallback) -> Self {
        self.fallback = fallback;
        self
    }

    /// How long a sink may stay down before it is reported again.
    pub const fn remind(mut self, every: Duration) -> Self {
        self.remind = every;
        self
    }

    /// The tiers.
    #[must_use]
    pub fn tiers(&self) -> &[Vec<String>] {
        &self.groups
    }
}

impl Alarm for Tiers {
    fn route(&self, _: &Incident) -> Vec<Vec<String>> {
        self.groups.clone()
    }

    fn fallback(&self) -> Fallback {
        self.fallback
    }

    fn remind(&self) -> Duration {
        self.remind
    }

    fn names(&self) -> Vec<String> {
        self.groups.iter().flatten().cloned().collect()
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        let mut m = serde_json::Map::new();
        m.insert("tiers".into(), serde_json::json!(self.groups));
        if self.fallback == Fallback::Panic {
            m.insert("on_fail".into(), "panic".into());
        }
        if self.remind != Duration::from_hours(1) {
            m.insert(
                "remind".into(),
                crate::config::units::show(self.remind).into(),
            );
        }
        Some(m)
    }
}

/// The default record for an incident.
#[must_use]
pub fn describe(i: &Incident) -> RecordBuf {
    let (level, msg) = match i.phase {
        Phase::Opened => (
            Level::ERROR,
            format!("sink `{}` failed: {}", i.sink, i.error),
        ),
        Phase::Reminder => (
            Level::ERROR,
            format!("sink `{}` is still failing: {}", i.sink, i.error),
        ),
        Phase::Resolved => (
            Level::INFO,
            format!(
                "sink `{}` works again after {}s",
                i.sink,
                i.since.elapsed().unwrap_or_default().as_secs()
            ),
        ),
    };
    let failures = i
        .failures
        .iter()
        .map(|f| format!("{}: {}", f.sink, f.error))
        .collect::<Vec<_>>()
        .join("; ");
    RecordBuf::new(level, "serlog", msg)
        .with("sink", &i.sink)
        .with("kind", i.kind)
        .with("phase", i.phase.as_str())
        .with("failures", failures)
        .with("skipped", i.skipped)
        .with("dropped", i.dropped)
        .with("since", crate::layout::time::stamp(i.since).to_string())
}

static DEFAULT: RwLock<Option<Arc<dyn Alarm>>> = RwLock::new(None);

/// Replaces the process-wide default alarm.
pub fn set_alarm(alarm: impl Alarm + 'static) {
    set_shared(Arc::new(alarm));
}

pub(crate) fn set_shared(alarm: Arc<dyn Alarm>) {
    *DEFAULT.write().unwrap_or_else(PoisonError::into_inner) = Some(alarm);
}

/// The default alarm in force, made on first use.
pub(crate) fn current() -> Arc<dyn Alarm> {
    if let Some(a) = DEFAULT
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        return a.clone();
    }
    DEFAULT
        .write()
        .unwrap_or_else(PoisonError::into_inner)
        .get_or_insert_with(|| Arc::new(Tiers::default()))
        .clone()
}

/// Stands for whatever the default alarm is when an incident happens.
struct Current;

impl Alarm for Current {
    fn route(&self, incident: &Incident) -> Vec<Vec<String>> {
        current().route(incident)
    }

    fn describe(&self, incident: &Incident) -> RecordBuf {
        current().describe(incident)
    }

    fn fallback(&self) -> Fallback {
        current().fallback()
    }

    fn remind(&self) -> Duration {
        current().remind()
    }

    fn names(&self) -> Vec<String> {
        current().names()
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        current().options()
    }
}

/// The handle every default [`OnFail`] holds.
pub(crate) fn handle() -> Arc<dyn Alarm> {
    static HANDLE: LazyLock<Arc<dyn Alarm>> = LazyLock::new(|| Arc::new(Current));
    HANDLE.clone()
}

/// Whether `alarm` is the default handle.
pub(crate) fn is_default(alarm: &Arc<dyn Alarm>) -> bool {
    Arc::ptr_eq(alarm, &handle())
}
