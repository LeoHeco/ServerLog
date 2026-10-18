//! Grafana Loki's push API, which Alloy's `loki.source.api` accepts as well.

use std::{
    collections::BTreeMap,
    fmt::Write as _,
    io::Write as _,
    sync::OnceLock,
    time::{Duration, UNIX_EPOCH},
};

use flate2::{Compression, write::GzEncoder};

use crate::{
    Entry, Error, Level, Result, Sink,
    layout::{Layout, Pattern, quote},
    meta,
    slot::Queue,
};

/// Pushes gzipped JSON, one stream per level; lines are the slot's layout, `{msg}` unless set.
#[must_use]
pub struct Loki {
    url: String,
    labels: Vec<(String, String)>,
    timeout: Duration,
    connect: bool,
    agent: OnceLock<ureq::Agent>,
}

impl Loki {
    /// Pushes to `url`, such as `http://localhost:3100/loki/api/v1/push`.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            labels: Vec::new(),
            timeout: Duration::from_secs(10),
            connect: false,
            agent: OnceLock::new(),
        }
    }

    /// Adds a stream label; keep their values few, as each combination is a stream.
    pub fn label(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.labels.push((key.into(), value.into()));
        self
    }

    /// Longest a push may take.
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Asks Loki's `/ready` at start-up.
    pub const fn connect(mut self) -> Self {
        self.connect = true;
        self
    }

    fn agent(&self) -> &ureq::Agent {
        self.agent.get_or_init(|| {
            ureq::Agent::config_builder()
                .timeout_global(Some(self.timeout))
                .http_status_as_error(false)
                .build()
                .into()
        })
    }

    /// The push request body: labels per stream, structured metadata per line.
    fn body(&self, entries: &[Entry<'_>]) -> String {
        let m = meta();
        let mut streams: BTreeMap<Level, Vec<&Entry<'_>>> = BTreeMap::new();
        for e in entries {
            streams.entry(e.record().level()).or_default().push(e);
        }
        let mut s = String::from("{\"streams\":[");
        for (i, (level, items)) in streams.iter().enumerate() {
            s.push_str(if i == 0 {
                "{\"stream\":"
            } else {
                ",{\"stream\":"
            });
            let base = [
                ("app", m.app.as_str()),
                ("env", &m.env),
                ("instance", &m.instance),
                ("level", level.name()),
            ];
            let own = self.labels.iter().map(|(k, v)| (k.as_str(), v.as_str()));
            object(
                &mut s,
                base.into_iter().filter(|(_, v)| !v.is_empty()).chain(own),
            );
            s.push_str(",\"values\":[");
            for (j, e) in items.iter().enumerate() {
                let rec = e.record();
                let ns = rec
                    .time()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| d.as_nanos());
                let _ = write!(s, "{}[\"{ns}\",", if j == 0 { "" } else { "," });
                let _ = quote(&mut s, e.line());
                s.push(',');
                let mut meta = vec![("target".to_owned(), rec.target().to_owned())];
                if let (Some(file), Some(n)) = (rec.file(), rec.line()) {
                    meta.push(("file".to_owned(), format!("{file}:{n}")));
                }
                crate::layout::flat(rec, &[], &mut |k, v| {
                    meta.push((k.to_owned(), v.to_string()));
                });
                object(&mut s, meta.iter().map(|(k, v)| (k.as_str(), v.as_str())));
                s.push(']');
            }
            s.push_str("]}");
        }
        s.push_str("]}");
        s
    }

    fn base(&self) -> &str {
        self.url
            .find("/loki/api/")
            .map_or(self.url.trim_end_matches('/'), |i| &self.url[..i])
    }
}

fn object<'a>(s: &mut String, pairs: impl Iterator<Item = (&'a str, &'a str)>) {
    s.push('{');
    for (i, (k, v)) in pairs.enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = quote(s, k);
        s.push(':');
        let _ = quote(s, v);
    }
    s.push('}');
}

impl Sink for Loki {
    fn emit(&self, entry: &Entry<'_>) -> Result {
        self.emit_batch(std::slice::from_ref(entry))
    }

    fn emit_batch(&self, entries: &[Entry<'_>]) -> Result {
        if entries.is_empty() {
            return Ok(());
        }
        let mut gz = GzEncoder::new(Vec::new(), Compression::fast());
        gz.write_all(self.body(entries).as_bytes())?;
        let data = gz.finish()?;
        let r = self
            .agent()
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Content-Encoding", "gzip")
            .send(&data[..])
            .map_err(Error::other)?;
        let status = r.status();
        match status.as_u16() {
            200..=299 => Ok(()),
            429 | 500..=599 => Err(Error::other(format!("loki answered {status}"))),
            _ => Err(Error::Rejected(format!("loki answered {status}"))),
        }
    }

    fn ready(&self) -> Result {
        if !(self.url.starts_with("http://") || self.url.starts_with("https://")) {
            return Err(Error::Config(format!(
                "loki url `{}` is not http or https",
                self.url
            )));
        }
        if self.connect {
            let r = self
                .agent()
                .get(format!("{}/ready", self.base()))
                .call()
                .map_err(Error::other)?;
            if !r.status().is_success() {
                return Err(Error::other(format!("loki /ready answered {}", r.status())));
            }
        }
        Ok(())
    }

    fn kind(&self) -> &'static str {
        "loki"
    }

    fn default_layout(&self) -> Layout {
        Pattern::message().into()
    }

    fn queued(&self) -> Option<Queue> {
        Some(Queue::new(8192))
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        let mut m = serde_json::Map::new();
        m.insert("url".into(), self.url.clone().into());
        if !self.labels.is_empty() {
            let l: serde_json::Map<String, serde_json::Value> = self
                .labels
                .iter()
                .map(|(k, v)| (k.clone(), v.clone().into()))
                .collect();
            m.insert("labels".into(), l.into());
        }
        if self.timeout != Duration::from_secs(10) {
            m.insert(
                "timeout".into(),
                crate::config::units::show(self.timeout).into(),
            );
        }
        if self.connect {
            m.insert("connect".into(), true.into());
        }
        Some(m)
    }
}
