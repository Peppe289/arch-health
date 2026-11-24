# ADR-0010: Status history

**Status:** Proposed — to be discussed

## Context
H1: keep a lightweight history of the general status, not raw data.

## Proposal
- One record per run: timestamp, overall status, and per `CheckId` the
  status and the number of items.
- Stored as JSON Lines in `$XDG_STATE_HOME/arch-health/history.jsonl`
  (no database dependency), written by the GUI/tray periodic runs and
  optionally by the CLI.
- Retention: e.g. 90 days, compacted to one record per day after 7 days.

## Open questions
- Which views does the GUI need (timeline of overall status? per check)?
- Should the CLI write history, or only the background monitor?
