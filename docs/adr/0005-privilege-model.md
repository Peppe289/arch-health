# ADR-0005: Privilege model

**Status:** Accepted (CLI part implemented; GUI part planned)

## Context
Some checks see more as root (all processes for stale libraries; the system
journal for users outside `wheel`/`adm`/`systemd-journal`). Future actions
(F14) will require root.

## Decision
- Everything runs **as the normal user** by default.
- Each check declares its privilege need: `User` (complete without root) or
  `Enhanced` (works, but is more complete as root). A report produced with
  insufficient privileges carries a `limitation` note.
- **CLI:** never elevates by itself. It prints which checks were limited and
  suggests rerunning with `sudo`.
- **GUI (planned):** a small privileged helper, activated on demand on the
  system D-Bus, authorized through **polkit**. The desktop's polkit agent
  shows the password dialog (on Plasma: polkit-kde-agent). The helper only
  exposes the minimal read operations that need root (and later the actions
  of F14).

## Consequences
- No setuid, no long-running root process.
- The helper is a separate binary with its own D-Bus policy and polkit
  action files; it is introduced only when a GUI check actually needs it.
