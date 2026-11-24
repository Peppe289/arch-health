//! "Sicurezza e news" section: Arch security tracker + Arch news feed.

use crate::checks::{Ctx, news, security};
use crate::pacman_conf::PacmanConf;
use crate::report::{CheckResult, Status};

pub fn run(ctx: &Ctx) -> Vec<CheckResult> {
    if ctx.offline {
        return vec![CheckResult::new("Vulnerabilità e news", Status::Info, "saltato (--offline)")];
    }
    match PacmanConf::load_default() {
        Ok(conf) => vec![security::check(&conf), news::check(&conf)],
        Err(e) => vec![CheckResult::error("Vulnerabilità e news", e)],
    }
}
