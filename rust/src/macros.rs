//! `log!`, one shorthand per level, and `bind!`; fields are written the `tracing` way and evaluated only when the record passes.

/// Logs at a level: `log!(Level::INFO, mirror = %name, tries = 3, "synced {} files", n)`.
///
/// Fields come before the message as `key = value` (any [`ToValue`](crate::ToValue)), `key = %value`
/// (`Display`), `key = ?value` (`Debug`), or the shorthands `key`, `%key` and `?key`.
/// `target: "x",` and `tags: ["a", "b"],` may lead, in that order.
#[macro_export]
macro_rules! log {
    (target: $target:expr, tags: [$($tag:expr),* $(,)?], $level:expr, $($rest:tt)+) => {
        $crate::log!(@ $target, [$($tag),*], $level, [] $($rest)+)
    };
    (target: $target:expr, $level:expr, $($rest:tt)+) => {
        $crate::log!(@ $target, [], $level, [] $($rest)+)
    };
    (tags: [$($tag:expr),* $(,)?], $level:expr, $($rest:tt)+) => {
        $crate::log!(@ module_path!(), [$($tag),*], $level, [] $($rest)+)
    };
    (@ $t:expr, $g:tt, $l:expr, [$($f:tt)*] $k:ident = % $v:expr, $($rest:tt)+) => {
        $crate::log!(@ $t, $g, $l, [$($f)* (stringify!($k), $crate::Value::Display(&$v)),] $($rest)+)
    };
    (@ $t:expr, $g:tt, $l:expr, [$($f:tt)*] $k:ident = ? $v:expr, $($rest:tt)+) => {
        $crate::log!(@ $t, $g, $l, [$($f)* (stringify!($k), $crate::Value::Debug(&$v)),] $($rest)+)
    };
    (@ $t:expr, $g:tt, $l:expr, [$($f:tt)*] $k:ident = $v:expr, $($rest:tt)+) => {
        $crate::log!(@ $t, $g, $l, [$($f)* (stringify!($k), $crate::ToValue::to_value(&$v)),] $($rest)+)
    };
    (@ $t:expr, $g:tt, $l:expr, [$($f:tt)*] % $k:ident, $($rest:tt)+) => {
        $crate::log!(@ $t, $g, $l, [$($f)* (stringify!($k), $crate::Value::Display(&$k)),] $($rest)+)
    };
    (@ $t:expr, $g:tt, $l:expr, [$($f:tt)*] ? $k:ident, $($rest:tt)+) => {
        $crate::log!(@ $t, $g, $l, [$($f)* (stringify!($k), $crate::Value::Debug(&$k)),] $($rest)+)
    };
    (@ $t:expr, $g:tt, $l:expr, [$($f:tt)*] $k:ident, $($rest:tt)+) => {
        $crate::log!(@ $t, $g, $l, [$($f)* (stringify!($k), $crate::ToValue::to_value(&$k)),] $($rest)+)
    };
    (@ $t:expr, [$($tag:expr),*], $l:expr, [$($f:tt)*] $($msg:tt)+) => {{
        let level = $l;
        if $crate::enabled(level) {
            fn __serlog_here() {}
            $crate::__private::log(
                level,
                $t,
                module_path!(),
                $crate::__private::func(::core::any::type_name_of_val(&__serlog_here)),
                file!(),
                line!(),
                &[$($tag),*],
                &[$($f)*],
                format_args!($($msg)+),
            );
        }
    }};
    ($level:expr, $($rest:tt)+) => {
        $crate::log!(@ module_path!(), [], $level, [] $($rest)+)
    };
}

/// Logs at [`Level::TRACE`](crate::Level::TRACE); takes what [`log!`] takes after the level.
#[macro_export]
macro_rules! trace {
    (target: $target:expr, tags: $tags:tt, $($rest:tt)+) => {
        $crate::log!(target: $target, tags: $tags, $crate::Level::TRACE, $($rest)+)
    };
    (target: $target:expr, $($rest:tt)+) => {
        $crate::log!(target: $target, $crate::Level::TRACE, $($rest)+)
    };
    (tags: $tags:tt, $($rest:tt)+) => {
        $crate::log!(tags: $tags, $crate::Level::TRACE, $($rest)+)
    };
    ($($rest:tt)+) => {
        $crate::log!($crate::Level::TRACE, $($rest)+)
    };
}

/// Logs at [`Level::DEBUG`](crate::Level::DEBUG); takes what [`log!`] takes after the level.
#[macro_export]
macro_rules! debug {
    (target: $target:expr, tags: $tags:tt, $($rest:tt)+) => {
        $crate::log!(target: $target, tags: $tags, $crate::Level::DEBUG, $($rest)+)
    };
    (target: $target:expr, $($rest:tt)+) => {
        $crate::log!(target: $target, $crate::Level::DEBUG, $($rest)+)
    };
    (tags: $tags:tt, $($rest:tt)+) => {
        $crate::log!(tags: $tags, $crate::Level::DEBUG, $($rest)+)
    };
    ($($rest:tt)+) => {
        $crate::log!($crate::Level::DEBUG, $($rest)+)
    };
}

