//! Shared HTTP client: ureq + native-tls, i.e. the system OpenSSL and the
//! system CA store (`ca-certificates`).

use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

pub fn agent() -> ureq::Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(concat!("arch-health/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}
