//! Privileged helper protocol and client (ADR-0005).
//!
//! `arch-health-helper` runs as root, D-Bus activated on the system bus, and
//! answers the few read-only queries that need root. Every call is
//! authorized by polkit (action [`POLKIT_ACTION`]). This module holds the
//! protocol shared with the helper and the client side: ports that read
//! through the helper and fall back to local, unprivileged reads when the
//! helper is missing or the authorization expired.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use arch_health_core::PortResult;
use arch_health_core::ports::{Coredump, JournalReader, LogEntry, StaleLibraries, SystemInfo};
use zbus::blocking::{Connection, Proxy};

use crate::journal::SdJournal;
use crate::linux::LinuxSystem;

pub const BUS_NAME: &str = "io.github.archhealth.Helper1";
pub const OBJECT_PATH: &str = "/io/github/archhealth/Helper1";
pub const INTERFACE: &str = "io.github.archhealth.Helper1";
pub const POLKIT_ACTION: &str = "io.github.archhealth.read-privileged";
/// Upper bound of entries returned per call.
pub const MAX_ENTRIES: usize = 5000;
/// Development only: talk to a helper on the session bus (`--session`),
/// which runs unprivileged and skips polkit.
pub const DEV_BUS_ENV: &str = "ARCH_HEALTH_HELPER_BUS";

// ------------------------------------------------------------- wire format

/// (time µs since epoch, source, priority, message)
pub type WireLogEntry = (u64, String, u8, String);
/// (time µs since epoch, executable, signal or "", current boot)
pub type WireCoredump = (u64, String, String, bool);
/// process name → number of processes
pub type WireStaleLibraries = HashMap<String, u32>;

fn to_usec(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_micros() as u64
}

fn from_usec(us: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_micros(us)
}

pub fn log_entry_to_wire(e: LogEntry) -> WireLogEntry {
    (to_usec(e.time), e.source, e.priority, e.message)
}

pub fn log_entry_from_wire((t, source, priority, message): WireLogEntry) -> LogEntry {
    LogEntry {
        time: from_usec(t),
        source,
        priority,
        message,
    }
}

pub fn coredump_to_wire(c: Coredump) -> WireCoredump {
    (
        to_usec(c.time),
        c.executable,
        c.signal.unwrap_or_default(),
        c.current_boot,
    )
}

pub fn coredump_from_wire((t, executable, signal, current_boot): WireCoredump) -> Coredump {
    Coredump {
        time: from_usec(t),
        executable,
        signal: (!signal.is_empty()).then_some(signal),
        current_boot,
    }
}

pub fn since_to_wire(since: SystemTime) -> u64 {
    to_usec(since)
}

pub fn since_from_wire(us: u64) -> SystemTime {
    from_usec(us)
}

// ------------------------------------------------------------------ client

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HelperError {
    /// No helper on the bus (not installed, or activation files missing).
    NotInstalled,
    /// polkit refused, or the user dismissed the password dialog.
    Denied,
    Other(String),
}

impl std::fmt::Display for HelperError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HelperError::NotInstalled => f.write_str("the privileged helper is not installed"),
            HelperError::Denied => f.write_str("authorization was denied"),
            HelperError::Other(e) => f.write_str(e),
        }
    }
}

impl From<zbus::Error> for HelperError {
    fn from(e: zbus::Error) -> Self {
        match zbus::fdo::Error::from(e) {
            zbus::fdo::Error::ServiceUnknown(_) | zbus::fdo::Error::NameHasNoOwner(_) => {
                HelperError::NotInstalled
            }
            zbus::fdo::Error::AccessDenied(_) | zbus::fdo::Error::AuthFailed(_) => {
                HelperError::Denied
            }
            other => HelperError::Other(other.to_string()),
        }
    }
}

pub struct HelperClient {
    conn: Connection,
}

impl HelperClient {
    /// Connects to the bus the helper lives on (system, or session in
    /// development when `ARCH_HEALTH_HELPER_BUS=session`).
    pub fn connect() -> Result<Self, HelperError> {
        let dev = std::env::var(DEV_BUS_ENV).is_ok_and(|v| v == "session");
        let conn = if dev {
            Connection::session()
        } else {
            Connection::system()
        }?;
        Ok(Self { conn })
    }

