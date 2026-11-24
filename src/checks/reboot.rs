//! Reboot / restart detection, natively:
//! - running kernel vs. modules installed in /usr/lib/modules
//! - processes still mapping shared libraries that were replaced on disk

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::checks::Ctx;
use crate::report::{CheckResult, Status};

pub fn run(ctx: &Ctx) -> Vec<CheckResult> {
    vec![kernel(), stale_libraries(ctx)]
}

fn kernel() -> CheckResult {
    let release = rustix::system::uname().release().to_string_lossy().into_owned();
    let modules = Path::new("/usr/lib/modules").join(&release);
    if modules.is_dir() {
        CheckResult::new("Kernel", Status::Ok, format!("in esecuzione {release}"))
    } else {
        let installed: Vec<String> = fs::read_dir("/usr/lib/modules")
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join("vmlinuz").exists())
            .map(|e| format!("installato: {}", e.file_name().to_string_lossy()))
            .collect();
        CheckResult::new(
            "Kernel",
            Status::Warn,
            format!("{release} non è più installato: riavvio necessario"),
        )
        .with_details(installed)
    }
}

/// A mapped file that was deleted/replaced shows up as "<path> (deleted)".
fn is_stale_lib(line: &str) -> Option<&str> {
    let path = line.strip_suffix(" (deleted)")?.rsplit(' ').next()?;
    let lib = path.starts_with("/usr/") || path.starts_with("/lib");
    (lib && path.contains(".so")).then_some(path)
}

fn stale_libraries(ctx: &Ctx) -> CheckResult {
    // comm -> number of processes
    let mut procs: BTreeMap<String, usize> = BTreeMap::new();

    for entry in fs::read_dir("/proc").into_iter().flatten().flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(maps) = fs::read_to_string(entry.path().join("maps")) else { continue };
        if maps.is_empty() {
            // Kernel threads, or processes we are not allowed to inspect.
            continue;
        }
        if maps.lines().any(|l| is_stale_lib(l).is_some()) {
            let comm = fs::read_to_string(entry.path().join("comm")).unwrap_or_default();
            *procs.entry(comm.trim().to_string()).or_default() += 1;
        }
    }

    let scope = if ctx.is_root { "" } else { " (solo processi dell'utente, esegui come root per tutti)" };
    if procs.is_empty() {
        CheckResult::new("Librerie aggiornate", Status::Ok, format!("nessun processo usa librerie vecchie{scope}"))
    } else {
        let details = procs
            .iter()
            .map(|(c, n)| if *n > 1 { format!("{c} (×{n})") } else { c.clone() })
            .collect();
        CheckResult::new(
            "Librerie aggiornate",
            Status::Warn,
            format!("{} programmi da riavviare (usano librerie sostituite){scope}", procs.len()),
        )
        .with_details(details)
    }
}

#[cfg(test)]
mod tests {
    use super::is_stale_lib;

    #[test]
    fn detects_deleted_libs() {
        let l = "7f00-7f01 r-xp 00000000 00:1f 123 /usr/lib/libssl.so.3 (deleted)";
        assert_eq!(is_stale_lib(l), Some("/usr/lib/libssl.so.3"));
        assert_eq!(is_stale_lib("7f00-7f01 rw-s 0 00:01 9 /memfd:foo (deleted)"), None);
        assert_eq!(is_stale_lib("7f00-7f01 r-xp 0 00:1f 1 /usr/lib/libc.so.6"), None);
    }
}
