//! Arch news published after the last full system upgrade should be read
//! before the next `pacman -Syu`.

use chrono::{Local, TimeDelta};

use super::{Check, Context};
use crate::error::PortResult;
use crate::model::{CheckId, CheckReport, Item, Status, plural};

pub struct ArchNews;

impl Check for ArchNews {
    fn id(&self) -> CheckId {
        CheckId::ArchNews
    }

    fn needs_network(&self) -> bool {
        true
    }

    fn run(&self, ctx: &Context<'_>) -> PortResult<CheckReport> {
        let mut items = ctx.ports.news.items()?;
        items.sort_by_key(|n| std::cmp::Reverse(n.published));
        let upgrade = ctx.ports.packages.last_full_upgrade()?;
        let since = upgrade.unwrap_or_else(|| {
            (Local::now() - TimeDelta::days(ctx.config.news_fallback_days)).fixed_offset()
        });
        let date = |d: &chrono::DateTime<chrono::FixedOffset>| d.format("%Y-%m-%d").to_string();

        let unread: Vec<Item> = items
            .iter()
            .filter(|n| n.published > since)
            .map(|n| {
                Item::new(
                    Status::Warning,
                    date(&n.published),
                    format!("{} ({})", n.title, n.link),
                )
            })
            .collect();
        let reference = match &upgrade {
            Some(u) => format!("since the last upgrade ({})", date(u)),
            None => format!("in the last {} days", ctx.config.news_fallback_days),
        };

        Ok(if unread.is_empty() {
            let latest = items
                .first()
                .map(|n| {
                    vec![Item::new(
                        Status::Ok,
                        date(&n.published),
                        format!("latest: {}", n.title),
                    )]
                })
                .unwrap_or_default();
            CheckReport::new(self.id(), Status::Ok, format!("no news {reference}"))
                .with_items(latest)
        } else {
            let summary = format!(
                "{} {reference}: read before upgrading",
                plural(unread.len(), "news item")
            );
            CheckReport::from_items(self.id(), unread, summary)
        })
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use super::*;
    use crate::fake::Fake;
    use crate::ports::NewsItem;

    fn news(title: &str, date: &str) -> NewsItem {
        NewsItem {
            title: title.into(),
            link: format!("https://archlinux.org/news/{title}/"),
            published: DateTime::parse_from_rfc3339(date).unwrap(),
        }
    }

    #[test]
    fn only_news_after_last_upgrade_are_unread() {
        let mut fake = Fake::default();
        fake.news = vec![
            news("old", "2026-10-01T10:00:00+00:00"),
            news("new", "2026-11-20T10:00:00+00:00"),
        ];
        fake.last_upgrade =
            Some(DateTime::parse_from_rfc3339("2026-11-01T00:00:00+01:00").unwrap());
        let (ports, config) = (fake.ports(), Default::default());
        let r = ArchNews
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();
        assert_eq!(r.status, Status::Warning);
        assert_eq!(r.items.len(), 1);
        assert!(r.items[0].detail.starts_with("new"));
    }

    #[test]
    fn nothing_unread_shows_latest() {
        let mut fake = Fake::default();
        fake.news = vec![news("old", "2026-10-01T10:00:00+00:00")];
        fake.last_upgrade =
            Some(DateTime::parse_from_rfc3339("2026-11-01T00:00:00+01:00").unwrap());
        let (ports, config) = (fake.ports(), Default::default());
        let r = ArchNews
            .run(&Context {
                ports: &ports,
                config: &config,
            })
            .unwrap();
        assert_eq!(r.status, Status::Ok);
        assert_eq!(r.items[0].detail, "latest: old");
    }
}
