//! Storage checks:
//! - free space / inodes on real filesystems (statvfs)
//! - btrfs device error counters (sysfs)
//! - SMART / NVMe health via the UDisks2 D-Bus API (no root required)

use std::collections::{BTreeSet, HashMap};
use std::fs;

use anyhow::Result;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

use crate::checks::Ctx;
use crate::report::{CheckResult, Status};

const REAL_FS: &[&str] = &[
    "ext2", "ext3", "ext4", "btrfs", "xfs", "f2fs", "vfat", "exfat", "ntfs3", "zfs", "bcachefs",
];
const SPACE_WARN: f64 = 85.0;
const SPACE_CRIT: f64 = 95.0;
const TEMP_WARN_C: f64 = 70.0;

pub fn run(_ctx: &Ctx) -> Vec<CheckResult> {
    vec![space(), btrfs_errors(), smart()]
}

/// Decodes the octal escapes (`\040` …) used in /proc/self/mounts.
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
            let v = (b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0');
            out.push(v);
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

struct Mount {
    device: String,
    target: String,
    fstype: String,
    read_only: bool,
}

fn mounts() -> Vec<Mount> {
    let text = fs::read_to_string("/proc/self/mounts").unwrap_or_default();
    let mut seen = BTreeSet::new();
    text.lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (dev, target, fstype, opts) = (f.next()?, f.next()?, f.next()?, f.next()?);
            if !REAL_FS.contains(&fstype) || !seen.insert(dev.to_string()) {
                // btrfs subvolumes share the device: report it once.
                return None;
            }
            Some(Mount {
                device: unescape(dev),
                target: unescape(target),
                fstype: fstype.to_string(),
                read_only: opts.split(',').any(|o| o == "ro"),
            })
        })
        .collect()
}

fn pct(used: u64, avail: u64) -> f64 {
    if used + avail == 0 { 0.0 } else { used as f64 * 100.0 / (used + avail) as f64 }
}

fn human(bytes: u64) -> String {
    const U: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < U.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", U[i])
}

fn space() -> CheckResult {
    let mut worst = Status::Ok;
    let mut details = Vec::new();
    for m in mounts() {
        let Ok(st) = rustix::fs::statvfs(m.target.as_str()) else { continue };
        let used = st.f_blocks.saturating_sub(st.f_bfree);
        let p = pct(used, st.f_bavail);
        let mut status = if p >= SPACE_CRIT {
            Status::Crit
        } else if p >= SPACE_WARN {
            Status::Warn
        } else {
            Status::Ok
        };
        let mut line = format!(
            "{} ({}, {}): {:.0}% usato, {} liberi",
            m.target,
            m.fstype,
            m.device,
            p,
            human(st.f_bavail * st.f_frsize)
        );
        // btrfs/vfat do not have a fixed inode table.
        if st.f_files > 0 && m.fstype != "btrfs" {
            let ip = pct(st.f_files - st.f_ffree, st.f_favail);
            if ip >= SPACE_WARN {
                status = status.max(Status::Warn);
                line.push_str(&format!(", inode {ip:.0}%"));
            }
        }
        if m.read_only && m.fstype != "vfat" {
            status = status.max(Status::Warn);
            line.push_str(", montato in sola lettura");
        }
        worst = worst.max(status);
        details.push(line);
    }
    let summary = match worst {
        Status::Ok => format!("{} filesystem, spazio sufficiente", details.len()),
        _ => "spazio in esaurimento o filesystem in sola lettura".to_string(),
    };
    CheckResult::new("Spazio disco", worst, summary).with_details(details)
}

fn btrfs_errors() -> CheckResult {
    let Ok(fss) = fs::read_dir("/sys/fs/btrfs") else {
        return CheckResult::new("Errori btrfs", Status::Ok, "nessun filesystem btrfs");
    };
    let mut details = Vec::new();
    let mut devices = 0;
    for fs_dir in fss.flatten() {
        for dev in fs::read_dir(fs_dir.path().join("devinfo")).into_iter().flatten().flatten() {
            let Ok(stats) = fs::read_to_string(dev.path().join("error_stats")) else { continue };
            devices += 1;
            let errors: Vec<String> = stats
                .lines()
                .filter_map(|l| l.split_once(' '))
                .filter(|(_, v)| v.trim() != "0")
                .map(|(k, v)| format!("{k}={}", v.trim()))
                .collect();
            if !errors.is_empty() {
                details.push(format!(
                    "fs {} dev {}: {}",
                    fs_dir.file_name().to_string_lossy(),
                    dev.file_name().to_string_lossy(),
                    errors.join(", ")
                ));
            }
        }
    }
    if details.is_empty() {
        CheckResult::new("Errori btrfs", Status::Ok, format!("{devices} dispositivi, nessun errore"))
    } else {
        CheckResult::new("Errori btrfs", Status::Crit, "contatori di errore non a zero").with_details(details)
    }
}