    fn proxy(&self) -> Result<Proxy<'_>, HelperError> {
        Ok(Proxy::new(&self.conn, BUS_NAME, OBJECT_PATH, INTERFACE)?)
    }

    /// Asks polkit for authorization. With `interactive` the desktop shows
    /// its password dialog; this call returns when the user answers.
    pub fn authorize(&self, interactive: bool) -> Result<bool, HelperError> {
        Ok(self.proxy()?.call("Authorize", &(interactive,))?)
    }

    pub fn journal_errors(&self) -> Result<Vec<LogEntry>, HelperError> {
        let wire: Vec<WireLogEntry> = self.proxy()?.call("JournalErrors", &())?;
        Ok(wire.into_iter().map(log_entry_from_wire).collect())
    }

    pub fn coredumps_since(&self, since: SystemTime) -> Result<Vec<Coredump>, HelperError> {
        let wire: Vec<WireCoredump> = self.proxy()?.call("Coredumps", &(since_to_wire(since),))?;
        Ok(wire.into_iter().map(coredump_from_wire).collect())
    }

    pub fn stale_libraries(&self) -> Result<StaleLibraries, HelperError> {
        let wire: WireStaleLibraries = self.proxy()?.call("StaleLibraries", &())?;
        Ok(StaleLibraries {
            processes: wire.into_iter().map(|(k, v)| (k, v as usize)).collect(),
            complete: true,
        })
    }
}

// --------------------------------------------- ports reading via the helper

/// `JournalReader` through the helper, falling back to the local journal
/// (with its usual limitation) when the helper cannot answer.
pub struct HelperJournal {
    client: Arc<HelperClient>,
    local: SdJournal,
    fell_back: AtomicBool,
}

impl HelperJournal {
    pub fn new(client: Arc<HelperClient>) -> Self {
        Self {
            client,
            local: SdJournal,
            fell_back: AtomicBool::new(false),
        }
    }

    fn fallback<T>(&self, local: impl FnOnce() -> PortResult<T>) -> PortResult<T> {
        self.fell_back.store(true, Ordering::Relaxed);
        local()
    }
}

impl JournalReader for HelperJournal {
    fn errors_current_boot(&self) -> PortResult<Vec<LogEntry>> {
        match self.client.journal_errors() {
            Ok(v) => Ok(v),
            Err(_) => self.fallback(|| self.local.errors_current_boot()),
        }
    }

    fn coredumps_since(&self, since: SystemTime) -> PortResult<Vec<Coredump>> {
        match self.client.coredumps_since(since) {
            Ok(v) => Ok(v),
            Err(_) => self.fallback(|| self.local.coredumps_since(since)),
        }
    }

    fn system_journal_readable(&self) -> bool {
        !self.fell_back.load(Ordering::Relaxed) || self.local.system_journal_readable()
    }
}

/// `SystemInfo` with process inspection through the helper; everything
/// else is read locally.
pub struct HelperSystem {
    client: Arc<HelperClient>,
    local: LinuxSystem,
}

impl HelperSystem {
    pub fn new(client: Arc<HelperClient>) -> Self {
        Self {
            client,
            local: LinuxSystem,
        }
    }
}

impl SystemInfo for HelperSystem {
    fn is_root(&self) -> bool {
        self.local.is_root()
    }

    fn running_kernel(&self) -> String {
        self.local.running_kernel()
    }

    fn installed_kernels(&self) -> Vec<String> {
        self.local.installed_kernels()
    }

    fn stale_libraries(&self) -> PortResult<StaleLibraries> {
        self.client
            .stale_libraries()
            .or_else(|_| self.local.stale_libraries())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_roundtrip() {
        let e = LogEntry {
            time: from_usec(1_700_000_000_123_456),
            source: "kernel".into(),
            priority: 2,
            message: "boom".into(),
        };
        assert_eq!(log_entry_from_wire(log_entry_to_wire(e.clone())), e);

        let c = Coredump {
            time: from_usec(42),
            executable: "/usr/bin/x".into(),
            signal: None,
            current_boot: true,
        };
        assert_eq!(coredump_from_wire(coredump_to_wire(c.clone())), c);
    }

    #[test]
    fn dbus_errors_map_to_helper_errors() {
        let unknown = zbus::Error::from(zbus::fdo::Error::ServiceUnknown("x".into()));
        assert_eq!(HelperError::from(unknown), HelperError::NotInstalled);
        let denied = zbus::Error::from(zbus::fdo::Error::AccessDenied("x".into()));
        assert_eq!(HelperError::from(denied), HelperError::Denied);
    }
}
