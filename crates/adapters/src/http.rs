//! HTTP adapters: AUR RPC, Arch security tracker, Arch news feed.
//!
//! ureq + native-tls: the system OpenSSL and CA store (`ca-certificates`).

use std::time::Duration;

use arch_health_core::ports::{
    Advisory, AurClient, AurPackage, NewsFeed, NewsItem, SecurityTracker,
};
use arch_health_core::{PortError, PortResult};
use chrono::DateTime;
use serde::Deserialize;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};

const AUR_RPC_URL: &str = "https://aur.archlinux.org/rpc/v5/info";
const TRACKER_URL: &str = "https://security.archlinux.org/issues/all.json";
const NEWS_URL: &str = "https://archlinux.org/feeds/news/";
/// Keep AUR requests well below the per-request argument limit.
const AUR_CHUNK: usize = 150;
const MAX_JSON: u64 = 32 * 1024 * 1024;

fn agent() -> ureq::Agent {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::NativeTls)
        .root_certs(RootCerts::PlatformVerifier)
        .build();
    ureq::Agent::config_builder()
        .tls_config(tls)
        .timeout_global(Some(Duration::from_secs(20)))
        .user_agent(concat!("arch-health/", env!("CARGO_PKG_VERSION")))
        .build()
        .new_agent()
}

fn net_err(what: &str) -> impl Fn(ureq::Error) -> PortError + '_ {
    move |e| PortError::network(format!("{what}: {e}"))
}

// --------------------------------------------------------------------- AUR

#[derive(Deserialize)]
struct RpcResponse {
    #[serde(rename = "type")]
    kind: String,
    error: Option<String>,
    #[serde(default)]
    results: Vec<RpcPackage>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RpcPackage {
    name: String,
    version: String,
    out_of_date: Option<i64>,
    maintainer: Option<String>,
}

pub struct AurRpc;

impl AurClient for AurRpc {
    fn info(&self, names: &[String]) -> PortResult<Vec<AurPackage>> {
        let agent = agent();
        let mut out = Vec::new();
        for chunk in names.chunks(AUR_CHUNK) {
            let resp: RpcResponse = agent
                .post(AUR_RPC_URL)
                .send_form(chunk.iter().map(|n| ("arg[]", n.as_str())))
                .map_err(net_err("AUR request"))?
                .body_mut()
                .read_json()
                .map_err(|e| PortError::data(format!("AUR response: {e}")))?;
            if resp.kind == "error" {
                return Err(PortError::data(format!(
                    "AUR RPC: {}",
                    resp.error.unwrap_or_default()
                )));
            }
            out.extend(resp.results.into_iter().map(|p| AurPackage {
                name: p.name,
                version: p.version,
                out_of_date: p.out_of_date.is_some(),
                orphaned: p.maintainer.is_none(),
            }));
        }
        Ok(out)
    }
}

// ---------------------------------------------------------- security tracker

#[derive(Deserialize)]
struct Avg {
    name: String,
    packages: Vec<String>,
    status: String,
    severity: String,
    fixed: Option<String>,
    #[serde(default)]
    issues: Vec<String>,
}

pub struct ArchSecurityTracker;

impl SecurityTracker for ArchSecurityTracker {
    fn advisories(&self) -> PortResult<Vec<Advisory>> {
        let avgs: Vec<Avg> = agent()
            .get(TRACKER_URL)
            .call()
            .map_err(net_err("security tracker request"))?
            .body_mut()
            .with_config()
            .limit(MAX_JSON)
            .read_json()
            .map_err(|e| PortError::data(format!("security tracker response: {e}")))?;
        Ok(avgs
            .into_iter()
            .map(|a| Advisory {
                name: a.name,
                packages: a.packages,
                status: a.status,
                severity: a.severity,
                fixed: a.fixed,
                issues: a.issues,
            })
            .collect())
    }
}

// --------------------------------------------------------------------- news

/// Parses the RSS 2.0 feed (`<item><title/><link/><pubDate/></item>`).
pub fn parse_feed(xml: &str) -> PortResult<Vec<NewsItem>> {
    let doc = roxmltree::Document::parse(xml)
        .map_err(|e| PortError::data(format!("invalid RSS feed: {e}")))?;
    let text = |item: roxmltree::Node, tag: &str| {
        item.children()
            .find(|c| c.has_tag_name(tag))
            .and_then(|c| c.text())
            .unwrap_or("")
            .trim()
            .to_string()
    };
    Ok(doc
        .descendants()
        .filter(|n| n.has_tag_name("item"))
        .filter_map(|item| {
            Some(NewsItem {
                published: DateTime::parse_from_rfc2822(&text(item, "pubDate")).ok()?,
                title: text(item, "title"),
                link: text(item, "link"),
            })
        })
        .collect())
}

pub struct ArchNewsFeed;

impl NewsFeed for ArchNewsFeed {
    fn items(&self) -> PortResult<Vec<NewsItem>> {
        let xml = agent()
            .get(NEWS_URL)
            .call()
            .map_err(net_err("news feed request"))?
            .body_mut()
            .read_to_string()
            .map_err(|e| PortError::data(format!("news feed: {e}")))?;
        parse_feed(&xml)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_feed() {
        let xml = r#"<?xml version="1.0"?><rss version="2.0"><channel>
            <item><title>Old</title><link>https://a/old</link><pubDate>Tue, 01 Sep 2026 10:00:00 +0000</pubDate></item>
            <item><title>New &amp; shiny</title><link>https://a/new</link><pubDate>Wed, 21 Oct 2026 19:29:36 +0000</pubDate></item>
            <item><title>No date</title><link>https://a/x</link></item>
            </channel></rss>"#;
        let items = parse_feed(xml).unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[1].title, "New & shiny");
    }
}
