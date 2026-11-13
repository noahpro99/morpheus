//! The quick switcher, Ctrl+K (`crates/ui/src/components/switcher.rs`): type
//! to filter rooms and actions, arrows to move, Enter to pick. Rooms come
//! first; actions match on their label. Opens the room it is given without
//! joining a voice channel's call. The settings entries of the Dioxus
//! palette are not ported.

use std::sync::Arc;

use gpui::{
    BoxShadow, Context, Entity, EventEmitter, FontWeight, IntoElement, MouseButton, Render, Window, div, point,
    prelude::*, px, rgb, rgba,
};
use morpheus_core::{OwnedRoomId, RoomSummary};

use crate::view_model::settings::find_settings;
use crate::{
    e2e::E2eExt as _,
    icons::icon,
    input::{Changed, Nav, NavKey, TextInput},
    media::avatar,
    theme,
};

/// What the switcher asks of the shell.
pub enum SwitcherEvent {
    /// One setting, opened on its page.
    Setting(&'static crate::view_model::settings::SettingEntry),
    Room(OwnedRoomId),
    Action(Action),
    Close,
}

impl EventEmitter<SwitcherEvent> for Switcher {}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    Settings,
    Shortcuts,
    Inbox,
    CheckForUpdates,
    Acknowledgements,
}

impl Action {
    const ALL: [Action; 5] = [
        Action::Settings,
        Action::Shortcuts,
        Action::Inbox,
        Action::CheckForUpdates,
        Action::Acknowledgements,
    ];

    fn label(self) -> &'static str {
        match self {
            Action::Settings => "Settings",
            Action::Shortcuts => "Keyboard shortcuts",
            Action::Inbox => "Inbox",
            Action::CheckForUpdates => "Check for updates",
            Action::Acknowledgements => "Acknowledgements",
        }
    }

    fn glyph(self) -> &'static str {
        match self {
            Action::Settings => "settings",
            Action::Shortcuts => "circle-help",
            Action::Inbox => "inbox",
            Action::CheckForUpdates => "refresh-cw",
            Action::Acknowledgements => "heart",
        }
    }
}

/// A room the switcher can open, with where it lives.
pub struct SwitchRoom {
    pub id: OwnedRoomId,
    pub name: String,
    /// Its server, "Direct message", or "Rooms".
    pub kind: String,
    /// What its picture's letter comes from: the server's name for a channel.
    pub letter_of: String,
    pub picture: Option<Arc<gpui::Image>>,
    /// A server's picture is a rounded square, a person's or room's a circle.
    pub squircle: bool,
    /// The name it has without your nickname (a DM's person), also matched.
    pub real_name: String,
}

#[derive(Clone)]
enum Entry {
    Setting(&'static crate::view_model::settings::SettingEntry),
    Room(usize),
    Action(Action),
}

pub struct Switcher {
    input: Entity<TextInput>,
    pub rooms: Vec<SwitchRoom>,
    cursor: usize,
    focus_next: bool,
}

impl Switcher {
    pub fn new(rooms: Vec<SwitchRoom>, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            TextInput::new("switcher", "Jump to a room, an action or a setting…", false, "", cx)
                .bare("input", "")
        });
        input.update(cx, |input, _| input.set_capture_nav(true));
        cx.subscribe(&input, |this, _, _: &Changed, cx| {
            this.cursor = 0;
            cx.notify();
        })
        .detach();
        cx.subscribe(&input, |this, _, nav: &Nav, cx| match nav.0 {
            NavKey::Down => {
                this.cursor += 1;
                cx.notify();
            }
            NavKey::Up => {
                this.cursor = this.cursor.saturating_sub(1);
                cx.notify();
            }
            NavKey::Enter => this.choose(this.cursor_now(cx), cx),
            NavKey::Escape => cx.emit(SwitcherEvent::Close),
            NavKey::Tab => {}
        })
        .detach();
        Self {
            input,
            rooms,
            cursor: 0,
            focus_next: true,
        }
    }

    fn entries(&self, cx: &gpui::App) -> Vec<Entry> {
        let needle = self.input.read(cx).value().to_lowercase();
        let mut list: Vec<Entry> = self
            .rooms
            .iter()
            .enumerate()
            .filter(|(_, room)| needle.is_empty() || room.name.to_lowercase().contains(&needle) || room.real_name.to_lowercase().contains(&needle))
            .take(8)
            .map(|(index, _)| Entry::Room(index))
            .collect();
        list.extend(
            Action::ALL
                .into_iter()
                .filter(|action| needle.is_empty() || action.label().to_lowercase().contains(&needle))
                .map(Entry::Action),
        );
        // Settings, found by their name or the words they answer to.
        list.extend(find_settings(&needle, true).into_iter().map(Entry::Setting));
        list
    }

    fn cursor_now(&self, cx: &gpui::App) -> usize {
        self.cursor
            .min(self.entries(cx).len().saturating_sub(1))
    }

    fn choose(&mut self, index: usize, cx: &mut Context<Self>) {
        match self.entries(cx).get(index) {
            Some(Entry::Room(room)) => {
                let id = self.rooms[*room].id.clone();
                cx.emit(SwitcherEvent::Room(id));
            }
            Some(Entry::Action(action)) => cx.emit(SwitcherEvent::Action(*action)),
            Some(Entry::Setting(setting)) => cx.emit(SwitcherEvent::Setting(setting)),
            None => cx.emit(SwitcherEvent::Close),
        }
    }
}

