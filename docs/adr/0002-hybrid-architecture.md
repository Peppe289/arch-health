# ADR-0002: Hybrid architecture: core library + frontends

**Status:** Accepted

## Context
We need a CLI (SSH, scripts), a GUI and a tray icon with periodic checks.
Options: a monolithic GUI; a daemon exposing D-Bus with thin clients; a
shared library used in-process by each frontend.

## Decision
A **core library** holds all the logic. Frontends use it in-process:

- `arch-health` (CLI)
- `arch-health-gui` (GTK4 window + tray icon + periodic checks)

The core API (checks → structured reports) is designed so that it can be
exposed over D-Bus later if a daemon becomes necessary (e.g. for actions,
F14, or a privileged helper, ADR-0005).

## Consequences
- Simple to build and debug now; no IPC.
- The tray process is the "monitor": it must be autostarted (XDG autostart
  or a systemd user unit).
- CLI and GUI may run checks at the same time; adapters must not rely on
  exclusive resources (the private pacman db copy uses its own lock).
