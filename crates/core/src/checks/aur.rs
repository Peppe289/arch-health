use std::cmp::Ordering;
use std::collections::HashMap;

use super::{Check, Context};
use crate::error::PortResult;
use crate::model::{CheckId, CheckReport, Item, Status, plural};
use crate::ports::InstalledPackage;

/// Updates and notices for foreign packages, looked up on the AUR.
pub struct AurUpdates;

impl Check for AurUpdates {
    fn id(&self) -> CheckId {
        CheckId::AurUpdates
    }

    fn needs_network(&self) -> bool {
        true
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let snap = ctx.ports.packages.snapshot()?;
        let foreign: Vec<&InstalledPackage> = snap.packages.iter().filter(|p| p.foreign).collect();
        if foreign.is_empty() {
            return Ok(CheckReport::new(
                self.id(),
                Status::Ok,
                "no foreign packages",
            ));
        }
        let names: Vec<String> = foreign.iter().map(|p| p.name.clone()).collect();
        let aur: HashMap<String, _> = ctx
            .ports
            .aur
            .info(&names)?
            .into_iter()
            .map(|p| (p.name.clone(), p))
            .collect();

        let mut updates = Vec::new();
        let mut notices = Vec::new();
        for p in &foreign {
            let Some(remote) = aur.get(&p.name) else {
                notices.push(Item::new(Status::Info, &p.name, missing_note(p, &foreign)));
                continue;
            };
            if ctx
                .ports
                .packages
                .compare_versions(&remote.version, &p.version)
                == Ordering::Greater
            {
                updates.push(Item::new(
                    Status::Warning,
                    &p.name,
                    format!("{} → {}", p.version, remote.version),
                ));
            }
            if remote.out_of_date {
                notices.push(Item::new(
                    Status::Info,
                    &p.name,
                    "flagged out-of-date on the AUR",
                ));
            }
            if remote.orphaned {
                notices.push(Item::new(
                    Status::Info,
                    &p.name,
                    "orphaned on the AUR (no maintainer)",
                ));
            }
        }

        let summary = match (updates.len(), notices.len()) {
            (0, 0) => format!("{} up to date", plural(aur.len(), "AUR package")),
            (0, n) => format!("up to date, {}", plural(n, "notice")),
            (u, 0) => format!("{} available", plural(u, "update")),
            (u, n) => format!("{} available, {}", plural(u, "update"), plural(n, "notice")),
        };
        updates.extend(notices);
        Ok(CheckReport::from_items(self.id(), updates, summary))
    }
}

/// `-debug` split packages are built locally by makepkg and never exist on
/// the AUR: compare them with their base package instead.
fn missing_note(p: &InstalledPackage, foreign: &[&InstalledPackage]) -> String {
    let base = p
        .name
        .strip_suffix("-debug")
        .and_then(|b| foreign.iter().find(|f| f.name == b));
    match base {
        Some(base) if base.version != p.version => format!(
            "debug package ({}) does not match {} {}, can be removed",
            p.version, base.name, base.version
        ),
        Some(base) => format!("debug package of {}", base.name),
        None => "not on the AUR (local package or removed)".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{Fake, pkg};
    use crate::ports::AurPackage;

    fn foreign(name: &str, v: &str) -> InstalledPackage {
        InstalledPackage {
            foreign: true,
            ..pkg(name, v)
        }
    }

    #[test]
    fn aur_updates_and_notices() {
        let mut fake = Fake::default();
        fake.snapshot.packages = vec![
            foreign("paru", "2.2.1-1"),
            foreign("paru-debug", "2.1.0-2"),
            foreign("old", "1.0-1"),
            foreign("gone", "1-1"),
        ];
        fake.aur = vec![
            AurPackage {
                name: "paru".into(),
                version: "2.2.1-1".into(),
                out_of_date: false,
                orphaned: false,
            },
            AurPackage {
                name: "old".into(),
                version: "1.1-1".into(),
                out_of_date: true,
                orphaned: true,
            },
        ];
        let (ports, config) = (fake.ports(), Default::default());
        let r = AurUpdates
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();

        assert_eq!(r.status, Status::Warning);
        let details: Vec<_> = r
            .items
            .iter()
            .map(|i| format!("{}: {}", i.subject, i.detail))
            .collect();
        assert!(details.contains(&"old: 1.0-1 → 1.1-1".to_string()));
        assert!(
            details
                .iter()
                .any(|d| d.starts_with("paru-debug: debug package (2.1.0-2) does not match"))
        );
        assert!(
            details
                .iter()
                .any(|d| d.starts_with("gone: not on the AUR"))
        );
        assert!(
            details
                .iter()
                .any(|d| d.starts_with("old: flagged out-of-date"))
        );
        assert!(details.iter().any(|d| d.starts_with("old: orphaned")));
    }
}
