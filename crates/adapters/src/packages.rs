//! `PackageDb` over libalpm.
//!
//! Like `checkupdates`, repository databases are refreshed into a private
//! copy (`$XDG_CACHE_HOME/arch-health/db`), so `/var/lib/pacman` is never
//! touched and no root is needed.

use std::cmp::Ordering;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use alpm::{Alpm, PackageReason};
use arch_health_core::ports::{DbFreshness, InstalledPackage, PackageDb, PackageSnapshot};
use arch_health_core::{PortError, PortResult};
use chrono::{DateTime, FixedOffset};

use crate::pacman_conf::PacmanConf;

pub struct AlpmPackageDb {
    sync: bool,
    snapshot: OnceLock<PortResult<Arc<PackageSnapshot>>>,
}

impl AlpmPackageDb {
    /// `sync`: refresh a private copy of the repository databases first.
    pub fn new(sync: bool) -> Self {
        Self {
            sync,
            snapshot: OnceLock::new(),
        }
    }
}

fn alpm_err(context: &str) -> impl Fn(alpm::Error) -> PortError + '_ {
    move |e| PortError::unavailable(format!("{context}: {e}"))
}

fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("arch-health").join("db")
}

/// Private db directory: `local` links to the system local db; sync dbs are
/// seeded from the system copy keeping their mtime, so libalpm downloads
/// only what changed on the mirror.
fn prepare_private_dbpath(conf: &PacmanConf) -> std::io::Result<PathBuf> {
    let dir = cache_dir();
    let sync = dir.join("sync");
    fs::create_dir_all(&sync)?;
    let local = dir.join("local");
    if fs::symlink_metadata(&local).is_err() {
        std::os::unix::fs::symlink(conf.db_path.join("local"), &local)?;
    }
    for repo in &conf.repos {
        let file = format!("{}.db", repo.name);
        let (src, dst) = (conf.db_path.join("sync").join(&file), sync.join(&file));
        if dst.exists() || !src.exists() {
            continue;
        }
        fs::copy(&src, &dst)?;
        if let Ok(mtime) = fs::metadata(&src).and_then(|m| m.modified()) {
            File::options()
                .write(true)
                .open(&dst)?
                .set_modified(mtime)?;
        }
    }
    Ok(dir)
}

fn open_handle(conf: &PacmanConf, db_path: &Path) -> PortResult<Alpm> {
    let root = conf.root_dir.to_string_lossy().into_owned();
    let db = db_path.to_string_lossy().into_owned();
    let mut handle = Alpm::new(root, db).map_err(alpm_err("initializing libalpm"))?;
    handle
        .set_gpgdir(conf.gpg_dir.to_string_lossy().into_owned())
        .map_err(alpm_err("setting GPGDir"))?;
    let archs: Vec<&str> = conf.architectures.iter().map(String::as_str).collect();
    handle
        .set_architectures(archs.iter())
        .map_err(alpm_err("setting Architecture"))?;
    handle.set_parallel_downloads(conf.parallel_downloads.max(1));
    for repo in &conf.repos {
        let db = handle
            .register_syncdb_mut(repo.name.as_str(), repo.sig_level)
            .map_err(alpm_err("registering repository"))?;
        for server in &repo.servers {
            db.add_server(server.as_str())
                .map_err(alpm_err("adding server"))?;
        }
    }
    Ok(handle)
}

fn build_snapshot(sync: bool) -> PortResult<Arc<PackageSnapshot>> {
    let conf = PacmanConf::load_default()?;
    let private_dir = if sync {
        Some(prepare_private_dbpath(&conf))
    } else {
        None
    };
    let (handle, freshness) = match private_dir {
        Some(Ok(dir)) => {
            let mut handle = open_handle(&conf, &dir)?;
            let freshness = match handle.syncdbs_mut().update(false) {
                Ok(_) => DbFreshness::Synced,
                Err(e) => DbFreshness::SyncFailed(e.to_string()),
            };
            (handle, freshness)
        }
        // Without a private copy the system databases still answer every
        // package question; only freshness is lost.
        Some(Err(e)) => (
            open_handle(&conf, &conf.db_path)?,
            DbFreshness::SyncFailed(format!("preparing the private database copy: {e}")),
        ),
        None => (
            open_handle(&conf, &conf.db_path)?,
            DbFreshness::SystemDatabases,
        ),
    };

    let syncdbs = handle.syncdbs();
    let mut packages = Vec::new();
    let mut pacnew_files = Vec::new();
    for p in handle.localdb().pkgs() {
        let ignored = conf.ignore_pkg.iter().any(|i| i == p.name());
        packages.push(InstalledPackage {
            name: p.name().to_string(),
            version: p.version().to_string(),
            explicit: p.reason() == PackageReason::Explicit,
            required: !p.required_by().is_empty() || !p.optional_for().is_empty(),
            foreign: syncdbs.iter().all(|db| db.pkg(p.name()).is_err()),
            repo_update: if ignored {
                None
            } else {
                p.sync_new_version(syncdbs).map(|n| n.version().to_string())
            },
        });
        for b in p.backup() {
            let path = conf.root_dir.join(format!("{}.pacnew", b.name()));
            if path.exists() {
                pacnew_files.push(path);
            }
        }
    }
    Ok(Arc::new(PackageSnapshot {
        packages,
        freshness,
        pacnew_files,
    }))
}

/// Timestamp of the last "starting full system upgrade" in pacman.log,
/// e.g. `[2026-11-24T17:35:12+0100] [PACMAN] starting full system upgrade`.
pub fn last_full_upgrade_in(log: &str) -> Option<DateTime<FixedOffset>> {
    log.lines()
        .rev()
        .find(|l| l.ends_with("starting full system upgrade"))
        .and_then(|l| l.strip_prefix('[')?.split_once(']'))
        .and_then(|(ts, _)| DateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%z").ok())
}

impl PackageDb for AlpmPackageDb {
    fn snapshot(&self) -> PortResult<Arc<PackageSnapshot>> {
        self.snapshot
            .get_or_init(|| build_snapshot(self.sync))
            .clone()
    }

    fn last_full_upgrade(&self) -> PortResult<Option<DateTime<FixedOffset>>> {
        let conf = PacmanConf::load_default()?;
        let log = fs::read_to_string(&conf.log_file).map_err(|e| {
            PortError::unavailable(format!("reading {}: {e}", conf.log_file.display()))
        })?;
        Ok(last_full_upgrade_in(&log))
    }

    fn compare_versions(&self, a: &str, b: &str) -> Ordering {
        alpm::vercmp(a, b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pacman_log() {
        let log = "[2026-11-01T10:00:00+0100] [PACMAN] starting full system upgrade\n\
                   [2026-11-20T09:30:00+0100] [PACMAN] starting full system upgrade\n\
                   [2026-11-20T09:31:00+0100] [ALPM] upgraded foo (1-1 -> 2-1)\n";
        let d = last_full_upgrade_in(log).unwrap();
        assert_eq!(d.to_rfc3339(), "2026-11-20T09:30:00+01:00");
        assert!(last_full_upgrade_in("").is_none());
    }

    #[test]
    fn vercmp_uses_pacman_rules() {
        let db = AlpmPackageDb::new(false);
        assert_eq!(db.compare_versions("1:1.0-1", "2.0-1"), Ordering::Greater);
        assert_eq!(db.compare_versions("1.0rc1-1", "1.0-1"), Ordering::Less);
    }
}
