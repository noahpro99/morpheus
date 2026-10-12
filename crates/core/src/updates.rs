//! "Check for updates": compare this build's version with a release feed.
//!
//! The feed is a small JSON document, `{"version": "0.2.0", "url": "…"}`,
//! at the address in `MORPHEUS_UPDATE_FEED` (read when checking, falling
//! back to the value the binary was built with). Nothing is downloaded or
//! installed; the caller shows the result and can open the release page.

use serde::Deserialize;

use crate::{CoreFuture, Error, Session};

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    /// This build has no feed to ask.
    NoFeed,
    UpToDate,
    Available {
        version: String,
        /// Where to get it, when the feed says.
        url: Option<String>,
    },
}

#[derive(Deserialize)]
struct Feed {
    version: String,
    url: Option<String>,
}

fn feed_url() -> Option<String> {
    std::env::var("MORPHEUS_UPDATE_FEED")
        .ok()
        .or_else(|| option_env!("MORPHEUS_UPDATE_FEED").map(str::to_owned))
        .filter(|url| !url.trim().is_empty())
}

/// Dotted numeric versions, compared field by field; a missing field is 0
/// and anything after a `-` or `+` (pre-release, build) is ignored.
fn is_newer(candidate: &str, current: &str) -> bool {
    fn fields(version: &str) -> Vec<u64> {
        version
            .trim()
            .trim_start_matches('v')
            .split(['-', '+'])
            .next()
            .unwrap_or_default()
            .split('.')
            .map(|field| field.parse().unwrap_or(0))
            .collect()
    }
    let (a, b) = (fields(candidate), fields(current));
    let len = a.len().max(b.len());
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    (0..len)
        .map(|i| at(&a, i).cmp(&at(&b, i)))
        .find(|order| order.is_ne())
        .is_some_and(|order| order.is_gt())
}

impl Session {
    /// Ask the release feed whether something newer than `current` exists.
    pub fn check_for_update(&self, current: &'static str) -> CoreFuture<UpdateStatus> {
        let http = self.client.http_client().clone();
        self.core.run(async move {
            let Some(url) = feed_url() else {
                return Ok(UpdateStatus::NoFeed);
            };
            let failed = |err: &dyn std::fmt::Display| Error::Other(format!("update feed: {err}"));
            let response = http
                .get(&url)
                .timeout(std::time::Duration::from_secs(15))
                .send()
                .await
                .and_then(|response| response.error_for_status())
                .map_err(|err| failed(&err))?;
            let body = response.text().await.map_err(|err| failed(&err))?;
            let feed: Feed = serde_json::from_str(&body).map_err(|err| failed(&err))?;
            Ok(if is_newer(&feed.version, current) {
                UpdateStatus::Available {
                    version: feed.version,
                    url: feed.url,
                }
            } else {
                UpdateStatus::UpToDate
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    fn versions_compare_by_field() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.99.99"));
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("0.1.0-beta.2", "0.1.0"));
    }
}
