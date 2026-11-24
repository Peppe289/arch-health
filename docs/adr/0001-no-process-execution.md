# ADR-0001: No process execution

**Status:** Accepted

## Context
Health tools often wrap shell commands (`checkupdates`, `systemctl`,
`smartctl`, ...). That makes them depend on the output format of other
programs, slow, and hard to test.

## Decision
arch-health never spawns processes. Every piece of information comes from a
library or a system API, like AUR helpers do with libalpm:

| Data | Source |
|------|--------|
| Packages | libalpm (`alpm` crate) |
| Units | systemd D-Bus API (`zbus`) |
| Journal | sd-journal (`systemd` crate, libsystemd) |
| Disks | UDisks2 D-Bus API, sysfs, `statvfs()` |
| Kernel / processes | `uname()`, procfs |
| AUR, security tracker, news | HTTPS (`ureq` + native-tls, i.e. system OpenSSL and CA store) |

Crates that shell out are not used (e.g. `pacmanconf` runs `pacman-conf`, so
`pacman.conf` is parsed natively).

## Consequences
- Native dependencies: libalpm, libsystemd, OpenSSL (all already on Arch).
- Libraries may spawn helpers internally (libalpm → gpgme → gpg for signature
  checks). Official repository databases are not signed, so this does not
  happen in practice. This is accepted.
