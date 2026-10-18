//! Email for serious records; a batch goes out as one message, plain text and HTML.

use std::{fmt::Write as _, sync::OnceLock, time::Duration};

use lettre::{
    Message, SmtpTransport, Transport,
    message::{Mailbox, MultiPart},
    transport::smtp::authentication::Credentials,
};

use crate::{Entry, Error, Level, Result, Sink, layout::time::local, meta, slot::Queue};

const PAGE: &str = include_str!("../../assets/mail.html");

/// How the SMTP connection is secured.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Security {
    /// TLS from the first byte, usually port 465.
    #[default]
    Tls,
    /// Plain connection upgraded with STARTTLS, usually port 587.
    StartTls,
    /// No encryption, for local relays and tests.
    None,
}

impl Security {
    /// The name used in config files.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Tls => "tls",
            Self::StartTls => "starttls",
            Self::None => "none",
        }
    }
}

struct Wired {
    smtp: SmtpTransport,
    from: Mailbox,
    to: Vec<Mailbox>,
}

/// Sends batches of records by SMTP; only errors and worse unless told otherwise.
#[must_use]
pub struct Mail {
    host: String,
    port: Option<u16>,
    security: Security,
    auth: Option<(String, String)>,
    from: Option<String>,
    to: Vec<String>,
    min: Level,
    timeout: Duration,
    connect: bool,
    wired: OnceLock<Wired>,
}

impl Mail {
    /// Sends through the SMTP server at `host`.
    pub fn new(host: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            port: None,
            security: Security::Tls,
            auth: None,
            from: None,
            to: Vec::new(),
            min: Level::ERROR,
            timeout: Duration::from_secs(10),
            connect: false,
            wired: OnceLock::new(),
        }
    }

    /// Server port, when not the default for the security mode.
    pub const fn port(mut self, port: u16) -> Self {
        self.port = Some(port);
        self
    }

    /// How the connection is secured.
    pub const fn security(mut self, security: Security) -> Self {
        self.security = security;
        self
    }

    /// Login; the user also serves as the sender unless [`from`](Self::from) is set.
    pub fn auth(mut self, user: impl Into<String>, pass: impl Into<String>) -> Self {
        self.auth = Some((user.into(), pass.into()));
        self
    }

    /// Sender address.
    pub fn from(mut self, addr: impl Into<String>) -> Self {
        self.from = Some(addr.into());
        self
    }

    /// Adds a recipient.
    pub fn to(mut self, addr: impl Into<String>) -> Self {
        self.to.push(addr.into());
        self
    }

    /// Lowest level mailed; `error` unless set.
    pub const fn level(mut self, min: Level) -> Self {
        self.min = min;
        self
    }

    /// Longest a send may take.
    pub const fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Connects and logs in at start-up.
    pub const fn connect(mut self) -> Self {
        self.connect = true;
        self
    }

    fn wire(&self) -> Result<Wired> {
        let bad = |e: &dyn std::fmt::Display| Error::Config(format!("mail: {e}"));
        let from = self
            .from
            .clone()
            .or_else(|| self.auth.as_ref().map(|(u, _)| u.clone()))
            .ok_or_else(|| bad(&"no sender"))?;
        let from: Mailbox = from.parse().map_err(|e| bad(&e))?;
        let to = self
            .to
            .iter()
            .map(|a| a.parse::<Mailbox>())
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| bad(&e))?;
        if to.is_empty() {
            return Err(bad(&"no recipients"));
        }
        let mut smtp = match self.security {
            Security::Tls => SmtpTransport::relay(&self.host).map_err(|e| bad(&e))?,
            Security::StartTls => SmtpTransport::starttls_relay(&self.host).map_err(|e| bad(&e))?,
            Security::None => SmtpTransport::builder_dangerous(&self.host),
        }
        .timeout(Some(self.timeout));
        if let Some(port) = self.port {
            smtp = smtp.port(port);
        }
        if let Some((user, pass)) = &self.auth {
            smtp = smtp.credentials(Credentials::new(user.clone(), pass.clone()));
        }
        Ok(Wired {
            smtp: smtp.build(),
            from,
            to,
        })
    }

    fn wired(&self) -> Result<&Wired> {
        if let Some(w) = self.wired.get() {
            return Ok(w);
        }
        let w = self.wire()?;
        Ok(self.wired.get_or_init(|| w))
    }

    /// Subject, plain text and HTML for one batch.
    fn render(entries: &[Entry<'_>]) -> (String, String, String) {
        let m = meta();
        let top = entries
            .iter()
            .map(|e| e.record().level())
            .max()
            .unwrap_or_default();
        let n = entries.len();
        let subject = format!(
            "[{}] {n} {} record{}",
            m.app,
            top.label(crate::compat()),
            if n == 1 { "" } else { "s" }
        );
        let mut text = String::new();
        let mut rows = String::new();
        for e in entries {
            let rec = e.record();
            let time = local(rec.time()).strftime("%Y-%m-%d %H:%M:%S").to_string();
            let _ = writeln!(text, "{}", e.line());
            let _ = write!(
                rows,
                "<tr><td>{time}</td><td class=\"{}\">{}</td><td>{}</td><td>{}</td></tr>",
                rec.level().name(),
                rec.level().label(crate::compat()),
                escape(rec.target()),
                escape(&rec.msg().to_string())
            );
        }
        let html = [
            ("{{app}}", escape(&m.app)),
            ("{{env}}", escape(&m.env)),
            ("{{instance}}", escape(&m.instance)),
            ("{{level}}", top.name().to_owned()),
            ("{{count}}", n.to_string()),
            ("{{rows}}", rows),
        ]
        .iter()
        .fold(PAGE.to_owned(), |page, (k, v)| page.replace(k, v));
        (subject, text, html)
    }
}

