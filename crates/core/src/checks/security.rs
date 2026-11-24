//! Installed packages matched against the Arch security tracker, with the
//! same logic as `arch-audit`.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use super::{Check, Context};
use crate::error::PortResult;
use crate::model::{CheckId, CheckReport, Item, Status, plural};
use crate::ports::{Advisory, PackageDb};

const MAX_CVES_SHOWN: usize = 3;

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
            Self::Critical => "critical",
            Self::High => "high",
            Self::Medium => "medium",
            Self::Low => "low",
            Self::Unknown => "unknown",
        }
    }

    /// Fixable issues must be acted on; unfixed tracker entries are often
    /// stale, so they only inform (except critical ones).
    fn status(self, fixable: bool) -> Status {
        match (fixable, self) {
            (true, Self::Critical | Self::High) => Status::Critical,
            (true, _) => Status::Warning,
            (false, Self::Critical) => Status::Warning,
            (false, _) => Status::Info,
        }
    }
}

#[derive(Debug)]
struct Finding {
    version: String,
    severity: Severity,
    /// Highest fixed version among the advisories that have a fix.
    fix: Option<String>,
    avgs: Vec<String>,
    cves: Vec<String>,
}

/// `installed`: name → version.
fn audit(
    advisories: &[Advisory],
    installed: &BTreeMap<&str, &str>,
    db: &dyn PackageDb,
) -> BTreeMap<String, Finding> {
    let mut out: BTreeMap<String, Finding> = BTreeMap::new();
    for avg in advisories.iter().filter(|a| a.status != "Not affected") {
        for pkg in &avg.packages {
            let Some(version) = installed.get(pkg.as_str()) else {
                continue;
            };
            if let Some(fixed) = &avg.fixed
                && db.compare_versions(version, fixed) != Ordering::Less
            {
                continue;
            }
            let f = out.entry(pkg.clone()).or_insert_with(|| Finding {
                version: version.to_string(),
                severity: Severity::Unknown,
                fix: None,
                avgs: vec![],
                cves: vec![],
            });
            f.severity = f.severity.max(Severity::parse(&avg.severity));
            f.avgs.push(avg.name.clone());
            f.cves.extend(avg.issues.iter().cloned());
            if let Some(fixed) = &avg.fixed {
                let newer = f
                    .fix
                    .as_ref()
                    .is_none_or(|cur| db.compare_versions(fixed, cur) == Ordering::Greater);
                if newer {
                    f.fix = Some(fixed.clone());
                }
            }
        }
    }
    out
}

pub struct Vulnerabilities;

impl Check for Vulnerabilities {
    fn id(&self) -> CheckId {
        CheckId::Vulnerabilities
    }

    fn needs_network(&self) -> bool {
        true
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let advisories = ctx.ports.security.advisories()?;
        let snap = ctx.ports.packages.snapshot()?;
        let installed: BTreeMap<&str, &str> = snap
            .packages
            .iter()
            .map(|p| (p.name.as_str(), p.version.as_str()))
            .collect();
        let findings = audit(&advisories, &installed, ctx.ports.packages.as_ref());

        let fixable = findings.values().filter(|f| f.fix.is_some()).count();
        let mut items: Vec<(Severity, bool, Item)> = findings
            .into_iter()
            .map(|(name, mut f)| {
                // CVEs come from several advisories: drop repeats, keep order.
                let mut seen = std::collections::HashSet::new();
                f.cves.retain(|c| seen.insert(c.clone()));
                let more = f.cves.len().saturating_sub(MAX_CVES_SHOWN);
                let mut ids = f
                    .cves
                    .iter()
                    .take(MAX_CVES_SHOWN)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                if more > 0 {
                    ids.push_str(&format!(" +{more}"));
                }
                let action = match &f.fix {
                    Some(fix) => format!("update to {fix}"),
                    None => "no fix available".into(),
                };
                let detail = format!(
                    "{} {}: {action} ({}; {ids})",
                    f.version,
                    f.severity.label(),
                    f.avgs.join(", ")
                );
                let can_fix = f.fix.is_some();
                (
                    f.severity,
                    can_fix,
                    Item::new(f.severity.status(can_fix), name, detail),
                )
            })
            .collect();
        // Most urgent first: fixable before unfixable, then by severity.
        items.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
        let items: Vec<Item> = items.into_iter().map(|i| i.2).collect();

        let summary = if items.is_empty() {
            "no installed package with known vulnerabilities".to_string()
        } else {
            format!(
                "{}, {fixable} fixable by updating",
                plural(items.len(), "vulnerable package")
            )
        };
        Ok(CheckReport::from_items(self.id(), items, summary))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fake::{Fake, pkg};

    fn avg(name: &str, pkg: &str, status: &str, sev: &str, fixed: Option<&str>) -> Advisory {
        Advisory {
            name: name.into(),
            packages: vec![pkg.into()],
            status: status.into(),
            severity: sev.into(),
            fixed: fixed.map(Into::into),
            issues: vec![format!("CVE-{name}")],
        }
    }

    #[test]
    fn repeated_cves_across_advisories_are_listed_once() {
        let mut fake = Fake::default();
        fake.snapshot.packages = vec![pkg("openssl", "3.1-1")];
        let mut a = avg("1", "openssl", "Fixed", "High", Some("3.2-1"));
        a.issues = vec!["CVE-A".into(), "CVE-B".into()];
        let mut b = avg("2", "openssl", "Fixed", "High", Some("3.2-1"));
        b.issues = vec!["CVE-C".into(), "CVE-A".into()];
        fake.advisories = vec![a, b];
        let (ports, config) = (fake.ports(), Default::default());
        let r = Vulnerabilities
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();
        assert!(
            r.items[0].detail.ends_with("CVE-A, CVE-B, CVE-C)"),
            "{}",
            r.items[0].detail
        );
    }

    #[test]
    fn audit_logic() {
        let mut fake = Fake::default();
        fake.snapshot.packages = vec![
            pkg("openssl", "3.1-1"),
            pkg("curl", "8.1-1"),
            pkg("vim", "9.1-1"),
            pkg("foo", "1-1"),
        ];
        fake.advisories = vec![
            avg("1", "openssl", "Fixed", "High", Some("3.2-1")), // older than fix → vulnerable
            avg("2", "curl", "Fixed", "Critical", Some("8.0-1")), // newer than fix → ok
            avg("3", "vim", "Vulnerable", "Medium", None),       // no fix → info
            avg("4", "foo", "Not affected", "High", None),       // ignored
            avg("5", "absent", "Vulnerable", "High", None),      // not installed
        ];
        let (ports, config) = (fake.ports(), Default::default());
        let r = Vulnerabilities
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();

        let subjects: Vec<_> = r.items.iter().map(|i| i.subject.as_str()).collect();
        assert_eq!(subjects, ["openssl", "vim"]);
        assert_eq!(r.items[0].status, Status::Critical);
        assert!(r.items[0].detail.contains("update to 3.2-1"));
        assert_eq!(r.items[1].status, Status::Info);
        assert_eq!(r.status, Status::Critical);
    }
}
