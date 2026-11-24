//! Storage integrity: space, read-only filesystems, btrfs device errors and
//! drive faults. Temperatures are deliberately ignored (requirement F9):
//! they are physical values that vary over time, not installation health.

use super::{Check, Context};
use crate::error::PortResult;
use crate::model::{CheckId, CheckReport, Gauge, Item, Status, plural};
use crate::ports::DriveKind;

fn pct(used: u64, available: u64) -> f64 {
    if used + available == 0 {
        0.0
    } else {
        used as f64 * 100.0 / (used + available) as f64
    }
}

fn human(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    format!("{v:.1} {}", UNITS[i])
}

pub struct DiskSpace;

impl Check for DiskSpace {
    fn id(&self) -> CheckId {
        CheckId::DiskSpace
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let cfg = ctx.config;
        let level = |p: f64| {
            if p >= cfg.space_critical_pct {
                Status::Critical
            } else if p >= cfg.space_warning_pct {
                Status::Warning
            } else {
                Status::Ok
            }
        };
        let mut items = Vec::new();
        let mut gauges = Vec::new();
        for fs in ctx.ports.storage.filesystems()? {
            let used = pct(fs.bytes_used, fs.bytes_available);
            let mut status = level(used);
            let mut detail = format!(
                "{}, {}: {used:.0}% used, {} free",
                fs.fstype,
                fs.device,
                human(fs.bytes_available)
            );
            if let (Some(u), Some(a)) = (fs.inodes_used, fs.inodes_available) {
                let ip = pct(u, a);
                if level(ip) > Status::Ok {
                    status = status.max(level(ip));
                    detail.push_str(&format!(", inodes {ip:.0}% used"));
                }
            }
            // A filesystem that turned read-only usually hit an error.
            // vfat (ESP) is often mounted read-only on purpose.
            if fs.read_only && fs.fstype != "vfat" {
                status = status.max(Status::Warning);
                detail.push_str(", mounted read-only");
            }
            gauges.push(Gauge {
                label: fs.mount_point.clone(),
                fraction: used / 100.0,
                text: format!(
                    "{used:.0}% used · {} free of {}",
                    human(fs.bytes_available),
                    human(fs.bytes_used + fs.bytes_available)
                ),
                status: level(used),
            });
            items.push(Item::new(status, fs.mount_point, detail));
        }
        let bad = items.iter().filter(|i| i.status > Status::Ok).count();
        let summary = if bad == 0 {
            format!("{}, enough free space", plural(items.len(), "filesystem"))
        } else {
            format!(
                "{} running out of space or read-only",
                plural(bad, "filesystem")
            )
        };
        Ok(CheckReport::from_items(self.id(), items, summary).with_gauges(gauges))
    }
}

pub struct BtrfsErrors;

impl Check for BtrfsErrors {
    fn id(&self) -> CheckId {
        CheckId::BtrfsErrors
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let devices = ctx.ports.storage.btrfs_devices()?;
        if devices.is_empty() {
            return Ok(CheckReport::new(
                self.id(),
                Status::Ok,
                "no btrfs filesystems",
            ));
        }
        let items: Vec<Item> = devices
            .iter()
            .filter_map(|d| {
                let errors: Vec<String> = d
                    .counters
                    .iter()
                    .filter(|(_, v)| **v > 0)
                    .map(|(k, v)| format!("{k}={v}"))
                    .collect();
                (!errors.is_empty()).then(|| {
                    Item::new(
                        Status::Critical,
                        format!("{} dev {}", d.fs_uuid, d.device_id),
                        errors.join(", "),
                    )
                })
            })
            .collect();
        let summary = if items.is_empty() {
            format!("{}, no errors", plural(devices.len(), "device"))
        } else {
            format!(
                "{} with error counters above zero",
                plural(items.len(), "device")
            )
        };
        Ok(CheckReport::from_items(self.id(), items, summary))
    }
}

pub struct DriveHealth;

