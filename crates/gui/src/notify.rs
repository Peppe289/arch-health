//! Desktop notifications through `org.freedesktop.Notifications` (D-Bus).

use std::collections::HashMap;

use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::Value;

/// Sends a notification. Blocking: call it off the UI thread.
pub fn send(icon: &str, summary: &str, body: &str) -> zbus::Result<()> {
    let conn = Connection::session()?;
    let proxy = Proxy::new(
        &conn,
        "org.freedesktop.Notifications",
        "/org/freedesktop/Notifications",
        "org.freedesktop.Notifications",
    )?;
    let mut hints: HashMap<&str, Value> = HashMap::new();
    hints.insert("desktop-entry", Value::from("arch-health"));
    let actions: Vec<&str> = Vec::new();
    // Notify(app_name, replaces_id, app_icon, summary, body, actions, hints, timeout)
    let _id: u32 = proxy.call(
        "Notify",
        &(
            "arch-health",
            0u32,
            icon,
            summary,
            body,
            actions,
            hints,
            -1i32,
        ),
    )?;
    Ok(())
}
