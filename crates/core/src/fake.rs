//! In-memory implementation of every port, for unit tests.

use std::cmp::Ordering;
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{DateTime, FixedOffset};

use crate::error::PortResult;
use crate::ports::*;

pub struct Fake {
    pub snapshot: PackageSnapshot,
    pub last_upgrade: Option<DateTime<FixedOffset>>,
    pub pacman_log_readable: bool,
    pub aur: Vec<AurPackage>,
    pub advisories: Vec<Advisory>,
    pub news: Vec<NewsItem>,
    pub manager: ManagerState,
    pub journal_entries: Vec<LogEntry>,
    pub coredumps: Vec<Coredump>,
    pub journal_readable: bool,
    pub filesystems: Vec<Filesystem>,
    pub btrfs: Vec<BtrfsDevice>,
    pub drives: Vec<Drive>,
    pub root: bool,
    pub running_kernel: String,
    pub installed_kernels: Vec<String>,
    pub stale: StaleLibraries,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            snapshot: PackageSnapshot {
                packages: vec![],
                freshness: DbFreshness::Synced,
                pacnew_files: vec![],
            },
            last_upgrade: None,
            pacman_log_readable: true,
            aur: vec![],
            advisories: vec![],
            news: vec![],
            manager: ManagerState {
                state: "running".into(),
                failed: vec![],
            },
            journal_entries: vec![],
            coredumps: vec![],
            journal_readable: true,
            filesystems: vec![],
            btrfs: vec![],
            drives: vec![],
            root: false,
            running_kernel: "6.0-arch1".into(),
            installed_kernels: vec!["6.0-arch1".into()],
            stale: StaleLibraries {
                processes: Default::default(),
                complete: true,
            },
        }
    }
}

impl Fake {
    pub fn ports(self) -> Ports {
        let f = Arc::new(self);
        Ports {
            packages: f.clone(),
            aur: f.clone(),
            security: f.clone(),
            news: f.clone(),
            services: f.clone(),
            journal: f.clone(),
            storage: f.clone(),
            system: f,
        }
    }
}

/// An explicitly installed repo package.
pub fn pkg(name: &str, version: &str) -> InstalledPackage {
    InstalledPackage {
        name: name.into(),
        version: version.into(),
        explicit: true,
        required: false,
        foreign: false,
        repo_update: None,
    }
}

/// Good enough for tests: compares numeric segments, then text.
fn naive_vercmp(a: &str, b: &str) -> Ordering {
    let seg = |s: &str| {
        s.split(['.', '-', ':'])
            .map(str::to_string)
            .collect::<Vec<_>>()
    };
    for (x, y) in seg(a).iter().zip(seg(b).iter()) {
        let o = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => x.cmp(y),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    seg(a).len().cmp(&seg(b).len())
}

impl PackageDb for Fake {
    fn snapshot(&self) -> PortResult<Arc<PackageSnapshot>> {
        Ok(Arc::new(self.snapshot.clone()))
    }
    fn last_full_upgrade(&self) -> PortResult<Option<DateTime<FixedOffset>>> {
        if !self.pacman_log_readable {
            return Err(crate::PortError::unavailable("pacman.log not readable"));
        }
        Ok(self.last_upgrade)
    }
    fn compare_versions(&self, a: &str, b: &str) -> Ordering {
        naive_vercmp(a, b)
    }
}

impl AurClient for Fake {
    fn info(&self, names: &[String]) -> PortResult<Vec<AurPackage>> {
        Ok(self
            .aur
            .iter()
            .filter(|p| names.contains(&p.name))
            .cloned()
            .collect())
    }
}

impl SecurityTracker for Fake {
    fn advisories(&self) -> PortResult<Vec<Advisory>> {
        Ok(self.advisories.clone())
    }
}

impl NewsFeed for Fake {
    fn items(&self) -> PortResult<Vec<NewsItem>> {
        Ok(self.news.clone())
    }
}

impl ServiceManager for Fake {
    fn state(&self, _scope: ManagerScope) -> PortResult<ManagerState> {
        Ok(self.manager.clone())
    }
}

impl JournalReader for Fake {
    fn errors_current_boot(&self) -> PortResult<Vec<LogEntry>> {
        Ok(self.journal_entries.clone())
    }
    fn coredumps_since(&self, since: SystemTime) -> PortResult<Vec<Coredump>> {
        Ok(self
            .coredumps
            .iter()
            .filter(|c| c.time >= since)
            .cloned()
            .collect())
    }
    fn system_journal_readable(&self) -> bool {
        self.journal_readable
    }
}

impl Storage for Fake {
    fn filesystems(&self) -> PortResult<Vec<Filesystem>> {
        Ok(self.filesystems.clone())
    }
    fn btrfs_devices(&self) -> PortResult<Vec<BtrfsDevice>> {
        Ok(self.btrfs.clone())
    }
    fn drives(&self) -> PortResult<Vec<Drive>> {
        Ok(self.drives.clone())
    }
}

impl SystemInfo for Fake {
    fn is_root(&self) -> bool {
        self.root
    }
    fn running_kernel(&self) -> String {
        self.running_kernel.clone()
    }
    fn installed_kernels(&self) -> Vec<String> {
        self.installed_kernels.clone()
    }
    fn stale_libraries(&self) -> PortResult<StaleLibraries> {
        Ok(self.stale.clone())
    }
}
