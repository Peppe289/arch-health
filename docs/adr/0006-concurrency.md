# ADR-0006: Concurrency: one thread per check

**Status:** Accepted

## Context
libalpm and sd-journal are blocking C libraries; network checks wait on I/O.

## Decision
The runner executes each check on its own thread (`std::thread::scope`) and
emits events (`Started`, `Finished(report)`) through a callback, so frontends
can show results progressively. No async runtime.

## Consequences
- Shared expensive data (the libalpm snapshot) is computed once behind a
  `OnceLock` in the adapter; other threads wait for it.
- Threads cannot be killed: every adapter must bound its own waiting time
  (HTTP timeout, D-Bus call timeout).
