//! Checks: pure rules over the data returned by the ports.

mod aur;
mod journal;
mod news;
mod packages;
mod security;
mod services;
mod storage;
mod system;

use crate::config::Config;
use crate::error::PortResult;
use crate::model::{CheckId, CheckReport};
use crate::ports::Ports;

/// What a check needs to be complete (ADR-0005).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privilege {
    /// Complete as a normal user.
    User,
    /// Works as a user, but sees more as root.
    Enhanced,
}

pub struct Context<'a> {
    pub ports: &'a Ports,
    pub config: &'a Config,
}

pub trait Check: Send + Sync {
    fn id(&self) -> CheckId;

    fn needs_network(&self) -> bool {
        false
    }

    fn privilege(&self) -> Privilege {
        Privilege::User
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport>;
}

/// Every available check, in display order.
pub fn all_checks() -> Vec<Box<dyn Check>> {
    vec![
        Box::new(packages::RepoUpdates),
        Box::new(aur::AurUpdates),
        Box::new(packages::Orphans),
        Box::new(packages::ForeignPackages),
        Box::new(packages::Pacnew),
        Box::new(security::Vulnerabilities),
        Box::new(news::ArchNews),
        Box::new(system::Kernel),
        Box::new(system::StaleLibraries),
        Box::new(services::Services::System),
        Box::new(services::Services::User),
        Box::new(storage::DiskSpace),
        Box::new(storage::BtrfsErrors),
        Box::new(storage::DriveHealth),
        Box::new(journal::JournalErrors),
        Box::new(journal::Coredumps),
    ]
}

/// Limitation note for checks that see more as root.
pub(crate) fn needs_root_note(what: &str) -> String {
    format!("{what}; run as root for a complete result")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_check_id_has_exactly_one_check() {
        let mut ids: Vec<_> = all_checks().iter().map(|c| c.id()).collect();
        ids.sort();
        let mut all = CheckId::ALL.to_vec();
        all.sort();
        assert_eq!(ids, all);
    }
}
