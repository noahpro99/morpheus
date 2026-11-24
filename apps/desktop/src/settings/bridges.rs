//! Settings > Bridges (#107): connect other networks through the mautrix bridges the
//! homeserver runs. The page looks for the bridge bots the server has (the well-known names and
//! the ones typed in), and "Connect" messages the bot in a direct message and drives its login:
//! the QR code is the bot's image, its questions are answered in the page, and the status is
//! what the bot last said. What the replies mean is `view_model::bridges`; this sends the
//! commands it names and draws the state.

use std::sync::Arc;

use gpui::{AnyElement, Context, Entity, FontWeight, IntoElement, div, prelude::*, px, rgb};
use morpheus_core::{OwnedUserId, TimelineHandle};

use super::{Kind, SettingsView, button, muted, note, section};
use crate::icons::icon;
use crate::{
    e2e::E2eExt as _,
    input::{Submit, TextInput},
    theme,
    view_model::{
        bridges::{self as vm, Candidate, Login, Reply, Status},
        i18n::{t, tf},
    },
};

/// One bridge bot the server has.
pub(super) struct Row {
    bot: String,
    network: Option<&'static str>,
    login: Login,
    /// The QR code of the bot's latest image.
    qr: Option<Arc<gpui::Image>>,
    /// The conversation with the bot, once it is open.
    handle: Option<TimelineHandle>,
    answer: Entity<TextInput>,
}

pub(super) struct Bridges {
    rows: Vec<Row>,
    /// Looked for the bots at least once.
    pub(super) looked: bool,
    finding: bool,
    /// Bots typed in (kept in the data folder).
    extra: Vec<String>,
    bot_input: Entity<TextInput>,
    note: Option<String>,
}

const SAVED: &str = "bridge_bots.json";

