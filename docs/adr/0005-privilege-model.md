# ADR-0005: Privilege model

**Status:** Accepted (implemented)

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

## Implementation notes

- Helper: `arch-health-helper`, bus name and interface
  `io.github.archhealth.Helper1`, object `/io/github/archhealth/Helper1`.
  Methods: `Authorize(b interactive) → b`, `JournalErrors() → a(tsys)`,
  `Coredumps(t since_usec) → a(tssb)`, `StaleLibraries() → a{su}`; at most
  5000 entries per call. Exits after 60 s without calls (a call waiting for
  the password keeps it alive).
- Every method checks polkit `CheckAuthorization` for the caller's bus name
  (action `io.github.archhealth.read-privileged`, `auth_admin_keep`). The
  GUI calls `Authorize(true)` once on Unlock, so a single dialog appears;
  data calls are non-interactive.
- Client side the helper is just another adapter: `HelperJournal` and
  `HelperSystem` implement the existing ports and fall back to local reads
  when the helper is missing or the authorization expired; the result then
  carries its limitation again and the GUI shows Unlock again.
- Files: `data/dbus/` (bus policy, activation), `data/polkit/` (action),
  `data/systemd/` (sandboxed unit, `systemd-analyze security` 1.6 OK).
- `--session` development mode: session bus, current user, no polkit.
