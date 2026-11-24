//! System adapters implementing the ports of `arch-health-core`.
//! Libraries and system APIs only, never process execution (ADR-0001).

mod dbus;
mod http;
mod journal;
mod linux;
mod packages;
mod pacman_conf;

use std::sync::Arc;

use arch_health_core::Ports;

pub use packages::AlpmPackageDb;

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Refresh a private copy of the repository databases before checking
    /// for updates (needs network).
    pub sync_databases: bool,
}

/// Ports backed by the real system.
pub fn system_ports(options: Options) -> Ports {
    Ports {
        packages: Arc::new(AlpmPackageDb::new(options.sync_databases)),
        aur: Arc::new(http::AurRpc),
        security: Arc::new(http::ArchSecurityTracker),
        news: Arc::new(http::ArchNewsFeed),
        services: Arc::new(dbus::SystemdManager),
        journal: Arc::new(journal::SdJournal),
        storage: Arc::new(linux::LinuxStorage),
        system: Arc::new(linux::LinuxSystem),
    }
}