impl Sink for Mail {
    fn emit(&self, entry: &Entry<'_>) -> Result {
        self.emit_batch(std::slice::from_ref(entry))
    }

    fn emit_batch(&self, entries: &[Entry<'_>]) -> Result {
        if entries.is_empty() {
            return Ok(());
        }
        let w = self.wired()?;
        let (subject, text, html) = Self::render(entries);
        let mut msg = Message::builder().from(w.from.clone()).subject(subject);
        for r in &w.to {
            msg = msg.to(r.clone());
        }
        let msg = msg
            .multipart(MultiPart::alternative_plain_html(text, html))
            .map_err(Error::other)?;
        w.smtp.send(&msg).map(drop).map_err(|e| {
            if e.is_permanent() {
                Error::Rejected(e.to_string())
            } else {
                Error::other(e)
            }
        })
    }

    fn enabled(&self, level: Level, _: &str) -> bool {
        level >= self.min
    }

    fn ready(&self) -> Result {
        if self.auth.as_ref().is_some_and(|(_, p)| p.is_empty()) {
            return Err(Error::Config("mail: a user needs a password".into()));
        }
        let w = self.wired()?;
        if self.connect {
            w.smtp.test_connection().map_err(Error::other)?;
        }
        Ok(())
    }

    fn kind(&self) -> &'static str {
        "mail"
    }

    fn queued(&self) -> Option<Queue> {
        Some(Queue::new(1024).wait(Duration::from_mins(1)))
    }

    #[cfg(feature = "config")]
    fn options(&self) -> Option<serde_json::Map<String, serde_json::Value>> {
        let mut m = serde_json::Map::new();
        m.insert("host".into(), self.host.clone().into());
        if let Some(p) = self.port {
            m.insert("port".into(), p.into());
        }
        if self.security != Security::Tls {
            m.insert("security".into(), self.security.name().into());
        }
        if let Some((user, _)) = &self.auth {
            m.insert("user".into(), user.clone().into());
            m.insert("pass".into(), "***".into());
        }
        if let Some(f) = &self.from {
            m.insert("from".into(), f.clone().into());
        }
        m.insert("to".into(), serde_json::json!(self.to));
        if self.min != Level::ERROR {
            m.insert("level".into(), self.min.name().into());
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

fn escape(s: &str) -> String {
    s.chars()
        .fold(String::with_capacity(s.len()), |mut out, c| {
            match c {
                '&' => out.push_str("&amp;"),
                '<' => out.push_str("&lt;"),
                '>' => out.push_str("&gt;"),
                '"' => out.push_str("&quot;"),
                '\'' => out.push_str("&#39;"),
                c => out.push(c),
            }
            out
        })
}
