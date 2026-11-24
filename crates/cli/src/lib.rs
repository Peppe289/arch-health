//! Command line definition of arch-health. Exposed as a library so other
//! frontends (the GUI's "Command line" section) can list the commands.

use clap::{CommandFactory, Parser};

/// Health report for an Arch Linux installation.
#[derive(Debug, Parser)]
#[command(
    name = "arch-health",
    version,
    about,
    after_help = "Exit status: 0 healthy, 1 warnings, 2 critical problems.\n\
                  Some checks are more complete as root: run with sudo for a full report."
)]
pub struct Args {
    /// Do not refresh the repository databases (use the system ones)
    #[arg(long)]
    pub no_sync: bool,

    /// No network access at all: no database refresh, AUR, vulnerabilities or news
    #[arg(long)]
    pub offline: bool,

    /// Run only these checks (comma separated keys, see --list-checks)
    #[arg(long, value_delimiter = ',', value_name = "KEYS")]
    pub only: Vec<String>,

    /// List the available checks and exit
    #[arg(long)]
    pub list_checks: bool,

    /// Show every item instead of the first ones
    #[arg(short, long)]
    pub verbose: bool,
}

/// The clap command, for help rendering in other frontends.
pub fn command() -> clap::Command {
    Args::command()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_definition_is_valid() {
        command().debug_assert();
    }

    #[test]
    fn parses_only_list() {
        let a = Args::parse_from(["arch-health", "--only", "kernel,disk-space"]);
        assert_eq!(a.only, ["kernel", "disk-space"]);
    }
}
