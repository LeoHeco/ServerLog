//! The rustls crypto provider behind the Loki and mail sinks: whichever the program installed, or the one picked
//! with the `ring` or `aws-lc-rs` feature, so serlog never adds a second provider to a program that has one.

use rustls::crypto::CryptoProvider;

use crate::{Error, Result};

/// Installs the provider serlog was built with as the process default, unless one is installed already.
///
/// # Errors
/// When no provider is installed and neither feature was enabled.
pub(crate) fn ensure() -> Result {
    if CryptoProvider::get_default().is_some() {
        return Ok(());
    }
    #[cfg(feature = "aws-lc-rs")]
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    #[cfg(all(feature = "ring", not(feature = "aws-lc-rs")))]
    let _ = rustls::crypto::ring::default_provider().install_default();
    if CryptoProvider::get_default().is_some() {
        Ok(())
    } else {
        Err(Error::Config(
            "no rustls crypto provider: enable serlog's `ring` or `aws-lc-rs` feature, or install one".into(),
        ))
    }
}
