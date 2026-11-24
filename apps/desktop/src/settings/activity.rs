//! My Account → Activity: which things may set my status (Steam, other apps) and
//! Discord Rich Presence (#329, #330).

use gpui::{AnyElement, Context, div, prelude::*};

use super::{SettingsView, pages::choice, section, video::switch_row};
use crate::view_model::{activity as vm, presence, steam};

impl SettingsView {
    fn set_activity_group(&mut self, group: &'static str, on: bool, cx: &mut Context<Self>) {
        if let Err(err) = vm::set_group_enabled(&self.data_dir, group, on) {
            eprintln!("morpheus-desktop: saving the {group} status setting: {err}");
        }
        cx.notify();
    }

    fn set_presence(&mut self, prefs: presence::Prefs, cx: &mut Context<Self>) {
        if let Err(err) = presence::save(&self.data_dir, prefs) {
            eprintln!("morpheus-desktop: saving the Discord presence setting: {err}");
        }
        cx.notify();
    }

    /// The status sources' switches, then Discord's.
    pub(super) fn activity_sections(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let steam_on = vm::group_enabled(&self.data_dir, steam::SOURCE);
        let external_on = vm::group_enabled(&self.data_dir, "external");
        let not_kept = self
            .session
            .as_ref()
            .and_then(|s| s.user_id())
            .is_some_and(|user| crate::status_feed::status_not_kept(user.as_str()));
        let mut children = vec![super::muted(vm::SECTION_NOTE, "muted").into_any_element()];
        if not_kept {
            children.push(super::muted(vm::NOT_KEPT_NOTICE, "muted status-not-kept").into_any_element());
        }
        children.extend([
                switch_row(
                    "activity-steam",
                    "activity-steam-switch",
                    vm::STEAM_LABEL,
                    vm::STEAM_HINT,
                    steam_on,
                    cx.listener(move |this, _, _, cx| this.set_activity_group(steam::SOURCE, !steam_on, cx)),
                ),
                switch_row(
                    "activity-external",
                    "activity-external-switch",
                    vm::EXTERNAL_LABEL,
                    vm::EXTERNAL_HINT,
                    external_on,
                    cx.listener(move |this, _, _, cx| this.set_activity_group("external", !external_on, cx)),
                ),
        ]);
        let sources = section("Activity status", "activity-status", children, self.hl("activity-status")).into_any_element();

        let prefs = presence::load(&self.data_dir);
        let mut rows = vec![switch_row(
            "discord-presence",
            "discord-presence-switch",
            presence::SETTING_LABEL,
            presence::SETTING_HINT,
            prefs.on,
            cx.listener(move |this, _, _, cx| {
                this.set_presence(presence::Prefs { on: !prefs.on, ..prefs }, cx)
            }),
        )];
        if !crate::rich_presence::available() {
            rows.push(
                super::muted("Discord Rich Presence is not set up in this build yet.", "muted")
                    .into_any_element(),
            );
        }
        let links = presence::LinkChoice::ALL
            .into_iter()
            .map(|link| {
                choice(
                    format!("discord-link-{}", link.key()),
                    "discord-link",
                    link.key(),
                    link.label(),
                    prefs.link == link,
                    false,
                    false,
                    cx.listener(move |this, _, _, cx| {
                        this.set_presence(presence::Prefs { link, ..prefs }, cx)
                    }),
                )
            })
            .collect::<Vec<_>>();
        rows.push(
            div()
                .flex()
                .flex_col()
                .gap(gpui::px(4.0))
                .mt(gpui::px(8.0))
                .child(super::muted("The button's link", "muted"))
                .children(links)
                .into_any_element(),
        );
        let discord = section("Discord", "discord-presence", rows, self.hl("discord-presence")).into_any_element();
        vec![sources, discord]
    }
}