/// Logs at [`Level::INFO`](crate::Level::INFO); takes what [`log!`] takes after the level.
#[macro_export]
macro_rules! info {
    (target: $target:expr, tags: $tags:tt, $($rest:tt)+) => {
        $crate::log!(target: $target, tags: $tags, $crate::Level::INFO, $($rest)+)
    };
    (target: $target:expr, $($rest:tt)+) => {
        $crate::log!(target: $target, $crate::Level::INFO, $($rest)+)
    };
    (tags: $tags:tt, $($rest:tt)+) => {
        $crate::log!(tags: $tags, $crate::Level::INFO, $($rest)+)
    };
    ($($rest:tt)+) => {
        $crate::log!($crate::Level::INFO, $($rest)+)
    };
}

/// Logs at [`Level::WARN`](crate::Level::WARN); takes what [`log!`] takes after the level.
#[macro_export]
macro_rules! warn {
    (target: $target:expr, tags: $tags:tt, $($rest:tt)+) => {
        $crate::log!(target: $target, tags: $tags, $crate::Level::WARN, $($rest)+)
    };
    (target: $target:expr, $($rest:tt)+) => {
        $crate::log!(target: $target, $crate::Level::WARN, $($rest)+)
    };
    (tags: $tags:tt, $($rest:tt)+) => {
        $crate::log!(tags: $tags, $crate::Level::WARN, $($rest)+)
    };
    ($($rest:tt)+) => {
        $crate::log!($crate::Level::WARN, $($rest)+)
    };
}

/// Logs at [`Level::ERROR`](crate::Level::ERROR); takes what [`log!`] takes after the level.
#[macro_export]
macro_rules! error {
    (target: $target:expr, tags: $tags:tt, $($rest:tt)+) => {
        $crate::log!(target: $target, tags: $tags, $crate::Level::ERROR, $($rest)+)
    };
    (target: $target:expr, $($rest:tt)+) => {
        $crate::log!(target: $target, $crate::Level::ERROR, $($rest)+)
    };
    (tags: $tags:tt, $($rest:tt)+) => {
        $crate::log!(tags: $tags, $crate::Level::ERROR, $($rest)+)
    };
    ($($rest:tt)+) => {
        $crate::log!($crate::Level::ERROR, $($rest)+)
    };
}

/// Logs at [`Level::FATAL`](crate::Level::FATAL); takes what [`log!`] takes after the level.
#[macro_export]
macro_rules! fatal {
    (target: $target:expr, tags: $tags:tt, $($rest:tt)+) => {
        $crate::log!(target: $target, tags: $tags, $crate::Level::FATAL, $($rest)+)
    };
    (target: $target:expr, $($rest:tt)+) => {
        $crate::log!(target: $target, $crate::Level::FATAL, $($rest)+)
    };
    (tags: $tags:tt, $($rest:tt)+) => {
        $crate::log!(tags: $tags, $crate::Level::FATAL, $($rest)+)
    };
    ($($rest:tt)+) => {
        $crate::log!($crate::Level::FATAL, $($rest)+)
    };
}

/// Binds fields to this thread until the returned guard drops: `let _g = bind!(mirror = %name, ?state);`.
#[macro_export]
macro_rules! bind {
    (@ [$($f:tt)*] $(,)?) => { $crate::Bound::new(&[$($f)*]) };
    (@ [$($f:tt)*] $k:ident = ? $v:expr, $($rest:tt)*) => {
        $crate::bind!(@ [$($f)* (stringify!($k), $crate::Value::Debug(&$v)),] $($rest)*)
    };
    (@ [$($f:tt)*] $k:ident = % $v:expr, $($rest:tt)*) => {
        $crate::bind!(@ [$($f)* (stringify!($k), $crate::Value::Display(&$v)),] $($rest)*)
    };
    (@ [$($f:tt)*] $k:ident = $v:expr, $($rest:tt)*) => {
        $crate::bind!(@ [$($f)* (stringify!($k), $crate::ToValue::to_value(&$v)),] $($rest)*)
    };
    (@ [$($f:tt)*] ? $k:ident, $($rest:tt)*) => {
        $crate::bind!(@ [$($f)* (stringify!($k), $crate::Value::Debug(&$k)),] $($rest)*)
    };
    (@ [$($f:tt)*] % $k:ident, $($rest:tt)*) => {
        $crate::bind!(@ [$($f)* (stringify!($k), $crate::Value::Display(&$k)),] $($rest)*)
    };
    (@ [$($f:tt)*] $k:ident, $($rest:tt)*) => {
        $crate::bind!(@ [$($f)* (stringify!($k), $crate::ToValue::to_value(&$k)),] $($rest)*)
    };
    ($($rest:tt)+) => {
        $crate::bind!(@ [] $($rest)+,)
    };
}

/// The enclosing function's name, from the type name of a function item declared inside it.
#[must_use]
pub fn func(here: &'static str) -> &'static str {
    let mut path = here.strip_suffix("::__serlog_here").unwrap_or(here);
    while let Some(p) = path.strip_suffix("::{{closure}}") {
        path = p;
    }
    path.rsplit("::").next().unwrap_or(path)
}
