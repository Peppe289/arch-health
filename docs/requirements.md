# Requirements

Status of each requirement: **Done**, **Planned** (agreed, not yet built),
**Future** (agreed direction, after the software is stable), **Deferred**
(taken out of scope for now, may come back), **TBD** (to be discussed).

## Goal

Tell the user whether their Arch Linux installation is healthy: up to date,
consistent, not corrupted, and without failing components. Physical
quantities that naturally vary over time (temperatures, load, ...) are
explicitly **out of scope**.

## Functional requirements

| ID   | Requirement | Status |
|------|-------------|--------|
| F1   | Report pending updates from the official repositories, without touching the system package databases. | Done |
| F2   | Report pending AUR updates, packages flagged out-of-date / orphaned / missing on the AUR, stale `-debug` packages. | Done |
| F3   | Report orphan packages, foreign packages and unmerged `.pacnew` files. | Done |
| F4   | Report installed packages with known vulnerabilities (Arch security tracker). | Done |
| F5   | Report Arch news published after the last full system upgrade. | Done |
| F6   | Report whether a reboot is needed (running kernel removed) and which processes still use replaced libraries. | Done |
| F7   | Report failed systemd units (system and user manager). | Done |
| F8   | Report filesystems running out of space/inodes or mounted read-only. | Done |
| F9   | Report storage hardware faults: SMART failure, bad sectors, NVMe critical warnings, btrfs device errors. No temperatures. | Done |
| F10  | Report journal errors of the current boot and recent coredumps. | Done |
| F11  | Report hardware errors found in the kernel log (I/O errors, filesystem errors, machine checks) as a **concise** finding, not a log dump. | Planned |
| F12  | Verify the integrity of installed package files (equivalent of `pacman -Qk`/`-Qkk`). | Planned |
| F13  | Read-only: the software never changes the system. | Done |
| F14  | Actions (update, restart a unit, remove orphans, merge `.pacnew`, ...). Must be possible to add without redesign. | Future |
| G1   | Desktop GUI showing the report grouped by category, with details per finding. | Done |
| G2   | System tray icon showing the overall status and that the monitor is running. | Done |
| G3   | Periodic checks in the background (every 6 h and at session start) with a desktop notification when the status gets worse. | Done |
| G4   | GUI section listing the CLI commands and options, generated from the CLI definition. | Done |
| G5   | Charts, not only text: status of all checks, disk usage, errors over time. | Done |
| G6   | Errors shown in a table with their time and details (sortable, filterable), per check and across checks on the overview. | Done |
| H1   | Keep a lightweight history of the overall status (not raw data). Proposal in ADR-0010. | Deferred (out of scope for now) |
| C1   | Command line interface with the same checks, usable over SSH. | Done |
| C2   | Exit code reflects the overall status (0 ok, 1 warnings, 2 critical). | Done |

## Non-functional requirements

| ID  | Requirement | Status |
|-----|-------------|--------|
| N1  | **No process execution.** Data comes only from libraries and system APIs (libalpm, D-Bus, sd-journal, procfs/sysfs, syscalls, HTTP). See ADR-0001. | Done |
| N2  | Runs as a normal user. Checks that are incomplete without root say so; elevation is requested only when needed: `sudo` from the CLI, the desktop polkit dialog from the GUI. See ADR-0005. | Partial (CLI hint, GUI note; polkit helper planned) |
| N3  | User-facing text in English. | Done |
| N4  | Published on GitHub; then CI; then packaging (PKGBUILD). | Planned |
| N5  | Domain logic is testable without a real system (fake adapters), so tests run in CI. | Done |
| N6  | `--offline` mode with no network access at all. | Done |
| N7  | Hardware faults are reported once and concisely; no noisy logging. | Done for F9 |
