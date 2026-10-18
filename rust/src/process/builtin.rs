//! The built-in steps: redact, sanitise, fields, match, exclude, sample, limit and dedupe.

use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::{Mutex, PoisonError},
    time::{Duration, Instant},
};

use super::{Draft, Processor};
use crate::{Level, Record, ToValue, Val, Value};

/// Masks fields by name or dotted path, and parts of the message by regex.
#[derive(Clone, Debug)]
#[must_use]
pub struct Redact {
    keys: Vec<String>,
    paths: Vec<Vec<String>>,
    #[cfg(feature = "regex")]
    patterns: Vec<regex::Regex>,
    mask: String,
}

impl Default for Redact {
    fn default() -> Self {
        Self {
            keys: Vec::new(),
            paths: Vec::new(),
            #[cfg(feature = "regex")]
            patterns: Vec::new(),
            mask: "***".into(),
        }
    }
}

impl Redact {
    /// Masks fields with these names at any depth.
    pub fn keys(keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self::default().key(keys)
    }

    /// Adds names to mask at any depth.
    pub fn key(mut self, keys: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keys.extend(keys.into_iter().map(Into::into));
        self
    }

    /// Masks the field at a dotted path such as `req.headers.authorization`.
    pub fn path(mut self, path: &str) -> Self {
        self.paths
            .push(path.split('.').map(str::to_owned).collect());
        self
    }

    /// Replaces whatever matches `pattern` in the message.
    ///
    /// # Errors
    /// When the regex does not compile.
    #[cfg(feature = "regex")]
    pub fn pattern(mut self, pattern: &str) -> Result<Self, regex::Error> {
        self.patterns.push(regex::Regex::new(pattern)?);
        Ok(self)
    }

    /// What masked values become; `***` unless set.
    pub fn mask(mut self, mask: impl Into<String>) -> Self {
        self.mask = mask.into();
        self
    }

    fn hit(&self, path: &mut Vec<String>, v: Value<'_>) -> bool {
        let key = path.last().map(String::as_str).unwrap_or_default();
        if self.keys.iter().any(|k| k == key) || self.paths.iter().any(|p| p == path) {
            return true;
        }
        let Value::Map(m) = v else {
            return false;
        };
        let mut found = false;
        m.each(&mut |k, v| {
            if !found {
                path.push(k.to_owned());
                found = self.hit(path, v);
                path.pop();
            }
        });
        found
    }

    fn scrub(&self, path: &mut Vec<String>, v: &mut Val) {
        let key = path.last().map(String::as_str).unwrap_or_default();
        if self.keys.iter().any(|k| k == key) || self.paths.iter().any(|p| p == path) {
            *v = Val::Str(self.mask.clone());
            return;
        }
        if let Val::Map(m) = v {
            for (k, v) in m {
                path.push(k.clone());
                self.scrub(path, v);
                path.pop();
            }
        }
    }

    #[cfg(feature = "regex")]
    fn msg_hit(&self, msg: &str) -> bool {
        self.patterns.iter().any(|p| p.is_match(msg))
    }

    #[cfg(not(feature = "regex"))]
    #[expect(clippy::unused_self, reason = "the same shape as the regex build")]
    fn msg_hit(&self, _: &str) -> bool {
        false
    }
}

impl Processor for Redact {
    fn process(&self, draft: &mut Draft<'_>) -> bool {
        let rec = draft.record();
        let mut path = Vec::new();
        let fields = rec.fields().any(|(k, v)| {
            path.clear();
            path.push(k.to_owned());
            self.hit(&mut path, v)
        });
        let msg = self.msg_hit(&rec.msg().to_string());
        if !fields && !msg {
            return true;
        }
        let buf = draft.edit();
        for (k, v) in buf.fields_mut() {
            let mut path = vec![k.clone()];
            self.scrub(&mut path, v);
        }
        #[cfg(feature = "regex")]
        if msg {
            let mut text = buf.msg().to_owned();
            for p in &self.patterns {
                text = p.replace_all(&text, self.mask.as_str()).into_owned();
            }
            buf.set_msg(text);
        }
        true
    }

