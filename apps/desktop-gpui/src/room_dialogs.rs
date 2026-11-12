//! The dialogs of rooms and servers (`crates/ui/src/components/
//! room_dialogs.rs`): invite people, edit a room or server (name, topic,
//! picture), create a room, a category or a server, privacy (who can join,
//! history, turning encryption on), your per-server profile, and leaving a
//! server. One entity serves all of them (`kind` says which); what they
//! accept and say is decided in `view_model::room_dialogs`.

use std::path::PathBuf;

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, Entity, EventEmitter, FocusHandle, FontWeight,
    IntoElement, PathPromptOptions, Render, SharedString, Window, div, prelude::*, px, rgb, rgba,
};
use morpheus_core::{History, OwnedRoomId, Privacy, RoomSummary, Session};

use crate::{
    dialogs::{Closed, actions, dialog_box, heading, muted, overlay},
    e2e::E2eExt as _,
    icons::icon,
    input::{Submit, TextInput},
    model::{Scope, rooms_in_scope},
    theme,
    view_model::room_dialogs::{
        self as vm, ENCRYPTION_OFF, ENCRYPTION_ON, ENCRYPTION_PERMANENT, ENCRYPTION_WARNING,
        Encryption, HISTORY_CHOICES, JOIN_CHOICES, Note, RoomDialogKind,
    },
};

/// What a dialog hands the shell when it did its job.
pub enum Done {
    /// A room was made: where it is (a server, or none) and which one.
    RoomCreated { room: OwnedRoomId, space: Option<OwnedRoomId> },
    /// A server or category was made (under `parent`, if a category).
    SpaceCreated { space: OwnedRoomId, parent: Option<OwnedRoomId> },
    LeftServer,
    /// A nickname was saved (empty: cleared).
    NicknameSaved { user: morpheus_core::OwnedUserId, nickname: String },
    /// A rail folder was renamed.
    FolderRenamed { folder: String, name: String },
    /// Your per-server profile changed: member lists read again.
    ProfileSaved,
    /// Your own name, picture or status changed (the footer reads them again).
    OwnProfileChanged,
}

pub struct RoomDialog {
    kind: RoomDialogKind,
    session: Session,
    rooms: Vec<RoomSummary>,
    scope: Scope,
    focus: FocusHandle,
    take_focus: bool,
    name: Option<Entity<TextInput>>,
    topic: Option<Entity<TextInput>>,
    user: Option<Entity<TextInput>>,
    busy: bool,
    note: Option<Note>,
    /// Create room: the categories of the server, and which one is chosen
    /// (0 is "None"); whether the list shows.
    categories: Vec<(OwnedRoomId, String)>,
    category: usize,
    category_open: bool,
    /// Privacy.
    encrypted: Option<bool>,
    confirming: bool,
    encryption_error: Option<String>,
    public: bool,
    history: History,
    /// Per-server profile: the picture chosen, not yet saved.
    avatar: Option<PathBuf>,
}

impl EventEmitter<Closed> for RoomDialog {}
impl EventEmitter<Done> for RoomDialog {}

