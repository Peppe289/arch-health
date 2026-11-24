//! Domain model: what a check produces.

use std::fmt;
use std::time::SystemTime;

/// Outcome of a check or of a single item. Ordered by severity, so
/// `max()` gives the worst one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Status {
    Ok,
    /// Not run on purpose (e.g. network check with `--offline`).
    Skipped,
    /// Worth knowing, nothing to fix.
    Info,
    Warning,
    /// The check could not run (service unavailable, permission, ...).
    Unknown,
    Critical,
}

impl Status {
    /// Process exit code contribution: 0 ok, 1 warning, 2 critical.
    pub fn exit_code(self) -> u8 {
        match self {
            Status::Ok | Status::Skipped | Status::Info => 0,
            Status::Warning | Status::Unknown => 1,
            Status::Critical => 2,
        }
    }
}

/// Group shown together in frontends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Category {
    Packages,
    Security,
    Reboot,
    Services,
    Storage,
    Logs,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Packages,
        Category::Security,
        Category::Reboot,
        Category::Services,
        Category::Storage,
        Category::Logs,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::Packages => "Packages",
            Category::Security => "Security & news",
            Category::Reboot => "Reboot",
            Category::Services => "Services",
            Category::Storage => "Storage",
            Category::Logs => "Logs",
        }
    }
}

macro_rules! check_ids {
    ($($variant:ident => ($key:literal, $title:literal, $cat:ident)),* $(,)?) => {
        /// Stable identifier of a check: used for configuration, frontends and
        /// the CLI. The key must never change once released.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum CheckId { $($variant),* }

        impl CheckId {
            pub const ALL: &'static [CheckId] = &[$(CheckId::$variant),*];

            pub fn key(self) -> &'static str {
                match self { $(CheckId::$variant => $key),* }
            }

            pub fn title(self) -> &'static str {
                match self { $(CheckId::$variant => $title),* }
            }

            pub fn category(self) -> Category {
                match self { $(CheckId::$variant => Category::$cat),* }
            }

            pub fn from_key(key: &str) -> Option<CheckId> {
                Self::ALL.iter().copied().find(|c| c.key() == key)
            }
        }
    };
}

check_ids! {
    RepoUpdates     => ("repo-updates",     "Repository updates", Packages),
    AurUpdates      => ("aur-updates",      "AUR",                Packages),
    Orphans         => ("orphans",          "Orphans",            Packages),
    ForeignPackages => ("foreign-packages", "Foreign packages",   Packages),
    Pacnew          => ("pacnew",           ".pacnew files",      Packages),
    Vulnerabilities => ("vulnerabilities",  "Vulnerabilities",    Security),
    ArchNews        => ("arch-news",        "Arch news",          Security),
    Kernel          => ("kernel",           "Kernel",             Reboot),
    StaleLibraries  => ("stale-libraries",  "Replaced libraries", Reboot),
    SystemServices  => ("system-services",  "System services",    Services),
    UserServices    => ("user-services",    "User services",      Services),
    DiskSpace       => ("disk-space",       "Disk space",         Storage),
    BtrfsErrors     => ("btrfs-errors",     "Btrfs errors",       Storage),
    DriveHealth     => ("drive-health",     "Drive health",       Storage),
    JournalErrors   => ("journal-errors",   "Journal errors",     Logs),
    Coredumps       => ("coredumps",        "Coredumps",          Logs),
}

impl fmt::Display for CheckId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}

/// One finding inside a report, e.g. a package to update or a failed unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub status: Status,
    /// What the item is about (package, unit, mount point, ...). Stable
    /// enough to be used by ignore rules.
    pub subject: String,
    pub detail: String,
}

impl Item {
    pub fn new(status: Status, subject: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            status,
            subject: subject.into(),
            detail: detail.into(),
        }
    }
}

/// Something that happened at a known time (a logged error, a crash, a unit
/// failure). Frontends show these as a timeline and a table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimedEvent {
    pub time: SystemTime,
    pub status: Status,
    /// Unit, program or executable the event belongs to.
    pub source: String,
    pub message: String,
}

/// A level between 0 and 1, e.g. used disk space. Frontends show it as a bar.
#[derive(Debug, Clone, PartialEq)]
pub struct Gauge {
    pub label: String,
    /// 0.0 ..= 1.0
    pub fraction: f64,
    /// Short text shown next to the bar, e.g. "26% used, 1.3 TiB free".
    pub text: String,
    pub status: Status,
}

