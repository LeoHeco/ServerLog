//! A sink of your own, from definition to use: texting warnings and errors to a phone.

use std::fmt;

use serde_json::Value;
use serlog::{
    Entry, Error, Sink,
    config::{self, Format, kinds},
};

// region stubs
fn foo_phone_is_ok(phone: &Number) -> bool {
    phone.to_string().bytes().all(|b| b.is_ascii_digit())
}

fn foo_use_phone_to_log(phone: &Number, text: &str) {
    println!("sms {phone}: {text}");
}
// endregion

// region sink
enum Number {
    Int(u64),
    Str(String),
}

impl fmt::Display for Number {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(n) => write!(f, "{n}"),
            Self::Str(s) => f.write_str(s),
        }
    }
}

struct Phone {
    phone: Number,
}

impl Phone {
    fn new(phone: Option<Number>) -> serlog::Result<Self> {
        match phone {
            Some(phone) if foo_phone_is_ok(&phone) => Ok(Self { phone }),
            Some(phone) => Err(Error::Config(format!("cannot text {phone}"))),
            None => Err(Error::Config("cannot text nobody".into())),
        }
    }
}

impl Sink for Phone {
    fn kind(&self) -> &'static str {
        "phone"
    }

    fn emit(&self, entry: &Entry<'_>) -> serlog::Result {
        foo_use_phone_to_log(&self.phone, entry.line());
        Ok(())
    }
}
// endregion

fn main() -> serlog::Result {
    // region register
    kinds::sink("phone", |o| {
        let phone = match o.take("phone") {
            Some(Value::Number(n)) => n.as_u64().map(Number::Int),
            Some(Value::String(s)) => Some(Number::Str(s)),
            _ => None,
        };
        Ok(Box::new(Phone::new(phone)?))
    });
    // endregion

    // region refuse
    if let Err(e) = Phone::new(Some(Number::Str("call me maybe".into()))) {
        println!("refused: {e}");
    }
    // endregion

    // region config
    let _log = config::load_str(
        r#"
app: mirror
sinks:
  - kind: console
    pattern: "{level:<5} {msg}{fields}"
  - kind: phone
    name: oncall
    phone: 13800138000
    level: warn
    pattern: "{level} {msg}"
"#,
        Format::Yaml,
    )?;
    // endregion

    // region use
    let _mirror = serlog::bind!(mirror = "debian");
    serlog::info!(target: "sync", files = 42, "synced");
    serlog::warn!(target: "sync", free = "9%", "disk nearly full");
    serlog::error!(target: "sync", tries = 3, "sync failed");
    serlog::flush();
    // endregion

    // region status
    for s in serlog::status() {
        println!("{}: {}, {}", s.name, s.kind, s.health);
    }
    // endregion
    Ok(())
}