impl RoomDialog {
    pub fn new(
        kind: RoomDialogKind,
        session: Session,
        rooms: Vec<RoomSummary>,
        scope: Scope,
        nicknames: &crate::view_model::nicknames::NickMap,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = |id: &OwnedRoomId| rooms.iter().find(|room| &room.id == id).cloned();
        let field = |cx: &mut Context<Self>, name: &'static str, placeholder: &str, value: &str| {
            let input = cx.new(|cx| TextInput::new(name, placeholder, false, value, cx).framed());
            cx.subscribe(&input, |this, _, _: &Submit, cx| this.submit(cx)).detach();
            Some(input)
        };
        let (mut name, mut topic, mut user) = (None, None, None);
        match &kind {
            RoomDialogKind::Invite(_) => user = field(cx, "user", "@friend:matrix.org", ""),
            RoomDialogKind::Edit(id) => {
                let room = current(id);
                name = field(
                    cx,
                    "name",
                    "",
                    room.as_ref().map(|r| r.name.as_str()).unwrap_or_default(),
                );
                topic = field(
                    cx,
                    "topic",
                    "What is this room about?",
                    room.as_ref().and_then(|r| r.topic.as_deref()).unwrap_or_default(),
                );
            }
            RoomDialogKind::Create => {
                name = field(cx, "name", "new-room", "");
                topic = field(cx, "topic", "", "");
            }
            RoomDialogKind::CreateSpace(_) => name = field(cx, "name", "", ""),
            RoomDialogKind::RoomProfile(_) => name = field(cx, "nickname", "", ""),
            RoomDialogKind::EditProfile => name = field(cx, "displayname", "", ""),
            RoomDialogKind::SetStatus => name = field(cx, "status", "In a meeting", ""),
            RoomDialogKind::Nickname(user, current) => {
                let existing = nicknames.get(user.as_str()).cloned().unwrap_or_default();
                name = field(cx, "nickname", current, &existing);
            }
            RoomDialogKind::RenameFolder(_, current) => name = field(cx, "name", "", current),
            RoomDialogKind::Privacy(_) | RoomDialogKind::LeaveSpace(_) => {}
        }
        let categories = match &kind {
            RoomDialogKind::Create => vm::categories(&rooms, scope.space_id()),
            _ => Vec::new(),
        };
        let mut this = Self {
            kind,
            session,
            rooms,
            scope,
            focus: cx.focus_handle(),
            take_focus: true,
            name,
            topic,
            user,
            busy: false,
            note: None,
            categories,
            category: 0,
            category_open: false,
            encrypted: None,
            confirming: false,
            encryption_error: None,
            public: true,
            history: History::Shared,
            avatar: None,
        };
        this.load(cx);
        this
    }

    /// What the form starts from, as the server has it.
    fn load(&mut self, cx: &mut Context<Self>) {
        match self.kind.clone() {
            RoomDialogKind::Privacy(room) => {
                let checking = self.session.is_encrypted(room.clone());
                cx.spawn(async move |this, cx| {
                    if let Ok(on) = checking.await {
                        this.update(cx, |this, cx| {
                            this.encrypted = Some(on);
                            cx.notify();
                        })
                        .ok();
                    }
                })
                .detach();
                let reading = self.session.privacy(room);
                cx.spawn(async move |this, cx| {
                    if let Ok(privacy) = reading.await {
                        this.update(cx, |this, cx| {
                            this.public = privacy.public;
                            this.history = privacy.history;
                            cx.notify();
                        })
                        .ok();
                    }
                })
                .detach();
            }
            RoomDialogKind::EditProfile | RoomDialogKind::SetStatus => {
                let status = matches!(self.kind, RoomDialogKind::SetStatus);
                let reading = self.session.own_profile();
                cx.spawn(async move |this, cx| {
                    if let Ok(profile) = reading.await {
                        this.update(cx, |this, cx| {
                            // Typing wins over a fetch that lands late.
                            let untouched = this.text(&this.name, cx).is_empty();
                            if let (Some(name), true) = (&this.name, untouched) {
                                let shown = if status {
                                    profile.status.clone().unwrap_or_default()
                                } else {
                                    profile.name.clone()
                                };
                                name.update(cx, |name, cx| name.set_value(&shown, cx));
                            }
                        })
                        .ok();
                    }
                })
                .detach();
            }
            RoomDialogKind::RoomProfile(room) => {
                let reading = self.session.room_profile(room);
                cx.spawn(async move |this, cx| {
                    if let Ok(profile) = reading.await {
                        this.update(cx, |this, cx| {
                            if let Some(name) = &this.name {
                                name.update(cx, |name, cx| name.set_value(&profile.name, cx));
                            }
                        })
                        .ok();
                    }
                })
                .detach();
            }
            _ => {}
        }
    }

    fn text(&self, field: &Option<Entity<TextInput>>, cx: &Context<Self>) -> String {
        field
            .as_ref()
            .map(|input| input.read(cx).value().to_owned())
            .unwrap_or_default()
    }

    /// Runs a call; `then` handles its result once it is back.
    fn run<T: Send + 'static>(
        &mut self,
        future: morpheus_core::CoreFuture<T>,
        cx: &mut Context<Self>,
        then: impl FnOnce(&mut Self, Result<T, String>, &mut Context<Self>) + 'static,
    ) {
        self.busy = true;
        cx.spawn(async move |this, cx| {
            let result = future.await.map_err(|err| err.to_string());
            this.update(cx, |this, cx| {
                this.busy = false;
                then(this, result, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        match self.kind.clone() {
            RoomDialogKind::Invite(room) => {
                let user = match vm::parse_invitee(&self.text(&self.user, cx)) {
                    Ok(user) => user,
                    Err(note) => {
                        self.note = Some(note);
                        cx.notify();
                        return;
                    }
                };
                let inviting = self.session.invite(room, user.clone());
                self.run(inviting, cx, move |this, result, cx| {
                    this.note = Some(match result {
                        Ok(()) => {
                            if let Some(input) = &this.user {
                                input.update(cx, |input, cx| input.set_value("", cx));
                            }
                            vm::invited_note(&user)
                        }
                        Err(err) => Note::error(err),
                    });
                });
            }
            RoomDialogKind::Edit(room) => {
                let (name, topic) = (self.text(&self.name, cx), self.text(&self.topic, cx));
                let saving = self.session.edit_room(room, Some(name), Some(topic));
                self.run(saving, cx, |this, result, _| {
                    this.note = Some(match result {
                        Ok(()) => Note::ok("Saved."),
                        Err(err) => Note::error(err),
                    });
                });
            }
            RoomDialogKind::Create => {
                let name = self.text(&self.name, cx);
                if !vm::can_create(&name) {
                    return;
                }
                let topic = self.text(&self.topic, cx);
                let space = self.scope.space_id().cloned();
                let category = self.category.checked_sub(1).and_then(|i| self.categories.get(i));
                let parent = vm::room_parent(category.map(|(id, _)| id), space.as_ref());
                let creating = self.session.create_room(name, topic, parent);
                self.run(creating, cx, move |this, result, cx| match result {
                    Ok(room) => cx.emit(Done::RoomCreated { room, space }),
                    Err(err) => this.note = Some(Note::error(err)),
                });
            }
            RoomDialogKind::CreateSpace(parent) => {
                let name = self.text(&self.name, cx);
                if !vm::can_create(&name) {
                    return;
                }
                let creating = self.session.create_space(name, parent.clone());
                self.run(creating, cx, move |this, result, cx| match result {
                    Ok(space) => cx.emit(Done::SpaceCreated { space, parent }),
                    Err(err) => this.note = Some(Note::error(err)),
                });
            }
            RoomDialogKind::Privacy(room) => {
                let privacy = Privacy {
                    public: self.public,
                    history: self.history,
                };
                let saving = self.session.set_privacy(room, privacy);
                self.run(saving, cx, |this, result, _| {
                    this.note = Some(match result {
                        Ok(()) => Note::ok("Saved."),
                        Err(err) => Note::error(err),
                    });
                });
            }
            RoomDialogKind::RoomProfile(room) => {
                let name = self.text(&self.name, cx);
                let targets = self.server_rooms(&room);
                let saving = self.session.set_room_profile(targets, name, self.avatar.clone());
                self.run(saving, cx, |this, result, cx| {
                    this.note = Some(match result {
                        Ok(()) => {
                            cx.emit(Done::ProfileSaved);
                            Note::ok("Saved for this server.")
                        }
                        Err(err) => Note::error(err),
                    });
                });
            }
            RoomDialogKind::EditProfile => {
                let name = self.text(&self.name, cx);
                let saving = self.session.set_display_name(name);
                self.run(saving, cx, |this, result, cx| {
                    this.note = Some(match result {
                        Ok(()) => {
                            cx.emit(Done::OwnProfileChanged);
                            Note::ok("Saved.")
                        }
                        Err(err) => Note::error(err),
                    });
                });
            }
            RoomDialogKind::SetStatus => {
                let status = Some(self.text(&self.name, cx));
                self.set_status(status, cx);
            }
            RoomDialogKind::Nickname(user, _) => {
                let nickname = self.text(&self.name, cx).trim().to_owned();
                let saving = self.session.set_nickname(user.clone(), nickname.clone());
                self.run(saving, cx, move |this, result, cx| match result {
                    Ok(()) => cx.emit(Done::NicknameSaved { user, nickname }),
                    Err(err) => this.note = Some(Note::error(err)),
                });
            }
            RoomDialogKind::RenameFolder(folder, _) => {
                let name = self.text(&self.name, cx);
                cx.emit(Done::FolderRenamed { folder, name });
            }
            RoomDialogKind::LeaveSpace(space) => {
                let children: Vec<OwnedRoomId> = self
                    .server_rooms(&space)
                    .into_iter()
                    .filter(|id| id != &space)
                    .collect();
                let leaving = self.session.leave_space(space, children);
                self.run(leaving, cx, |this, result, cx| match result {
                    Ok(()) => cx.emit(Done::LeftServer),
                    Err(err) => this.note = Some(Note::error(err)),
                });
            }
        }
    }

    /// Sets (or, with `None`, clears) the status message, then closes.
    fn set_status(&mut self, status: Option<String>, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let saving = self.session.set_status(status);
        self.run(saving, cx, |this, result, cx| match result {
            Ok(()) => {
                cx.emit(Done::OwnProfileChanged);
                cx.emit(Closed);
            }
            Err(err) => this.note = Some(Note::error(err)),
        });
    }

    /// The server and every room under it.
    fn server_rooms(&self, space: &OwnedRoomId) -> Vec<OwnedRoomId> {
        let mut ids = vec![space.clone()];
        ids.extend(
            rooms_in_scope(&self.rooms, &Scope::Space(space.clone()))
                .into_iter()
                .map(|room| room.id),
        );
        ids
    }

    fn turn_on_encryption(&mut self, cx: &mut Context<Self>) {
        let RoomDialogKind::Privacy(room) = self.kind.clone() else {
            return;
        };
        if self.busy {
            return;
        }
        let enabling = self.session.enable_encryption(room);
        self.run(enabling, cx, |this, result, _| match result {
            Ok(()) => {
                this.encrypted = Some(true);
                this.confirming = false;
            }
            Err(err) => this.encryption_error = Some(err),
        });
    }

    /// "Change icon…" and "Choose avatar…": pick a picture.
    fn pick_picture(&mut self, cx: &mut Context<Self>) {
        let picking = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a picture".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picking.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| match this.kind.clone() {
                RoomDialogKind::RoomProfile(_) => {
                    this.avatar = Some(path);
                    cx.notify();
                }
                RoomDialogKind::EditProfile => {
                    let setting = this.session.set_avatar(path);
                    this.run(setting, cx, |this, result, cx| {
                        this.note = Some(match result {
                            Ok(()) => {
                                cx.emit(Done::OwnProfileChanged);
                                Note::ok("Avatar updated.")
                            }
                            Err(err) => Note::error(err),
                        });
                    });
                }
                RoomDialogKind::Edit(room) => {
                    let setting = this.session.set_room_avatar(room, path);
                    this.run(setting, cx, |this, result, _| {
                        this.note = Some(match result {
                            Ok(()) => Note::ok("Icon updated."),
                            Err(err) => Note::error(err),
                        });
                    });
                }
                _ => {}
            })
            .ok();
        })
        .detach();
    }
}