/// The rooms the switcher offers: not servers, not invites; each with its
/// place. (`scope_name` says where a room lives.)
pub fn rooms_for(
    rooms: &[RoomSummary],
    nicknames: &crate::view_model::nicknames::NickMap,
    scope_name: impl Fn(&RoomSummary) -> (String, Option<Arc<gpui::Image>>, Option<String>),
) -> Vec<SwitchRoom> {
    rooms
        .iter()
        .filter(|room| !room.is_space && !room.invited)
        .map(|room| {
            let (kind, picture, letter_of) = scope_name(room);
            SwitchRoom {
                id: room.id.clone(),
                name: crate::view_model::nicknames::room_title(nicknames, room),
                real_name: room.name.clone(),
                squircle: letter_of.is_some(),
                kind,
                picture,
                letter_of: letter_of.unwrap_or_else(|| room.name.clone()),
            }
        })
        .collect()
}

impl Render for Switcher {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.focus_next) {
            self.input.read(cx).focus(window);
        }
        let viewport = window.viewport_size();
        let entries = self.entries(cx);
        let cursor = self.cursor.min(entries.len().saturating_sub(1));
        let mut list = div().flex().flex_col().p(px(6.0)).gap(px(0.0));
        for (index, entry) in entries.iter().enumerate() {
            let active = index == cursor;
            let (classes, text) = match entry {
                Entry::Room(room) => ("switcher-item", self.rooms[*room].name.clone()),
                Entry::Action(action) => ("switcher-item action", action.label().to_owned()),
                Entry::Setting(setting) => ("switcher-item", setting.label.to_owned()),
            };
            let classes = if active {
                format!("{classes} active")
            } else {
                classes.to_owned()
            };
            let row = div()
                .id(("switch", index))
                .flex()
                .items_center()
                .px(px(10.0))
                .py(px(8.0))
                .rounded(px(theme::RADIUS_SM))
                .cursor_pointer()
                .when(active, |d| d.bg(rgba(theme::bg_selected())))
                .hover(|s| s.bg(rgba(theme::bg_selected())))
                .on_click(cx.listener(move |this, _, _, cx| this.choose(index, cx)));
            let row = match entry {
                Entry::Room(room) => {
                    let room = &self.rooms[*room];
                    row.child(avatar(room.picture.clone(), &room.letter_of, 24.0, room.squircle.then_some(8.0)))
                        .child(
                            div().ml(px(8.0))
                                .child(room.name.clone())
                                .e2e("span", "name")
                                .text(room.name.clone()),
                        )
                        .child(
                            div()
                                .ml_auto()
                                .text_size(px(0.8 * theme::REM))
                                .text_color(rgb(theme::text_faint()))
                                .child(room.kind.clone())
                                .e2e("span", "kind")
                                .text(room.kind.clone()),
                        )
                }
                Entry::Setting(setting) => row
                    .child(icon(setting.page.glyph(), 16.0).text_color(rgb(theme::text_faint())))
                    .child(
                        div().ml(px(8.0))
                            .child(setting.label)
                            .e2e("span", "name")
                            .text(setting.label),
                    )
                    .child(
                        div()
                            .ml_auto()
                            .text_size(px(0.8 * theme::REM))
                            .text_color(rgb(theme::text_faint()))
                            .child(format!("Settings · {}", setting.page.label()))
                            .e2e("span", "kind")
                            .text(format!("Settings · {}", setting.page.label())),
                    ),
                Entry::Action(action) => row
                    .child(icon(action.glyph(), 16.0).text_color(rgb(theme::text_faint())))
                    .child(div().ml(px(8.0)).child(action.label()).e2e("span", "action-label").text(action.label())),
            };
            let key = match entry {
                Entry::Room(room) => Some(self.rooms[*room].id.to_string()),
                _ => None,
            };
            let item = row.e2e("li", &classes).text(text);
            list = list.child(match key {
                Some(key) => item.attr("data-key", key),
                None => item,
            });
        }
        if entries.is_empty() {
            list = list.child(
                div()
                    .px(px(10.0))
                    .py(px(8.0))
                    .text_color(rgb(theme::text_muted()))
                    .child("Nothing matches")
                    .e2e("li", "switcher-item muted")
                    .text("Nothing matches"),
            );
        }
        let panel = div()
            .w(px(520.0).min(viewport.width * 0.9))
            .rounded(px(theme::RADIUS_XL))
            .bg(rgb(theme::bg_sidebar()))
            .shadow(vec![BoxShadow {
                color: gpui::hsla(0.0, 0.0, 0.0, 0.6),
                offset: point(px(0.0), px(24.0)),
                blur_radius: px(64.0),
                spread_radius: px(0.0),
            }])
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .px(px(16.0))
                    .py(px(0.0))
                    .border_b_1()
                    .border_color(rgb(theme::bg_elevated()))
                    .font_weight(FontWeight::NORMAL)
                    .child(self.input.clone()),
            )
            .child(list.e2e("ul", "switcher-results"))
            .e2e("div", "switcher");
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .justify_center()
            .pt(viewport.height * 0.15)
            .bg(rgba(0x0000008c))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(SwitcherEvent::Close)),
            )
            .child(panel)
            .e2e("div", "overlay")
    }
}

