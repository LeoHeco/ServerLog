//! A ring buffer in memory, for tests and for looking back at recent records.

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex, PoisonError},
};

use crate::{Entry, RecordBuf, Result, Sink};

/// Keeps the latest records and their lines; clones share the buffer, so keep one to read from.
#[derive(Clone, Debug)]
#[must_use]
pub struct Memory {
    size: usize,
    kept: Arc<Mutex<VecDeque<(RecordBuf, String)>>>,
}

impl Memory {
    /// Keeps up to `size` records, dropping the oldest.
    pub fn new(size: usize) -> Self {
        Self {
            size: size.max(1),
            kept: Arc::default(),
        }
    }

    fn with<T>(&self, f: impl FnOnce(&mut VecDeque<(RecordBuf, String)>) -> T) -> T {
        f(&mut self.kept.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// The records kept, oldest first.
    #[must_use]
    pub fn records(&self) -> Vec<RecordBuf> {
        self.with(|k| k.iter().map(|(r, _)| r.clone()).collect())
    }

    /// The lines kept, oldest first.
    #[must_use]
    pub fn lines(&self) -> Vec<String> {
        self.with(|k| k.iter().map(|(_, l)| l.clone()).collect())
    }

    /// The records for which `f` holds.
    #[must_use]
    pub fn find(&self, f: impl Fn(&RecordBuf) -> bool) -> Vec<RecordBuf> {
        self.with(|k| {
            k.iter()
                .filter(|(r, _)| f(r))
                .map(|(r, _)| r.clone())
                .collect()
        })
    }

    /// How many are kept.
    #[must_use]
    pub fn len(&self) -> usize {
        self.with(|k| k.len())
    }

    /// Whether nothing is kept.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Forgets everything.
    pub fn clear(&self) {
        self.with(VecDeque::clear);
    }
}

impl Sink for Memory {
    fn emit(&self, entry: &Entry<'_>) -> Result {
        let item = (entry.record().to_buf(), entry.line().to_owned());
        self.with(|k| {
            if k.len() == self.size {
                k.pop_front();
            }
            k.push_back(item);
        });
        Ok(())
    }

    fn kind(&self) -> &'static str {
        "memory"
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        let mut m = serde_json::Map::new();
        m.insert("size".into(), self.size.into());
        Some(m)
    }
}
