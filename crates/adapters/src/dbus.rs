//! D-Bus adapters: systemd (failed units) and UDisks2 (drive health).

use std::collections::HashMap;

use arch_health_core::ports::{
    Drive, DriveKind, FailedUnit, ManagerScope, ManagerState, ServiceManager,
};
use arch_health_core::{PortError, PortResult};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

fn bus_err(what: &str) -> impl Fn(zbus::Error) -> PortError + '_ {
    move |e| PortError::unavailable(format!("{what}: {e}"))
}

// ------------------------------------------------------------------ systemd

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

pub struct SystemdManager;

impl ServiceManager for SystemdManager {
    fn state(&self, scope: ManagerScope) -> PortResult<ManagerState> {
        let conn = match scope {
            ManagerScope::System => Connection::system(),
            ManagerScope::User => Connection::session(),
        }
        .map_err(bus_err("connecting to D-Bus"))?;
        let mgr = Proxy::new(
            &conn,
            "org.freedesktop.systemd1",
            "/org/freedesktop/systemd1",
            "org.freedesktop.systemd1.Manager",
        )
        .map_err(bus_err("systemd manager"))?;
        let state: String = mgr
            .get_property("SystemState")
            .map_err(bus_err("SystemState"))?;
        let units: Vec<UnitInfo> = mgr.call("ListUnits", &()).map_err(bus_err("ListUnits"))?;
        let failed = units
            .into_iter()
            .filter(|u| u.3 == "failed")
            .map(|u| FailedUnit {
                name: u.0,
                description: u.1,
            })
            .collect();
        Ok(ManagerState { state, failed })
    }
}

// ------------------------------------------------------------------ UDisks2

type Props = HashMap<String, OwnedValue>;
type ManagedObjects = HashMap<OwnedObjectPath, HashMap<String, Props>>;

fn prop<T: TryFrom<OwnedValue>>(p: &Props, key: &str) -> Option<T> {
    p.get(key)?.try_clone().ok()?.try_into().ok()
}

/// Drives with SMART (ATA) or health (NVMe) data, via UDisks2. No root
/// needed; UDisks2 is D-Bus activated.
pub fn udisks_drives() -> PortResult<Vec<Drive>> {
    let conn = Connection::system().map_err(bus_err("connecting to the system bus"))?;
    let om = Proxy::new(
        &conn,
        "org.freedesktop.UDisks2",
        "/org/freedesktop/UDisks2",
        "org.freedesktop.DBus.ObjectManager",
    )
    .map_err(bus_err("UDisks2"))?;
    let objects: ManagedObjects = om
        .call("GetManagedObjects", &())
        .map_err(bus_err("UDisks2 GetManagedObjects"))?;

    let mut drives: Vec<(&OwnedObjectPath, Drive)> = objects
        .iter()
        .filter_map(|(path, ifaces)| {
            let drive = ifaces.get("org.freedesktop.UDisks2.Drive")?;
            let model: String = prop(drive, "Model").unwrap_or_default();
            let model = if model.is_empty() {
                prop(drive, "Serial").unwrap_or_default()
            } else {
                model
            };

            if let Some(ata) = ifaces.get("org.freedesktop.UDisks2.Drive.Ata") {
                if !prop::<bool>(ata, "SmartSupported").unwrap_or(false) {
                    return None;
                }
                let bad: i64 = prop(ata, "SmartNumBadSectors").unwrap_or(0);
                return Some((
                    path,
                    Drive {
                        model,
                        kind: DriveKind::Ata,
                        smart_failing: prop(ata, "SmartFailing").unwrap_or(false),
                        bad_sectors: bad.max(0) as u64,
                        critical_warnings: vec![],
                    },
                ));
            }
            let nvme = ifaces.get("org.freedesktop.UDisks2.NVMe.Controller")?;
            Some((
                path,
                Drive {
                    model,
                    kind: DriveKind::Nvme,
                    smart_failing: false,
                    bad_sectors: 0,
                    critical_warnings: prop(nvme, "SmartCriticalWarning").unwrap_or_default(),
                },
            ))
        })
        .collect();
    drives.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    Ok(drives.into_iter().map(|d| d.1).collect())
}
