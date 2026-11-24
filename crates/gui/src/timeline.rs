//! Pure helpers for time-based views: bucketing events for the histogram
//! and formatting times. No GTK widgets here, so it is unit-tested.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use arch_health_core::Status;
use gtk::glib;

/// Candidate bucket sizes, smallest first.
const STEPS: [u64; 7] = [300, 900, 3600, 3 * 3600, 6 * 3600, 12 * 3600, 86400];
/// Upper bound of bars in a histogram.
pub const MAX_BUCKETS: u64 = 48;

/// Counts per severity in one time slot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub critical: usize,
    pub warning: usize,
    pub other: usize,
}

impl Counts {
    pub fn total(&self) -> usize {
        self.critical + self.warning + self.other
    }

    fn add(&mut self, s: Status) {
        match s {
            Status::Critical => self.critical += 1,
            Status::Warning | Status::Unknown => self.warning += 1,
            _ => self.other += 1,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Buckets {
    /// Start of the first bucket, seconds since the epoch.
    pub start: u64,
    pub step: u64,
    pub counts: Vec<Counts>,
}

impl Buckets {
    pub fn bucket_start(&self, i: usize) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(self.start + i as u64 * self.step)
    }

    pub fn max_total(&self) -> usize {
        self.counts.iter().map(Counts::total).max().unwrap_or(0)
    }
}

fn secs(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}

/// Groups events into at most [`MAX_BUCKETS`] equal slots from the oldest
/// event to `now`. `utc_offset` (seconds) aligns slots to local time, so
/// daily bars start at local midnight.
pub fn bucketize(
    events: &[(SystemTime, Status)],
    now: SystemTime,
    utc_offset: i64,
) -> Option<Buckets> {
    let oldest = events.iter().map(|e| secs(e.0)).min()?;
    let now = secs(now).max(oldest + 1);
    let span = now - oldest;
    let step = STEPS
        .iter()
        .copied()
        .find(|s| span / s < MAX_BUCKETS)
        .unwrap_or_else(|| span.div_ceil(MAX_BUCKETS));
    // Align the first slot to a multiple of `step` in local time.
    let local_oldest = oldest as i64 + utc_offset;
    let start = (local_oldest - local_oldest.rem_euclid(step as i64) - utc_offset).max(0) as u64;
    let n = ((now - start) / step + 1) as usize;
    let mut counts = vec![Counts::default(); n];
    for (t, s) in events {
        let i = ((secs(*t).saturating_sub(start)) / step) as usize;
        counts[i.min(n - 1)].add(*s);
    }
    Some(Buckets {
        start,
        step,
        counts,
    })
}

/// Local UTC offset in seconds.
pub fn local_offset() -> i64 {
    glib::DateTime::now_local()
        .map(|d| d.utc_offset().as_seconds())
        .unwrap_or(0)
}

fn local(t: SystemTime) -> Option<glib::DateTime> {
    glib::DateTime::from_unix_local(secs(t) as i64).ok()
}

/// "14:03:27" today, "Mon 23 Nov 14:03" otherwise.
pub fn format_time(t: SystemTime) -> String {
    let Some(dt) = local(t) else {
        return String::new();
    };
    let today = glib::DateTime::now_local()
        .ok()
        .map(|n| (n.year(), n.day_of_year()));
    let fmt = if today == Some((dt.year(), dt.day_of_year())) {
        "%H:%M:%S"
    } else {
        "%a %d %b %H:%M"
    };
    dt.format(fmt).map(|s| s.to_string()).unwrap_or_default()
}

/// Axis label for a bucket start: time of day for sub-day steps.
pub fn format_tick(t: SystemTime, step: u64) -> String {
    let Some(dt) = local(t) else {
        return String::new();
    };
    let fmt = if step >= 86400 { "%d %b" } else { "%H:%M" };
    dt.format(fmt).map(|s| s.to_string()).unwrap_or_default()
}

/// Human description of a bucket, for tooltips.
pub fn describe_bucket(b: &Buckets, i: usize) -> String {
    let from = format_time(b.bucket_start(i));
    let to = format_tick(b.bucket_start(i + 1), b.step);
    let c = b.counts[i];
    let mut parts = Vec::new();
    if c.critical > 0 {
        parts.push(format!("{} critical", c.critical));
    }
    if c.warning > 0 {
        parts.push(format!("{} warning", c.warning));
    }
    if c.other > 0 {
        parts.push(format!("{} other", c.other));
    }
    if parts.is_empty() {
        parts.push("nothing".into());
    }
    format!("{from} – {to}\n{}", parts.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(s: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(s)
    }

    #[test]
    fn empty_has_no_buckets() {
        assert!(bucketize(&[], at(1000), 0).is_none());
    }

    #[test]
    fn picks_smallest_step_with_at_most_48_bars() {
        // 10 hours of events → 15 minute slots (40 bars).
        let base = 1_700_000_000 - 1_700_000_000 % 3600;
        let ev = [
            (at(base), Status::Critical),
            (at(base + 36_000), Status::Warning),
        ];
        let b = bucketize(&ev, at(base + 36_000), 0).unwrap();
        assert_eq!(b.step, 900);
        assert!(b.counts.len() as u64 <= MAX_BUCKETS + 1);
        assert_eq!(b.counts[0].critical, 1);
        assert_eq!(b.counts.last().unwrap().warning, 1);
        assert_eq!(b.counts.iter().map(Counts::total).sum::<usize>(), 2);
    }

    #[test]
    fn daily_slots_align_to_local_midnight() {
        // A week of events, UTC+1: slots start at 23:00 UTC.
        let midnight_utc = 1_700_000_000 - 1_700_000_000 % 86400;
        let ev = [(at(midnight_utc + 3600), Status::Info)];
        let b = bucketize(&ev, at(midnight_utc + 7 * 86400), 3600).unwrap();
        assert_eq!(b.step, 6 * 3600);
        assert_eq!((b.start as i64 + 3600) % b.step as i64, 0);
    }

    #[test]
    fn counts_by_severity() {
        let mut c = Counts::default();
        c.add(Status::Critical);
        c.add(Status::Warning);
        c.add(Status::Unknown);
        c.add(Status::Info);
        assert_eq!(
            c,
            Counts {
                critical: 1,
                warning: 2,
                other: 1
            }
        );
        assert_eq!(c.total(), 4);
    }
}