    fn kind(&self) -> &'static str {
        "redact"
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Value> {
        let mut m = serde_json::Map::new();
        if !self.keys.is_empty() {
            m.insert("keys".into(), serde_json::json!(self.keys));
        }
        if !self.paths.is_empty() {
            let paths: Vec<String> = self.paths.iter().map(|p| p.join(".")).collect();
            m.insert("paths".into(), serde_json::json!(paths));
        }
        #[cfg(feature = "regex")]
        if !self.patterns.is_empty() {
            let pats: Vec<&str> = self.patterns.iter().map(regex::Regex::as_str).collect();
            m.insert("patterns".into(), serde_json::json!(pats));
        }
        if self.mask != "***" {
            m.insert("mask".into(), self.mask.clone().into());
        }
        Some(serde_json::json!({ "redact": m }))
    }
}

/// Escapes line breaks and control characters in the message and string fields, against log forging.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sanitise;

fn dirty(s: &str) -> bool {
    s.chars().any(char::is_control)
}

fn clean(s: &str) -> String {
    s.chars()
        .fold(String::with_capacity(s.len()), |mut out, c| {
            match c {
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if c.is_control() => {
                    let _ = write!(out, "\\u{{{:04x}}}", u32::from(c));
                }
                c => out.push(c),
            }
            out
        })
}

impl Processor for Sanitise {
    fn process(&self, draft: &mut Draft<'_>) -> bool {
        let rec = draft.record();
        let msg = rec.msg().to_string();
        let fields = rec.fields().any(|(_, v)| v.as_str().is_some_and(dirty));
        if !dirty(&msg) && !fields {
            return true;
        }
        let buf = draft.edit();
        buf.set_msg(clean(&msg));
        for (_, v) in buf.fields_mut() {
            if let Val::Str(s) = v
                && dirty(s)
            {
                *s = clean(s);
            }
        }
        true
    }

    fn kind(&self) -> &'static str {
        "sanitise"
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({ "sanitise": true }))
    }
}

/// Adds the same fields to every record.
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct Fields(Vec<(String, Val)>);

impl Fields {
    /// No fields yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a field.
    pub fn with(mut self, key: impl Into<String>, value: impl ToValue) -> Self {
        self.0.push((key.into(), value.to_value().to_owned()));
        self
    }
}

impl Processor for Fields {
    fn process(&self, draft: &mut Draft<'_>) -> bool {
        if !self.0.is_empty() {
            let buf = draft.edit();
            for (k, v) in &self.0 {
                buf.set(k, v);
            }
        }
        true
    }

    fn kind(&self) -> &'static str {
        "fields"
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Value> {
        let m: serde_json::Map<String, serde_json::Value> = self
            .0
            .iter()
            .map(|(k, v)| (k.clone(), crate::config::to_json(v)))
            .collect();
        Some(serde_json::json!({ "fields": m }))
    }
}

/// Conditions on tags, fields and level; every one given must hold.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[must_use]
pub struct Match {
    tags: Vec<String>,
    fields: Vec<(String, String)>,
    level: Option<Level>,
}

impl Match {
    /// No conditions yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requires this tag.
    pub fn tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.push(tag.into());
        self
    }

    /// Requires a field shown as `value`.
    pub fn field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.push((key.into(), value.into()));
        self
    }

    /// Requires `min` or above.
    pub const fn level(mut self, min: Level) -> Self {
        self.level = Some(min);
        self
    }

    fn holds(&self, rec: &Record<'_>) -> bool {
        self.level.is_none_or(|l| rec.level() >= l)
            && self.tags.iter().all(|t| rec.tags().any(|x| x == t))
            && self
                .fields
                .iter()
                .all(|(k, want)| rec.field(k).is_some_and(|v| v.to_string() == *want))
    }

    #[cfg(feature = "config")]
    fn json(&self) -> serde_json::Value {
        let mut m = serde_json::Map::new();
        if !self.tags.is_empty() {
            m.insert("tags".into(), serde_json::json!(self.tags));
        }
        if !self.fields.is_empty() {
            let f: serde_json::Map<String, serde_json::Value> = self
                .fields
                .iter()
                .map(|(k, v)| (k.clone(), v.clone().into()))
                .collect();
            m.insert("fields".into(), f.into());
        }
        if let Some(l) = self.level {
            m.insert("level".into(), l.name().into());
        }
        m.into()
    }
}

impl Processor for Match {
    fn process(&self, draft: &mut Draft<'_>) -> bool {
        self.holds(&draft.record())
    }

    fn kind(&self) -> &'static str {
        "match"
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({ "match": self.json() }))
    }
}

