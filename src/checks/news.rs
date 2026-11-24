//! Arch Linux news: items published after the last full system upgrade
//! (taken from pacman.log) should be read before the next `pacman -Syu`.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, Local, TimeDelta};

use crate::net;
use crate::pacman_conf::PacmanConf;
use crate::report::{CheckResult, Status};

const FEED_URL: &str = "https://archlinux.org/feeds/news/";
/// Without any upgrade in the log, news this recent count as unread.
const FALLBACK_WINDOW_DAYS: i64 = 30;

pub struct NewsItem {
    pub title: String,
    pub link: String,
    pub date: DateTime<FixedOffset>,
}

/// Parses the RSS 2.0 feed (`<item><title/><link/><pubDate/></item>`).
pub fn parse_feed(xml: &str) -> Result<Vec<NewsItem>> {
    let doc = roxmltree::Document::parse(xml).context("feed RSS non valido")?;
    let text = |item: roxmltree::Node, tag: &str| {
        item.children()
            .find(|c| c.has_tag_name(tag))
            .and_then(|c| c.text())
            .unwrap_or("")
            .trim()
            .to_string()
    };
    let mut items: Vec<NewsItem> = doc
        .descendants()
        .filter(|n| n.has_tag_name("item"))
        .filter_map(|item| {
            Some(NewsItem {
                date: DateTime::parse_from_rfc2822(&text(item, "pubDate")).ok()?,
                title: text(item, "title"),
                link: text(item, "link"),
            })
        })
        .collect();
    items.sort_by(|a, b| b.date.cmp(&a.date));
    Ok(items)
}

/// Timestamp of the last "starting full system upgrade" in pacman.log,
/// e.g. `[2026-11-24T17:35:12+0100] [PACMAN] starting full system upgrade`.
pub fn last_full_upgrade(log: &str) -> Option<DateTime<FixedOffset>> {
    log.lines()
        .rev()
        .find(|l| l.ends_with("starting full system upgrade"))
        .and_then(|l| l.strip_prefix('[')?.split_once(']'))
        .and_then(|(ts, _)| DateTime::parse_from_str(ts, "%Y-%m-%dT%H:%M:%S%z").ok())
}

fn fmt_date(d: &DateTime<FixedOffset>) -> String {
    d.with_timezone(&Local).format("%d/%m/%Y").to_string()
}

pub fn check(conf: &PacmanConf) -> CheckResult {
    check_inner(conf).unwrap_or_else(|e| CheckResult::error("News Arch", e))
}

fn check_inner(conf: &PacmanConf) -> Result<CheckResult> {
    let xml = net::agent()
        .get(FEED_URL)
        .call()
        .context("download del feed delle news")?
        .body_mut()
        .read_to_string()?;
    let items = parse_feed(&xml)?;

    let log = fs::read_to_string(Path::new(&conf.log_file)).unwrap_or_default();
    let upgrade = last_full_upgrade(&log);
    let since = upgrade.unwrap_or_else(|| {
        (Local::now() - TimeDelta::days(FALLBACK_WINDOW_DAYS)).fixed_offset()
    });

    let unread: Vec<String> = items
        .iter()
        .filter(|n| n.date > since)
        .map(|n| format!("{} — {} ({})", fmt_date(&n.date), n.title, n.link))
        .collect();
    let ref_point = match &upgrade {
        Some(u) => format!("dall'ultimo aggiornamento ({})", fmt_date(u)),
        None => format!("negli ultimi {FALLBACK_WINDOW_DAYS} giorni"),
    };

    Ok(if unread.is_empty() {
        let latest = items
            .first()
            .map(|n| vec![format!("ultima: {} — {}", fmt_date(&n.date), n.title)])
            .unwrap_or_default();
        CheckResult::new("News Arch", Status::Ok, format!("nessuna news {ref_point}")).with_details(latest)
    } else {
        CheckResult::new(
            "News Arch",
            Status::Warn,
            format!("{} news {ref_point}: leggile prima di aggiornare", unread.len()),
        )
        .with_details(unread)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pacman_log() {
        let log = "[2026-11-01T10:00:00+0100] [PACMAN] starting full system upgrade\n\
                   [2026-11-20T09:30:00+0100] [PACMAN] starting full system upgrade\n\
                   [2026-11-20T09:31:00+0100] [ALPM] upgraded foo (1-1 -> 2-1)\n";
        let d = last_full_upgrade(log).unwrap();
        assert_eq!(d.to_rfc3339(), "2026-11-20T09:30:00+01:00");
        assert!(last_full_upgrade("").is_none());
    }

    #[test]
    fn parses_feed() {
        let xml = r#"<?xml version="1.0"?><rss version="2.0"><channel>
            <item><title>Old</title><link>https://a/old</link><pubDate>Tue, 01 Sep 2026 10:00:00 +0000</pubDate></item>
            <item><title>New &amp; shiny</title><link>https://a/new</link><pubDate>Wed, 21 Oct 2026 19:29:36 +0000</pubDate></item>
            </channel></rss>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "New & shiny");
    }
}
