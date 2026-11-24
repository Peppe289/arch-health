//! Terminal rendering of a report.

use std::io::IsTerminal;

use arch_health_core::{CheckReport, Status};

const MAX_ITEMS: usize = 10;

pub struct Renderer {
    color: bool,
    verbose: bool,
}

fn color(s: Status) -> &'static str {
    match s {
        Status::Ok => "\x1b[32m",
        Status::Skipped => "\x1b[2m",
        Status::Info => "\x1b[36m",
        Status::Warning => "\x1b[33m",
        Status::Unknown => "\x1b[35m",
        Status::Critical => "\x1b[31m",
    }
}

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";

impl Renderer {
    pub fn new(verbose: bool) -> Self {
        let color = std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none();
        Self { color, verbose }
    }

    fn paint(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("{code}{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }

    pub fn section(&self, title: &str) {
        println!("\n{}", self.paint(BOLD, &format!("== {title} ==")));
    }

    pub fn report(&self, r: &CheckReport) {
        println!(
            " {} {:<20} {}",
            self.paint(color(r.status), r.status.symbol()),
            r.id.title(),
            r.summary
        );
        if let Some(l) = &r.limitation {
            println!("     {}", self.paint(DIM, &format!("(!) {l}")));
        }
        let limit = if self.verbose { usize::MAX } else { MAX_ITEMS };
        for i in r.items.iter().take(limit) {
            let mark = if i.status > Status::Info {
                self.paint(color(i.status), i.status.symbol())
            } else {
                " ".into()
            };
            let text = if i.detail.is_empty() {
                i.subject.clone()
            } else {
                format!("{}: {}", i.subject, i.detail)
            };
            println!("   {mark} {}", self.paint(DIM, &text));
        }
        if r.items.len() > limit {
            let more = format!("… and {} more (use --verbose)", r.items.len() - limit);
            println!("     {}", self.paint(DIM, &more));
        }
    }

    pub fn footer(&self, overall: Status, limited: bool) {
        let msg = overall.headline();
        println!(
            "\n{} {}",
            self.paint(color(overall), overall.symbol()),
            self.paint(BOLD, msg)
        );
        if limited {
            println!("{}", self.paint(DIM, "Some checks are incomplete without root: run `sudo arch-health` for a full report."));
        }
    }
}
