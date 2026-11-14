//! The update button in the top bar: the app looks for a newer release by itself
//! (when it starts and every few hours) and offers it; the top bar button
//! shows an update/download action. Update downloads this platform's build, puts it
//! in place of the running one and starts it (`morpheus_core::Session::install_update`).

use std::time::Duration;

use gpui::{AnyElement, Context, IntoElement, div, prelude::*, px, rgb, rgba};
use morpheus_core::UpdateStatus;

use crate::{
    chrome,
    e2e::E2eExt as _,
    icons::icon,
    shell::Shell,
    theme,
    view_model::{
        about::VERSION,
        updates::{Action, CHECK_EVERY_SECS, Offer},
    },
};

impl Shell {
    /// Look for a newer release now and then, for as long as the app runs.
    pub fn start_update_checks(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        cx.spawn(async move |this, cx| {
            loop {
                match session.check_for_update(VERSION).await {
                    Ok(UpdateStatus::Available { version, url, download }) => {
                        let offer = Offer { version, url, download };
                        let alive = this
                            .update(cx, |this, cx| {
                                if this.update.as_ref() != Some(&offer) {
                                    this.update = Some(offer);
                                    this.update_later = false;
                                    cx.notify();
                                }
                            })
                            .is_ok();
                        if !alive {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(err) => eprintln!("morpheus-desktop: update check failed: {err}"),
                }
                cx.background_executor()
                    .timer(Duration::from_secs(CHECK_EVERY_SECS))
                    .await;
            }
        })
        .detach();
    }

    /// Update: install the download, then leave to the new version (it waits
    /// for this one to let go of the data).
    pub fn install_update(&mut self, download: String, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        if self.update_busy {
            return;
        }
        self.update_busy = true;
        self.toast("Downloading update…", cx);
        cx.notify();
        let installing = session.install_update(download);
        cx.spawn(async move |this, cx| {
            match installing.await {
                Ok(()) => std::process::exit(0),
                Err(err) => {
                    this.update(cx, |this, cx| {
                        this.update_busy = false;
                        this.fail("updating", &err.to_string(), cx);
                    })
                    .ok();
                }
            }
        })
        .detach();
    }
}

/// The download/update button in the top bar when a newer release is out.
pub fn topbar_button(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let offer = shell.update.clone()?;
    let action = offer.action(morpheus_core::installs_updates());
    let headline = offer.headline();
    let version = offer.version.clone();
    let busy = shell.update_busy;

    let button_el = match action {
        Action::Install(download) => {
            let mut b = div()
                .id("update-now")
                .relative()
                .size(px(28.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::RADIUS_SM))
                .text_color(rgb(theme::green()))
                .cursor_pointer()
                .hover(|s| s.bg(rgba(theme::alpha(theme::green(), 0x26))))
                .child(icon(if busy { "refresh-cw" } else { "download" }, 16.0));
            if busy {
                b = b.opacity(0.6).cursor_default();
            } else {
                b = b.on_click(cx.listener(move |this, _, _, cx| {
                    this.install_update(download.clone(), cx);
                }));
            }
            b.e2e("button", "primary update-button")
                .attr("title", headline.clone())
                .text(version.clone())
                .into_any_element()
        }
        Action::Download(link) => {
            let target = link.clone();
            div()
                .id("update-download")
                .relative()
                .size(px(28.0))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(theme::RADIUS_SM))
                .text_color(rgb(theme::green()))
                .cursor_pointer()
                .hover(|s| s.bg(rgba(theme::alpha(theme::green(), 0x26))))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.link_clicked(&target, cx);
                }))
                .child(icon("download", 16.0))
                .e2e("a", "button primary update-button")
                .attr("href", link)
                .attr("title", headline.clone())
                .text(version.clone())
                .into_any_element()
        }
        Action::None => return None,
    };

    Some(
        chrome::no_drag(
            div()
                .id("titlebar-update")
                .flex()
                .items_center()
                .child(button_el),
        )
        .e2e("div", "titlebar-update update-card")
        .attr("role", "status")
        .attr("title", headline)
        .text(version)
        .into_any_element(),
    )
}
