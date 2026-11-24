mod render;

use std::process::ExitCode;

use anyhow::bail;
use arch_health_adapters::{Options, system_ports};
use arch_health_cli::Args;
use arch_health_core::checks::{Context, Privilege, all_checks};
use arch_health_core::{Category, CheckId, Config, run};
use clap::Parser;

use render::Renderer;

fn main() -> ExitCode {
    match real_main() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("arch-health: {e:#}");
            ExitCode::from(3)
        }
    }
}

fn real_main() -> anyhow::Result<ExitCode> {
    let args = Args::parse();

    if args.list_checks {
        for id in CheckId::ALL {
            println!(
                "{:<18} {:<18} {}",
                id.key(),
                id.category().title(),
                id.title()
            );
        }
        return Ok(ExitCode::SUCCESS);
    }

    let mut checks = all_checks();
    if !args.only.is_empty() {
        for key in &args.only {
            if CheckId::from_key(key).is_none() {
                bail!("unknown check `{key}` (see --list-checks)");
            }
        }
        checks.retain(|c| args.only.iter().any(|k| k == c.id().key()));
    }

    let ports = system_ports(Options {
        sync_databases: !args.no_sync && !args.offline,
    });
    let config = Config {
        offline: args.offline,
        ..Config::default()
    };
    let ctx = Context {
        ports: &ports,
        config: &config,
    };
    let report = run(&checks, &ctx, |_| {});

    let out = Renderer::new(args.verbose);
    for category in Category::ALL {
        let mut reports = report.by_category(category).peekable();
        if reports.peek().is_none() {
            continue;
        }
        out.section(category.title());
        reports.for_each(|r| out.report(r));
    }

    let is_root = ports.system.is_root();
    let limited = !is_root
        && checks
            .iter()
            .filter(|c| c.privilege() == Privilege::Enhanced)
            .any(|c| {
                report
                    .reports
                    .iter()
                    .any(|r| r.id == c.id() && r.limitation.is_some())
            });
    let overall = report.overall();
    out.footer(overall, limited);
    Ok(ExitCode::from(overall.exit_code()))
}
