//! Ports: what the checks need from the system, as traits returning plain
//! data. Implemented by `arch-health-adapters`; faked in tests.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, FixedOffset};

use crate::error::PortResult;

// ---------------------------------------------------------------- packages

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledPackage {
    pub name: String,
    pub version: String,
    /// Installed explicitly (not as a dependency).
    pub explicit: bool,
    /// Required or optionally required by another installed package.
    pub required: bool,
    /// Not provided by any configured repository (AUR or local build).
    pub foreign: bool,
    /// Newer version available in the repositories (IgnorePkg excluded).
    pub repo_update: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbFreshness {
    /// Repository databases were just refreshed (private copy).
    Synced,
    /// The system databases were used as they are (no sync requested).
    SystemDatabases,
    /// Refresh failed; data may be stale.
    SyncFailed(String),
}

#[derive(Debug, Clone)]
pub struct PackageSnapshot {
    pub packages: Vec<InstalledPackage>,
    pub freshness: DbFreshness,
    /// `.pacnew` files existing next to packages' backup files.
    pub pacnew_files: Vec<PathBuf>,
}

pub trait PackageDb: Send + Sync {
    /// Computed once per run and shared.
    fn snapshot(&self) -> PortResult<Arc<PackageSnapshot>>;
    /// Time of the last `pacman -Syu`, from the pacman log.
    fn last_full_upgrade(&self) -> PortResult<Option<DateTime<FixedOffset>>>;
    /// pacman's version ordering (`vercmp`).
    fn compare_versions(&self, a: &str, b: &str) -> Ordering;
}

// --------------------------------------------------------------- remote data

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AurPackage {
    pub name: String,
    pub version: String,
    pub out_of_date: bool,
    pub orphaned: bool,
}

pub trait AurClient: Send + Sync {
    /// Packages found on the AUR among `names` (missing ones are omitted).
    fn info(&self, names: &[String]) -> PortResult<Vec<AurPackage>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Advisory {
    /// AVG identifier.
    pub name: String,
    pub packages: Vec<String>,
    /// "Fixed", "Vulnerable", "Testing", "Not affected", "Unknown".
    pub status: String,
    /// "Critical", "High", "Medium", "Low", "Unknown".
    pub severity: String,
    pub fixed: Option<String>,
    pub issues: Vec<String>,
}

pub trait SecurityTracker: Send + Sync {
    fn advisories(&self) -> PortResult<Vec<Advisory>>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewsItem {
    pub title: String,
    pub link: String,
    pub published: DateTime<FixedOffset>,
}

pub trait NewsFeed: Send + Sync {
    fn items(&self) -> PortResult<Vec<NewsItem>>;
}

// ------------------------------------------------------------------ services

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManagerScope {
    System,
    User,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FailedUnit {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerState {
    /// systemd `SystemState`: running, degraded, starting, ...
    pub state: String,
    pub failed: Vec<FailedUnit>,
}

pub trait ServiceManager: Send + Sync {
    fn state(&self, scope: ManagerScope) -> PortResult<ManagerState>;
}

// ------------------------------------------------------------------- journal

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    /// Unit, syslog identifier or command that logged the message.
    pub source: String,
    /// syslog priority, 0 (emerg) .. 7 (debug).
    pub priority: u8,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coredump {
    pub executable: String,
    pub signal: Option<String>,
    pub time: SystemTime,
    pub current_boot: bool,
}

pub trait JournalReader: Send + Sync {
    /// Entries with priority <= err logged since the current boot.
    fn errors_current_boot(&self) -> PortResult<Vec<LogEntry>>;
    fn coredumps_since(&self, since: SystemTime) -> PortResult<Vec<Coredump>>;
    /// Whether the system journal is readable (otherwise only the user's).
    fn system_journal_readable(&self) -> bool;
}

// ------------------------------------------------------------------- storage

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Filesystem {
    pub mount_point: String,
    pub device: String,
    pub fstype: String,
    pub read_only: bool,
    pub bytes_used: u64,
    pub bytes_available: u64,
    /// `None` when the filesystem has no fixed inode table (btrfs, vfat).
    pub inodes_used: Option<u64>,
    pub inodes_available: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BtrfsDevice {
    pub fs_uuid: String,
    pub device_id: String,
    /// Counter name → value, e.g. "read_errs" → 0.
    pub counters: BTreeMap<String, u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DriveKind {
    Ata,
    Nvme,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drive {
    pub model: String,
    pub kind: DriveKind,
    /// ATA: SMART overall assessment says the drive is failing.
    pub smart_failing: bool,
    /// ATA: reallocated/pending bad sectors.
    pub bad_sectors: u64,
    /// NVMe: critical warning flags (e.g. "spare", "reliability", "readonly").
    pub critical_warnings: Vec<String>,
}

pub trait Storage: Send + Sync {
    fn filesystems(&self) -> PortResult<Vec<Filesystem>>;
    fn btrfs_devices(&self) -> PortResult<Vec<BtrfsDevice>>;
    /// Drives with SMART/health data available.
    fn drives(&self) -> PortResult<Vec<Drive>>;
}

// -------------------------------------------------------------------- system

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StaleLibraries {
    /// Process name → number of processes using replaced libraries.
    pub processes: BTreeMap<String, usize>,
    /// False when some processes could not be inspected (not root).
    pub complete: bool,
}

pub trait SystemInfo: Send + Sync {
    fn is_root(&self) -> bool;
    /// Release of the running kernel (`uname -r`).
    fn running_kernel(&self) -> String;
    /// Kernel releases with modules installed in /usr/lib/modules.
    fn installed_kernels(&self) -> Vec<String>;
    fn stale_libraries(&self) -> PortResult<StaleLibraries>;
}

/// All ports a run needs.
#[derive(Clone)]
pub struct Ports {
    pub packages: Arc<dyn PackageDb>,
    pub aur: Arc<dyn AurClient>,
    pub security: Arc<dyn SecurityTracker>,
    pub news: Arc<dyn NewsFeed>,
    pub services: Arc<dyn ServiceManager>,
    pub journal: Arc<dyn JournalReader>,
    pub storage: Arc<dyn Storage>,
    pub system: Arc<dyn SystemInfo>,
}
