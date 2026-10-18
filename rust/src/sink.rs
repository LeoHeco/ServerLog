//! The one trait every output implements, built-in or not, and the entry it is handed.

use std::{
    cell::{OnceCell, RefCell},
    rc::Rc,
    sync::Arc,
};

use crate::{
    Level, Record, Result,
    layout::{Layout, Theme},
    slot::Queue,
};

/// What a sink is handed: the record, and the line its slot's layout makes of it.
pub struct Entry<'e> {
    rec: Record<'e>,
    layout: &'e Layout,
    theme: Option<&'e Theme>,
    cache: Option<&'e Cache>,
    line: OnceCell<Rc<str>>,
}

/// Lines already rendered for one record, shared by the slots it goes to.
#[derive(Default)]
pub(crate) struct Cache(RefCell<Vec<(usize, usize, Rc<str>)>>);

impl<'e> Entry<'e> {
    /// An entry laid out by `layout`, uncoloured.
    #[must_use]
    pub const fn new(rec: Record<'e>, layout: &'e Layout) -> Self {
        Self::with(rec, layout, None, None)
    }

    pub(crate) const fn with(
        rec: Record<'e>,
        layout: &'e Layout,
        theme: Option<&'e Theme>,
        cache: Option<&'e Cache>,
    ) -> Self {
        Self {
            rec,
            layout,
            theme,
            cache,
            line: OnceCell::new(),
        }
    }

    /// The record.
    #[must_use]
    pub const fn record(&self) -> &Record<'e> {
        &self.rec
    }

    /// Whether the line is coloured.
    #[must_use]
    pub const fn coloured(&self) -> bool {
        self.theme.is_some()
    }

    /// The line, laid out on first use and shared with other slots using the same layout.
    pub fn line(&self) -> &str {
        self.line.get_or_init(|| {
            let key = (
                self.layout.id(),
                self.theme.map_or(0, |t| std::ptr::from_ref(t).addr()),
            );
            if let Some(c) = self.cache
                && let Some((_, _, s)) = c.0.borrow().iter().find(|(l, t, _)| (*l, *t) == key)
            {
                return s.clone();
            }
            let mut s = String::with_capacity(160);
            self.layout.render(&mut s, &self.rec, self.theme);
            let s: Rc<str> = s.into();
            if let Some(c) = self.cache {
                c.0.borrow_mut().push((key.0, key.1, s.clone()));
            }
            s
        })
    }
}

/// Somewhere entries go.
///
/// Only [`emit`](Self::emit) is required; queues, retries, health, alarms and layout come from the slot
/// the sink is registered in, so a sink just delivers and reports what went wrong.
#[diagnostic::on_unimplemented(
    message = "`{Self}` is not a serlog sink",
    note = "implement `serlog::Sink`, or wrap a closure with `serlog::from_fn`"
)]
pub trait Sink: Send + Sync {
    /// Delivers one entry.
    ///
    /// # Errors
    /// Whatever stopped it; [`Error::Rejected`](crate::Error::Rejected) when retrying cannot help.
    fn emit(&self, entry: &Entry<'_>) -> Result;

    /// Delivers several entries, as a queue hands them over; one by one unless overridden.
    ///
    /// # Errors
    /// As for [`emit`](Self::emit).
    fn emit_batch(&self, entries: &[Entry<'_>]) -> Result {
        entries.iter().try_for_each(|e| self.emit(e))
    }

    /// Whether records at `level` from `target` are wanted, checked before anything else.
    fn enabled(&self, level: Level, target: &str) -> bool {
        let _ = (level, target);
        true
    }

    /// Pushes out anything buffered.
    ///
    /// # Errors
    /// Whatever stopped the flush.
    fn flush(&self) -> Result {
        Ok(())
    }

    /// Checks at registration that the sink can work, so mistakes surface at start-up.
    ///
    /// # Errors
    /// Why it cannot work; registration then fails.
    fn ready(&self) -> Result {
        Ok(())
    }

    /// A resource only this sink may use, such as a file path; a second sink claiming it is refused.
    fn claim(&self) -> Option<String> {
        None
    }

    /// The kind name, also the start of the sink's default name.
    fn kind(&self) -> &'static str {
        "custom"
    }

    /// Whether it writes to a terminal, for colour.
    fn tty(&self) -> bool {
        false
    }

    /// The layout used when the slot sets none.
    fn default_layout(&self) -> Layout {
        Layout::Inherit
    }

    /// The queue it runs behind when the slot sets none; `None` writes directly.
    fn queued(&self) -> Option<Queue> {
        None
    }

    /// The config entry that rebuilds this sink, without `kind` and slot keys; `None` when it cannot be exported.
    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        None
    }
}

macro_rules! forward {
    ($($ty:ty),+) => {$(
        impl<S: Sink + ?Sized> Sink for $ty {
            fn emit(&self, entry: &Entry<'_>) -> Result {
                (**self).emit(entry)
            }

            fn emit_batch(&self, entries: &[Entry<'_>]) -> Result {
                (**self).emit_batch(entries)
            }

            fn enabled(&self, level: Level, target: &str) -> bool {
                (**self).enabled(level, target)
            }

            fn flush(&self) -> Result {
                (**self).flush()
            }

            fn ready(&self) -> Result {
                (**self).ready()
            }

            fn claim(&self) -> Option<String> {
                (**self).claim()
            }

            fn kind(&self) -> &'static str {
                (**self).kind()
            }

            fn tty(&self) -> bool {
                (**self).tty()
            }

            fn default_layout(&self) -> Layout {
                (**self).default_layout()
            }

            fn queued(&self) -> Option<Queue> {
                (**self).queued()
            }

            #[cfg(feature = "config")]
            fn options(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
                (**self).options()
            }
        }
    )+};
}

forward!(&S, Box<S>, Arc<S>);

/// What a closure given to [`from_fn`] may return.
pub trait Written {
    /// As a sink result.
    ///
    /// # Errors
    /// When the closure reported one.
    fn written(self) -> Result;
}

impl Written for () {
    fn written(self) -> Result {
        Ok(())
    }
}

impl Written for Result {
    fn written(self) -> Result {
        self
    }
}

/// A sink made from a closure.
#[derive(Clone, Copy, Debug)]
pub struct FromFn<F>(F);

/// Turns `f` into a sink; it may return `()` or a [`Result`].
pub const fn from_fn<F, R>(f: F) -> FromFn<F>
where
    F: Fn(&Entry<'_>) -> R + Send + Sync,
    R: Written,
{
    FromFn(f)
}

impl<F, R> Sink for FromFn<F>
where
    F: Fn(&Entry<'_>) -> R + Send + Sync,
    R: Written,
{
    fn emit(&self, entry: &Entry<'_>) -> Result {
        (self.0)(entry).written()
    }

    fn kind(&self) -> &'static str {
        "fn"
    }
}
