//! Journal checks through sd-journal (libsystemd):
//! - error-or-worse messages logged during the current boot
//! - coredumps recorded by systemd-coredump in the last days
//!
//! Reading the system journal needs membership in `wheel`, `adm` or
//! `systemd-journal` (Arch grants it via ACL); otherwise only the user's own
//! journal is visible and the summary says so.

use std::collections::HashMap;
use std::fs;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use systemd::journal::{self, Journal, JournalRecord};

use crate::checks::Ctx;
use crate::report::{CheckResult, Status};

/// MESSAGE_ID logged by systemd-coredump for every crash.
const COREDUMP_MESSAGE_ID: &str = "fc2e22bc6ee647b6b90729ab34a250b1";
const COREDUMP_WINDOW: Duration = Duration::from_secs(7 * 24 * 3600);
const MSG_MAX: usize = 100;

pub fn run(_ctx: &Ctx) -> Vec<CheckResult> {
    let scope = if system_journal_readable() { "" } else { " (solo journal utente: serve gruppo wheel/adm)" };
    vec![
        errors(scope).unwrap_or_else(|e| CheckResult::error("Errori journal", e)),
        coredumps(scope).unwrap_or_else(|e| CheckResult::error("Coredump", e)),
    ]
}

fn open() -> Result<Journal> {
    journal::OpenOptions::default()
        .local_only(true)
        .open()
        .context("apertura del journal")
}

fn system_journal_readable() -> bool {
    let Ok(id) = fs::read_to_string("/etc/machine-id") else { return false };
    ["/var/log/journal", "/run/log/journal"].iter().any(|base| {
        let dir = format!("{base}/{}", id.trim());
        fs::File::open(format!("{dir}/system.journal")).is_ok()
    })
}

fn boot_id() -> Result<String> {
    let raw = fs::read_to_string("/proc/sys/kernel/random/boot_id").context("lettura del boot id")?;
    Ok(raw.trim().replace('-', ""))
}

/// Which program an entry belongs to, for grouping.
fn source(r: &JournalRecord) -> String {
    for key in ["_SYSTEMD_UNIT", "_SYSTEMD_USER_UNIT", "SYSLOG_IDENTIFIER", "_COMM"] {
        if let Some(v) = r.get(key) {
            return v.clone();
        }
    }
    if r.get("_TRANSPORT").map(String::as_str) == Some("kernel") {
        return "kernel".into();
    }
    "sconosciuto".into()
}

fn truncate(s: &str) -> String {
    let s = s.lines().next().unwrap_or("").trim();
    if s.chars().count() > MSG_MAX {
        format!("{}…", s.chars().take(MSG_MAX).collect::<String>())
    } else {
        s.to_string()
    }
}

fn ago(t: SystemTime) -> String {
    let s = SystemTime::now().duration_since(t).unwrap_or_default().as_secs();
    match s {
        0..60 => "adesso".into(),
        60..3600 => format!("{} min fa", s / 60),
        3600..86400 => format!("{} h fa", s / 3600),
        _ => format!("{} g fa", s / 86400),
    }
}

struct Group {
    count: usize,
    worst_prio: u8,
    last_msg: String,
}

fn errors(scope: &str) -> Result<CheckResult> {
    let mut j = open()?;
    j.match_add("_BOOT_ID", boot_id()?)?;
    j.match_and()?;
    // Matches on the same field are OR-ed by sd-journal.
    for p in 0..=3 {
        j.match_add("PRIORITY", p.to_string())?;
    }
    j.seek_head()?;

    let mut groups: HashMap<String, Group> = HashMap::new();
    let mut total = 0usize;
    let mut worst = 3u8;
    while let Some(r) = j.next_entry()? {
        let prio: u8 = r.get("PRIORITY").and_then(|p| p.parse().ok()).unwrap_or(3);
        total += 1;
        worst = worst.min(prio);
        let g = groups.entry(source(&r)).or_insert(Group { count: 0, worst_prio: 7, last_msg: String::new() });
        g.count += 1;
        g.worst_prio = g.worst_prio.min(prio);
        g.last_msg = truncate(r.get("MESSAGE").map(String::as_str).unwrap_or(""));
    }

    let mut sorted: Vec<_> = groups.into_iter().collect();
    sorted.sort_by(|a, b| a.1.worst_prio.cmp(&b.1.worst_prio).then(b.1.count.cmp(&a.1.count)));
    let details = sorted
        .iter()
        .map(|(src, g)| {
            let level = if g.worst_prio <= 2 { "CRIT " } else { "" };
            format!("{level}{src} ×{}: {}", g.count, g.last_msg)
        })
        .collect();

    // emerg/alert/crit are real trouble; plain errors are common noise on
    // desktops, so they are reported but don't degrade the overall status.
    let (status, summary) = match (total, worst) {
        (0, _) => (Status::Ok, format!("nessun errore dall'avvio{scope}")),
        (_, 0..=2) => (Status::Crit, format!("{total} messaggi di errore, alcuni critici, da {} fonti{scope}", sorted.len())),
        _ => (Status::Info, format!("{total} errori dall'avvio da {} fonti{scope}", sorted.len())),
    };
    Ok(CheckResult::new("Errori journal", status, summary).with_details(details))
}

fn coredumps(scope: &str) -> Result<CheckResult> {
    let mut j = open()?;
    j.match_add("MESSAGE_ID", COREDUMP_MESSAGE_ID)?;
    let since = SystemTime::now() - COREDUMP_WINDOW;
    j.seek_realtime_usec(since.duration_since(UNIX_EPOCH)?.as_micros() as u64)?;

    let current_boot = boot_id()?;
    let mut this_boot = 0usize;
    // exe -> (count, last time, last signal)
    let mut by_exe: HashMap<String, (usize, SystemTime, String)> = HashMap::new();
    while let Some(r) = j.next_entry()? {
        let when = j.timestamp().unwrap_or(UNIX_EPOCH);
        if when < since {
            continue;
        }
        if r.get("_BOOT_ID") == Some(&current_boot) {
            this_boot += 1;
        }
        let exe = r
            .get("COREDUMP_EXE")
            .or_else(|| r.get("COREDUMP_COMM"))
            .cloned()
            .unwrap_or_else(|| "sconosciuto".into());
        let signal = r.get("COREDUMP_SIGNAL_NAME").cloned().unwrap_or_default();
        let e = by_exe.entry(exe).or_insert((0, UNIX_EPOCH, String::new()));
        e.0 += 1;
        if when >= e.1 {
            e.1 = when;
            e.2 = signal;
        }
    }

    let total: usize = by_exe.values().map(|v| v.0).sum();
    let mut sorted: Vec<_> = by_exe.into_iter().collect();
    sorted.sort_by(|a, b| b.1.1.cmp(&a.1.1));
    let details = sorted
        .iter()
        .map(|(exe, (n, when, sig))| {
            let sig = if sig.is_empty() { String::new() } else { format!(", {sig}") };
            format!("{exe} ×{n} (ultimo {}{sig})", ago(*when))
        })
        .collect();

    let (status, summary) = if total == 0 {
        (Status::Ok, format!("nessun crash negli ultimi 7 giorni{scope}"))
    } else if this_boot > 0 {
        (Status::Warn, format!("{total} crash in 7 giorni, {this_boot} da questo avvio{scope}"))
    } else {
        (Status::Info, format!("{total} crash in 7 giorni, nessuno da questo avvio{scope}"))
    };
    Ok(CheckResult::new("Coredump", status, summary).with_details(details))
}
