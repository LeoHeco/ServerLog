//! Typed field values, borrowed while a record is dispatched and owned once it is queued.

use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap},
    error::Error,
    fmt::{self, Display},
    sync::Arc,
};

/// A field value; layouts write each type its own way.
#[derive(Clone, Copy)]
#[non_exhaustive]
pub enum Value<'a> {
    /// Nothing.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A signed integer.
    I64(i64),
    /// An unsigned integer.
    U64(u64),
    /// A float.
    F64(f64),
    /// Text.
    Str(&'a str),
    /// Anything shown with `Display`.
    Display(&'a dyn Display),
    /// Anything shown with `Debug`.
    Debug(&'a dyn fmt::Debug),
    /// An error, with its chain of causes.
    Error(&'a (dyn Error + 'static)),
    /// A sequence.
    List(&'a dyn List),
    /// Named values, a group in slog's terms.
    Map(&'a dyn Map),
}

/// A sequence of values.
pub trait List {
    /// Calls `f` with each item in order.
    fn each<'b>(&'b self, f: &mut dyn FnMut(Value<'b>));
}

/// Named values in order.
pub trait Map {
    /// Calls `f` with each key and value in order.
    fn each<'b>(&'b self, f: &mut dyn FnMut(&'b str, Value<'b>));
}

/// Anything that can be a field.
#[diagnostic::on_unimplemented(
    message = "`{Self}` cannot be a serlog field value",
    note = "write `%name` to show it with `Display`, or `?name` with `Debug`"
)]
pub trait ToValue {
    /// Borrows it as a [`Value`].
    fn to_value(&self) -> Value<'_>;
}

impl ToValue for Value<'_> {
    fn to_value(&self) -> Value<'_> {
        *self
    }
}

impl<T: ToValue + ?Sized> ToValue for &T {
    fn to_value(&self) -> Value<'_> {
        (**self).to_value()
    }
}

impl<T: ToValue> ToValue for Option<T> {
    fn to_value(&self) -> Value<'_> {
        self.as_ref().map_or(Value::Null, ToValue::to_value)
    }
}

impl ToValue for bool {
    fn to_value(&self) -> Value<'_> {
        Value::Bool(*self)
    }
}

macro_rules! ints {
    ($variant:ident: $($ty:ty),+) => {$(
        impl ToValue for $ty {
            fn to_value(&self) -> Value<'_> {
                Value::$variant((*self).into())
            }
        }
    )+};
}

ints!(I64: i8, i16, i32, i64);
ints!(U64: u8, u16, u32, u64);
ints!(F64: f32, f64);

impl ToValue for isize {
    fn to_value(&self) -> Value<'_> {
        i64::try_from(*self).map_or(Value::Display(self), Value::I64)
    }
}

impl ToValue for usize {
    fn to_value(&self) -> Value<'_> {
        u64::try_from(*self).map_or(Value::Display(self), Value::U64)
    }
}

impl ToValue for char {
    fn to_value(&self) -> Value<'_> {
        Value::Display(self)
    }
}

impl ToValue for str {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self)
    }
}

impl ToValue for String {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self)
    }
}

impl ToValue for Cow<'_, str> {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self)
    }
}

impl ToValue for Arc<str> {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self)
    }
}

impl ToValue for Box<str> {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self)
    }
}

impl ToValue for dyn Error + 'static {
    fn to_value(&self) -> Value<'_> {
        Value::Error(self)
    }
}

impl ToValue for dyn Error + Send + Sync + 'static {
    fn to_value(&self) -> Value<'_> {
        Value::Error(self)
    }
}

impl<T: ToValue> List for Vec<T> {
    fn each<'b>(&'b self, f: &mut dyn FnMut(Value<'b>)) {
        for v in self {
            f(v.to_value());
        }
    }
}

impl<T: ToValue, const N: usize> List for [T; N] {
    fn each<'b>(&'b self, f: &mut dyn FnMut(Value<'b>)) {
        for v in self {
            f(v.to_value());
        }
    }
}

impl<T: ToValue> ToValue for Vec<T> {
    fn to_value(&self) -> Value<'_> {
        Value::List(self)
    }
}

impl<T: ToValue, const N: usize> ToValue for [T; N] {
    fn to_value(&self) -> Value<'_> {
        Value::List(self)
    }
}

impl<K: AsRef<str>, V: ToValue> Map for Vec<(K, V)> {
    fn each<'b>(&'b self, f: &mut dyn FnMut(&'b str, Value<'b>)) {
        for (k, v) in self {
            f(k.as_ref(), v.to_value());
        }
    }
}

impl<K: AsRef<str>, V: ToValue, const N: usize> Map for [(K, V); N] {
    fn each<'b>(&'b self, f: &mut dyn FnMut(&'b str, Value<'b>)) {
        for (k, v) in self {
            f(k.as_ref(), v.to_value());
        }
    }
}

impl<K: AsRef<str>, V: ToValue> Map for BTreeMap<K, V> {
    fn each<'b>(&'b self, f: &mut dyn FnMut(&'b str, Value<'b>)) {
        for (k, v) in self {
            f(k.as_ref(), v.to_value());
        }
    }
}

impl<K: AsRef<str>, V: ToValue, S> Map for HashMap<K, V, S> {
    fn each<'b>(&'b self, f: &mut dyn FnMut(&'b str, Value<'b>)) {
        for (k, v) in self {
            f(k.as_ref(), v.to_value());
        }
    }
}

impl<K: AsRef<str>, V: ToValue> ToValue for BTreeMap<K, V> {
    fn to_value(&self) -> Value<'_> {
        Value::Map(self)
    }
}

