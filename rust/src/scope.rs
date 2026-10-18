//! Fields bound to the current thread for as long as a guard lives.

use std::marker::PhantomData;

use crate::{Field, Val};

use tls::BOUND;

mod tls {
    #![cfg_attr(
        all(windows, target_env = "gnu"),
        expect(
            clippy::missing_const_for_thread_local,
            reason = "the initialiser is already `const`; clippy misreads the windows-gnu expansion"
        )
    )]

    use std::cell::RefCell;

    use crate::Val;

    thread_local! {
        pub static BOUND: RefCell<Vec<(String, Val)>> = const { RefCell::new(Vec::new()) };
    }
}

/// Adds fields to every record this thread makes until it drops; [`bind!`](crate::bind!) builds one.
#[must_use = "fields stay bound only while the guard lives"]
pub struct Bound {
    n: usize,
    _thread: PhantomData<*const ()>,
}

impl Bound {
    /// Binds `fields` to this thread.
    pub fn new(fields: &[Field<'_>]) -> Self {
        BOUND.with_borrow_mut(|b| {
            b.extend(fields.iter().map(|(k, v)| ((*k).to_owned(), v.to_owned())));
        });
        Self {
            n: fields.len(),
            _thread: PhantomData,
        }
    }
}

impl Drop for Bound {
    fn drop(&mut self) {
        BOUND.with_borrow_mut(|b| b.truncate(b.len().saturating_sub(self.n)));
    }
}

pub(crate) fn with<R>(f: impl FnOnce(&[(String, Val)]) -> R) -> R {
    BOUND.with_borrow(|b| f(b))
}
