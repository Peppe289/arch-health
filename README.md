# arch-health

Health report for an Arch Linux installation: is it up to date, consistent,
not corrupted, without failing components?

arch-health uses only libraries and system APIs (libalpm, D-Bus, sd-journal,
procfs/sysfs, syscalls, HTTPS) and **never executes other programs**
([ADR-0001](docs/adr/0001-no-process-execution.md)).

## Usage

```
cargo build --release
./target/release/arch-health                # private db refresh + all checks
./target/release/arch-health --no-sync      # use the system databases as they are
./target/release/arch-health --offline      # no network: no refresh, AUR, CVEs, news
./target/release/arch-health --only kernel,disk-space
./target/release/arch-health --list-checks
sudo ./target/release/arch-health           # complete results for root-only data
```

Exit status: `0` healthy · `1` warnings · `2` critical · `3` usage/startup error.

### Desktop

```
./target/release/arch-health-gui               # window + tray icon
./target/release/arch-health-gui --background  # tray only (session autostart)
./target/release/arch-health-gui --page logs    # open a page directly
```

- Overview: ring chart of the checks by status, clickable category tiles,
  what needs attention, and a timeline + table of recent errors across checks.
- One page per category; each check is a card. Disk usage as bars; journal
  errors, coredumps and failed units as a histogram over time and a table
  (time, level, source, message) that can be sorted and filtered.
- Tray icon (StatusNotifierItem) colored by the overall status; left click opens
  the window, the menu offers *Run checks now* and *Quit*. Closing the window
  keeps the monitor running in the tray.
- Checks run at start and every 6 hours; a desktop notification is sent only
  when the status gets worse (on the first run: only if critical).
- *Command line* page generated from the CLI definition.
- Single instance: launching it again shows the existing window.

To start the monitor with the session:

```
install -Dm644 data/arch-health-autostart.desktop ~/.config/autostart/arch-health.desktop
install -Dm644 data/arch-health.desktop ~/.local/share/applications/arch-health.desktop
```

(`arch-health-gui` must be in `PATH`; packaging will install everything.)

### Privileged helper (Unlock…)

Some checks see more as root (the system journal for users outside
`wheel`/`adm`, and the processes of every user). In the GUI the **Unlock…**
button asks for the administrator password through polkit (the desktop's
own dialog) and completes those results through `arch-health-helper`:

- D-Bus activated on the system bus, runs as root only while needed and
  exits after 60 s idle; sandboxed by its systemd unit;
- read-only: it exposes four queries (`Authorize`, `JournalErrors`,
  `Coredumps`, `StaleLibraries`), each authorized by polkit action
  `io.github.archhealth.read-privileged` (`auth_admin_keep`: the password
  is remembered for a few minutes, then asked again).

Install it (as root):

```
cargo build --release -p arch-health-helper
sudo install -Dm755 target/release/arch-health-helper /usr/lib/arch-health/arch-health-helper
sudo install -Dm644 data/dbus/io.github.archhealth.Helper1.conf /usr/share/dbus-1/system.d/io.github.archhealth.Helper1.conf
sudo install -Dm644 data/dbus/io.github.archhealth.Helper1.service /usr/share/dbus-1/system-services/io.github.archhealth.Helper1.service
sudo install -Dm644 data/polkit/io.github.archhealth.policy /usr/share/polkit-1/actions/io.github.archhealth.policy
sudo install -Dm644 data/systemd/arch-health-helper.service /usr/lib/systemd/system/arch-health-helper.service
sudo systemctl daemon-reload
```

Without the helper the GUI keeps working and says so when Unlock is used.
Over SSH use `sudo arch-health` instead.

Development without root: `cargo run -p arch-health-helper -- --session`
serves on the session bus as your user and skips polkit; start the GUI with
`ARCH_HEALTH_HELPER_BUS=session`. The client test runs against it with
`ARCH_HEALTH_HELPER_BUS=session cargo test -p arch-health-adapters -- --ignored`.

## Checks

