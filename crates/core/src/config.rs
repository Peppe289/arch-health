//! Run configuration. Defaults only for now; loading from a file comes
//! with the configuration requirement.

#[derive(Debug, Clone)]
pub struct Config {
    /// Do not run checks that need the network.
    pub offline: bool,
    /// Used space (%) that makes a filesystem a warning / critical.
    pub space_warning_pct: f64,
    pub space_critical_pct: f64,
    /// How far back coredumps are reported.
    pub coredump_window_days: u64,
    /// News window when the pacman log has no full upgrade.
    pub news_fallback_days: i64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            offline: false,
            space_warning_pct: 85.0,
            space_critical_pct: 95.0,
            coredump_window_days: 7,
            news_fallback_days: 30,
        }
    }
}
