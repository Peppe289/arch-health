//! Runs checks in parallel, one thread each (ADR-0006).

use std::thread;

use crate::checks::{Check, Context, Privilege};
use crate::model::{CheckId, CheckReport, Report, Status};

#[derive(Debug, Clone)]
pub enum Event {
    Started(CheckId),
    Finished(CheckReport),
}

/// Runs `checks` and returns their reports in the given order. `on_event`
/// is called from worker threads as checks start and finish.
pub fn run<F>(checks: &[Box<dyn Check>], ctx: &Context<'_>, on_event: F) -> Report
where
    F: Fn(Event) + Sync,
{
    let reports: Vec<CheckReport> = thread::scope(|s| {
        let handles: Vec<_> = checks
            .iter()
            .map(|check| {
                let on_event = &on_event;
                s.spawn(move || {
                    let id = check.id();
                    on_event(Event::Started(id));
                    let report = run_one(check.as_ref(), ctx);
                    on_event(Event::Finished(report.clone()));
                    report
                })
            })
            .collect();
        handles
            .into_iter()
            .zip(checks)
            .map(|(h, check)| {
                h.join().unwrap_or_else(|_| {
                    CheckReport::new(
                        check.id(),
                        Status::Unknown,
                        "check crashed (internal error)",
                    )
                })
            })
            .collect()
    });
    // Root helps only checks that see more as root and reported a limit.
    let root_would_help = !ctx.ports.system.is_root()
        && checks
            .iter()
            .zip(&reports)
            .any(|(c, r)| c.privilege() == Privilege::Enhanced && r.limitation.is_some());
    Report {
        reports,
        root_would_help,
    }
}

fn run_one(check: &dyn Check, ctx: &Context<'_>) -> CheckReport {
    if check.needs_network() && ctx.config.offline {
        return CheckReport::new(check.id(), Status::Skipped, "skipped (offline)");
    }
    check.run(ctx).unwrap_or_else(|e| {
        CheckReport::new(check.id(), Status::Unknown, format!("could not run: {e}"))
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::Config;
    use crate::checks::all_checks;
    use crate::fake::Fake;

    #[test]
    fn runs_all_checks_in_order_and_emits_events() {
        let fake = Fake::default();
        let ports = fake.ports();
        let config = Config {
            offline: true,
            ..Config::default()
        };
        let ctx = Context {
            ports: &ports,
            config: &config,
        };
        let checks = all_checks();
        let events = Mutex::new(0usize);

        let report = run(&checks, &ctx, |_| *events.lock().unwrap() += 1);

        let ids: Vec<_> = report.reports.iter().map(|r| r.id).collect();
        let expected: Vec<_> = checks.iter().map(|c| c.id()).collect();
        assert_eq!(ids, expected);
        assert_eq!(*events.lock().unwrap(), checks.len() * 2);
        let aur = report
            .reports
            .iter()
            .find(|r| r.id == CheckId::AurUpdates)
            .unwrap();
        assert_eq!(aur.status, Status::Skipped);
        assert!(!report.root_would_help);
    }

    #[test]
    fn root_would_help_only_for_enhanced_checks() {
        // Journal not readable → Enhanced checks report a limitation.
        let mut fake = Fake::default();
        fake.journal_readable = false;
        let ports = fake.ports();
        let config = Config {
            offline: true,
            ..Config::default()
        };
        let ctx = Context {
            ports: &ports,
            config: &config,
        };
        assert!(run(&all_checks(), &ctx, |_| {}).root_would_help);

        // Already root: nothing to suggest.
        let mut fake = Fake::default();
        fake.journal_readable = false;
        fake.root = true;
        let ports = fake.ports();
        let ctx = Context {
            ports: &ports,
            config: &config,
        };
        assert!(!run(&all_checks(), &ctx, |_| {}).root_would_help);
    }
}
