use std::collections::HashMap;
use std::time::{Duration, SystemTime};

use super::{Check, Context, Privilege};
use crate::error::PortResult;
use crate::model::{CheckId, CheckReport, Item, Status, TimedEvent, plural};

/// Grouped item messages are short; event messages keep more context.
const MESSAGE_MAX: usize = 100;
const EVENT_MESSAGE_MAX: usize = 400;

fn journal_limitation(ctx: &Context<'_>) -> Option<String> {
    (!ctx.ports.journal.system_journal_readable()).then(|| {
        "only the user journal is readable (join the wheel or adm group, or run as root)"
            .to_string()
    })
}

fn truncate(s: &str, max: usize) -> String {
    let s = s.lines().next().unwrap_or("").trim();
    if s.chars().count() > max {
        format!("{}…", s.chars().take(max).collect::<String>())
    } else {
        s.to_string()
    }
}

/// emerg/alert/crit → critical; err → warning (for display of single events).
fn priority_status(priority: u8) -> Status {
    if priority <= 2 {
        Status::Critical
    } else {
        Status::Warning
    }
}

/// Errors of the current boot, grouped by source.
pub struct JournalErrors;

impl Check for JournalErrors {
    fn id(&self) -> CheckId {
        CheckId::JournalErrors
    }

    fn privilege(&self) -> Privilege {
        Privilege::Enhanced
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        struct Group {
            count: usize,
            worst: u8,
            last: String,
        }
        let entries = ctx.ports.journal.errors_current_boot()?;
        let mut groups: HashMap<&str, Group> = HashMap::new();
        for e in &entries {
            let g = groups.entry(&e.source).or_insert(Group {
                count: 0,
                worst: 7,
                last: String::new(),
            });
            g.count += 1;
            g.worst = g.worst.min(e.priority);
            g.last = truncate(&e.message, MESSAGE_MAX);
        }
        let mut groups: Vec<_> = groups.into_iter().collect();
        groups.sort_by(|a, b| a.1.worst.cmp(&b.1.worst).then(b.1.count.cmp(&a.1.count)));

        // emerg/alert/crit are real trouble; plain errors are common noise on
        // desktops, so they inform without degrading the overall status.
        let items: Vec<Item> = groups
            .iter()
            .map(|(src, g)| {
                let status = if g.worst <= 2 {
                    Status::Critical
                } else {
                    Status::Info
                };
                Item::new(status, *src, format!("×{}: {}", g.count, g.last))
            })
            .collect();
        let summary = if entries.is_empty() {
            "no errors since boot".to_string()
        } else {
            format!(
                "{} since boot from {}",
                plural(entries.len(), "error"),
                plural(items.len(), "source")
            )
        };
        let events = entries
            .iter()
            .map(|e| TimedEvent {
                time: e.time,
                status: priority_status(e.priority),
                source: e.source.clone(),
                message: truncate(&e.message, EVENT_MESSAGE_MAX),
            })
            .collect();
        Ok(CheckReport::from_items(self.id(), items, summary)
            .with_events(events)
            .with_limitation(journal_limitation(ctx)))
    }
}

/// Crashes recorded by systemd-coredump.
pub struct Coredumps;

impl Check for Coredumps {
    fn id(&self) -> CheckId {
        CheckId::Coredumps
    }

