//! Settings > Help & Feedback and the update notice (the desktop's Help page, `update.rs`): the version, the release notes page, the
//! diagnostics a bug report carries, and whether a newer release is out. The wording and rules are `view_model`'s.

use morpheus_view_model::{about, diagnostics, release_notes, settings, updates};

use crate::{FfiError, Morpheus};

/// What the Help page shows first.
#[derive(uniffi::Record)]
pub struct HelpInfo {
    /// "Morpheus 0.2.9".
    pub version_line: String,
    /// This version's release notes on the web.
    pub release_page: String,
}

/// A newer release, as the notice words it.
#[derive(Clone, uniffi::Record)]
pub struct UpdateOffer {
    pub version: String,
    pub headline: String,
    pub detail: String,
    /// Where to get it (the release page).
    pub link: String,
}

#[uniffi::export]
pub fn help_info() -> HelpInfo {
    HelpInfo { version_line: settings::version_line(), release_page: release_notes::release_page(about::VERSION) }
}

impl Morpheus {
    fn phone_report(&self, os: &str, device: &str, verified: Option<bool>) -> diagnostics::Report {
        let account = self.account();
        let (homeserver, device_id) = account.map(|a| (a.homeserver, a.device_id)).unwrap_or_default();
        diagnostics::phone_report(about::VERSION, os, device, &homeserver, &device_id, verified)
    }
}

#[uniffi::export]
impl Morpheus {
    /// The diagnostics as text (what "Copy all" puts on the clipboard), for this phone (`os`, `device` as the system names them).
    pub async fn diagnostics_text(&self, os: String, device: String) -> String {
        let verified = match self.session() {
            Ok(session) => session.encryption_status().await.ok().map(|s| s.device_verified),
            Err(_) => None,
        };
        self.phone_report(&os, &device, verified).text()
    }

    /// The page that starts a bug report with the diagnostics filled in.
    pub async fn bug_report_url(&self, os: String, device: String) -> String {
        let report = self.phone_report(&os, &device, None);
        morpheus_core::bug_report_url_with(about::VERSION, &self.dir, &report.text_within(2200))
    }

    /// Whether a newer release than this one is out (the desktop's check; `None` when this is the latest or the feed is unreachable).
    pub async fn check_for_update(&self) -> Result<Option<UpdateOffer>, FfiError> {
        let status = self.session()?.check_for_update(about::VERSION).await?;
        Ok(match status {
            morpheus_core::UpdateStatus::Available { version, url, download } => {
                let offer = updates::Offer { version: version.clone(), url, download };
                // A phone is updated by what installed it: the link is the release page.
                match offer.action(false) {
                    updates::Action::Download(link) => Some(UpdateOffer {
                        version,
                        headline: morpheus_view_model::i18n::tf("Morpheus %{version} is available", &[("version", &offer.version)]),
                        detail: morpheus_view_model::i18n::t(offer.detail(false)),
                        link,
                    }),
                    _ => None,
                }
            }
            _ => None,
        })
    }
}
