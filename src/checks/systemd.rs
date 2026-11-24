//! Failed units through systemd's D-Bus API (system and user manager).

use anyhow::Result;
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::OwnedObjectPath;

use crate::checks::Ctx;
use crate::report::{CheckResult, Status};

/// Signature `(ssssssouso)` returned by `Manager.ListUnits`.
type UnitInfo = (
    String,          // name
    String,          // description
    String,          // load state
    String,          // active state
    String,          // sub state
    String,          // following
    OwnedObjectPath, // unit path
    u32,             // job id
    String,          // job type
    OwnedObjectPath, // job path
);

fn manager(conn: &Connection) -> Result<Proxy<'_>> {
    Ok(Proxy::new(
        conn,
        "org.freedesktop.systemd1",
        "/org/freedesktop/systemd1",
        "org.freedesktop.systemd1.Manager",
    )?)
}

fn check(title: &str, conn: Result<Connection, zbus::Error>) -> CheckResult {
    let res = (|| -> Result<CheckResult> {
        let conn = conn?;
        let mgr = manager(&conn)?;
        let state: String = mgr.get_property("SystemState")?;
        let units: Vec<UnitInfo> = mgr.call("ListUnits", &())?;
        let failed: Vec<String> = units
            .into_iter()
            .filter(|u| u.3 == "failed")
            .map(|u| format!("{} — {}", u.0, u.1))
            .collect();
        let status = if !failed.is_empty() || state == "degraded" {
            Status::Crit
        } else if state == "running" {
            Status::Ok
        } else {
            // starting, stopping, maintenance, ...
            Status::Warn
        };
        let summary = if failed.is_empty() {
            format!("stato {state}, nessuna unità fallita")
        } else {
            format!("stato {state}, {} unità fallite", failed.len())
        };
        Ok(CheckResult::new(title, status, summary).with_details(failed))
    })();
    res.unwrap_or_else(|e| CheckResult::error(title, e))
}

pub fn run(ctx: &Ctx) -> Vec<CheckResult> {
    let mut out = vec![check("Servizi di sistema", Connection::system())];
    if !ctx.is_root {
        out.push(check("Servizi utente", Connection::session()));
    }
    out
}