    fn privilege(&self) -> Privilege {
        Privilege::Enhanced
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let days = ctx.config.coredump_window_days;
        let since = SystemTime::now() - Duration::from_secs(days * 24 * 3600);
        let dumps = ctx.ports.journal.coredumps_since(since)?;

        // executable → (count, last time, last signal, any in current boot)
        let mut by_exe: HashMap<&str, (usize, SystemTime, Option<&str>, bool)> = HashMap::new();
        for d in &dumps {
            let e = by_exe
                .entry(&d.executable)
                .or_insert((0, SystemTime::UNIX_EPOCH, None, false));
            e.0 += 1;
            e.3 |= d.current_boot;
            if d.time >= e.1 {
                e.1 = d.time;
                e.2 = d.signal.as_deref();
            }
        }
        let mut rows: Vec<_> = by_exe.into_iter().collect();
        rows.sort_by_key(|r| std::cmp::Reverse(r.1.1));
        let this_boot = dumps.iter().filter(|d| d.current_boot).count();

        let items: Vec<Item> = rows
            .into_iter()
            .map(|(exe, (n, when, signal, current))| {
                let status = if current {
                    Status::Warning
                } else {
                    Status::Info
                };
                let signal = signal.map(|s| format!(", {s}")).unwrap_or_default();
                Item::new(status, exe, format!("×{n}, last {}{signal}", ago(when)))
            })
            .collect();
        let summary = match (dumps.len(), this_boot) {
            (0, _) => format!("no crashes in the last {days} days"),
            (n, 0) => format!("{} in {days} days, none since boot", plural(n, "crash")),
            (n, b) => format!("{} in {days} days, {b} since boot", plural(n, "crash")),
        };
        let events = dumps
            .iter()
            .map(|d| TimedEvent {
                time: d.time,
                status: if d.current_boot {
                    Status::Warning
                } else {
                    Status::Info
                },
                source: d.executable.clone(),
                message: match &d.signal {
                    Some(sig) => format!("crashed ({sig})"),
                    None => "crashed".into(),
                },
            })
            .collect();
        Ok(CheckReport::from_items(self.id(), items, summary)
            .with_events(events)
            .with_limitation(journal_limitation(ctx)))
    }
}

fn ago(t: SystemTime) -> String {
    let s = SystemTime::now()
        .duration_since(t)
        .unwrap_or_default()
        .as_secs();
    match s {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", s / 60),
        3600..86400 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::Fake;
    use crate::ports::{Coredump, LogEntry};

    fn entry(source: &str, priority: u8) -> LogEntry {
        LogEntry {
            time: SystemTime::now(),
            source: source.into(),
            priority,
            message: format!("{source} failed"),
        }
    }

    #[test]
    fn errors_are_info_crit_is_critical() {
        let mut fake = Fake::default();
        fake.journal_entries = vec![
            entry("bluetooth", 3),
            entry("bluetooth", 3),
            entry("kernel", 2),
        ];
        let (ports, config) = (fake.ports(), Default::default());
        let r = JournalErrors
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();
        assert_eq!(r.status, Status::Critical);
        assert_eq!(r.items[0].subject, "kernel");
        assert_eq!(r.items[1].detail, "×2: bluetooth failed");
        assert_eq!(r.items[1].status, Status::Info);
        assert!(r.limitation.is_none());
        // One event per entry, with per-event severity.
        assert_eq!(r.events.len(), 3);
        assert_eq!(
            r.events
                .iter()
                .filter(|e| e.status == Status::Critical)
                .count(),
            1
        );
    }

    #[test]
    fn coredumps_in_current_boot_warn() {
        let mut fake = Fake::default();
        fake.journal_readable = false;
        let now = SystemTime::now();
        fake.coredumps = vec![
            Coredump {
                executable: "/usr/bin/a".into(),
                signal: Some("SIGSEGV".into()),
                time: now,
                current_boot: true,
            },
            Coredump {
                executable: "/usr/bin/a".into(),
                signal: None,
                time: now - Duration::from_secs(7200),
                current_boot: false,
            },
        ];
        let (ports, config) = (fake.ports(), Default::default());
        let r = Coredumps
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();
        assert_eq!(r.status, Status::Warning);
        assert_eq!(r.summary, "2 crashes in 7 days, 1 since boot");
        assert!(r.items[0].detail.starts_with("×2, last just now, SIGSEGV"));
        assert!(r.limitation.is_some());
        assert_eq!(r.events.len(), 2);
        assert_eq!(r.events[0].message, "crashed (SIGSEGV)");
        assert_eq!(r.events[0].status, Status::Warning);
    }
}
