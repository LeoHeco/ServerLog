//! Marks code running inside a sink, so records it makes are dropped instead of looping back.

use std::sync::{Arc, atomic::Ordering};

use tls::{CORE, INSIDE};

use crate::slot::Core;

mod tls {
    #![cfg_attr(
        all(windows, target_env = "gnu"),
        expect(
            clippy::missing_const_for_thread_local,
            reason = "the initialisers are already `const`; clippy misreads the windows-gnu expansion"
        )
    )]

    use std::{
        cell::{Cell, RefCell},
        sync::Arc,
    };

    use crate::slot::Core;

    thread_local! {
        pub static INSIDE: Cell<bool> = const { Cell::new(false) };
        pub static CORE: RefCell<Option<Arc<Core>>> = const { RefCell::new(None) };
    }
}

/// Restores the previous mark when dropped.
pub(crate) struct Inside {
    was: bool,
    prev: Option<Arc<Core>>,
}

/// Marks this thread as running `core`'s sink until the result drops.
pub(crate) fn enter(core: &Arc<Core>) -> Inside {
    Inside {
        was: INSIDE.replace(true),
        prev: CORE.replace(Some(core.clone())),
    }
}

impl Drop for Inside {
    fn drop(&mut self) {
        INSIDE.set(self.was);
        CORE.set(self.prev.take());
    }
}

/// Marks this thread as running serlog's own code, such as a processor, until the result drops.
pub(crate) fn quiet() -> Inside {
    Inside {
        was: INSIDE.replace(true),
        prev: CORE.replace(None),
    }
}

/// Marks the whole thread, for serlog's own background tasks.
pub(crate) fn adopt(core: Option<Arc<Core>>) {
    INSIDE.set(true);
    CORE.set(core);
}

/// Counts and reports a record made inside a sink, which must then be dropped.
pub(crate) fn suppress() -> bool {
    if !INSIDE.get() {
        return false;
    }
    CORE.with_borrow(|c| {
        if let Some(c) = c {
            c.suppressed.fetch_add(1, Ordering::Relaxed);
        }
    });
    true
}
