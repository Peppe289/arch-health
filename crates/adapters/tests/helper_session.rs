//! End-to-end test of the helper client against a development helper.
//!
//! Needs a helper on the session bus, so it is ignored by default:
//!
//!     cargo run -p arch-health-helper -- --session &
//!     ARCH_HEALTH_HELPER_BUS=session cargo test -p arch-health-adapters -- --ignored

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use arch_health_adapters::helper::HelperClient;
use arch_health_adapters::{Options, system_ports_with_helper};

#[test]
#[ignore = "needs `arch-health-helper --session` running and ARCH_HEALTH_HELPER_BUS=session"]
fn client_reads_through_session_helper() {
    let client = HelperClient::connect().expect("session bus");
    assert_eq!(client.authorize(false), Ok(true));
    client.journal_errors().expect("journal errors");
    client
        .coredumps_since(SystemTime::now() - Duration::from_secs(86400))
        .expect("coredumps");
    assert!(client.stale_libraries().expect("stale libraries").complete);

    let ports = system_ports_with_helper(
        Options {
            sync_databases: false,
        },
        Arc::new(client),
    );
    ports.journal.errors_current_boot().expect("via port");
    assert!(ports.journal.system_journal_readable());
}

/// Real end-to-end check of an installed helper: system bus, polkit dialog,
/// systemd sandbox. Manual only (asks for the administrator password):
///
///     cargo test -p arch-health-adapters --test helper_session -- --ignored installed --nocapture
#[test]
#[ignore = "needs the installed helper and an administrator password"]
fn installed_helper_with_polkit() {
    assert!(
        std::env::var("ARCH_HEALTH_HELPER_BUS").is_err(),
        "unset ARCH_HEALTH_HELPER_BUS"
    );
    let client = HelperClient::connect().expect("system bus");
    assert_eq!(
        client.authorize(true),
        Ok(true),
        "authorization not granted"
    );

    let errors = client
        .journal_errors()
        .expect("journal errors through the helper");
    let dumps = client
        .coredumps_since(SystemTime::now() - Duration::from_secs(7 * 86400))
        .expect("coredumps through the helper");
    let stale = client
        .stale_libraries()
        .expect("stale libraries through the helper");
    println!(
        "helper: {} journal errors, {} coredumps, {} programs with replaced libraries, \
         all processes inspected: {}",
        errors.len(),
        dumps.len(),
        stale.processes.len(),
        stale.complete
    );
    assert!(
        stale.complete,
        "the sandbox hides some processes from the helper"
    );
}