impl crate::shell::Shell {
    /// The rooms the switcher offers, with where each lives.
    fn switch_rooms(&self) -> Vec<SwitchRoom> {
        rooms_for(&self.rooms, &self.nicknames, |room| {
            use crate::model::Scope;
            match crate::model::scope_for(&self.rooms, room) {
                // A server's channel shows the server's picture.
                Scope::Space(id) => {
                    let space = self.rooms.iter().find(|r| r.id == id);
                    (
                        space.map(|s| s.name.clone()).unwrap_or_default(),
                        space.and_then(|s| self.picture(&s.avatar, 40)),
                        space.map(|s| s.name.clone()),
                    )
                }
                Scope::Dms => ("Direct message".to_owned(), self.picture(&room.avatar, 32), None),
                _ => ("Rooms".to_owned(), self.picture(&room.avatar, 32), None),
            }
        })
    }

    /// The room list changed under an open switcher.
    pub fn refresh_switcher(&mut self, cx: &mut Context<Self>) {
        if let Some(switcher) = self.switcher.clone() {
            let list = self.switch_rooms();
            switcher.update(cx, |switcher, cx| {
                switcher.rooms = list;
                cx.notify();
            });
        }
    }

    /// Ctrl+K: opens the switcher, or closes it.
    pub fn toggle_switcher(&mut self, cx: &mut Context<Self>) {
        if self.switcher.take().is_some() {
            cx.notify();
            return;
        }
        let list = self.switch_rooms();
        let switcher = cx.new(|cx| Switcher::new(list, cx));
        cx.subscribe(&switcher, |this: &mut Self, _, event: &SwitcherEvent, cx| {
            this.switcher = None;
            match event {
                SwitcherEvent::Room(id) => {
                    // Scope first, so the sidebar follows the room.
                    if let Some(room) = this.rooms.iter().find(|r| &r.id == id) {
                        let scope = crate::model::scope_for(&this.rooms, room);
                        if scope != this.scope {
                            this.set_scope(scope, cx);
                        }
                    }
                    this.open_room(id.clone(), cx);
                }
                SwitcherEvent::Action(Action::Settings) => {
                    this.open_settings(crate::view_model::keys::SettingsFocus::None, cx)
                }
                SwitcherEvent::Action(Action::Shortcuts) => {
                    this.dialog_open(crate::dialogs::Dialog::Help, cx)
                }
                SwitcherEvent::Action(Action::Inbox) => this.toggle_inbox(cx),
                SwitcherEvent::Action(Action::CheckForUpdates) => this.check_for_updates(true, cx),
                SwitcherEvent::Action(Action::Acknowledgements) => this.open_about(cx),
                SwitcherEvent::Setting(entry) => this.open_setting_entry(entry, cx),
                SwitcherEvent::Close => {}
            }
            cx.notify();
        })
        .detach();
        self.switcher = Some(switcher);
        cx.notify();
    }
}

impl crate::shell::Shell {
    /// Opens Settings on the page of one setting, flashing it.
    fn open_setting_entry(
        &mut self,
        entry: &'static crate::view_model::settings::SettingEntry,
        cx: &mut Context<Self>,
    ) {
        self.open_settings(crate::view_model::keys::SettingsFocus::None, cx);
        if let Some(settings) = self.settings.clone() {
            settings.update(cx, |settings, cx| settings.show_setting(entry, cx));
        }
    }
}
