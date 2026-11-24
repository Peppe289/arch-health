mod checks;
mod net;
mod pacman_conf;
mod report;

use std::process::ExitCode;

use clap::Parser;

use checks::{Ctx, SECTIONS};
use report::{Renderer, Status};

/// Stato di salute di un sistema Arch Linux.
///
/// Exit code: 0 = ok, 1 = avvisi, 2 = problemi critici.
#[derive(Parser)]
#[command(version, about)]
struct Args {
    /// Non aggiornare i database dei repo (usa quelli di sistema)
    #[arg(long)]
    no_sync: bool,

    /// Nessun accesso alla rete (implica --no-sync, salta il controllo AUR)
    #[arg(long)]
    offline: bool,

    /// Mostra tutti i dettagli
    #[arg(short, long)]
    verbose: bool,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let ctx = Ctx {
        no_sync: args.no_sync,
        offline: args.offline,
        is_root: rustix::process::geteuid().is_root(),
    };
    let out = Renderer::new(args.verbose);

    let mut worst = Status::Ok;
    for section in SECTIONS {
        out.section(section.name);
        for r in (section.run)(&ctx) {
            worst = worst.max(r.status);
            out.result(&r);
        }
    }
    out.footer(worst);
    ExitCode::from(worst.severity())
}