| Key | Category | What | Source |
|-----|----------|------|--------|
| `repo-updates` | Packages | pending repository updates | libalpm, private db copy in `~/.cache/arch-health/db` |
| `aur-updates` | Packages | AUR updates; out-of-date, orphaned, missing, stale `-debug` packages | libalpm + AUR RPC v5 |
| `orphans` | Packages | packages no longer required | libalpm |
| `foreign-packages` | Packages | packages not in the repositories | libalpm |
| `pacnew` | Packages | unmerged `.pacnew` files | libalpm backup files |
| `vulnerabilities` | Security | installed packages with known issues (same logic as `arch-audit`) | security.archlinux.org |
| `arch-news` | Security | news published since the last `-Syu` | archlinux.org RSS + `pacman.log` |
| `kernel` | Reboot | running kernel removed → reboot needed | `uname()`, `/usr/lib/modules` |
| `stale-libraries` | Reboot | processes using replaced libraries (all processes as root) | `/proc/*/maps` |
| `system-services`, `user-services` | Services | failed units, manager state | systemd D-Bus |
| `disk-space` | Storage | space/inodes, read-only filesystems | `/proc/self/mounts`, `statvfs()` |
| `btrfs-errors` | Storage | device error counters | `/sys/fs/btrfs` |
| `drive-health` | Storage | SMART failure, bad sectors, NVMe critical warnings (no temperatures) | UDisks2 D-Bus |
| `journal-errors` | Logs | errors of the current boot, grouped by source | sd-journal |
| `coredumps` | Logs | crashes in the last 7 days | sd-journal |

## Architecture

Cargo workspace, hexagonal core ([ADR-0003](docs/adr/0003-hexagonal-core.md)):

```
crates/core      domain model, checks (pure rules), runner, ports (traits)
                 no system dependencies: builds and tests anywhere
crates/adapters  ports implemented with libalpm, D-Bus, sd-journal, procfs/sysfs, HTTP
crates/cli       the `arch-health` command
crates/gui       `arch-health-gui`: GTK4 window, tray icon, periodic checks, notifications
crates/helper    `arch-health-helper`: root, read-only, polkit-authorized D-Bus helper
```

- Requirements: [docs/requirements.md](docs/requirements.md)
- Decisions: [docs/adr/](docs/adr/README.md)

## Dependencies

### Build

| Package | Why |
|---------|-----|
| `rust` (or `rustup`) | compiler and cargo |
| `pkgconf` | `alpm-sys` finds libalpm through pkg-config |
| `pacman` | libalpm headers and `libalpm.pc` |
| `systemd-libs` | libsystemd for sd-journal |
| `openssl` | native-tls |
| `gtk4` | GUI (headers and pkg-config) |

### Runtime

| Package | Why |
|---------|-----|
| `pacman` | `libalpm.so` |
| `systemd-libs` | `libsystemd.so` |
| `openssl`, `ca-certificates` | HTTPS with the system CA store |
| `gtk4` | GUI only |
| `glibc`, `gcc-libs` | Rust runtime |

### Services used at runtime (optional: a missing one makes its check report "could not run")

| Package | Why |
|---------|-----|
| `systemd`, `dbus` | failed units |
| `udisks2` | drive health (D-Bus activated) |
| `polkit` + a polkit agent | Unlock… in the GUI (Plasma ships `polkit-kde-agent`) |
| a StatusNotifierItem host | tray icon (Plasma has it; GNOME needs the AppIndicator extension). Without it the GUI works as a normal window |
| a notification daemon | desktop notifications |

D-Bus is spoken by `zbus` (pure Rust): no libdbus needed.

## Development

```
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Commit subjects follow `type(component): title`
([ADR-0009](docs/adr/0009-development-process.md)).

## Notes

- The system journal is readable by root and by the `wheel`, `adm` and
  `systemd-journal` groups; otherwise only the user journal is read and the
  report says so.
- Journal messages at `err` level are informational (often noise on
  desktops); `crit`, `alert` and `emerg` are critical.
- Vulnerability severity: fixable critical/high → critical; other fixable →
  warning; without a fix → informational (critical → warning). Many unfixed
  tracker entries are stale, so they do not raise alarms.
- AUR requests send only the names of the installed foreign packages, like
  any AUR helper, with User-Agent `arch-health/<version>`.
- With the `breeze-gtk` theme GTK4 prints `GtkGizmo … reported min width -2`
  warnings for scrollbars: a known theme issue, harmless.
- libalpm uses gpgme internally for signature checks; official repository
  databases are not signed, so gpg is not started during the refresh.

## License

GPL-3.0-or-later ([ADR-0008](docs/adr/0008-license.md)).
