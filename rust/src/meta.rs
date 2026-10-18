//! Who is logging: application, environment and instance, fixed at start-up, and the host.

use std::sync::{Arc, LazyLock, PoisonError, RwLock};

/// Identity attached to records that leave the process.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    /// Application name; defaults to the executable's file stem.
    pub app: String,
    /// Deployment environment, such as `prod`.
    pub env: String,
    /// Host or replica; defaults to the host name.
    pub instance: String,
}

static META: LazyLock<RwLock<Arc<Meta>>> = LazyLock::new(|| RwLock::new(Arc::new(Meta::detect())));

static HOST: LazyLock<String> = LazyLock::new(|| {
    ["HOSTNAME", "COMPUTERNAME"]
        .into_iter()
        .find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty()))
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty())
        })
        .unwrap_or_default()
});

/// The identity given at start-up, or the defaults.
pub fn meta() -> Arc<Meta> {
    META.read().unwrap_or_else(PoisonError::into_inner).clone()
}

/// The host name, as far as the environment tells.
pub fn host() -> &'static str {
    &HOST
}

impl Meta {
    /// Executable stem and host name.
    #[must_use]
    pub fn detect() -> Self {
        let app = std::env::current_exe()
            .ok()
            .and_then(|p| Some(p.file_stem()?.to_string_lossy().into_owned()))
            .unwrap_or_default();
        Self {
            app,
            env: String::new(),
            instance: host().to_owned(),
        }
    }
}

pub(crate) fn set(meta: Meta) {
    *META.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(meta);
}
