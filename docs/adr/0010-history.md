# ADR-0010: Status history

**Status:** Deferred (2026-11-24): taken out of scope for now. The
proposal below and the options discussed are kept for when it comes back.

## Context
H1: keep a lightweight history of the general status, not raw data.

## Proposal
- One record per run: timestamp, overall status, and per `CheckId` the
  status and the number of items.
- Stored as JSON Lines in `$XDG_STATE_HOME/arch-health/history.jsonl`
  (no database dependency), written by the GUI/tray periodic runs and
  optionally by the CLI.
- Retention: e.g. 90 days, compacted to one record per day after 7 days.

## Options discussed

- What to record: overall status only; + per-check metrics (disk %, updates,
  errors, crashes, failed units, fixable vulnerabilities); + open problems
  (check + subject) to know since when a problem exists.
- Who writes: background monitor only; monitor + CLI with `--record`; every
  run.
- Format: JSON Lines (no new dependency) or SQLite.
- Views: daily overall-status strip, trend charts, problems table with
  appeared/resolved dates.
- With a stored baseline, notify on *new* problems instead of on a worse
  overall status.

## Open questions
- Which views does the GUI need (timeline of overall status? per check)?
- Should the CLI write history, or only the background monitor?