/// Upper bound of events kept per report, to bound memory and UI size.
pub const MAX_EVENTS: usize = 1000;

#[derive(Debug, Clone, PartialEq)]
pub struct CheckReport {
    pub id: CheckId,
    pub status: Status,
    pub summary: String,
    pub items: Vec<Item>,
    /// Set when the result is incomplete, e.g. because root is needed.
    pub limitation: Option<String>,
    /// Newest first, at most [`MAX_EVENTS`].
    pub events: Vec<TimedEvent>,
    pub gauges: Vec<Gauge>,
}

impl CheckReport {
    pub fn new(id: CheckId, status: Status, summary: impl Into<String>) -> Self {
        Self {
            id,
            status,
            summary: summary.into(),
            items: Vec::new(),
            limitation: None,
            events: Vec::new(),
            gauges: Vec::new(),
        }
    }

    pub fn with_items(mut self, items: Vec<Item>) -> Self {
        self.items = items;
        self
    }

    /// Keeps the newest [`MAX_EVENTS`] events, newest first.
    pub fn with_events(mut self, mut events: Vec<TimedEvent>) -> Self {
        events.sort_by_key(|e| std::cmp::Reverse(e.time));
        events.truncate(MAX_EVENTS);
        self.events = events;
        self
    }

    pub fn with_gauges(mut self, gauges: Vec<Gauge>) -> Self {
        self.gauges = gauges;
        self
    }

    pub fn with_limitation(mut self, limitation: Option<String>) -> Self {
        self.limitation = limitation;
        self
    }

    /// Status is the worst among the items, or `empty` when there are none.
    pub fn from_items(id: CheckId, items: Vec<Item>, summary: impl Into<String>) -> Self {
        let status = items.iter().map(|i| i.status).max().unwrap_or(Status::Ok);
        Self::new(id, status, summary).with_items(items)
    }
}

/// Result of a whole run.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub reports: Vec<CheckReport>,
}

impl Report {
    pub fn overall(&self) -> Status {
        self.reports
            .iter()
            .map(|r| r.status)
            .max()
            .unwrap_or(Status::Ok)
    }

    pub fn by_category(&self, category: Category) -> impl Iterator<Item = &CheckReport> {
        self.reports
            .iter()
            .filter(move |r| r.id.category() == category)
    }
}

/// Plural helper for English summaries: `plural(2, "crash")` → "2 crashes".
pub fn plural(n: usize, word: &str) -> String {
    if n == 1 {
        return format!("1 {word}");
    }
    let es = ["s", "x", "sh", "ch"].iter().any(|e| word.ends_with(e));
    format!("{n} {word}{}", if es { "es" } else { "s" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_keys_are_unique_and_roundtrip() {
        let mut keys: Vec<_> = CheckId::ALL.iter().map(|c| c.key()).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), CheckId::ALL.len());
        for id in CheckId::ALL {
            assert_eq!(CheckId::from_key(id.key()), Some(*id));
        }
    }

    #[test]
    fn plurals() {
        assert_eq!(plural(1, "crash"), "1 crash");
        assert_eq!(plural(2, "crash"), "2 crashes");
        assert_eq!(plural(0, "package"), "0 packages");
    }

    #[test]
    fn events_are_sorted_newest_first_and_capped() {
        let t0 = SystemTime::UNIX_EPOCH;
        let ev = |s: u64| TimedEvent {
            time: t0 + std::time::Duration::from_secs(s),
            status: Status::Info,
            source: "x".into(),
            message: String::new(),
        };
        let events: Vec<_> = (0..(MAX_EVENTS as u64 + 5)).map(ev).collect();
        let r = CheckReport::new(CheckId::Kernel, Status::Ok, "").with_events(events);
        assert_eq!(r.events.len(), MAX_EVENTS);
        assert!(r.events[0].time > r.events[1].time);
        assert_eq!(
            r.events[0].time,
            t0 + std::time::Duration::from_secs(MAX_EVENTS as u64 + 4)
        );
    }

    #[test]
    fn status_ordering() {
        assert!(Status::Critical > Status::Unknown);
        assert!(Status::Warning > Status::Info);
        assert_eq!(Status::Unknown.exit_code(), 1);
    }
}
