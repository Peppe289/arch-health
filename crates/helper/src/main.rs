//! arch-health-helper: privileged, read-only D-Bus service (ADR-0005).
//!
//! Started by D-Bus activation on the system bus, runs as root, and exits
//! after a short idle time: no long-running root process. Each method asks
//! polkit whether the *caller* may use action `POLKIT_ACTION`; nothing is
//! ever written or changed on the system.
//!
//! `--session` (development only): serve on the session bus as the current
//! user, without polkit. It grants nothing a user cannot already read.

use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use arch_health_adapters::helper::{
    self, BUS_NAME, MAX_ENTRIES, OBJECT_PATH, POLKIT_ACTION, WireCoredump, WireLogEntry,
    WireStaleLibraries,
};
use arch_health_adapters::{LinuxSystem, SdJournal};
use arch_health_core::ports::{JournalReader, SystemInfo};
use zbus::message::Header;
use zbus::zvariant::Value;
use zbus::{Connection, fdo, interface};

/// Exit after this long without calls.
const IDLE_EXIT: Duration = Duration::from_secs(60);

/// Tracks calls in progress and the last activity, for the idle exit. A
/// call waiting on the password dialog keeps the helper alive.
#[derive(Default)]
struct Activity {
    in_flight: AtomicUsize,
    last: Mutex<Option<Instant>>,
}

impl Activity {
    fn touch(&self) {
        *self.last.lock().expect("activity lock") = Some(Instant::now());
    }

    fn idle_for(&self, started: Instant) -> Duration {
        if self.in_flight.load(Ordering::SeqCst) > 0 {
            return Duration::ZERO;
        }
        self.last
            .lock()
            .expect("activity lock")
            .unwrap_or(started)
            .elapsed()
    }
}

struct CallGuard<'a>(&'a Activity);

impl<'a> CallGuard<'a> {
    fn new(a: &'a Activity) -> Self {
        a.in_flight.fetch_add(1, Ordering::SeqCst);
        a.touch();
        Self(a)
    }
}

impl Drop for CallGuard<'_> {
    fn drop(&mut self) {
        self.0.touch();
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

struct Helper {
    activity: Arc<Activity>,
    /// Development mode: no polkit, session bus.
    dev: bool,
}

impl Helper {
    /// polkit `CheckAuthorization` for the sender of the current message.
    async fn authorized(
        &self,
        conn: &Connection,
        hdr: &Header<'_>,
        interactive: bool,
    ) -> fdo::Result<bool> {
        if self.dev {
            return Ok(true);
        }
        let sender = hdr
            .sender()
            .ok_or_else(|| fdo::Error::AccessDenied("no sender".into()))?
            .to_string();
        let polkit = zbus::Proxy::new(
            conn,
            "org.freedesktop.PolicyKit1",
            "/org/freedesktop/PolicyKit1/Authority",
            "org.freedesktop.PolicyKit1.Authority",
        )
        .await?;
        let mut subject_details: HashMap<&str, Value> = HashMap::new();
        subject_details.insert("name", Value::from(sender.as_str()));
        let subject = ("system-bus-name", subject_details);
        let details: HashMap<&str, &str> = HashMap::new();
        // 0x1: AllowUserInteraction (the agent may show a password dialog).
        let flags: u32 = if interactive { 1 } else { 0 };
        let (authorized, _challenge, _details): (bool, bool, HashMap<String, String>) = polkit
            .call(
                "CheckAuthorization",
                &(subject, POLKIT_ACTION, details, flags, ""),
            )
            .await?;
        Ok(authorized)
    }

    async fn require(&self, conn: &Connection, hdr: &Header<'_>) -> fdo::Result<()> {
        if self.authorized(conn, hdr, false).await? {
            Ok(())
        } else {
            Err(fdo::Error::AccessDenied("not authorized".into()))
        }
    }
}

fn failed(e: impl std::fmt::Display) -> fdo::Error {
    fdo::Error::Failed(e.to_string())
}

#[interface(name = "io.github.archhealth.Helper1")]
impl Helper {
    /// Asks for authorization; with `interactive` polkit may show the
    /// password dialog. Later calls succeed while polkit keeps it.
    async fn authorize(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        interactive: bool,
    ) -> fdo::Result<bool> {
        let _g = CallGuard::new(&self.activity);
        self.authorized(conn, &hdr, interactive).await
    }

    /// Errors (priority <= err) of the current boot, from every journal.
    async fn journal_errors(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<Vec<WireLogEntry>> {
        let _g = CallGuard::new(&self.activity);
        self.require(conn, &hdr).await?;
        let mut entries = SdJournal.errors_current_boot().map_err(failed)?;
        // Keep the newest ones.
        let skip = entries.len().saturating_sub(MAX_ENTRIES);
        entries.drain(..skip);
        Ok(entries.into_iter().map(helper::log_entry_to_wire).collect())
    }

    /// Coredumps since `since_usec` (µs since the epoch).
    async fn coredumps(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
        since_usec: u64,
    ) -> fdo::Result<Vec<WireCoredump>> {
        let _g = CallGuard::new(&self.activity);
        self.require(conn, &hdr).await?;
        let mut dumps = SdJournal
            .coredumps_since(helper::since_from_wire(since_usec))
            .map_err(failed)?;
        let skip = dumps.len().saturating_sub(MAX_ENTRIES);
        dumps.drain(..skip);
        Ok(dumps.into_iter().map(helper::coredump_to_wire).collect())
    }

    /// Processes of every user still using replaced shared libraries.
    async fn stale_libraries(
        &self,
        #[zbus(header)] hdr: Header<'_>,
        #[zbus(connection)] conn: &Connection,
    ) -> fdo::Result<WireStaleLibraries> {
        let _g = CallGuard::new(&self.activity);
        self.require(conn, &hdr).await?;
        let stale = LinuxSystem.stale_libraries().map_err(failed)?;
        Ok(helper::stale_to_wire(stale))
    }
}

fn main() -> ExitCode {
    let dev = std::env::args().skip(1).any(|a| a == "--session");
    if !dev && !rustix::process::geteuid().is_root() {
        eprintln!(
            "arch-health-helper: must run as root; it is started by D-Bus activation \
             (use --session only for development)"
        );
        return ExitCode::FAILURE;
    }

    let activity = Arc::new(Activity::default());
    let helper = Helper {
        activity: activity.clone(),
        dev,
    };
    let builder = if dev {
        zbus::blocking::connection::Builder::session()
    } else {
        zbus::blocking::connection::Builder::system()
    };
    let conn = builder
        .and_then(|b| b.name(BUS_NAME))
        .and_then(|b| b.serve_at(OBJECT_PATH, helper))
        .and_then(|b| b.build());
    let _conn = match conn {
        Ok(c) => c,
        Err(e) => {
            eprintln!("arch-health-helper: cannot register {BUS_NAME}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let started = Instant::now();
    while activity.idle_for(started) < IDLE_EXIT {
        thread::sleep(Duration::from_secs(5));
    }
    ExitCode::SUCCESS
}