/// Drops records that meet every condition of a [`Match`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exclude(pub Match);

impl Processor for Exclude {
    fn process(&self, draft: &mut Draft<'_>) -> bool {
        !self.0.holds(&draft.record())
    }

    fn kind(&self) -> &'static str {
        "exclude"
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({ "exclude": self.0.json() }))
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

type Key = (Level, String);
type Repeat = (Level, String, String);

/// Keeps the first records of each level and message in every period, then one in so many.
#[derive(Debug)]
pub struct Sample {
    first: u64,
    every: u64,
    per: Duration,
    seen: Mutex<HashMap<Key, (Instant, u64)>>,
}

impl Sample {
    /// Keeps the first `first` per `per`, then one in `every`.
    #[must_use]
    pub fn new(first: u64, every: u64, per: Duration) -> Self {
        Self {
            first,
            every: every.max(1),
            per,
            seen: Mutex::default(),
        }
    }
}

impl Processor for Sample {
    fn process(&self, draft: &mut Draft<'_>) -> bool {
        let rec = draft.record();
        let now = Instant::now();
        let mut seen = lock(&self.seen);
        if seen.len() > 10_000 {
            seen.retain(|_, (t, _)| now.duration_since(*t) < self.per);
        }
        let slot = seen
            .entry((rec.level(), rec.msg().to_string()))
            .or_insert((now, 0));
        if now.duration_since(slot.0) >= self.per {
            *slot = (now, 0);
        }
        slot.1 += 1;
        slot.1 <= self.first || (slot.1 - self.first).is_multiple_of(self.every)
    }

    fn kind(&self) -> &'static str {
        "sample"
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({ "sample": {
            "first": self.first, "every": self.every, "per": crate::config::units::show(self.per)
        }}))
    }
}

/// A token bucket: `rate` records a second, up to `burst` at once.
#[derive(Debug)]
pub struct Limit {
    rate: f64,
    burst: f64,
    bucket: Mutex<(f64, Instant)>,
}

impl Limit {
    /// Lets `rate` records a second through, with bursts of up to `burst`.
    #[must_use]
    pub fn new(rate: f64, burst: f64) -> Self {
        Self {
            rate,
            burst: burst.max(1.0),
            bucket: Mutex::new((burst.max(1.0), Instant::now())),
        }
    }
}

impl Processor for Limit {
    fn process(&self, _: &mut Draft<'_>) -> bool {
        let mut b = lock(&self.bucket);
        let now = Instant::now();
        b.0 = now
            .duration_since(b.1)
            .as_secs_f64()
            .mul_add(self.rate, b.0)
            .min(self.burst);
        b.1 = now;
        if b.0 >= 1.0 {
            b.0 -= 1.0;
            true
        } else {
            false
        }
    }

    fn kind(&self) -> &'static str {
        "limit"
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({ "limit": { "rate": self.rate, "burst": self.burst } }))
    }
}

/// Drops a record repeated within `within`; the next one kept carries `repeated`.
#[derive(Debug)]
pub struct Dedupe {
    within: Duration,
    seen: Mutex<HashMap<Repeat, (Instant, u64)>>,
}

impl Dedupe {
    /// Drops repeats within `within`.
    #[must_use]
    pub fn new(within: Duration) -> Self {
        Self {
            within,
            seen: Mutex::default(),
        }
    }
}

impl Processor for Dedupe {
    fn process(&self, draft: &mut Draft<'_>) -> bool {
        let rec = draft.record();
        let key = (rec.level(), rec.target().to_owned(), rec.msg().to_string());
        let now = Instant::now();
        let repeated = {
            let mut seen = lock(&self.seen);
            if seen.len() > 10_000 {
                seen.retain(|_, (t, _)| now.duration_since(*t) < self.within);
            }
            match seen.get_mut(&key) {
                Some((t, n)) if now.duration_since(*t) < self.within => {
                    *n += 1;
                    return false;
                }
                Some(slot) => std::mem::replace(slot, (now, 0)).1,
                None => {
                    seen.insert(key, (now, 0));
                    0
                }
            }
        };
        if repeated > 0 {
            draft.edit().set("repeated", repeated);
        }
        true
    }

    fn kind(&self) -> &'static str {
        "dedupe"
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Value> {
        Some(serde_json::json!({ "dedupe": { "within": crate::config::units::show(self.within) } }))
    }
}
