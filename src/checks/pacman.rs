//! Package checks through libalpm: pending updates, orphans, foreign packages
//! and unmerged `.pacnew` files.
//!
//! Like `checkupdates`, repo databases are refreshed into a private copy
//! (`$XDG_CACHE_HOME/arch-health/db`) so the system databases under
//! `/var/lib/pacman` are never touched and no root is needed.

use std::fs::{self, File};
use std::path::{Path, PathBuf};

use alpm::{Alpm, PackageReason};
use anyhow::{Context, Result};

use crate::checks::{Ctx, aur};
use crate::pacman_conf::PacmanConf;
use crate::report::{CheckResult, Status};

fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("arch-health").join("db")
}

/// Prepares the private db directory: `local` is a symlink to the system
/// local db; sync dbs are seeded from the system copy (keeping mtime, so
/// libalpm only downloads what actually changed on the mirror).
fn prepare_private_dbpath(conf: &PacmanConf) -> Result<PathBuf> {
    let dir = cache_dir();
    let sync = dir.join("sync");
    fs::create_dir_all(&sync).with_context(|| format!("creazione di {}", sync.display()))?;

    let local = dir.join("local");
    if fs::symlink_metadata(&local).is_err() {
        std::os::unix::fs::symlink(conf.db_path.join("local"), &local)
            .context("creazione del link al database locale")?;
    }

    for repo in &conf.repos {
        let file = format!("{}.db", repo.name);
        let (src, dst) = (conf.db_path.join("sync").join(&file), sync.join(&file));
        if dst.exists() || !src.exists() {
            continue;
        }
        fs::copy(&src, &dst)?;
        if let Ok(mtime) = fs::metadata(&src).and_then(|m| m.modified()) {
            File::options().write(true).open(&dst)?.set_modified(mtime)?;
        }
    }
    Ok(dir)
}

fn open_handle(conf: &PacmanConf, db_path: &Path) -> Result<Alpm> {
    let root = conf.root_dir.to_string_lossy().into_owned();
    let db = db_path.to_string_lossy().into_owned();
    let mut handle = Alpm::new(root, db).context("inizializzazione di libalpm")?;
    handle.set_gpgdir(conf.gpg_dir.to_string_lossy().into_owned())?;
    let archs: Vec<&str> = conf.architectures.iter().map(String::as_str).collect();
    handle.set_architectures(archs.iter())?;
    handle.set_parallel_downloads(conf.parallel_downloads.max(1));

    for repo in &conf.repos {
        let db = handle
            .register_syncdb_mut(repo.name.as_str(), repo.sig_level)
            .with_context(|| format!("registrazione del repo {}", repo.name))?;
        for server in &repo.servers {
            db.add_server(server.as_str())?;
        }
    }
    Ok(handle)
}

pub fn run(ctx: &Ctx) -> Vec<CheckResult> {
    match run_inner(ctx) {
        Ok(r) => r,
        Err(e) => vec![CheckResult::error("Pacchetti", e)],
    }
}

fn run_inner(ctx: &Ctx) -> Result<Vec<CheckResult>> {
    let conf = PacmanConf::load_default()?;
    let mut results = Vec::new();

    // Updates: private, freshly synced dbs (unless --no-sync).
    let (handle, sync_note) = if ctx.no_sync || ctx.offline {
        (open_handle(&conf, &conf.db_path)?, Some("database di sistema, senza sync"))
    } else {
        let dir = prepare_private_dbpath(&conf)?;
        let mut handle = open_handle(&conf, &dir)?;
        match handle.syncdbs_mut().update(false) {
            Ok(_) => (handle, None),
            Err(e) => {
                results.push(CheckResult::new(
                    "Sync database",
                    Status::Warn,
                    format!("aggiornamento dei database fallito: {e}"),
                ));
                (handle, Some("database potenzialmente non aggiornati"))
            }
        }
    };

    results.push(updates(&handle, &conf, sync_note));
    results.push(orphans(&handle));
    let foreign = foreign_pkgs(&handle);
    results.push(foreign_summary(&foreign));
    results.push(pacnew(&handle, &conf.root_dir));
    if !ctx.offline {
        results.extend(aur::check(&foreign));
    }
    Ok(results)
}

fn updates(handle: &Alpm, conf: &PacmanConf, note: Option<&str>) -> CheckResult {
    let syncdbs = handle.syncdbs();
    let mut list = Vec::new();
    for pkg in handle.localdb().pkgs() {
        if conf.ignore_pkg.iter().any(|i| i == pkg.name()) {
            continue;
        }
        if let Some(new) = pkg.sync_new_version(syncdbs) {
            list.push(format!("{} {} → {}", pkg.name(), pkg.version(), new.version()));
        }
    }
    let note = note.map(|n| format!(" ({n})")).unwrap_or_default();
    let (status, summary) = if list.is_empty() {
        (Status::Ok, format!("sistema aggiornato{note}"))
    } else {
        (Status::Warn, format!("{} aggiornamenti disponibili{note}", list.len()))
    };
    CheckResult::new("Aggiornamenti", status, summary).with_details(list)
}

fn orphans(handle: &Alpm) -> CheckResult {
    let list: Vec<String> = handle
        .localdb()
        .pkgs()
        .iter()
        .filter(|p| {
            p.reason() == PackageReason::Depend
                && p.required_by().is_empty()
                && p.optional_for().is_empty()
        })
        .map(|p| p.name().to_string())
        .collect();
    let (status, summary) = if list.is_empty() {
        (Status::Ok, "nessun pacchetto orfano".to_string())
    } else {
        (Status::Info, format!("{} pacchetti orfani", list.len()))
    };
    CheckResult::new("Orfani", status, summary).with_details(list)
}

/// (name, version) of installed packages not provided by any sync repo.
fn foreign_pkgs(handle: &Alpm) -> Vec<(String, String)> {
    let syncdbs = handle.syncdbs();
    handle
        .localdb()
        .pkgs()
        .iter()
        .filter(|p| syncdbs.iter().all(|db| db.pkg(p.name()).is_err()))
        .map(|p| (p.name().to_string(), p.version().to_string()))
        .collect()
}

fn foreign_summary(foreign: &[(String, String)]) -> CheckResult {
    let list = foreign.iter().map(|(n, v)| format!("{n} {v}")).collect();
    let summary = format!("{} pacchetti non presenti nei repo (AUR/locali)", foreign.len());
    CheckResult::new("Pacchetti esterni", Status::Info, summary).with_details(list)
}

fn pacnew(handle: &Alpm, root: &Path) -> CheckResult {
    let mut list = Vec::new();
    for pkg in handle.localdb().pkgs() {
        for b in pkg.backup() {
            let p = root.join(format!("{}.pacnew", b.name()));
            if p.exists() {
                list.push(p.display().to_string());
            }
        }
    }
    let (status, summary) = if list.is_empty() {
        (Status::Ok, "nessun file .pacnew da unire".to_string())
    } else {
        (Status::Warn, format!("{} file .pacnew da unire", list.len()))
    };
    CheckResult::new("File .pacnew", status, summary).with_details(list)
}
