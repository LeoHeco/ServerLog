//! A config entry being read: typed getters that consume keys, and an error for any key left over.

use std::{fmt::Display, str::FromStr, time::Duration};

use serde_json::{Map, Value};

use super::units;
use crate::{Error, Level, Result};

/// The keys of one config entry, taken one at a time; what is left at the end is an error.
#[derive(Debug)]
pub struct Opts {
    map: Map<String, Value>,
    what: String,
    pub(crate) slot_level: bool,
}

impl Opts {
    /// Wraps `map`; `what` names the entry in errors, such as `file sink`.
    #[must_use]
    pub fn new(map: Map<String, Value>, what: impl Into<String>) -> Self {
        Self {
            map,
            what: what.into(),
            slot_level: false,
        }
    }

    /// An error about this entry.
    #[must_use]
    pub fn bad(&self, msg: impl Display) -> Error {
        Error::Config(format!("{}: {msg}", self.what))
    }

    /// The keys not taken yet.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        self.map.keys().cloned().collect()
    }

    /// Takes a key as it is.
    pub fn take(&mut self, key: &str) -> Option<Value> {
        self.map.remove(key)
    }

    /// Whether the slot set its own level or filter, which sinks with a level of their own defer to.
    #[must_use]
    pub const fn slot_level(&self) -> bool {
        self.slot_level
    }

    /// Takes a string.
    ///
    /// # Errors
    /// When the value is not a string.
    pub fn str(&mut self, key: &str) -> Result<Option<String>> {
        match self.take(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => Ok(Some(s)),
            Some(v) => Err(self.bad(format!("`{key}` must be text, not {v}"))),
        }
    }

    /// Takes a string that must be there.
    ///
    /// # Errors
    /// When it is missing or not a string.
    pub fn need(&mut self, key: &str) -> Result<String> {
        self.str(key)?
            .ok_or_else(|| self.bad(format!("`{key}` is required")))
    }

    /// Takes a boolean.
    ///
    /// # Errors
    /// When the value is not a boolean.
    pub fn bool(&mut self, key: &str) -> Result<Option<bool>> {
        match self.take(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Bool(b)) => Ok(Some(b)),
            Some(v) => Err(self.bad(format!("`{key}` must be true or false, not {v}"))),
        }
    }

    /// Takes a whole number.
    ///
    /// # Errors
    /// When the value is not a non-negative whole number.
    pub fn uint(&mut self, key: &str) -> Result<Option<u64>> {
        match self.take(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(n)) if n.as_u64().is_some() => Ok(n.as_u64()),
            Some(v) => Err(self.bad(format!("`{key}` must be a whole number, not {v}"))),
        }
    }

    /// Takes a number.
    ///
    /// # Errors
    /// When the value is not a number.
    pub fn num(&mut self, key: &str) -> Result<Option<f64>> {
        match self.take(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(n)) => Ok(n.as_f64()),
            Some(v) => Err(self.bad(format!("`{key}` must be a number, not {v}"))),
        }
    }

    /// Takes a byte count: a number, or text such as `64MiB`.
    ///
    /// # Errors
    /// When it is neither.
    pub fn size(&mut self, key: &str) -> Result<Option<u64>> {
        match self.take(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(n)) if n.as_u64().is_some() => Ok(n.as_u64()),
            Some(Value::String(s)) => units::size(&s)
                .map(Some)
                .ok_or_else(|| self.bad(format!("`{key}`: bad size `{s}`"))),
            Some(v) => Err(self.bad(format!("`{key}` must be a size, not {v}"))),
        }
    }

    /// Takes a duration: seconds, or text such as `250ms` or `5m`.
    ///
    /// # Errors
    /// When it is neither.
    pub fn duration(&mut self, key: &str) -> Result<Option<Duration>> {
        match self.take(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(n)) => n
                .as_f64()
                .filter(|f| *f >= 0.0)
                .map(|f| Some(Duration::from_secs_f64(f)))
                .ok_or_else(|| self.bad(format!("`{key}` must not be negative"))),
            Some(Value::String(s)) => units::duration(&s)
                .map(Some)
                .ok_or_else(|| self.bad(format!("`{key}`: bad duration `{s}`"))),
            Some(v) => Err(self.bad(format!("`{key}` must be a duration, not {v}"))),
        }
    }

    /// Takes one string or a list of them.
    ///
    /// # Errors
    /// When an item is not a string.
    pub fn strs(&mut self, key: &str) -> Result<Vec<String>> {
        match self.take(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::String(s)) => Ok(vec![s]),
            Some(Value::Array(a)) => a
                .into_iter()
                .map(|v| match v {
                    Value::String(s) => Ok(s),
                    v => Err(self.bad(format!("`{key}` items must be text, not {v}"))),
                })
                .collect(),
            Some(v) => Err(self.bad(format!("`{key}` must be a list, not {v}"))),
        }
    }

    /// Takes a map of strings, numbers and booleans, all as text.
    ///
    /// # Errors
    /// When it is not such a map.
    pub fn pairs(&mut self, key: &str) -> Result<Vec<(String, String)>> {
        match self.take(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Object(m)) => m
                .into_iter()
                .map(|(k, v)| match v {
                    Value::String(s) => Ok((k, s)),
                    Value::Number(_) | Value::Bool(_) => Ok((k, v.to_string())),
                    v => Err(self.bad(format!("`{key}.{k}` must be text, not {v}"))),
                })
                .collect(),
            Some(v) => Err(self.bad(format!("`{key}` must be a map, not {v}"))),
        }
    }

    /// Takes a map.
    ///
    /// # Errors
    /// When it is not one.
    pub fn map(&mut self, key: &str) -> Result<Option<Map<String, Value>>> {
        match self.take(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Object(m)) => Ok(Some(m)),
            Some(v) => Err(self.bad(format!("`{key}` must be a map, not {v}"))),
        }
    }

    /// Takes a list.
    ///
    /// # Errors
    /// When it is not one.
    pub fn list(&mut self, key: &str) -> Result<Vec<Value>> {
        match self.take(key) {
            None | Some(Value::Null) => Ok(Vec::new()),
            Some(Value::Array(a)) => Ok(a),
            Some(v) => Err(self.bad(format!("`{key}` must be a list, not {v}"))),
        }
    }

    /// Takes text and parses it.
    ///
    /// # Errors
    /// When it is not text or does not parse.
    pub fn parse<T>(&mut self, key: &str) -> Result<Option<T>>
    where
        T: FromStr,
        T::Err: Display,
    {
        let Some(s) = self.str(key)? else {
            return Ok(None);
        };
        s.parse()
            .map(Some)
            .map_err(|e| self.bad(format!("`{key}`: {e}")))
    }

    /// Takes a level name or number.
    ///
    /// # Errors
    /// When it is neither.
    pub fn level(&mut self, key: &str) -> Result<Option<Level>> {
        match self.take(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::Number(n)) => n
                .as_u64()
                .and_then(|n| u8::try_from(n).ok())
                .map(|n| Some(Level::new(n)))
                .ok_or_else(|| self.bad(format!("`{key}` must be 0 to 255"))),
            Some(Value::String(s)) => s
                .parse()
                .map(Some)
                .map_err(|e| self.bad(format!("`{key}`: {e}"))),
            Some(v) => Err(self.bad(format!("`{key}` must be a level, not {v}"))),
        }
    }

    /// Fails if any key was not taken.
    ///
    /// # Errors
    /// Naming the first unknown key.
    pub fn finish(self) -> Result {
        match self.map.keys().next() {
            Some(k) => Err(self.bad(format!("unknown key `{k}`"))),
            None => Ok(()),
        }
    }
}
