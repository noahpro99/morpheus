//! The card offering a newer release (`UpdateCard` in
//! `crates/ui/src/components/about.rs`): what it says, and whether it
//! installs in place or links to the release.

/// How often the app looks for a new release while it runs.
pub const CHECK_EVERY_SECS: u64 = 6 * 60 * 60;

/// A newer release the feed named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub version: String,
    /// The release page.
    pub url: Option<String>,
    /// This platform's build.
    pub download: Option<String>,
}

/// What the card does about an [`Offer`] on this install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Update installs the download and restarts.
    Install(String),
    /// Not the app's to change: a link to fetch the release by.
    Download(String),
    None,
}

impl Offer {
    /// `installs`: the app's folder takes new files.
    pub fn action(&self, installs: bool) -> Action {
        if installs && let Some(download) = &self.download {
            return Action::Install(download.clone());
        }
        // An install that isn't the app's to change wants its own package: the
        // release page rather than the zip.
        let link = if installs {
            self.download.clone().or_else(|| self.url.clone())
        } else {
            self.url.clone().or_else(|| self.download.clone())
        };
        match link.filter(|link| !link.is_empty()) {
            Some(link) => Action::Download(link),
            None => Action::None,
        }
    }

    pub fn headline(&self) -> String {
        format!("Morpheus {} is available", self.version)
    }

    pub fn detail(&self, installing: bool) -> &'static str {
        if installing {
            "Update now: Morpheus restarts on the new version."
        } else {
            "Download it to update."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offer(url: Option<&str>, download: Option<&str>) -> Offer {
        Offer {
            version: "99.0.0".into(),
            url: url.map(str::to_owned),
            download: download.map(str::to_owned),
        }
    }

    #[test]
    fn a_writable_install_updates_in_place() {
        let o = offer(Some("https://r/release"), Some("https://r/app.zip"));
        assert_eq!(o.action(true), Action::Install("https://r/app.zip".into()));
    }

    #[test]
    fn a_read_only_install_links_to_the_release_page() {
        let o = offer(Some("https://r/release"), Some("https://r/app.zip"));
        assert_eq!(o.action(false), Action::Download("https://r/release".into()));
        assert_eq!(offer(None, Some("https://r/app.zip")).action(false), Action::Download("https://r/app.zip".into()));
    }

    #[test]
    fn without_a_download_a_writable_install_links_to_the_page() {
        assert_eq!(offer(Some("https://r/release"), None).action(true), Action::Download("https://r/release".into()));
        assert_eq!(offer(None, None).action(true), Action::None);
    }

    #[test]
    fn the_card_names_the_version() {
        let o = offer(None, None);
        assert_eq!(o.headline(), "Morpheus 99.0.0 is available");
        assert!(o.detail(true).contains("restarts"));
    }
}
