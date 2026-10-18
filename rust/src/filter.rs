//! Per-target levels written the `RUST_LOG` way: `info,app=debug,app::db=trace,hyper=off,app:*=warn/regex`.

use std::{fmt, str::FromStr};

use crate::{Level, ParseLevelError};

/// Which records pass, by target, level and optionally message.
///
/// The most specific matching directive wins: literal targets by length, then wildcards. A target
/// matches its own name and anything below it, split at `::`, `.`, `:` or `/`.
#[derive(Clone, Debug)]
pub struct Filter {
    base: Option<Level>,
    rules: Vec<Rule>,
    #[cfg(feature = "regex")]
    pattern: Option<regex::Regex>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Rule {
    target: String,
    glob: bool,
    level: Option<Level>,
}

impl PartialEq for Filter {
    fn eq(&self, other: &Self) -> bool {
        self.to_string() == other.to_string()
    }
}

impl Default for Filter {
    fn default() -> Self {
        Self::level(Level::INFO)
    }
}

impl Filter {
    /// Lets through `min` and above for every target.
    #[must_use]
    pub const fn level(min: Level) -> Self {
        Self {
            base: Some(min),
            rules: Vec::new(),
            #[cfg(feature = "regex")]
            pattern: None,
        }
    }

    /// Lets nothing through.
    #[must_use]
    pub const fn off() -> Self {
        Self {
            base: None,
            rules: Vec::new(),
            #[cfg(feature = "regex")]
            pattern: None,
        }
    }

    /// Whether a record at `level` from `target` passes, before its message is looked at.
    #[must_use]
    pub fn allows(&self, level: Level, target: &str) -> bool {
        self.rules
            .iter()
            .find(|r| r.matches(target))
            .map_or(self.base, |r| r.level)
            .is_some_and(|min| level >= min)
    }

    /// Whether the message passes the `/regex` part, if there is one.
    #[must_use]
    pub fn admits(&self, msg: &str) -> bool {
        #[cfg(feature = "regex")]
        if let Some(p) = &self.pattern {
            return p.is_match(msg);
        }
        let _ = msg;
        true
    }

    /// Whether there is a `/regex` part.
    #[must_use]
    pub const fn has_pattern(&self) -> bool {
        #[cfg(feature = "regex")]
        return self.pattern.is_some();
        #[cfg(not(feature = "regex"))]
        false
    }

    /// The lowest level any target lets through, or `None` when everything is off.
    #[must_use]
    pub fn floor(&self) -> Option<Level> {
        self.rules
            .iter()
            .map(|r| r.level)
            .chain([self.base])
            .flatten()
            .min()
    }
}

impl Rule {
    fn matches(&self, target: &str) -> bool {
        if self.glob {
            return glob(self.target.as_bytes(), target.as_bytes());
        }
        target
            .strip_prefix(self.target.as_str())
            .is_some_and(|rest| {
                rest.is_empty()
                    || ["::", ".", ":", "/"]
                        .iter()
                        .any(|sep| rest.starts_with(sep))
            })
    }

    fn rank(&self) -> (bool, std::cmp::Reverse<usize>) {
        (
            self.glob,
            std::cmp::Reverse(self.target.bytes().filter(|&b| b != b'*').count()),
        )
    }
}

fn glob(pat: &[u8], s: &[u8]) -> bool {
    match pat.split_first() {
        None => s.is_empty(),
        Some((b'*', rest)) => (0..=s.len()).any(|i| glob(rest, &s[i..])),
        Some((c, rest)) => s
            .split_first()
            .is_some_and(|(d, tail)| c == d && glob(rest, tail)),
    }
}

/// A filter spec that does not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseFilterError(String);

impl fmt::Display for ParseFilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseFilterError {}

impl From<ParseLevelError> for ParseFilterError {
    fn from(e: ParseLevelError) -> Self {
        Self(e.to_string())
    }
}

impl FromStr for Filter {
    type Err = ParseFilterError;

