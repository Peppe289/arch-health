# ADR-0008: License: GPL-3.0-or-later

**Status:** Accepted

## Context
The `alpm` crate is GPL-3.0 and the `systemd` crate is LGPL-2.1-or-later.
Linking them requires a compatible license for the whole program.

## Decision
The project is licensed **GPL-3.0-or-later**. Dependency licenses will be
verified in CI with `cargo-deny`.
