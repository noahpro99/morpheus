//! The update card (`UpdateCard` in `crates/ui/src/components/about.rs`): the
//! app looks for a newer release by itself (when it starts and every few
//! hours) and offers it; Update downloads this platform's build, puts it in
//! place of the running one and starts it (`morpheus_core::Session::install_update`).

use std::time::Duration;

use gpui::{AnyElement, Context, FontWeight, IntoElement, SharedString, div, prelude::*, px, rgb, rgba};
use morpheus_core::UpdateStatus;

use crate::{
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
                    Err(err) => eprintln!("morpheus-gpui: update check failed: {err}"),
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
    fn install_update(&mut self, download: String, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        if self.update_busy {
            return;
        }
        self.update_busy = true;
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

/// The card, when a newer release is out and "Later" was not pressed.
pub fn card(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let offer = shell.update.clone().filter(|_| !shell.update_later)?;
    let action = offer.action(morpheus_core::installs_updates());
    let button = |id: &'static str, label: &'static str, primary: bool| {
        div()
            .id(id)
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(theme::RADIUS))
            .cursor_pointer()
            .bg(if primary { rgb(theme::accent()) } else { rgba(theme::bg_hover()) })
            .text_color(if primary { rgb(0xffffff) } else { rgb(theme::text()) })
            .child(label)
    };
    let mut actions = div().flex().gap(px(8.0)).justify_end().mt(px(6.0)).child(
        button("update-later", "Later", false)
            .on_click(cx.listener(|this, _, _, cx| {
                this.update_later = true;
                cx.notify();
            }))
            .e2e("button", "secondary")
            .text("Later"),
    );
    match &action {
        Action::Install(download) => {
            let download = download.clone();
            let busy = shell.update_busy;
            let label = if busy { "Updating…" } else { "Update" };
            actions = actions.child(
                button("update-now", label, true)
                    .when(busy, |d| d.opacity(0.6))
                    .on_click(cx.listener(move |this, _, _, cx| this.install_update(download.clone(), cx)))
                    .e2e("button", "primary update-button")
                    .text(label),
            );
        }
        Action::Download(link) => {
            let link = link.clone();
            let target = link.clone();
            actions = actions.child(
                button("update-download", "Download", true)
                    .on_click(cx.listener(move |this, _, _, cx| this.link_clicked(&target, cx)))
                    .e2e("a", "button primary update-button")
                    .attr("href", link)
                    .text("Download"),
            );
        }
        Action::None => {}
    }
    let headline = offer.headline();
    let detail = offer.detail(matches!(action, Action::Install(_)));
    Some(
        div()
            .absolute()
            .right(px(16.0))
            .top(px(48.0))
            .w(px(360.0))
            .flex()
            .gap(px(12.0))
            .px(px(16.0))
            .py(px(14.0))
            .rounded(px(10.0))
            .bg(rgb(theme::bg_elevated()))
            .shadow_md()
            .occlude()
            .child(div().mt(px(2.0)).child(icon("refresh-cw", 18.0).text_color(rgb(theme::accent()))))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(4.0))
                    .text_size(px(0.9 * theme::REM))
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .child(headline.clone())
                            .e2e("strong", "")
                            .text(headline),
                    )
                    .child(
                        div()
                            .text_color(rgb(theme::text_muted()))
                            .child(SharedString::from(detail))
                            .e2e("span", "")
                            .text(detail),
                    )
                    .child(actions),
            )
            .e2e("div", "backup-prompt update-card")
            .attr("role", "status")
            .into_any_element(),
    )
}