type Props = HashMap<String, OwnedValue>;
type ManagedObjects = HashMap<OwnedObjectPath, HashMap<String, Props>>;

fn prop<T: TryFrom<OwnedValue>>(p: &Props, key: &str) -> Option<T> {
    p.get(key)?.try_clone().ok()?.try_into().ok()
}

fn smart() -> CheckResult {
    let res = (|| -> Result<CheckResult> {
        let conn = Connection::system()?;
        let om = Proxy::new(
            &conn,
            "org.freedesktop.UDisks2",
            "/org/freedesktop/UDisks2",
            "org.freedesktop.DBus.ObjectManager",
        )?;
        let objects: ManagedObjects = om.call("GetManagedObjects", &())?;

        let mut worst = Status::Ok;
        let mut details = Vec::new();
        let mut drives: Vec<_> = objects
            .iter()
            .filter_map(|(path, ifaces)| Some((path, ifaces.get("org.freedesktop.UDisks2.Drive")?, ifaces)))
            .collect();
        drives.sort_by_key(|(p, ..)| p.as_str().to_string());

        for (_, drive, ifaces) in drives {
            let model: String = prop(drive, "Model").unwrap_or_default();
            let serial: String = prop(drive, "Serial").unwrap_or_default();
            let name = if model.is_empty() { serial } else { model };

            if let Some(ata) = ifaces.get("org.freedesktop.UDisks2.Drive.Ata") {
                if !prop::<bool>(ata, "SmartSupported").unwrap_or(false) {
                    continue;
                }
                let failing = prop::<bool>(ata, "SmartFailing").unwrap_or(false);
                let bad: i64 = prop(ata, "SmartNumBadSectors").unwrap_or(0);
                let temp_k: f64 = prop(ata, "SmartTemperature").unwrap_or(0.0);
                let mut st = Status::Ok;
                let mut notes = vec![];
                if failing {
                    st = Status::Crit;
                    notes.push("SMART: GUASTO IMMINENTE".to_string());
                }
                if bad > 0 {
                    st = st.max(Status::Warn);
                    notes.push(format!("{bad} settori danneggiati"));
                }
                push_temp(&mut st, &mut notes, temp_k);
                worst = worst.max(st);
                details.push(format!("{name} (ATA): {}", notes_or_ok(notes)));
            } else if let Some(nvme) = ifaces.get("org.freedesktop.UDisks2.NVMe.Controller") {
                let warnings: Vec<String> = prop(nvme, "SmartCriticalWarning").unwrap_or_default();
                let temp_k = prop::<u16>(nvme, "SmartTemperature").unwrap_or(0) as f64;
                let mut st = Status::Ok;
                let mut notes = vec![];
                if !warnings.is_empty() {
                    st = Status::Crit;
                    notes.push(format!("avvisi critici: {}", warnings.join(", ")));
                }
                push_temp(&mut st, &mut notes, temp_k);
                worst = worst.max(st);
                details.push(format!("{name} (NVMe): {}", notes_or_ok(notes)));
            }
        }

        let summary = match (details.len(), worst) {
            (0, _) => "nessun disco con dati SMART disponibili".to_string(),
            (n, Status::Ok) => format!("{n} dischi in salute"),
            (_, Status::Crit) => "uno o più dischi segnalano guasti".to_string(),
            _ => "ci sono avvisi sui dischi".to_string(),
        };
        Ok(CheckResult::new("Salute dischi", worst, summary).with_details(details))
    })();
    res.unwrap_or_else(|e| CheckResult::error("Salute dischi", format!("UDisks2 non raggiungibile: {e}")))
}

fn push_temp(st: &mut Status, notes: &mut Vec<String>, kelvin: f64) {
    if kelvin <= 0.0 {
        return;
    }
    let c = kelvin - 273.15;
    if c >= TEMP_WARN_C {
        *st = (*st).max(Status::Warn);
        notes.push(format!("temperatura alta {c:.0}°C"));
    } else {
        notes.push(format!("{c:.0}°C"));
    }
}

fn notes_or_ok(notes: Vec<String>) -> String {
    if notes.iter().all(|n| n.ends_with("°C")) {
        let mut v = vec!["OK".to_string()];
        v.extend(notes);
        v.join(", ")
    } else {
        notes.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::unescape;

    #[test]
    fn unescapes_mount_paths() {
        assert_eq!(unescape("/mnt/my\\040disk"), "/mnt/my disk");
        assert_eq!(unescape("/plain"), "/plain");
    }
}
