# ADR-0004: GUI toolkit: GTK4 (gtk4-rs), tray via StatusNotifierItem

**Status:** Accepted

## Context
Candidates: Qt6/QML via `cxx-qt`, GTK4 via `gtk4-rs`, Slint, iced/egui,
Tauri. The main desktop is KDE Plasma. The maintainer must be able to review
the GUI code.

## Decision
- **GTK4 through `gtk4-rs`**, without libadwaita. It is the most mature and
  best documented Rust GUI binding; idiomatic Rust, no C++ bridge. Without
  libadwaita GTK follows the system theme, and with `breeze-gtk` it looks
  native on Plasma.
- **Tray icon:** StatusNotifierItem over D-Bus (the `ksni` crate, pure Rust),
  the protocol used by Plasma (and by GNOME with the AppIndicator extension).
- **Notifications:** `org.freedesktop.Notifications` over D-Bus (`zbus`).

## Consequences
- New runtime dependency: `gtk4` (already installed on most desktops).
- No exec anywhere (ADR-0001) is preserved.
