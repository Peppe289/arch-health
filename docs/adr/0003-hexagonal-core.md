# ADR-0003: Hexagonal core (ports & adapters)

**Status:** Accepted

## Context
The first prototype queried libalpm/D-Bus directly inside each check and
built user-facing strings there: impossible to unit-test, and UI coupled to
data access.

## Decision
Cargo workspace with three layers:

```
crates/core      domain model, checks (rules), runner, ports (traits)
                 no system dependencies → builds and tests anywhere
crates/adapters  port implementations: libalpm, D-Bus, sd-journal,
                 procfs/sysfs, HTTP
crates/cli       CLI frontend        crates/gui   GUI frontend (next)
```

- **Ports** are traits returning plain data (e.g. `PackageSnapshot`,
  `ManagerState`, `Drive`). Expensive data is computed once per run by the
  adapter (e.g. the libalpm snapshot is cached).
- **Checks** are pure rules over port data. Each check has a stable
  `CheckId`, declares whether it needs the network and whether it benefits
  from root, and produces one `CheckReport`: status, summary and a list of
  items (`subject` + `detail` + status).
- Even version comparison is a port (`PackageDb::compare_versions`), so the
  core does not link libalpm and uses exactly pacman's ordering at runtime.
- Tests use fake ports.

## Consequences
- Every rule is unit-tested in CI without Arch-specific libraries.
- Frontends only see `CheckReport`; the same data feeds CLI, GUI, the tray,
  a future `--json` output and the history (ADR-0010).
- Report text is English (N3); `CheckId` and item `subject` stay stable for
  configuration (ignore rules) and history.
