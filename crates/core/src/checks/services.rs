use super::{Check, Context};
use crate::error::PortResult;
use crate::model::{CheckId, CheckReport, Item, Status, TimedEvent, plural};
use crate::ports::ManagerScope;

/// Failed units of the system or user service manager.
pub enum Services {
    System,
    User,
}

impl Check for Services {
    fn id(&self) -> CheckId {
        match self {
            Services::System => CheckId::SystemServices,
            Services::User => CheckId::UserServices,
        }
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let scope = match self {
            Services::System => ManagerScope::System,
            Services::User => {
                if ctx.ports.system.is_root() {
                    return Ok(CheckReport::new(
                        self.id(),
                        Status::Skipped,
                        "not applicable as root",
                    ));
                }
                ManagerScope::User
            }
        };
        let st = ctx.ports.services.state(scope)?;
        let items: Vec<Item> = st
            .failed
            .iter()
            .map(|u| Item::new(Status::Critical, &u.name, &u.description))
            .collect();
        let status = match (items.is_empty(), st.state.as_str()) {
            (false, _) | (_, "degraded") => Status::Critical,
            (true, "running") => Status::Ok,
            // starting, stopping, maintenance, ...
            _ => Status::Warning,
        };
        let summary = if items.is_empty() {
            format!("{}, no failed units", st.state)
        } else {
            format!("{}, {} failed", st.state, plural(items.len(), "unit"))
        };
        let events = st
            .failed
            .iter()
            .filter_map(|u| {
                Some(TimedEvent {
                    time: u.since?,
                    status: Status::Critical,
                    source: u.name.clone(),
                    message: format!("failed: {}", u.description),
                })
            })
            .collect();
        Ok(CheckReport::new(self.id(), status, summary)
            .with_items(items)
            .with_events(events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::Fake;
    use crate::ports::FailedUnit;

    #[test]
    fn failed_unit_is_critical() {
        let mut fake = Fake::default();
        fake.manager.state = "degraded".into();
        fake.manager.failed = vec![FailedUnit {
            name: "foo.service".into(),
            description: "Foo".into(),
            since: Some(std::time::SystemTime::now()),
        }];
        let (ports, config) = (fake.ports(), Default::default());
        let r = Services::System
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();
        assert_eq!(r.status, Status::Critical);
        assert_eq!(r.summary, "degraded, 1 unit failed");
        assert_eq!(r.events[0].source, "foo.service");
    }

    #[test]
    fn user_services_skipped_as_root() {
        let mut fake = Fake::default();
        fake.root = true;
        let (ports, config) = (fake.ports(), Default::default());
        let r = Services::User
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();
        assert_eq!(r.status, Status::Skipped);
    }
}
