//! `JournalReader` over sd-journal (libsystemd).

use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

use arch_health_core::ports::{Coredump, JournalReader, LogEntry};
use arch_health_core::{PortError, PortResult};
use systemd::journal::{self, Journal, JournalRecord};

/// MESSAGE_ID logged by systemd-coredump for every crash.
const COREDUMP_MESSAGE_ID: &str = "fc2e22bc6ee647b6b90729ab34a250b1";

pub struct SdJournal;

fn io_err<E: std::fmt::Display>(what: &str) -> impl Fn(E) -> PortError + '_ {
    move |e| PortError::unavailable(format!("{what}: {e}"))
}

fn open() -> PortResult<Journal> {
    journal::OpenOptions::default()
        .local_only(true)
        .open()
        .map_err(io_err("opening the journal"))
}

fn boot_id() -> PortResult<String> {
    let raw = fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(io_err("reading the boot id"))?;
    Ok(raw.trim().replace('-', ""))
}

/// Which program an entry belongs to, for grouping.
/// In the user session `_SYSTEMD_UNIT` is always `user@UID.service`, so the
/// user unit (or the program name) is the meaningful source there.
fn source(r: &JournalRecord) -> String {
    if let Some(u) = r.get("_SYSTEMD_USER_UNIT") {
        return u.clone();
    }
    if let Some(u) = r.get("_SYSTEMD_UNIT")
        && !u.starts_with("user@")
    {
        return u.clone();
    }
    for key in ["SYSLOG_IDENTIFIER", "_COMM"] {
        if let Some(v) = r.get(key) {
            return v.clone();
        }
    }
    if r.get("_TRANSPORT").map(String::as_str) == Some("kernel") {
        return "kernel".into();
    }
    "unknown".into()
}

impl JournalReader for SdJournal {
    fn errors_current_boot(&self) -> PortResult<Vec<LogEntry>> {
        let mut j = open()?;
        let err = |e: systemd::Error| PortError::unavailable(format!("querying the journal: {e}"));
        j.match_add("_BOOT_ID", boot_id()?).map_err(&err)?;
        j.match_and().map_err(&err)?;
        // Matches on the same field are OR-ed by sd-journal.
        for p in 0..=3 {
            j.match_add("PRIORITY", p.to_string()).map_err(&err)?;
        }
        j.seek_head().map_err(&err)?;
        let mut out = Vec::new();
        while let Some(r) = j.next_entry().map_err(&err)? {
            // An entry without a timestamp cannot be placed in time; dating it
            // 1970 would distort every timeline.
            let Ok(time) = j.timestamp() else { continue };
            out.push(LogEntry {
                time,
                source: source(&r),
                priority: r.get("PRIORITY").and_then(|p| p.parse().ok()).unwrap_or(3),
                message: r.get("MESSAGE").cloned().unwrap_or_default(),
            });
        }
        Ok(out)
    }

    fn coredumps_since(&self, since: SystemTime) -> PortResult<Vec<Coredump>> {
        let mut j = open()?;
        let err = |e: systemd::Error| PortError::unavailable(format!("querying the journal: {e}"));
        j.match_add("MESSAGE_ID", COREDUMP_MESSAGE_ID)
            .map_err(&err)?;
        let usec = since
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u64;
        j.seek_realtime_usec(usec).map_err(&err)?;
        let current = boot_id()?;
        let mut out = Vec::new();
        while let Some(r) = j.next_entry().map_err(&err)? {
            let time = j.timestamp().unwrap_or(UNIX_EPOCH);
            if time < since {
                continue;
            }
            out.push(Coredump {
                executable: r
                    .get("COREDUMP_EXE")
                    .or_else(|| r.get("COREDUMP_COMM"))
                    .cloned()
                    .unwrap_or_else(|| "unknown".into()),
                signal: r.get("COREDUMP_SIGNAL_NAME").cloned(),
                time,
                current_boot: r.get("_BOOT_ID") == Some(&current),
            });
        }
        Ok(out)
    }

    /// Readable for root and the wheel/adm/systemd-journal groups.
    fn system_journal_readable(&self) -> bool {
        let Ok(id) = fs::read_to_string("/etc/machine-id") else {
            return false;
        };
        ["/var/log/journal", "/run/log/journal"]
            .iter()
            .any(|base| fs::File::open(format!("{base}/{}/system.journal", id.trim())).is_ok())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(fields: &[(&str, &str)]) -> JournalRecord {
        fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn source_prefers_meaningful_names() {
        let user_app = record(&[
            ("_SYSTEMD_UNIT", "user@1000.service"),
            ("_SYSTEMD_USER_UNIT", "plasma-plasmashell.service"),
            ("_COMM", "plasmashell"),
        ]);
        assert_eq!(source(&user_app), "plasma-plasmashell.service");

        let session_process = record(&[("_SYSTEMD_UNIT", "user@1000.service"), ("_COMM", "steam")]);
        assert_eq!(source(&session_process), "steam");

        let service = record(&[
            ("_SYSTEMD_UNIT", "bluetooth.service"),
            ("_COMM", "bluetoothd"),
        ]);
        assert_eq!(source(&service), "bluetooth.service");

        assert_eq!(source(&record(&[("_TRANSPORT", "kernel")])), "kernel");
    }
}