impl Check for DriveHealth {
    fn id(&self) -> CheckId {
        CheckId::DriveHealth
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let drives = ctx.ports.storage.drives()?;
        let items: Vec<Item> = drives
            .iter()
            .map(|d| {
                let mut problems = Vec::new();
                let mut status = Status::Ok;
                if d.smart_failing {
                    status = Status::Critical;
                    problems.push("SMART reports the drive is failing".to_string());
                }
                if d.bad_sectors > 0 {
                    status = status.max(Status::Warning);
                    problems.push(plural(d.bad_sectors as usize, "bad sector"));
                }
                let warnings: Vec<&str> = d
                    .critical_warnings
                    .iter()
                    .map(String::as_str)
                    .filter(|w| *w != "temperature")
                    .collect();
                if !warnings.is_empty() {
                    status = Status::Critical;
                    problems.push(format!("critical warnings: {}", warnings.join(", ")));
                }
                let kind = match d.kind {
                    DriveKind::Ata => "ATA",
                    DriveKind::Nvme => "NVMe",
                };
                let detail = if problems.is_empty() {
                    format!("{kind}, healthy")
                } else {
                    format!("{kind}, {}", problems.join(", "))
                };
                Item::new(status, &d.model, detail)
            })
            .collect();
        let bad = items.iter().filter(|i| i.status > Status::Ok).count();
        let summary = match (items.len(), bad) {
            (0, _) => "no drive with health data".to_string(),
            (n, 0) => format!("{} healthy", plural(n, "drive")),
            (_, b) => format!("{} reporting faults", plural(b, "drive")),
        };
        Ok(CheckReport::from_items(self.id(), items, summary))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::Config;
    use crate::fake::Fake;
    use crate::ports::{BtrfsDevice, Drive, Filesystem};

    fn fs(mount: &str, used: u64, avail: u64, ro: bool) -> Filesystem {
        Filesystem {
            mount_point: mount.into(),
            device: "/dev/x".into(),
            fstype: "ext4".into(),
            read_only: ro,
            bytes_used: used,
            bytes_available: avail,
            inodes_used: Some(1),
            inodes_available: Some(99),
        }
    }

    #[test]
    fn space_thresholds_and_read_only() {
        let mut fake = Fake::default();
        fake.filesystems = vec![
            fs("/", 50, 50, false),
            fs("/home", 90, 10, false),
            fs("/data", 96, 4, false),
            fs("/ro", 1, 99, true),
        ];
        let (ports, config) = (fake.ports(), Config::default());
        let r = DiskSpace
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();
        let st: Vec<_> = r.items.iter().map(|i| i.status).collect();
        assert_eq!(
            st,
            [
                Status::Ok,
                Status::Warning,
                Status::Critical,
                Status::Warning
            ]
        );
        assert_eq!(r.status, Status::Critical);
        assert_eq!(r.gauges.len(), 4);
        assert!((r.gauges[1].fraction - 0.9).abs() < 1e-9);
        assert_eq!(r.gauges[1].status, Status::Warning);
        // Read-only is not a space problem: the gauge stays ok.
        assert_eq!(r.gauges[3].status, Status::Ok);
    }

    #[test]
    fn btrfs_counters() {
        let mut fake = Fake::default();
        let counters = BTreeMap::from([
            ("read_errs".to_string(), 0),
            ("corruption_errs".to_string(), 2),
        ]);
        fake.btrfs = vec![BtrfsDevice {
            fs_uuid: "u".into(),
            device_id: "1".into(),
            counters,
        }];
        let (ports, config) = (fake.ports(), Config::default());
        let r = BtrfsErrors
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();
        assert_eq!(r.status, Status::Critical);
        assert_eq!(r.items[0].detail, "corruption_errs=2");
    }

    #[test]
    fn drive_faults_ignore_temperature() {
        let mut fake = Fake::default();
        fake.drives = vec![
            Drive {
                model: "hot".into(),
                kind: DriveKind::Nvme,
                smart_failing: false,
                bad_sectors: 0,
                critical_warnings: vec!["temperature".into()],
            },
            Drive {
                model: "dying".into(),
                kind: DriveKind::Ata,
                smart_failing: true,
                bad_sectors: 8,
                critical_warnings: vec![],
            },
        ];
        let (ports, config) = (fake.ports(), Config::default());
        let r = DriveHealth
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();
        assert_eq!(r.items[0].status, Status::Ok);
        assert_eq!(r.items[1].status, Status::Critical);
        assert_eq!(r.summary, "1 drive reporting faults");
    }
}
