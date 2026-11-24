use super::{Check, Context};
use crate::error::PortResult;
use crate::model::{CheckId, CheckReport, Item, Status, plural};
use crate::ports::DbFreshness;

pub struct RepoUpdates;

impl Check for RepoUpdates {
    fn id(&self) -> CheckId {
        CheckId::RepoUpdates
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let snap = ctx.ports.packages.snapshot()?;
        let items: Vec<Item> = snap
            .packages
            .iter()
            .filter_map(|p| {
                let new = p.repo_update.as_ref()?;
                Some(Item::new(
                    Status::Warning,
                    &p.name,
                    format!("{} → {new}", p.version),
                ))
            })
            .collect();
        let summary = if items.is_empty() {
            "system is up to date".to_string()
        } else {
            format!("{} available", plural(items.len(), "update"))
        };
        let mut report = CheckReport::from_items(self.id(), items, summary);
        match &snap.freshness {
            DbFreshness::Synced => {}
            DbFreshness::SystemDatabases => {
                report.limitation = Some("system databases used without refreshing them".into());
            }
            DbFreshness::SyncFailed(e) => {
                report.status = report.status.max(Status::Warning);
                report.limitation =
                    Some(format!("could not refresh the repository databases: {e}"));
            }
        }
        Ok(report)
    }
}

pub struct Orphans;

impl Check for Orphans {
    fn id(&self) -> CheckId {
        CheckId::Orphans
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let snap = ctx.ports.packages.snapshot()?;
        let items: Vec<Item> = snap
            .packages
            .iter()
            .filter(|p| !p.explicit && !p.required)
            .map(|p| Item::new(Status::Info, &p.name, &p.version))
            .collect();
        let summary = if items.is_empty() {
            "no orphan packages".to_string()
        } else {
            format!("{} no longer required", plural(items.len(), "package"))
        };
        Ok(CheckReport::from_items(self.id(), items, summary))
    }
}

pub struct ForeignPackages;

impl Check for ForeignPackages {
    fn id(&self) -> CheckId {
        CheckId::ForeignPackages
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let snap = ctx.ports.packages.snapshot()?;
        let items: Vec<Item> = snap
            .packages
            .iter()
            .filter(|p| p.foreign)
            .map(|p| Item::new(Status::Info, &p.name, &p.version))
            .collect();
        let summary = format!(
            "{} not in the repositories (AUR or local)",
            plural(items.len(), "package")
        );
        Ok(CheckReport::from_items(self.id(), items, summary))
    }
}

pub struct Pacnew;

impl Check for Pacnew {
    fn id(&self) -> CheckId {
        CheckId::Pacnew
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let snap = ctx.ports.packages.snapshot()?;
        let items: Vec<Item> = snap
            .pacnew_files
            .iter()
            .map(|p| Item::new(Status::Warning, p.display().to_string(), "to be merged"))
            .collect();
        let summary = if items.is_empty() {
            "no .pacnew files to merge".to_string()
        } else {
            format!("{} to merge", plural(items.len(), ".pacnew file"))
        };
        Ok(CheckReport::from_items(self.id(), items, summary))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{Fake, pkg};

    #[test]
    fn updates_orphans_foreign() {
        let mut fake = Fake::default();
        let mut a = pkg("a", "1-1");
        a.repo_update = Some("2-1".into());
        let mut orphan = pkg("dep", "1-1");
        orphan.explicit = false;
        let mut aur = pkg("aurpkg", "3-1");
        aur.foreign = true;
        fake.snapshot.packages = vec![a, orphan, aur];
        fake.snapshot.pacnew_files = vec!["/etc/x.conf.pacnew".into()];
        let (ports, config) = (fake.ports(), Default::default());
        let ctx = Context {
            ports: &ports,
            config: &config,
        };

        let r = RepoUpdates.run(&ctx).unwrap();
        assert_eq!(r.status, Status::Warning);
        assert_eq!(r.items[0].detail, "1-1 → 2-1");

        let r = Orphans.run(&ctx).unwrap();
        assert_eq!(
            r.items
                .iter()
                .map(|i| i.subject.as_str())
                .collect::<Vec<_>>(),
            ["dep"]
        );

        let r = ForeignPackages.run(&ctx).unwrap();
        assert_eq!(r.items.len(), 1);
        assert_eq!(r.status, Status::Info);

        assert_eq!(Pacnew.run(&ctx).unwrap().status, Status::Warning);
    }

    #[test]
    fn failed_sync_is_a_warning() {
        let mut fake = Fake::default();
        fake.snapshot.freshness = DbFreshness::SyncFailed("timeout".into());
        let (ports, config) = (fake.ports(), Default::default());
        let r = RepoUpdates
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();
        assert_eq!(r.status, Status::Warning);
        assert!(r.limitation.unwrap().contains("timeout"));
    }
}
