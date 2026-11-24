pub mod pacman;
pub mod reboot;
pub mod storage;
pub mod systemd;

use crate::report::CheckResult;

/// Options shared by all checks.
pub struct Ctx {
    pub no_sync: bool,
    pub is_root: bool,
}

pub struct Section {
    pub name: &'static str,
    pub run: fn(&Ctx) -> Vec<CheckResult>,
}

pub const SECTIONS: &[Section] = &[
    Section { name: "Pacchetti", run: pacman::run },
    Section { name: "Riavvio", run: reboot::run },
    Section { name: "Servizi", run: systemd::run },
    Section { name: "Dischi", run: storage::run },
];