    /// Comma-separated directives: a level alone sets the default, `target=level` one target,
    /// a bare target turns everything on for it, `off` turns things off, and `*` is a wildcard.
    /// Anything after the first `/` is a regex the message must match.
    ///
    /// As with `env_logger`, unmatched targets are off unless a default is given, and an empty
    /// spec lets errors through.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (spec, pattern) = s.split_once('/').map_or((s, None), |(a, b)| (a, Some(b)));
        let level = |l: &str| -> Result<Option<Level>, ParseFilterError> {
            if l.eq_ignore_ascii_case("off") {
                Ok(None)
            } else {
                Ok(Some(l.parse()?))
            }
        };
        let mut f = if spec.trim().is_empty() {
            Self::level(Level::ERROR)
        } else {
            Self::off()
        };
        for d in spec.split(',').map(str::trim).filter(|d| !d.is_empty()) {
            let (target, lvl) = match d.split_once('=') {
                Some((t, l)) => (t.trim(), level(l.trim())?),
                None => match level(d) {
                    Ok(l) => {
                        f.base = l;
                        continue;
                    }
                    Err(_) => (d, Some(Level::new(0))),
                },
            };
            f.rules.push(Rule {
                glob: target.contains('*'),
                target: target.to_owned(),
                level: lvl,
            });
        }
        f.rules.sort_by_key(Rule::rank);
        if let Some(p) = pattern {
            #[cfg(feature = "regex")]
            {
                f.pattern =
                    Some(regex::Regex::new(p).map_err(|e| ParseFilterError(e.to_string()))?);
            }
            #[cfg(not(feature = "regex"))]
            return Err(ParseFilterError(format!(
                "`/{p}` needs the `regex` feature"
            )));
        }
        Ok(f)
    }
}

impl fmt::Display for Filter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = |l: Option<Level>| match l {
            None => "off".to_owned(),
            Some(l) if l.name() == "custom" => l.value().to_string(),
            Some(l) => l.name().to_owned(),
        };
        f.write_str(&name(self.base))?;
        self.rules.iter().try_for_each(|r| match r.level {
            Some(l) if l.value() == 0 => write!(f, ",{}", r.target),
            l => write!(f, ",{}={}", r.target, name(l)),
        })?;
        #[cfg(feature = "regex")]
        if let Some(p) = &self.pattern {
            write!(f, "/{p}")?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::Filter;
    use crate::Level;

    #[test]
    fn picks_the_most_specific_directive() {
        let f: Filter = "warn, app=debug, app::db=trace, app::net=off, noisy, web:*=error"
            .parse()
            .unwrap();
        assert!(f.allows(Level::WARN, "other") && !f.allows(Level::INFO, "other"));
        assert!(f.allows(Level::DEBUG, "app") && f.allows(Level::DEBUG, "app::sync"));
        assert!(!f.allows(Level::TRACE, "app::sync") && f.allows(Level::TRACE, "app::db::pool"));
        assert!(!f.allows(Level::ERROR, "app::net"));
        assert!(!f.allows(Level::DEBUG, "apparent"));
        assert!(f.allows(Level::TRACE, "noisy::x"));
        assert!(!f.allows(Level::WARN, "web:api") && f.allows(Level::ERROR, "web:api"));
        assert_eq!(f.floor(), Some(Level::new(0)));
        assert_eq!("off".parse::<Filter>().unwrap().floor(), None);
        assert!("app=loud".parse::<Filter>().is_err());
        assert_eq!(Filter::default().to_string(), "info");
    }

    #[cfg(feature = "regex")]
    #[test]
    fn matches_messages_after_a_slash() {
        let f: Filter = "info/sync(ed)?".parse().unwrap();
        assert!(f.admits("synced 3") && !f.admits("idle"));
        assert_eq!(f.to_string(), "info/sync(ed)?");
    }
}