impl<K: AsRef<str>, V: ToValue, S> ToValue for HashMap<K, V, S> {
    fn to_value(&self) -> Value<'_> {
        Value::Map(self)
    }
}

/// Shows an error as a field: `info!(error = serlog::err(&e); "sync failed")`.
#[must_use]
pub fn err<'a>(e: &'a (dyn Error + 'static)) -> Value<'a> {
    Value::Error(e)
}

/// Named values as one grouped field: `info!(req = serlog::group(&[("id", &7), ("path", &path)]), "handled")`.
#[must_use]
pub const fn group<'a, const N: usize>(pairs: &'a [(&'a str, &'a dyn ToValue); N]) -> Value<'a> {
    Value::Map(pairs)
}

impl Value<'_> {
    /// Copies the value out.
    #[must_use]
    pub fn to_owned(&self) -> Val {
        match *self {
            Self::Null => Val::Null,
            Self::Bool(b) => Val::Bool(b),
            Self::I64(n) => Val::I64(n),
            Self::U64(n) => Val::U64(n),
            Self::F64(n) => Val::F64(n),
            Self::Str(s) => Val::Str(s.to_owned()),
            Self::Display(d) => Val::Str(d.to_string()),
            Self::Debug(d) => Val::Str(format!("{d:?}")),
            Self::Error(e) => Val::Error(ErrorBuf::of(e)),
            Self::List(l) => {
                let mut out = Vec::new();
                l.each(&mut |v| out.push(v.to_owned()));
                Val::List(out)
            }
            Self::Map(m) => {
                let mut out = Vec::new();
                m.each(&mut |k, v| out.push((k.to_owned(), v.to_owned())));
                Val::Map(out)
            }
        }
    }

    /// The text when it is a string, without copying.
    #[must_use]
    pub const fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }
}

impl Display for Value<'_> {
    /// Text form: lists as `[a, b]`, maps as `{k=v, …}`, errors with their causes.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Null => f.write_str("null"),
            Self::Bool(b) => write!(f, "{b}"),
            Self::I64(n) => write!(f, "{n}"),
            Self::U64(n) => write!(f, "{n}"),
            Self::F64(n) => write!(f, "{n}"),
            Self::Str(s) => f.write_str(s),
            Self::Display(d) => write!(f, "{d}"),
            Self::Debug(d) => write!(f, "{d:?}"),
            Self::Error(e) => {
                write!(f, "{e}")?;
                let mut cause = e.source();
                while let Some(c) = cause {
                    write!(f, ": {c}")?;
                    cause = c.source();
                }
                Ok(())
            }
            Self::List(l) => {
                let mut r = f.write_str("[");
                let mut first = true;
                l.each(&mut |v| {
                    if r.is_ok() {
                        r = if first {
                            write!(f, "{v}")
                        } else {
                            write!(f, ", {v}")
                        };
                        first = false;
                    }
                });
                r.and_then(|()| f.write_str("]"))
            }
            Self::Map(m) => {
                let mut r = f.write_str("{");
                let mut first = true;
                m.each(&mut |k, v| {
                    if r.is_ok() {
                        r = write!(f, "{}{k}={v}", if first { "" } else { ", " });
                        first = false;
                    }
                });
                r.and_then(|()| f.write_str("}"))
            }
        }
    }
}

impl fmt::Debug for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Str(s) => fmt::Debug::fmt(s, f),
            v => write!(f, "{v}"),
        }
    }
}

/// An owned field value, as queued records and alarms hold them.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Val {
    /// Nothing.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A signed integer.
    I64(i64),
    /// An unsigned integer.
    U64(u64),
    /// A float.
    F64(f64),
    /// Text.
    Str(String),
    /// An error, with its chain of causes.
    Error(ErrorBuf),
    /// A sequence.
    List(Vec<Self>),
    /// Named values.
    Map(Vec<(String, Self)>),
}

impl ToValue for Val {
    fn to_value(&self) -> Value<'_> {
        match self {
            Self::Null => Value::Null,
            Self::Bool(b) => Value::Bool(*b),
            Self::I64(n) => Value::I64(*n),
            Self::U64(n) => Value::U64(*n),
            Self::F64(n) => Value::F64(*n),
            Self::Str(s) => Value::Str(s),
            Self::Error(e) => Value::Error(e),
            Self::List(l) => Value::List(l),
            Self::Map(m) => Value::Map(m),
        }
    }
}

impl From<String> for Val {
    fn from(s: String) -> Self {
        Self::Str(s)
    }
}

impl From<&str> for Val {
    fn from(s: &str) -> Self {
        Self::Str(s.to_owned())
    }
}

/// An error copied out of its source: message, type name and causes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorBuf {
    msg: String,
    source: Option<Box<Self>>,
}

impl ErrorBuf {
    /// Copies `e` and its causes.
    #[must_use]
    pub fn of(e: &(dyn Error + 'static)) -> Self {
        Self {
            msg: e.to_string(),
            source: e.source().map(|s| Box::new(Self::of(s))),
        }
    }

    /// An error with just a message.
    #[must_use]
    pub fn new(msg: impl Into<String>) -> Self {
        Self {
            msg: msg.into(),
            source: None,
        }
    }

    /// Adds the error this one was caused by, after any causes it already has.
    #[must_use]
    pub fn because(mut self, cause: Self) -> Self {
        self.source = Some(Box::new(match self.source.take() {
            Some(s) => s.because(cause),
            None => cause,
        }));
        self
    }
}

impl Display for ErrorBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.msg)
    }
}

impl Error for ErrorBuf {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.source.as_deref().map(|s| s as &(dyn Error + 'static))
    }
}
