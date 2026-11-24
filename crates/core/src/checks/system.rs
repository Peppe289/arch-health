use super::{Check, Context, Privilege, needs_root_note};
use crate::error::PortResult;
use crate::model::{CheckId, CheckReport, Item, Status, plural};

/// The running kernel must still be installed, otherwise modules cannot be
/// loaded and a reboot is needed.
pub struct Kernel;

impl Check for Kernel {
    fn id(&self) -> CheckId {
        CheckId::Kernel
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let running = ctx.ports.system.running_kernel();
        let installed = ctx.ports.system.installed_kernels();
        Ok(if installed.contains(&running) {
            CheckReport::new(self.id(), Status::Ok, format!("running {running}"))
        } else {
            let items = installed
                .iter()
                .map(|k| Item::new(Status::Info, k, "installed"))
                .collect();
            CheckReport::new(
                self.id(),
                Status::Warning,
                format!("{running} is no longer installed: reboot needed"),
            )
            .with_items(items)
        })
    }
}

/// Processes still mapping shared libraries replaced by an update.
pub struct StaleLibraries;

impl Check for StaleLibraries {
    fn id(&self) -> CheckId {
        CheckId::StaleLibraries
    }

    fn privilege(&self) -> Privilege {
        Privilege::Enhanced
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let stale = ctx.ports.system.stale_libraries()?;
        let items: Vec<Item> = stale
            .processes
            .iter()
            .map(|(name, n)| {
                let detail = if *n == 1 {
                    "1 process".to_string()
                } else {
                    format!("{n} processes")
                };
                Item::new(Status::Warning, name, detail)
            })
            .collect();
        let summary = if items.is_empty() {
            "no process uses replaced libraries".to_string()
        } else {
            format!("{} to restart", plural(items.len(), "program"))
        };
        let limitation = (!stale.complete)
            .then(|| needs_root_note("only the current user's processes were inspected"));
        Ok(CheckReport::from_items(self.id(), items, summary).with_limitation(limitation))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::Fake;

    #[test]
    fn kernel_removed_needs_reboot() {
        let mut fake = Fake::default();
        fake.running_kernel = "6.1-arch1".into();
        fake.installed_kernels = vec!["6.2-arch1".into()];
        let (ports, config) = (fake.ports(), Default::default());
        let r = Kernel
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();
        assert_eq!(r.status, Status::Warning);
    }

    #[test]
    fn incomplete_scan_has_limitation() {
        let mut fake = Fake::default();
        fake.stale.complete = false;
        fake.stale.processes.insert("firefox".into(), 3);
        let (ports, config) = (fake.ports(), Default::default());
        let r = StaleLibraries
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();
        assert_eq!(r.status, Status::Warning);
        assert_eq!(r.items[0].detail, "3 processes");
        assert!(r.limitation.is_some());
    }
}
