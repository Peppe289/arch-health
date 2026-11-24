//! Result model and terminal rendering.

use std::fmt;
use std::io::IsTerminal;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Status {
    Ok,
    Info,
    Warn,
    Crit,
    /// The check could not run (missing permission, service unavailable, ...).
    Unknown,
}

impl Status {
    fn icon(self) -> &'static str {
        match self {
            Status::Ok => "✔",
            Status::Info => "ℹ",
            Status::Warn => "⚠",
            Status::Crit => "✘",
            Status::Unknown => "?",
        }
    }

    fn color(self) -> &'static str {
        match self {
            Status::Ok => "\x1b[32m",
            Status::Info => "\x1b[36m",
            Status::Warn => "\x1b[33m",
            Status::Crit => "\x1b[31m",
            Status::Unknown => "\x1b[35m",
        }
    }

    /// Severity used for the process exit code.
    pub fn severity(self) -> u8 {
        match self {
            Status::Ok | Status::Info => 0,
            Status::Warn | Status::Unknown => 1,
            Status::Crit => 2,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CheckResult {
    pub title: String,
    pub status: Status,
    pub summary: String,
    pub details: Vec<String>,
}

impl CheckResult {
    pub fn new(title: impl Into<String>, status: Status, summary: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            status,
            summary: summary.into(),
            details: Vec::new(),
        }
    }

    pub fn with_details(mut self, details: Vec<String>) -> Self {
        self.details = details;
        self
    }

    pub fn error(title: impl Into<String>, err: impl fmt::Display) -> Self {
        Self::new(title, Status::Unknown, format!("controllo non eseguito: {err:#}"))
    }
}

pub struct Renderer {
    color: bool,
    verbose: bool,
}

const MAX_DETAILS: usize = 10;

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

    pub fn section(&self, name: &str) {
        println!("\n{}", self.paint("\x1b[1m", &format!("== {name} ==")));
    }

    pub fn result(&self, r: &CheckResult) {
        let icon = self.paint(r.status.color(), r.status.icon());
        println!(" {icon} {:<22} {}", r.title, r.summary);
        let limit = if self.verbose { usize::MAX } else { MAX_DETAILS };
        for d in r.details.iter().take(limit) {
            println!("     {}", self.paint("\x1b[2m", d));
        }
        if r.details.len() > limit {
            let more = format!("… e altri {} (usa --verbose)", r.details.len() - limit);
            println!("     {}", self.paint("\x1b[2m", &more));
        }
    }

    pub fn footer(&self, worst: Status) {
        let msg = match worst {
            Status::Ok | Status::Info => "Sistema in salute",
            Status::Warn | Status::Unknown => "Ci sono avvisi da controllare",
            Status::Crit => "Ci sono problemi critici",
        };
        println!("\n{} {}", self.paint(worst.color(), worst.icon()), self.paint("\x1b[1m", msg));
    }
}