fn load_extra(dir: &std::path::Path) -> Vec<String> {
    std::fs::read_to_string(dir.join(SAVED))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn save_extra(dir: &std::path::Path, bots: &[String]) {
    if let Ok(text) = serde_json::to_string(bots) {
        let _ = std::fs::write(dir.join(SAVED), text);
    }
}

impl Bridges {
    pub(super) fn new(dir: &std::path::Path, cx: &mut Context<SettingsView>) -> Self {
        let bot_input = cx.new(|cx| {
            TextInput::new("bridge-bot", &t("Bot name, e.g. discordbot or @bot:example.org"), false, "", cx).framed()
        });
        cx.subscribe(&bot_input, |this, _, _: &Submit, cx| this.add_bridge_bot(cx)).detach();
        Self {
            rows: Vec::new(),
            looked: false,
            finding: false,
            extra: load_extra(dir),
            bot_input,
            note: None,
        }
    }

    fn row(&mut self, bot: &str) -> Option<&mut Row> {
        self.rows.iter_mut().find(|row| row.bot == bot)
    }
}

impl SettingsView {
    /// Look for the bots again (the page was opened).
    pub(super) fn find_bridges(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let Some(me) = session.user_id() else {
            return;
        };
        if self.bridges.finding {
            return;
        }
        self.bridges.finding = true;
        self.bridges.looked = true;
        let home = me.server_name().to_string();
        let extra = self.bridges.extra.clone();
        cx.spawn(async move |this, cx| {
            // The bots the rooms name (an m.bridge's bridgebot) too, not only the usual names.
            let named = session.room_bridges().await.map(|b| vm::bots_in_rooms(&b)).unwrap_or_default();
            let all: Vec<String> = extra.into_iter().chain(named).collect();
            let candidates = vm::candidates(&home, &all);
            let asks: Vec<_> = candidates.iter().map(|c| session.find_user(c.bot.clone())).collect();
            let answers = futures::future::join_all(asks).await;
            let found: Vec<(Candidate, bool)> = candidates
                .into_iter()
                .zip(answers)
                .map(|(candidate, answer)| (candidate, answer.is_ok()))
                .collect();
            this.update(cx, |this, cx| this.bridges_found(found, cx)).ok();
        })
        .detach();
    }

    fn bridges_found(&mut self, found: Vec<(Candidate, bool)>, cx: &mut Context<Self>) {
        self.bridges.finding = false;
        let mut missing = Vec::new();
        for (candidate, exists) in found {
            let typed = self
                .bridges
                .extra
                .iter()
                .any(|e| vm::normalise_bot(e, candidate.bot.rsplit(':').next().unwrap_or_default()).as_deref() == Some(candidate.bot.as_str()));
            if !exists {
                if typed {
                    missing.push(candidate.bot.clone());
                }
                continue;
            }
            if self.bridges.rows.iter().any(|row| row.bot == candidate.bot) {
                continue;
            }
            let answer = cx.new(|cx| TextInput::new("bridge-answer", &t("Type your answer"), false, "", cx).framed());
            let bot = candidate.bot.clone();
            cx.subscribe(&answer, move |this, input, _: &Submit, cx| {
                let typed = input.read(cx).value().to_owned();
                if typed.trim().is_empty() {
                    return;
                }
                input.update(cx, |input, cx| input.set_value("", cx));
                this.bridge_answer(&bot, &typed, cx);
            })
            .detach();
            self.bridges.rows.push(Row {
                bot: candidate.bot,
                network: candidate.network,
                login: Login::new(candidate.network),
                qr: None,
                handle: None,
                answer,
            });
        }
        self.bridges.note = (!missing.is_empty()).then(|| {
            tf("No bridge bot was found on this server called %{bot}.", &[("bot", &missing.join(", "))])
        });
        cx.notify();
    }

    /// The bot name typed in the field is added to the list to look for.
    pub(super) fn add_bridge_bot(&mut self, cx: &mut Context<Self>) {
        let typed = self.bridges.bot_input.read(cx).value().to_owned();
        let Some(me) = self.session.as_ref().and_then(|s| s.user_id()) else {
            return;
        };
        match vm::normalise_bot(&typed, me.server_name().as_str()) {
            Some(bot) => {
                self.bridges.note = None;
                if !self.bridges.extra.contains(&bot) {
                    self.bridges.extra.push(bot);
                    save_extra(&self.data_dir, &self.bridges.extra);
                }
                self.bridges.bot_input.update(cx, |input, cx| input.set_value("", cx));
                self.find_bridges(cx);
            }
            None => {
                self.bridges.note = Some(tf("\"%{bot}\" is not a bot name.", &[("bot", typed.trim())]));
                cx.notify();
            }
        }
    }

    /// Send commands to a bot, opening the direct message with it first when need be.
    fn bridge_send(&mut self, bot: &str, commands: Vec<String>, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let Some(row) = self.bridges.row(bot) else {
            return;
        };
        if let Some(handle) = row.handle.clone() {
            cx.spawn(async move |this, cx| {
                for command in commands {
                    if let Err(err) = handle.send_text(command).await {
                        this.update(cx, |this, cx| this.bridge_failed(&err.to_string(), cx)).ok();
                        break;
                    }
                }
            })
            .detach();
            return;
        }
        let bot = bot.to_owned();
        let Ok(user) = OwnedUserId::try_from(bot.as_str()) else {
            return;
        };
        cx.spawn(async move |this, cx| {
            let opened = async {
                let room = session.bridge_dm_with(user).await?;
                session.timeline(room).await
            }
            .await;
            let mut open = match opened {
                Ok(open) => open,
                Err(err) => {
                    this.update(cx, |this, cx| this.bridge_error(&bot, &err.to_string(), cx)).ok();
                    return;
                }
            };
            let handle = open.handle.clone();
            let mut seen: std::collections::HashMap<String, String> = std::collections::HashMap::new();
            let mut entries: morpheus_core::Vector<morpheus_core::TimelineEntry> = morpheus_core::Vector::new();
            // What was said before is not an answer: the first batch is the history.
            if let Some(first) = open.updates.next().await {
                for diff in &first {
                    diff.clone().apply(&mut entries);
                    for message in vm::messages_in(diff, &bot) {
                        seen.insert(message.0, vm::fingerprint(&message.1));
                    }
                }
            }
            let anchor: Option<String> = vm::message_ids(&entries).last().map(|id| (*id).to_owned());
            this.update(cx, |this, _| {
                if let Some(row) = this.bridges.row(&bot) {
                    row.handle = Some(handle.clone());
                }
            })
            .ok();
            for command in commands {
                if let Err(err) = handle.send_text(command).await {
                    this.update(cx, |this, cx| this.bridge_error(&bot, &err.to_string(), cx)).ok();
                    return;
                }
            }
            while let Some(diffs) = open.updates.next().await {
                for diff in &diffs {
                    diff.clone().apply(&mut entries);
                    for (id, kind) in vm::messages_in(diff, &bot) {
                        // Older pages load after the first batch and arrive above my message: history, not
                        // what the bot said to this command (#319, Android's #316).
                        if !seen.contains_key(&id) {
                            if !vm::after_anchor(&vm::message_ids(&entries), anchor.as_deref(), &id) {
                                seen.insert(id, vm::fingerprint(&kind));
                                continue;
                            }
                        }
                        if seen.insert(id, vm::fingerprint(&kind)).is_some_and(|before| before == vm::fingerprint(&kind)) {
                            continue;
                        }
                        let Some((reply, media)) = vm::reply_of(&kind) else {
                            continue;
                        };
                        let picture = match media {
                            Some(media) => session
                                .media_file(media)
                                .await
                                .ok()
                                .flatten()
                                .and_then(|file| crate::crop_view::picture_from(file.bytes)),
                            None => vm::local_qr(&reply).and_then(crate::crop_view::picture_from),
                        };
                        let gone = this
                            .update(cx, |this, cx| this.bridge_reply(&bot, reply, picture, cx))
                            .is_err();
                        if gone {
                            return;
                        }
                    }
                }
            }
        })
        .detach();
    }

    fn bridge_failed(&mut self, what: &str, cx: &mut Context<Self>) {
        self.bridges.note = Some(what.to_owned());
        cx.notify();
    }

    fn bridge_error(&mut self, bot: &str, what: &str, cx: &mut Context<Self>) {
        if let Some(row) = self.bridges.row(bot) {
            row.login.status = Status::Error(what.to_owned());
        }
        cx.notify();
    }

    /// The bot said something.
    fn bridge_reply(&mut self, bot: &str, reply: Reply, picture: Option<Arc<gpui::Image>>, cx: &mut Context<Self>) {
        let Some(row) = self.bridges.row(bot) else {
            return;
        };
        if picture.is_some() {
            row.qr = picture;
        }
        let sends = row.login.on_reply(&reply);
        if !matches!(row.login.status, Status::Qr) {
            row.qr = None;
        }
        if !sends.is_empty() {
            self.bridge_send(bot, sends, cx);
        }
        cx.notify();
    }

    fn bridge_do(&mut self, bot: &str, what: fn(&mut Login) -> Vec<String>, cx: &mut Context<Self>) {
        let Some(row) = self.bridges.row(bot) else {
            return;
        };
        let commands = what(&mut row.login);
        row.qr = None;
        self.bridge_send(bot, commands, cx);
        cx.notify();
    }

    fn bridge_answer(&mut self, bot: &str, typed: &str, cx: &mut Context<Self>) {
        let Some(row) = self.bridges.row(bot) else {
            return;
        };
        let commands = row.login.answer(typed);
        self.bridge_send(bot, commands, cx);
        cx.notify();
    }

    pub(super) fn bridges_page(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        if !self.bridges.looked {
            self.find_bridges(cx);
        }
        let mut rows: Vec<AnyElement> = Vec::new();
        let bots: Vec<String> = self.bridges.rows.iter().map(|r| r.bot.clone()).collect();
        for (index, bot) in bots.iter().enumerate() {
            rows.push(self.bridge_row(index, bot, cx));
        }
        if rows.is_empty() {
            let (text, class) = if self.bridges.finding || !self.bridges.looked {
                ("Looking for bridges...", "muted bridge-looking")
            } else {
                (
                    "No bridge bots were found on your server. If your bridge's bot has another name, add it below.",
                    "muted bridge-none",
                )
            };
            rows.push(muted(text, class).into_any_element());
        }
        let mut add: Vec<AnyElement> = vec![
            muted(
                "Bridges are run by your server's administrator. If the bot of yours has another name, type it here.",
                "muted",
            )
            .into_any_element(),
            self.bridges.bot_input.clone().into_any_element(),
        ];
        if let Some(text) = &self.bridges.note {
            add.push(note(text, "error bridge-note", theme::danger()).into_any_element());
        }
        vec![
            section(
                "Connect a network",
                "bridges",
                std::iter::once(
                    muted(
                        "Connect chats from other networks. Morpheus messages the bridge's bot for you and shows what to scan or type.",
                        "muted",
                    )
                    .into_any_element(),
                )
                .chain(rows)
                .collect(),
                self.hl("bridges"),
            )
            .into_any_element(),
            section("Another bridge bot", "bridge-bot", add, self.hl("bridge-bot")).into_any_element(),
        ]
    }

    fn bridge_row(&mut self, index: usize, bot: &str, cx: &mut Context<Self>) -> AnyElement {
        let Some(row) = self.bridges.rows.iter().find(|r| r.bot == bot) else {
            return div().into_any_element();
        };
        let network = row.network.and_then(vm::network);
        let name = network.map_or_else(|| bot.to_owned(), |n| n.name.to_owned());
        let status = row.login.status.clone();
        let label = match &status {
            Status::LoggedIn(Some(who)) => format!("{} {}", t(status.label()), tf("as %{who}", &[("who", who)])),
            _ => t(status.label()),
        };
        let colour = match &status {
            Status::LoggedIn(_) => theme::accent(),
            Status::Error(_) | Status::NeedsLogin(_) => theme::danger(),
            _ => theme::text_muted(),
        };
        let header = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(name.clone())
                            .e2e("span", "bridge-name")
                            .text(name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(0.75 * theme::REM))
                            .text_color(rgb(theme::text_muted()))
                            .child(bot.to_owned())
                            .e2e("span", "bridge-bot muted")
                            .text(bot.to_owned()),
                    ),
            )
            .child(
                div()
                    .text_color(rgb(colour))
                    .child(label.clone())
                    .e2e("span", "bridge-status")
                    .text(label),
            );
        let mut body: Vec<AnyElement> = Vec::new();
        if status.is_connecting() {
            let steps = network.map_or(vm::GENERIC_STEPS, |n| n.steps);
            body.push(muted(steps, "muted bridge-steps").into_any_element());
        }
        if let (Status::Qr, Some(qr)) = (&status, &row.qr) {
            body.push(
                div()
                    .flex()
                    .justify_center()
                    .p(px(12.0))
                    .rounded(px(theme::RADIUS_SM))
                    .bg(rgb(0xffffff)) // fixed: a QR code is black on white
                    .child(
                        gpui::img(crate::media::drawn(qr.clone()))
                            .size(px(220.0))
                            .object_fit(gpui::ObjectFit::Contain)
                            .e2e("img", "bridge-qr"),
                    )
                    .into_any_element(),
            );
        }
        if let Status::Prompt(question) = &status {
            body.push(
                div()
                    .child(question.clone())
                    .e2e("p", "bridge-question")
                    .text(question.clone())
                    .into_any_element(),
            );
            body.push(row.answer.clone().into_any_element());
        }
        if let Status::Error(what) | Status::NeedsLogin(what) = &status {
            let text = what.clone();
            body.push(
                div()
                    .text_color(rgb(theme::danger()))
                    .child(text.clone())
                    .e2e("p", "bridge-error")
                    .text(text)
                    .into_any_element(),
            );
        }
        let id = |what: &str| format!("bridge-{what}-{index}");
        let mut buttons = div().flex().gap(px(8.0));
        let bot_id = bot.to_owned();
        let connecting = status.is_connecting() || status == Status::Checking;
        if status.is_logged_in() {
            let bot = bot_id.clone();
            buttons = buttons.child(
                button(id("logout"), "Log out", Kind::Danger, "bridge-logout")
                    .child(icon("log-out", 16.0).text_color(rgb(theme::danger())))
                    .child(t("Log out"))
                    .on_click(cx.listener(move |this, _, _, cx| this.bridge_do(&bot, Login::log_out, cx)))
                    .e2e("button", "button danger bridge-logout")
                    .text(t("Log out")),
            );
        } else if connecting {
            let bot = bot_id.clone();
            buttons = buttons.child(
                button(id("cancel"), "Cancel", Kind::Secondary, "bridge-cancel")
                    .child(icon("x", 16.0).text_color(rgb(theme::text())))
                    .child(t("Cancel"))
                    .on_click(cx.listener(move |this, _, _, cx| this.bridge_do(&bot, Login::cancel, cx)))
                    .e2e("button", "button secondary bridge-cancel")
                    .text(t("Cancel")),
            );
        } else {
            let bot = bot_id.clone();
            let text = if matches!(status, Status::NeedsLogin(_)) { "Log in again" } else { "Connect" };
            buttons = buttons.child(
                button(id("connect"), text, Kind::Primary, "bridge-connect")
                    .child(icon("link", 16.0).text_color(rgb(theme::on_accent_solid())))
                    .child(t(text))
                    .on_click(cx.listener(move |this, _, _, cx| this.bridge_do(&bot, Login::start, cx)))
                    .e2e("button", "button primary bridge-connect")
                    .text(t(text)),
            );
        }
        if !connecting {
            let bot = bot_id.clone();
            buttons = buttons.child(
                button(id("check"), "Check status", Kind::Secondary, "bridge-check")
                    .child(icon("refresh-cw", 16.0).text_color(rgb(theme::text())))
                    .child(t("Check status"))
                    .on_click(cx.listener(move |this, _, _, cx| this.bridge_do(&bot, Login::check, cx)))
                    .e2e("button", "button secondary bridge-check")
                    .text(t("Check status")),
            );
        }
        let mut card = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(12.0))
            .rounded(px(theme::RADIUS))
            .bg(theme::fill_elevated())
            .child(header);
        for part in body {
            card = card.child(part);
        }
        card.child(buttons)
            .e2e("div", "bridge-row")
            .attr("data-network", network.map_or("", |n| n.id))
            .attr("data-bot", bot.to_owned())
            .into_any_element()
    }
}
