//! Steps a record goes through before it is written: add fields, change the message, or drop it.
//!
//! The hub runs one chain on every record; each slot may run one more before its sink.

mod builtin;

use std::sync::Arc;

pub use builtin::{Dedupe, Exclude, Fields, Limit, Match, Redact, Sample, Sanitise};

use crate::{Record, RecordBuf};

/// A record on its way, copied only once a step edits it.
pub struct Draft<'a> {
    rec: Record<'a>,
    buf: Option<RecordBuf>,
}

impl<'a> Draft<'a> {
    /// Starts from `rec`.
    #[must_use]
    pub const fn new(rec: Record<'a>) -> Self {
        Self { rec, buf: None }
    }

    /// The record as it stands.
    #[must_use]
    pub fn record(&self) -> Record<'_> {
        self.buf.as_ref().map_or(self.rec, RecordBuf::as_record)
    }

    /// The record for editing; the first call copies it.
    pub fn edit(&mut self) -> &mut RecordBuf {
        self.buf.get_or_insert_with(|| self.rec.to_buf())
    }

    /// Whether a step edited it.
    #[must_use]
    pub const fn edited(&self) -> bool {
        self.buf.is_some()
    }

    pub(crate) fn into_buf(self) -> Option<RecordBuf> {
        self.buf
    }
}

/// One step of a chain.
pub trait Processor: Send + Sync {
    /// Looks at or edits the record; `false` drops it.
    fn process(&self, draft: &mut Draft<'_>) -> bool;

    /// The name it is registered under, for export.
    fn kind(&self) -> &'static str {
        "custom"
    }

    /// The config entry that rebuilds it; `None` when it cannot be exported.
    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Value> {
        None
    }
}

impl<F: Fn(&mut Draft<'_>) -> bool + Send + Sync> Processor for F {
    fn process(&self, draft: &mut Draft<'_>) -> bool {
        self(draft)
    }
}

/// An ordered list of steps.
#[derive(Clone, Default)]
pub struct Chain(Vec<Arc<dyn Processor>>);

impl Chain {
    /// Adds a step at the end.
    #[must_use]
    pub fn then(mut self, step: impl Processor + 'static) -> Self {
        self.0.push(Arc::new(step));
        self
    }

    pub(crate) fn push(&mut self, step: Arc<dyn Processor>) {
        self.0.push(step);
    }

    /// Whether there are no steps.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The steps.
    #[must_use]
    pub fn steps(&self) -> &[Arc<dyn Processor>] {
        &self.0
    }

    /// Runs every step; `false` when one dropped the record.
    pub fn run(&self, draft: &mut Draft<'_>) -> bool {
        self.0.iter().all(|s| s.process(draft))
    }
}

impl std::fmt::Debug for Chain {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|s| s.kind()))
            .finish()
    }
}
