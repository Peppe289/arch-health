//! Known vulnerabilities: installed packages (libalpm local db) matched
//! against the Arch Linux security tracker (security.archlinux.org), with
//! the same logic as `arch-audit`.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use alpm::Alpm;
use anyhow::{Context, Result};
use serde::Deserialize;

use crate::net;
use crate::pacman_conf::PacmanConf;
use crate::report::{CheckResult, Status};

const TRACKER_URL: &str = "https://security.archlinux.org/issues/all.json";
const MAX_CVES_SHOWN: usize = 3;

#[derive(Deserialize)]
pub struct Avg {
    pub name: String,
    pub packages: Vec<String>,
    pub status: String,
    pub severity: String,
    pub fixed: Option<String>,
    #[serde(default)]
    pub issues: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Severity {
    Unknown,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    fn parse(s: &str) -> Self {
        match s {
            "Critical" => Self::Critical,
            "High" => Self::High,
            "Medium" => Self::Medium,
            "Low" => Self::Low,
            _ => Self::Unknown,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Critical => "Critical",
            Self::High => "High",
            Self::Medium => "Medium",
            Self::Low => "Low",
            Self::Unknown => "Unknown",
        }
    }

    fn status(self, fixable: bool) -> Status {
        match (fixable, self) {
            (true, Self::Critical | Self::High) => Status::Crit,
            (true, _) => Status::Warn,
            (false, Self::Critical) => Status::Warn,
            (false, _) => Status::Info,
        }
    }
}

/// One vulnerable installed package, possibly affected by several AVGs.
struct Finding {
    version: String,
    severity: Severity,
    /// Highest fixed version among the AVGs that have a fix.
    fix: Option<String>,
    avgs: Vec<String>,
    cves: Vec<String>,
}

/// `installed`: name → version. Returns findings keyed by package name.
fn audit(avgs: &[Avg], installed: &BTreeMap<String, String>) -> BTreeMap<String, Finding> {
    let mut out: BTreeMap<String, Finding> = BTreeMap::new();
    for avg in avgs.iter().filter(|a| a.status != "Not affected") {
        for pkg in &avg.packages {
            let Some(version) = installed.get(pkg) else { continue };
            if let Some(fixed) = &avg.fixed
                && alpm::vercmp(version.as_str(), fixed.as_str()) != Ordering::Less
            {
                continue;
            }
            let sev = Severity::parse(&avg.severity);
            let f = out.entry(pkg.clone()).or_insert_with(|| Finding {
                version: version.clone(),
                severity: Severity::Unknown,
                fix: None,
                avgs: vec![],
                cves: vec![],
            });
            f.severity = f.severity.max(sev);
            f.avgs.push(avg.name.clone());
            f.cves.extend(avg.issues.iter().cloned());
            if let Some(fixed) = &avg.fixed {
                let newer = f
                    .fix
                    .as_ref()
                    .is_none_or(|cur| alpm::vercmp(fixed.as_str(), cur.as_str()) == Ordering::Greater);
                if newer {
                    f.fix = Some(fixed.clone());
                }
            }
        }
    }
    out
}

pub fn check(conf: &PacmanConf) -> CheckResult {
    check_inner(conf).unwrap_or_else(|e| CheckResult::error("Vulnerabilità", e))
}

fn check_inner(conf: &PacmanConf) -> Result<CheckResult> {
    let avgs: Vec<Avg> = net::agent()
        .get(TRACKER_URL)
        .call()
        .context("download dal security tracker")?
        .body_mut()
        .with_config()
        .limit(32 * 1024 * 1024)
        .read_json()
        .context("risposta del security tracker non valida")?;

    let handle = Alpm::new(
        conf.root_dir.to_string_lossy().into_owned(),
        conf.db_path.to_string_lossy().into_owned(),
    )
    .context("inizializzazione di libalpm")?;
    let installed: BTreeMap<String, String> = handle
        .localdb()
        .pkgs()
        .iter()
        .map(|p| (p.name().to_string(), p.version().to_string()))
        .collect();

    let findings = audit(&avgs, &installed);
    let mut worst = Status::Ok;
    let mut fixable = 0;
    let mut rows: Vec<(Severity, bool, String)> = findings
        .iter()
        .map(|(name, f)| {
            let can_fix = f.fix.is_some();
            fixable += can_fix as usize;
            worst = worst.max(f.severity.status(can_fix));
            let action = match &f.fix {
                Some(fix) => format!("aggiorna a {fix}"),
                None => "nessuna correzione disponibile".into(),
            };
            let mut cves: Vec<&str> = f.cves.iter().map(String::as_str).collect();
            cves.dedup();
            let more = cves.len().saturating_sub(MAX_CVES_SHOWN);
            let mut ids = cves.into_iter().take(MAX_CVES_SHOWN).collect::<Vec<_>>().join(", ");
            if more > 0 {
                ids.push_str(&format!(" +{more}"));
            }
            let line = format!(
                "[{}] {name} {}: {action} ({}; {ids})",
                f.severity.label(),
                f.version,
                f.avgs.join(", ")
            );
            (f.severity, can_fix, line)
        })
        .collect();
    // Most urgent first: fixable before unfixable, then by severity.
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));

    let summary = if findings.is_empty() {
        "nessun pacchetto installato con vulnerabilità note".to_string()
    } else {
        format!("{} pacchetti vulnerabili, {fixable} risolvibili aggiornando", findings.len())
    };
    Ok(CheckResult::new("Vulnerabilità", worst, summary)
        .with_details(rows.into_iter().map(|r| r.2).collect()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn avg(name: &str, pkg: &str, status: &str, sev: &str, fixed: Option<&str>) -> Avg {
        Avg {
            name: name.into(),
            packages: vec![pkg.into()],
            status: status.into(),
            severity: sev.into(),
            fixed: fixed.map(Into::into),
            issues: vec![format!("CVE-{name}")],
        }
    }

    #[test]
    fn audit_logic() {
        let avgs = vec![
            avg("1", "openssl", "Fixed", "High", Some("3.2-1")),   // installed older → vulnerable
            avg("2", "curl", "Fixed", "Critical", Some("8.0-1")),   // installed newer → ok
            avg("3", "vim", "Vulnerable", "Medium", None),          // no fix → vulnerable
            avg("4", "foo", "Not affected", "High", None),          // ignored
            avg("5", "absent", "Vulnerable", "High", None),         // not installed
        ];
        let installed: BTreeMap<_, _> = [("openssl", "3.1-1"), ("curl", "8.1-1"), ("vim", "9.1-1"), ("foo", "1-1")]
            .into_iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect();
        let f = audit(&avgs, &installed);
        assert_eq!(f.keys().collect::<Vec<_>>(), ["openssl", "vim"]);
        assert_eq!(f["openssl"].fix.as_deref(), Some("3.2-1"));
        assert_eq!(f["openssl"].severity.status(true), Status::Crit);
        assert_eq!(f["vim"].severity.status(false), Status::Info);
    }
}