// ---- drawing --------------------------------------------------------------------

/// A button of these dialogs: danger a red block, others plain text; with a glyph.
pub(crate) fn button(
    id: &str,
    glyph: Option<&'static str>,
    label: &str,
    class: &str,
    submit: bool,
    busy: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let danger = class.contains("danger");
    let color = if danger { 0xffffff } else { theme::text() };
    let el = div()
        .id(ElementId::from(SharedString::from(id.to_owned())))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .cursor_pointer()
        .rounded(px(if danger { 6.0 } else { theme::RADIUS }))
        .when(danger, |d| d.bg(rgb(theme::danger())).px(px(14.0)).py(px(8.0)))
        .when(!danger, |d| d.px(px(6.0)).py(px(1.0)))
        .when(busy, |d| d.opacity(0.6))
        .text_color(rgb(color))
        .children(glyph.map(|glyph| icon(glyph, 16.0).text_color(rgb(color))))
        .child(label.to_owned())
        .on_click(on_click);
    let tagged = el.e2e("button", class).text(label);
    if submit {
        tagged.attr("type", "submit").into_any_element()
    } else {
        tagged.attr("type", "button").into_any_element()
    }
}

/// A labelled field: the label over its box.
fn field(label: &str, input: &Entity<TextInput>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .text_size(px(0.85 * theme::REM))
        .text_color(rgb(theme::text_muted()))
        .child(label.to_owned())
        .child(
            div()
                .text_size(px(theme::REM))
                .text_color(rgb(theme::text()))
                .child(input.clone()),
        )
        .e2e("label", "")
        .text(label)
        .into_any_element()
}

