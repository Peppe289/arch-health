# ADR-0011: Charts and tables in the GUI

**Status:** Accepted

## Context
The GUI must not be text only: it needs charts, and errors must be shown
in a table with their time and details. The core only produced text items.

## Decision
- **Core model:** `CheckReport` gains structured data for visualization,
  still free of UI concerns:
  - `events: Vec<TimedEvent>` (time, status, source, message), newest
    first, capped at `MAX_EVENTS` (1000): journal errors, coredumps,
    failed units (time from systemd's `StateChangeTimestamp`);
  - `gauges: Vec<Gauge>` (label, fraction 0..1, text, status): disk usage.
- **Charts:** drawn with cairo on `gtk::DrawingArea`, text through pango
  with the widget's font (theme font, light/dark aware). No charting
  library: three small charts are enough and keep dependencies minimal.
  - donut: number of checks per status (overview);
  - gauge bar: one per filesystem;
  - stacked histogram: events per time slot (≤ 48 slots, slot size picked
    from 5 min … 1 day, aligned to local time), tooltip per bar.
- **Tables:** `gtk::ColumnView` over a `gio::ListStore` of
  `glib::BoxedAnyObject` (no GObject subclass needed), sortable columns
  (time, level, check, source), text filter on source/message, newest
  first by default.
- Bucketing and time formatting are pure functions (`gui/src/timeline.rs`)
  with unit tests.

## Consequences
- The CLI can use the same data later (e.g. `--json`, or times in output).
- New GUI dependency `pangocairo` (bindings only; pango/cairo are already
  required by GTK).
