//! A slot's health: up, down, or probing after a failure; free of I/O so the shared vectors can drive it.

use std::{
    fmt,
    time::{Duration, Instant, SystemTime},
};

use crate::alarm::Phase;

const LONGEST: Duration = Duration::from_mins(5);

/// Whether a sink is working.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Health {
    /// Writing.
    #[default]
    Up,
    /// Failed; skipped until the next probe.
    Down,
    /// One write is testing whether it works again.
    Probing,
}

impl Health {
    /// The lower-case name used in config, metrics and test vectors.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
            Self::Probing => "probing",
        }
    }
}

impl fmt::Display for Health {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How a failure is told.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tell {
    /// Opens an incident and reminds.
    Report,
    /// Stays quiet.
    Silent,
    /// Rides in another sink's incident, so only the recovery is told.
    Folded,
}

/// The state machine behind one slot.
#[derive(Debug)]
pub struct Breaker {
    health: Health,
    base: Duration,
    gap: Duration,
    next: Instant,
    told: Option<Instant>,
    since: SystemTime,
    opened_at: u64,
    skipped: u64,
    error: Option<String>,
}

impl Breaker {
    /// A healthy machine that first probes `probe` after a failure.
    #[must_use]
    pub fn new(probe: Duration, now: Instant) -> Self {
        Self {
            health: Health::Up,
            base: probe,
            gap: probe,
            next: now,
            told: None,
            since: SystemTime::now(),
            opened_at: 0,
            skipped: 0,
            error: None,
        }
    }

    /// Current health.
    #[must_use]
    pub const fn health(&self) -> Health {
        self.health
    }

    /// Records skipped while down, over the slot's life.
    #[must_use]
    pub const fn skipped(&self) -> u64 {
        self.skipped
    }

    /// Records skipped since the current failure began.
    #[must_use]
    pub const fn skipped_now(&self) -> u64 {
        self.skipped - self.opened_at
    }

    /// When the current health began.
    #[must_use]
    pub const fn since(&self) -> SystemTime {
        self.since
    }

    /// The latest error.
    #[must_use]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Whether a write may go ahead at `now`; turns a due `Down` into `Probing`, and counts a skip otherwise.
    pub fn admit(&mut self, now: Instant) -> bool {
        match self.health {
            Health::Up => true,
            Health::Down if now >= self.next => {
                self.health = Health::Probing;
                true
            }
            _ => {
                self.skipped += 1;
                false
            }
        }
    }

    /// Whether a sink is skipped at `now` without counting anything, for picking alarm targets.
    #[must_use]
    pub fn resting(&self, now: Instant) -> bool {
        match self.health {
            Health::Up => false,
            Health::Down => now < self.next,
            Health::Probing => true,
        }
    }

    /// How long until the next probe.
    #[must_use]
    pub fn wait(&self, now: Instant) -> Duration {
        self.next.saturating_duration_since(now)
    }

    /// A write succeeded; returns `Resolved` when the failure had been told.
    pub fn pass(&mut self) -> Option<Phase> {
        if self.health == Health::Up {
            return None;
        }
        self.health = Health::Up;
        self.gap = self.base;
        self.since = SystemTime::now();
        self.error = None;
        self.told.take().map(|_| Phase::Resolved)
    }

    /// A write failed; returns `Opened` or `Reminder` when that is to be told.
    pub fn fault(
        &mut self,
        now: Instant,
        error: String,
        remind: Duration,
        tell: Tell,
    ) -> Option<Phase> {
        self.error = Some(error);
        match self.health {
            Health::Up => {
                self.health = Health::Down;
                self.gap = self.base;
                self.next = now + self.gap;
                self.since = SystemTime::now();
                self.opened_at = self.skipped;
                match tell {
                    Tell::Report => {
                        self.told = Some(now);
                        Some(Phase::Opened)
                    }
                    Tell::Folded => {
                        self.told = Some(now);
                        None
                    }
                    Tell::Silent => None,
                }
            }
            Health::Probing => {
                self.health = Health::Down;
                self.gap = (self.gap * 2).min(LONGEST);
                self.next = now + self.gap;
                match self.told {
                    Some(at) if tell == Tell::Report && now.duration_since(at) >= remind => {
                        self.told = Some(now);
                        Some(Phase::Reminder)
                    }
                    _ => None,
                }
            }
            Health::Down => None,
        }
    }
}