fn note_el(note: &Note) -> AnyElement {
    match note {
        Note::Ok(text) => div()
            .text_color(rgb(theme::ok()))
            .child(text.clone())
            .e2e("p", "ok")
            .text(text.clone())
            .into_any_element(),
        Note::Error(text) => div()
            .text_color(rgb(theme::danger()))
            .text_size(px(0.9 * theme::REM))
            .child(text.clone())
            .e2e("p", "error")
            .text(text.clone())
            .into_any_element(),
    }
}

/// A radio choice: its circle (filled when chosen) and its words.
fn choice(
    id: String,
    group: &'static str,
    label: &str,
    checked: bool,
    on_click: impl Fn(&ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    let circle = div()
        .size(px(16.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(rgb(if checked { theme::accent() } else { theme::text_faint() }))
        .when(checked, |d| {
            d.child(div().size(px(8.0)).rounded_full().bg(rgb(theme::accent())))
        })
        .e2e("input", "")
        .attr("type", "radio")
        .attr("name", group);
    let circle = if checked { circle.attr("checked", "true") } else { circle };
    div()
        .id(ElementId::from(SharedString::from(id)))
        .flex()
        .items_center()
        .gap(px(8.0))
        .cursor_pointer()
        .text_size(px(0.95 * theme::REM))
        .text_color(rgb(theme::text()))
        .child(circle)
        .child(div().child(label.to_owned()).e2e("span", "").text(label))
        .on_click(on_click)
        .e2e("label", "choice")
        .into_any_element()
}

/// A group of choices with its legend.
fn choice_group(legend: &str, choices: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .mb(px(4.0))
                .text_size(px(0.85 * theme::REM))
                .text_color(rgb(theme::text_muted()))
                .child(legend.to_owned())
                .e2e("legend", "")
                .text(legend),
        )
        .children(choices)
        .e2e("fieldset", "choice-group")
        .into_any_element()
}

impl RoomDialog {
    fn close_button(&self, label: &str, cx: &mut Context<Self>) -> AnyElement {
        button(
            "room-dialog-close",
            None,
            label,
            "secondary",
            false,
            false,
            cx.listener(|_, _, _, cx| cx.emit(Closed)),
        )
    }

    fn submit_button(&self, glyph: Option<&'static str>, label: &str, cx: &mut Context<Self>) -> AnyElement {
        button(
            "room-dialog-submit",
            glyph,
            label,
            "primary",
            true,
            self.busy,
            cx.listener(|this, _, _, cx| this.submit(cx)),
        )
    }

    fn name_of(&self, id: &OwnedRoomId) -> Option<String> {
        vm::room_name(&self.rooms, id)
    }

    fn category_select(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut options = vec!["None".to_owned()];
        options.extend(self.categories.iter().map(|(_, name)| name.clone()));
        let shown = options.get(self.category).cloned().unwrap_or_default();
        let open = self.category_open;
        let head = div()
            .id("category-select")
            .flex()
            .items_center()
            .justify_between()
            .px(px(12.0))
            .py(px(10.0))
            .rounded(px(theme::RADIUS))
            .border_1()
            .border_color(rgb(theme::bg_elevated()))
            .bg(rgb(theme::bg_input()))
            .text_size(px(theme::REM))
            .text_color(rgb(theme::text()))
            .cursor_pointer()
            .child(shown)
            .child(icon("chevron-down", 14.0).text_color(rgb(theme::text_muted())))
            .on_click(cx.listener(|this, _, _, cx| {
                this.category_open = !this.category_open;
                cx.notify();
            }));
        let list = open.then(|| {
            div()
                .flex()
                .flex_col()
                .mt(px(2.0))
                .rounded(px(theme::RADIUS))
                .bg(rgb(theme::bg_input()))
                .border_1()
                .border_color(rgb(theme::bg_elevated()))
                .text_size(px(theme::REM))
                .text_color(rgb(theme::text()))
                .children(options.iter().enumerate().map(|(i, label)| {
                    let value = self
                        .categories
                        .get(i.wrapping_sub(1))
                        .map(|(id, _)| id.to_string())
                        .unwrap_or_default();
                    div()
                        .id(ElementId::from(SharedString::from(format!("category-{i}"))))
                        .px(px(12.0))
                        .py(px(8.0))
                        .cursor_pointer()
                        .hover(|s| s.bg(rgba(theme::bg_hover())))
                        .child(label.clone())
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.category = i;
                            this.category_open = false;
                            cx.notify();
                        }))
                        .e2e("option", "")
                        .attr("value", value)
                        .text(label.clone())
                }))
        });
        let select = div()
            .flex()
            .flex_col()
            .child(head)
            .children(list)
            .e2e("select", "")
            .attr("name", "category");
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .text_size(px(0.85 * theme::REM))
            .text_color(rgb(theme::text_muted()))
            .child("Category")
            .child(select)
            .e2e("label", "")
            .into_any_element()
    }

    fn encryption_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = Encryption::from(self.encrypted, self.confirming);
        let on = self.encrypted == Some(true);
        let para = |text: &str, class: &str, color: u32| {
            div()
                .text_size(px(0.85 * theme::REM))
                .text_color(rgb(color))
                .child(text.to_owned())
                .e2e("p", class)
                .text(text)
                .into_any_element()
        };
        let mut parts: Vec<AnyElement> = vec![
            div()
                .flex()
                .items_center()
                .gap(px(8.0))
                .text_size(px(0.95 * theme::REM))
                .font_weight(FontWeight::BOLD)
                .child(
                    icon(if on { "lock" } else { "lock-open" }, 16.0)
                        .text_color(rgb(theme::text())),
                )
                .child(div().child("Encryption").e2e("span", "").text("Encryption"))
                .e2e("h3", "")
                .into_any_element(),
        ];
        match state {
            Encryption::Checking => parts.push(para("Checking…", "muted", theme::text_muted())),
            Encryption::On => {
                parts.push(para(ENCRYPTION_ON, "ok", theme::ok()));
                parts.push(para(ENCRYPTION_PERMANENT, "muted", theme::text_muted()));
            }
            Encryption::Off => {
                parts.push(para(ENCRYPTION_OFF, "muted", theme::text_muted()));
                parts.push(
                    div()
                        .flex()
                        .child(button(
                            "turn-on-encryption",
                            Some("lock"),
                            "Turn on encryption",
                            "secondary turn-on-encryption",
                            false,
                            false,
                            cx.listener(|this, _, _, cx| {
                                this.confirming = true;
                                cx.notify();
                            }),
                        ))
                        .into_any_element(),
                );
            }
            Encryption::Confirming => {
                parts.push(para(ENCRYPTION_WARNING, "warning", theme::warn()));
                parts.push(actions(vec![
                    button(
                        "confirm-encryption",
                        Some("lock"),
                        "Turn on for good",
                        "danger confirm-encryption",
                        false,
                        self.busy,
                        cx.listener(|this, _, _, cx| this.turn_on_encryption(cx)),
                    ),
                    button(
                        "cancel-encryption",
                        None,
                        "Cancel",
                        "secondary",
                        false,
                        false,
                        cx.listener(|this, _, _, cx| {
                            this.confirming = false;
                            cx.notify();
                        }),
                    ),
                ])
                .into_any_element());
            }
        }
        if let Some(err) = &self.encryption_error {
            parts.push(note_el(&Note::error(err.clone())));
        }
        div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .px(px(14.0))
            .py(px(12.0))
            .rounded(px(theme::RADIUS))
            .bg(rgb(theme::bg_elevated()))
            .children(parts)
            .e2e("section", "encryption-setting")
            .into_any_element()
    }

    /// The dialog's contents and its class.
    fn body(&mut self, cx: &mut Context<Self>) -> (&'static str, Vec<AnyElement>) {
        let mut parts: Vec<AnyElement> = Vec::new();
        let note = self.note.as_ref().map(note_el);
        match self.kind.clone() {
            RoomDialogKind::Invite(_) => {
                parts.push(heading("Invite people").into_any_element());
                if let Some(user) = &self.user {
                    parts.push(field("Matrix ID", user));
                }
                parts.extend(note);
                parts.push(
                    actions(vec![
                        self.submit_button(Some("user-plus"), "Invite", cx),
                        self.close_button("Close", cx),
                    ])
                    .into_any_element(),
                );
                ("invite-dialog", parts)
            }
            RoomDialogKind::Edit(_) => {
                parts.push(heading("Edit room").into_any_element());
                if let Some(name) = &self.name {
                    parts.push(field("Name", name));
                }
                if let Some(topic) = &self.topic {
                    parts.push(field("Topic", topic));
                }
                parts.push(
                    actions(vec![button(
                        "change-icon",
                        None,
                        "Change icon…",
                        "secondary change-icon",
                        false,
                        self.busy,
                        cx.listener(|this, _, _, cx| this.pick_picture(cx)),
                    )])
                    .into_any_element(),
                );
                parts.extend(note);
                parts.push(
                    actions(vec![
                        self.submit_button(None, "Save", cx),
                        self.close_button("Close", cx),
                    ])
                    .into_any_element(),
                );
                ("edit-room-dialog", parts)
            }
            RoomDialogKind::Create => {
                parts.push(heading("Create room").into_any_element());
                let space_name = self.scope.space_id().and_then(|id| self.name_of(id));
                parts.push(muted(&vm::create_room_scope(space_name.as_deref()), "muted").into_any_element());
                if let Some(name) = &self.name {
                    parts.push(field("Name", name));
                }
                if let Some(topic) = &self.topic {
                    parts.push(field("Topic (optional)", topic));
                }
                if !self.categories.is_empty() {
                    parts.push(self.category_select(cx));
                }
                if let Some(Note::Error(text)) = &self.note {
                    parts.push(note_el(&Note::error(text.clone())));
                }
                parts.push(
                    actions(vec![
                        self.submit_button(Some("plus"), "Create", cx),
                        self.close_button("Cancel", cx),
                    ])
                    .into_any_element(),
                );
                ("create-room-dialog", parts)
            }
            RoomDialogKind::CreateSpace(parent) => {
                let is_category = parent.is_some();
                parts.push(heading(vm::create_space_title(is_category)).into_any_element());
                parts.push(muted(vm::create_space_blurb(is_category), "muted").into_any_element());
                if let Some(name) = &self.name {
                    parts.push(field("Name", name));
                }
                if let Some(Note::Error(text)) = &self.note {
                    parts.push(note_el(&Note::error(text.clone())));
                }
                parts.push(
                    actions(vec![
                        self.submit_button(Some("plus"), "Create", cx),
                        self.close_button("Cancel", cx),
                    ])
                    .into_any_element(),
                );
                ("create-space-dialog", parts)
            }
            RoomDialogKind::Privacy(room) => {
                parts.push(heading("Privacy").into_any_element());
                let shows = vm::privacy_parts(&self.rooms, &room);
                if shows.encryption {
                    parts.push(self.encryption_section(cx));
                }
                if shows.join_and_history {
                    let join = JOIN_CHOICES
                        .iter()
                        .enumerate()
                        .map(|(i, (public, label))| {
                            let public = *public;
                            choice(
                                format!("join-{i}"),
                                "join",
                                label,
                                self.public == public,
                                cx.listener(move |this, _, _, cx| {
                                    this.public = public;
                                    cx.notify();
                                }),
                            )
                        })
                        .collect();
                    parts.push(choice_group("Who can join", join));
                    let history = HISTORY_CHOICES
                        .iter()
                        .enumerate()
                        .map(|(i, (history, label))| {
                            let history = *history;
                            choice(
                                format!("history-{i}"),
                                "history",
                                label,
                                self.history == history,
                                cx.listener(move |this, _, _, cx| {
                                    this.history = history;
                                    cx.notify();
                                }),
                            )
                        })
                        .collect();
                    parts.push(choice_group("History new members can read", history));
                }
                parts.extend(note);
                let mut buttons = Vec::new();
                if shows.join_and_history {
                    buttons.push(self.submit_button(None, "Save", cx));
                }
                buttons.push(self.close_button("Close", cx));
                parts.push(actions(buttons).into_any_element());
                ("privacy-dialog", parts)
            }
            RoomDialogKind::RoomProfile(room) => {
                parts.push(heading("Server profile").into_any_element());
                let rooms = self.server_rooms(&room).len() - 1;
                parts.push(muted(&vm::room_profile_blurb(rooms), "muted").into_any_element());
                if let Some(name) = &self.name {
                    parts.push(field("Nickname", name));
                }
                let mut pick = vec![button(
                    "choose-avatar",
                    Some("image"),
                    "Choose avatar…",
                    "secondary",
                    false,
                    false,
                    cx.listener(|this, _, _, cx| this.pick_picture(cx)),
                )];
                if let Some(path) = &self.avatar {
                    let file = vm::picked_file(path);
                    pick.push(
                        div()
                            .text_color(rgb(theme::text_muted()))
                            .child(file.clone())
                            .e2e("span", "muted")
                            .text(file)
                            .into_any_element(),
                    );
                }
                parts.push(actions(pick).into_any_element());
                parts.extend(note);
                parts.push(
                    actions(vec![
                        self.submit_button(None, "Save", cx),
                        self.close_button("Close", cx),
                    ])
                    .into_any_element(),
                );
                ("room-profile-dialog", parts)
            }
            RoomDialogKind::RenameFolder(..) => {
                parts.push(heading("Rename folder").into_any_element());
                if let Some(name) = &self.name {
                    parts.push(field("Name", name));
                }
                parts.push(
                    actions(vec![
                        self.submit_button(None, "Save", cx),
                        self.close_button("Cancel", cx),
                    ])
                    .into_any_element(),
                );
                ("rename-folder-dialog", parts)
            }
            RoomDialogKind::Nickname(_, current) => {
                parts.push(heading("Nickname").into_any_element());
                parts.push(muted(&crate::view_model::nicknames::nickname_blurb(&current), "muted").into_any_element());
                if let Some(name) = &self.name {
                    parts.push(name.clone().into_any_element());
                }
                parts.extend(note);
                parts.push(
                    actions(vec![
                        self.submit_button(None, "Save", cx),
                        self.close_button("Cancel", cx),
                    ])
                    .into_any_element(),
                );
                ("nickname-editor", parts)
            }
            RoomDialogKind::EditProfile => {
                parts.push(heading("Edit profile").into_any_element());
                parts.push(
                    actions(vec![button(
                        "change-avatar",
                        Some("image"),
                        "Change avatar…",
                        "secondary",
                        false,
                        self.busy,
                        cx.listener(|this, _, _, cx| this.pick_picture(cx)),
                    )])
                    .into_any_element(),
                );
                if let Some(name) = &self.name {
                    parts.push(field("Display name", name));
                }
                parts.extend(note);
                parts.push(
                    actions(vec![
                        self.submit_button(None, "Save", cx),
                        self.close_button("Close", cx),
                    ])
                    .into_any_element(),
                );
                ("profile-editor", parts)
            }
            RoomDialogKind::SetStatus => {
                parts.push(heading("Set status").into_any_element());
                if let Some(name) = &self.name {
                    parts.push(field("What are you up to?", name));
                }
                parts.extend(note);
                parts.push(
                    actions(vec![
                        self.submit_button(None, "Save", cx),
                        button(
                            "clear-status",
                            None,
                            "Clear status",
                            "secondary",
                            false,
                            self.busy,
                            cx.listener(|this, _, _, cx| this.set_status(None, cx)),
                        ),
                        self.close_button("Close", cx),
                    ])
                    .into_any_element(),
                );
                ("profile-editor status-editor", parts)
            }
            RoomDialogKind::LeaveSpace(space) => {
                let name = self.name_of(&space);
                let count = self.server_rooms(&space).len() - 1;
                parts.push(heading(&vm::leave_space_title(name.as_deref())).into_any_element());
                parts.push(muted(&vm::leave_space_body(count), "muted").into_any_element());
                if let Some(Note::Error(text)) = &self.note {
                    parts.push(note_el(&Note::error(text.clone())));
                }
                parts.push(
                    actions(vec![
                        button(
                            "leave-server",
                            Some("log-out"),
                            "Leave server",
                            "danger",
                            false,
                            self.busy,
                            cx.listener(|this, _, _, cx| this.submit(cx)),
                        ),
                        self.close_button("Cancel", cx),
                    ])
                    .into_any_element(),
                );
                ("leave-space-dialog", parts)
            }
        }
    }
}

impl Render for RoomDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.take_focus) {
            let first = self
                .user
                .as_ref()
                .or(self.name.as_ref())
                .or(self.topic.as_ref());
            match first {
                Some(input) => input.read(cx).focus(window),
                None => window.focus(&self.focus),
            }
        }
        let (class, parts) = self.body(cx);
        let leave = matches!(self.kind, RoomDialogKind::LeaveSpace(_));
        let boxed = dialog_box()
            .id("room-dialog")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Closed);
                }
            }))
            .children(parts)
            .e2e(
                if leave { "div" } else { "form" },
                &format!("message-dialog room-dialog {class}"),
            )
            .focusable(&self.focus);
        let this = cx.weak_entity();
        overlay(window, boxed.into_any_element(), move |_, cx| {
            this.update(cx, |_, cx| cx.emit(Closed)).ok();
        })
    }
}
