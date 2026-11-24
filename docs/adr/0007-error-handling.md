# ADR-0007: Error handling and logging

**Status:** Accepted

## Decision
- Libraries (`core`, `adapters`) use typed errors with `thiserror`. Ports
  return `PortError { kind, message }` (`Unavailable`, `PermissionDenied`,
  `Network`, `Data`).
- A failing port never aborts the run: the runner turns the error into a
  report with status `Unknown`.
- Binaries use `anyhow` for startup errors.
- Diagnostics (not user output) go through `tracing`, off by default.
