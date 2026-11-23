//! The dialogs of rooms and servers (`crates/ui/src/components/
//! room_dialogs.rs`): invite people, edit a room or server (name, topic,
//! picture), create a room, a category or a server, privacy (who can join,
//! history, turning encryption on), your per-server profile, and leaving a
//! server. One entity serves all of them (`kind` says which); what they
//! accept and say is decided in `view_model::room_dialogs`.

use crate::view_model::i18n::t;
use std::path::PathBuf;

use gpui::{
    AnyElement, ClickEvent, Context, ElementId, Entity, EventEmitter, FocusHandle, FontWeight,
    IntoElement, PathPromptOptions, Render, SharedString, Window, div, img, prelude::*, px, rgb, rgba,
};
use morpheus_core::{History, OwnedRoomId, Privacy, RoomSummary, Session};

use crate::tab::Tabbable as _;
use crate::{
    dialogs::{Closed, actions, dialog_box, heading, muted, overlay},
    e2e::E2eExt as _,
    friend_picker::{FriendPicker, Picked, RowState},
    icons::icon,
    input::{Submit, TextInput},
    model::{Scope, rooms_in_scope},
    theme,
    view_model::locks as vm_locks,
    view_model::permissions as vm_perm,
    view_model::room_dialogs::{
        self as vm, ENCRYPTION_OFF, ENCRYPTION_ON, ENCRYPTION_PERMANENT, ENCRYPTION_WARNING,
        Encryption, HISTORY_CHOICES, JOIN_CHOICES, Note, RoomDialogKind,
    },
};

/// What a dialog hands the shell when it did its job.
pub enum Done {
    /// A room was made: where it is (a server, or none) and which one.
    RoomCreated {
        room: OwnedRoomId,
        space: Option<OwnedRoomId>,
    },
    /// A server or category was made (under `parent`, if a category).
    SpaceCreated {
        space: OwnedRoomId,
        parent: Option<OwnedRoomId>,
    },
    LeftServer,
    /// "Join a server" was chosen in Create a server: Join by address instead.
    JoinByAddress,
    /// A nickname was saved (empty: cleared).
    NicknameSaved {
        user: morpheus_core::OwnedUserId,
        nickname: String,
    },
    /// A rail folder was renamed.
    FolderRenamed {
        folder: String,
        name: String,
    },
    /// Your per-server profile changed: member lists read again.
    ProfileSaved,
    /// A room's or server's emoji changed: the packs in reach are read again.
    EmotesChanged,
    /// Your own name, picture or status changed (the footer reads them again).
    OwnProfileChanged,
    /// A lock was set: the room and the hash of its password.
    LockSet {
        room: OwnedRoomId,
        hash: String,
    },
    /// A lock was taken off.
    LockRemoved(OwnedRoomId),
}

pub struct RoomDialog {
    kind: RoomDialogKind,
    session: Session,
    rooms: Vec<RoomSummary>,
    scope: Scope,
    focus: FocusHandle,
    take_focus: bool,
    /// The contents' scroll position when taller than the window.
    scroll: gpui::ScrollHandle,
    name: Option<Entity<TextInput>>,
    topic: Option<Entity<TextInput>>,
    user: Option<Entity<TextInput>>,
    /// Invite: the friends to pick from (its box is `user`).
    picker: Option<Entity<FriendPicker>>,
    busy: bool,
    note: Option<Note>,
    /// Create room: the categories of the server, and which one is chosen
    /// (0 is "None"); whether the list shows.
    categories: Vec<(OwnedRoomId, String)>,
    category: usize,
    category_open: bool,
    /// Create room: a voice channel rather than a text one.
    voice: bool,
    /// Privacy.
    encrypted: Option<bool>,
    confirming: bool,
    encryption_error: Option<String>,
    public: bool,
    history: History,
    /// Per-server profile: the picture chosen, not yet saved.
    avatar: Option<PathBuf>,
    /// Edit room: who may do what (#175), once read.
    powers: Option<morpheus_core::RoomPowers>,
    /// Edit room: the last attempt to change it failed.
    powers_note: Option<Note>,
    /// Edit room: the room's emoji pack (#193), once read; the box for a new name,
    /// the emoji being renamed with its box, and how the last change went.
    emoji_pack: Option<morpheus_core::EmotePack>,
    emoji_name: Option<Entity<TextInput>>,
    renaming: Option<(String, Entity<TextInput>)>,
    emoji_note: Option<Note>,
    /// A picture chosen and being cropped (#267), and what it is for.
    cropping: Option<(crate::crop_view::Picked, CropFor)>,
    /// Invite: the link was just copied.
    link_copied: bool,
}

/// What a cropped picture becomes.
enum CropFor {
    /// The profile picture, a server or room icon, a per-server picture.
    Picture { stem: String },
    /// A custom emoji of this name.
    Emoji { code: String },
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
        friends: Vec<morpheus_core::OwnedUserId>,
        cx: &mut Context<Self>,
    ) -> Self {
        let current = |id: &OwnedRoomId| rooms.iter().find(|room| &room.id == id).cloned();
        let field = |cx: &mut Context<Self>, name: &'static str, placeholder: &str, value: &str| {
            let input = cx.new(|cx| TextInput::new(name, placeholder, false, value, cx).framed());
            cx.subscribe(&input, |this, _, _: &Submit, cx| this.submit(cx))
                .detach();
            Some(input)
        };
        let (mut name, mut topic, mut user) = (None, None, None);
        let mut chooser = None;
        let mut roster = None;
        match &kind {
            RoomDialogKind::Invite(room) => {
                let picker = cx.new(|cx| {
                    FriendPicker::new(
                        Some(session.clone()),
                        friends,
                        nicknames.clone(),
                        "user",
                        "Search friends or enter @friend:matrix.org",
                        "Invite",
                        "Invited",
                        cx,
                    )
                });
                let input = picker.read(cx).input().clone();
                cx.subscribe(&input, |this, _, _: &Submit, cx| this.submit(cx))
                    .detach();
                cx.subscribe(&picker, |this, _, picked: &Picked, cx| {
                    this.invite(picked.0.clone(), cx)
                })
                .detach();
                user = Some(input);
                chooser = Some(picker);
                roster = Some((
                    session.members(room.clone()),
                    session.invited_members(room.clone()),
                ));
            }
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
                    room.as_ref()
                        .and_then(|r| r.topic.as_deref())
                        .unwrap_or_default(),
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
            RoomDialogKind::LockSet(_) | RoomDialogKind::LockRemove(_) => {
                let secret = |cx: &mut Context<Self>, name: &'static str| {
                    let input = cx.new(|cx| TextInput::new(name, "", true, "", cx).framed());
                    cx.subscribe(&input, |this, _, _: &Submit, cx| this.submit(cx))
                        .detach();
                    Some(input)
                };
                name = secret(cx, "password");
                if matches!(&kind, RoomDialogKind::LockSet(_)) {
                    topic = secret(cx, "again");
                }
            }
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
            scroll: gpui::ScrollHandle::new(),
            name,
            topic,
            user,
            picker: chooser,
            busy: false,
            note: None,
            categories,
            category: 0,
            category_open: false,
            voice: false,
            encrypted: None,
            confirming: false,
            encryption_error: None,
            public: true,
            history: History::Shared,
            avatar: None,
            powers: None,
            powers_note: None,
            emoji_pack: None,
            emoji_name: None,
            renaming: None,
            emoji_note: None,
            cropping: None,
            link_copied: false,
        };
        this.load(cx);
        this.load_powers(cx);
        if matches!(this.kind, RoomDialogKind::Edit(_)) {
            this.emoji_name = Some(cx.new(|cx| TextInput::new("emoji-name", "wave", false, "", cx).framed()));
            this.load_emoji(cx);
        }
        if let (Some((joined, invited)), Some(picker)) = (roster, this.picker.clone()) {
            cx.spawn(async move |_, cx| {
                let joined: Vec<_> = joined
                    .await
                    .map(|members| members.into_iter().map(|m| m.user_id).collect())
                    .unwrap_or_default();
                let invited = invited.await.unwrap_or_default();
                picker
                    .update(cx, |picker, cx| picker.set_roster(joined, invited, cx))
                    .ok();
            })
            .detach();
        }
        this
    }

    /// Edit room: reads who may do what in the room.
    fn load_powers(&mut self, cx: &mut Context<Self>) {
        let RoomDialogKind::Edit(room) = self.kind.clone() else {
            return;
        };
        let reading = self.session.room_powers(room);
        cx.spawn(async move |this, cx| {
            if let Ok(powers) = reading.await {
                this.update(cx, |this, cx| {
                    this.powers = Some(powers);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// Edit room: choose who may `action`; shown at once, and read again if it failed.
    fn choose_level(
        &mut self,
        action: vm_perm::Action,
        level: vm_perm::Level,
        cx: &mut Context<Self>,
    ) {
        let (RoomDialogKind::Edit(room), Some(current)) =
            (self.kind.clone(), self.powers.clone())
        else {
            return;
        };
        let known = vm_perm::Powers {
            content: current.content,
            own: current.own,
            can_change: current.can_change,
        };
        let content = known.with_level(action, level);
        if let Some(powers) = &mut self.powers {
            powers.content = content.clone();
        }
        self.powers_note = None;
        let setting = self.session.set_room_powers(room, content);
        cx.spawn(async move |this, cx| {
            let result = setting.await;
            this.update(cx, |this, cx| {
                if let Err(err) = result {
                    this.powers_note = Some(Note::error(err.to_string()));
                    this.load_powers(cx);
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Edit room: the Permissions section, for those who may change it.
    fn permissions_section(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let powers = self.powers.as_ref()?;
        let known = vm_perm::Powers {
            content: powers.content.clone(),
            own: powers.own,
            can_change: powers.can_change,
        };
        let mut section = div().flex().flex_col().gap(px(6.0)).child(
            div()
                .font_weight(FontWeight::BOLD)
                .child(t("Permissions"))
                .e2e("h3", "")
                .text(t("Permissions")),
        );
        if !known.can_change {
            return Some(
                section
                    .child(muted(vm_perm::CANNOT_CHANGE, "muted perm-note"))
                    .e2e("div", "perm-section")
                    .into_any_element(),
            );
        }
        for action in vm_perm::Action::ALL {
            let chosen = known.level(action);
            let mut options = div().flex().flex_wrap().gap(px(6.0));
            for level in vm_perm::Level::ALL {
                let selected = chosen == level;
                options = options.child(
                    div()
                        .id(ElementId::from(SharedString::from(format!(
                            "perm-{}-{}",
                            action.id(),
                            level.power()
                        ))))
                        .px(px(10.0))
                        .py(px(4.0))
                        .rounded(px(theme::RADIUS))
                        .cursor_pointer()
                        .bg(rgb(if selected { theme::accent() } else { theme::bg_elevated() }))
                        .text_color(rgb(if selected { theme::on_accent() } else { theme::text() }))
                        .child(level.label())
                        .on_click(cx.listener(move |this, _, _, cx| this.choose_level(action, level, cx)))
                        .e2e("button", if selected { "perm-option selected" } else { "perm-option" })
                        .text(level.label()),
                );
            }
            section = section.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .child(div().child(action.label()))
                    .child(options)
                    .e2e("div", &format!("perm-row perm-{}", action.id())),
            );
        }
        if let Some(note) = &self.powers_note {
            section = section.child(note_el(note));
        }
        Some(section.e2e("div", "perm-section").into_any_element())
    }

    // ---- Edit room: the room's (or server's) custom emoji (#193) -----------------

    /// Edit room: reads the room's emoji pack and whether you may change it.
    fn load_emoji(&mut self, cx: &mut Context<Self>) {
        let RoomDialogKind::Edit(room) = self.kind.clone() else {
            return;
        };
        let reading = self.session.room_emote_pack(room);
        cx.spawn(async move |this, cx| {
            if let Ok(pack) = reading.await {
                this.update(cx, |this, cx| {
                    this.emoji_pack = Some(pack);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    /// What a change to the pack leaves to do: read it again (the sync may
    /// not have brought it yet, so it shows at once) and tell the app.
    fn emoji_changed(&mut self, cx: &mut Context<Self>) {
        cx.emit(Done::EmotesChanged);
        self.load_emoji(cx);
    }

    /// "Add emoji…": choose a picture and crop it; it goes up under the typed
    /// name (or one made from the file's name).
    fn add_emoji(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.kind, RoomDialogKind::Edit(_)) || self.emoji_pack.is_none() || self.busy {
            return;
        }
        self.emoji_note = None;
        let typed = self.text(&self.emoji_name, cx);
        #[cfg(feature = "e2e")]
        let hooked = crate::e2e::take_picked();
        #[cfg(not(feature = "e2e"))]
        let hooked: Option<PathBuf> = None;
        let picking = hooked.is_none().then(|| {
            cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some("Choose a picture".into()),
                filters: crate::view_model::room_dialogs::picture_filters(),
            })
        });
        cx.spawn(async move |this, cx| {
            let path = match (hooked, picking) {
                (Some(path), _) => path,
                (None, Some(picking)) => {
                    let Ok(Ok(Some(paths))) = picking.await else {
                        return;
                    };
                    let Some(path) = paths.into_iter().next() else {
                        return;
                    };
                    path
                }
                (None, None) => return,
            };
            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned();
            let code = morpheus_core::clean_shortcode(&typed)
                .or_else(|| morpheus_core::shortcode_from_file_name(&file_name));
            this.update(cx, |this, cx| match code {
                Some(code) if vm::picture_mime(&file_name).is_some() => {
                    this.picture_chosen(path, Some(code), cx)
                }
                _ => {
                    this.emoji_note = Some(Note::error(
                        "Choose a picture (PNG, JPEG, GIF or WebP) and give it a name.",
                    ));
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }


    /// Start renaming the emoji `code`.
    fn start_rename(&mut self, code: &str, cx: &mut Context<Self>) {
        let input = cx.new(|cx| TextInput::new("emoji-rename", "name", false, code, cx).framed());
        cx.subscribe(&input, |this, _, _: &Submit, cx| this.save_rename(cx))
            .detach();
        self.renaming = Some((code.to_owned(), input));
        cx.notify();
    }

    fn save_rename(&mut self, cx: &mut Context<Self>) {
        let (RoomDialogKind::Edit(room), Some(pack), Some((old, input))) = (
            self.kind.clone(),
            self.emoji_pack.clone(),
            self.renaming.clone(),
        ) else {
            return;
        };
        if self.busy {
            return;
        }
        self.emoji_note = None;
        let new = input.read(cx).value().to_owned();
        let renaming = self
            .session
            .rename_emote(Some(room), pack.state_key, old, new);
        self.run(renaming, cx, |this, result, cx| match result {
            Ok(()) => {
                this.renaming = None;
                this.emoji_changed(cx);
            }
            Err(err) => this.emoji_note = Some(Note::error(err)),
        });
    }

    fn remove_emoji(&mut self, code: &str, cx: &mut Context<Self>) {
        let (RoomDialogKind::Edit(room), Some(pack)) = (self.kind.clone(), self.emoji_pack.clone())
        else {
            return;
        };
        if self.busy {
            return;
        }
        self.emoji_note = None;
        let removing = self
            .session
            .remove_emote(Some(room), pack.state_key, code.to_owned());
        self.run(removing, cx, |this, result, cx| match result {
            Ok(()) => this.emoji_changed(cx),
            Err(err) => this.emoji_note = Some(Note::error(err)),
        });
    }

    /// Edit room: the Emoji section: the pack's emoji, and for those who may
    /// change it, adding, renaming and removing.
    fn emoji_section(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let pack = self.emoji_pack.clone()?;
        let mut section = div().flex().flex_col().gap(px(6.0)).child(
            div()
                .font_weight(FontWeight::BOLD)
                .child(t("Emoji"))
                .e2e("h3", "")
                .text(t("Emoji")),
        );
        if !pack.editable {
            section = section.child(muted(vm::EMOJI_CANNOT_CHANGE, "muted emoji-note"));
        }
        if pack.emotes.is_empty() && !pack.editable {
            section = section.child(muted("This room has no custom emoji.", "muted"));
        }
        for emote in &pack.emotes {
            crate::custom_emoji::want(&self.session, &emote.url, cx);
            let name = format!(":{}:", emote.shortcode);
            let picture = match crate::custom_emoji::image(&emote.url) {
                Some(image) => div()
                    .child(img(crate::media::drawn(image)).size(px(28.0)))
                    .e2e("img", "emote")
                    .attr("alt", name.clone())
                    .into_any_element(),
                None => div()
                    .size(px(28.0))
                    .rounded(px(4.0))
                    .bg(gpui::rgba(theme::bg_hover()))
                    .into_any_element(),
            };
            let code = emote.shortcode.clone();
            let renaming_this = self
                .renaming
                .as_ref()
                .filter(|(old, _)| *old == code)
                .map(|(_, input)| input.clone());
            let mut row = div()
                .flex()
                .items_center()
                .gap(px(10.0))
                .child(picture);
            row = match renaming_this {
                Some(input) => row
                    .child(div().flex_1().child(input))
                    .child(button(
                        &format!("save-emoji-{code}"),
                        Some("check"),
                        "Save",
                        "save-emoji-name",
                        false,
                        self.busy,
                        cx.listener(|this, _, _, cx| this.save_rename(cx)),
                    ))
                    .child(button(
                        &format!("cancel-emoji-{code}"),
                        None,
                        "Cancel",
                        "cancel-emoji-name",
                        false,
                        false,
                        cx.listener(|this, _, _, cx| {
                            this.renaming = None;
                            cx.notify();
                        }),
                    )),
                None => {
                    let row = row.child(
                        div()
                            .flex_1()
                            .child(name.clone())
                            .e2e("span", "emoji-name")
                            .text(name.clone()),
                    );
                    if pack.editable {
                        let (rename, remove) = (code.clone(), code.clone());
                        row.child(button(
                            &format!("rename-emoji-{code}"),
                            Some("pencil"),
                            "Rename",
                            "rename-emoji",
                            false,
                            self.busy,
                            cx.listener(move |this, _, _, cx| this.start_rename(&rename, cx)),
                        ))
                        .child(button(
                            &format!("remove-emoji-{code}"),
                            Some("trash-2"),
                            "Remove",
                            "remove-emoji",
                            false,
                            self.busy,
                            cx.listener(move |this, _, _, cx| this.remove_emoji(&remove, cx)),
                        ))
                    } else {
                        row
                    }
                }
            };
            section = section.child(row.e2e("div", "emoji-row").text(name));
        }
        if pack.editable {
            if let Some(name) = &self.emoji_name {
                section = section.child(field("New emoji name", name));
            }
            section = section.child(actions(vec![button(
                "add-emoji",
                Some("plus"),
                "Add emoji…",
                "secondary add-emoji",
                false,
                self.busy,
                cx.listener(|this, _, _, cx| this.add_emoji(cx)),
            )]));
        }
        if let Some(note) = &self.emoji_note {
            section = section.child(note_el(note));
        }
        Some(section.e2e("div", "emoji-section").into_any_element())
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

    /// Invite: the one row at the very bottom to copy the link to the room or
    /// server (#368), under the friends list, which scrolls by itself.
    fn invite_link_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let RoomDialogKind::Invite(room) = self.kind.clone() else {
            return div().into_any_element();
        };
        let link = crate::call_invite::channel_link(&room, None);
        let copied = self.link_copied;
        let label = if copied { "Copied" } else { "Copy link" };
        div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .flex_none()
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(0.85 * theme::REM))
                    .text_color(rgb(theme::text_muted()))
                    .child(link.clone())
                    .e2e("div", "invite-link")
                    .text(link.clone()),
            )
            .child(
                div()
                    .id("copy-invite-link")
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(px(6.0))
                    .px(px(12.0))
                    .py(px(6.0))
                    .rounded(px(theme::RADIUS_SM))
                    .cursor_pointer()
                    .bg(rgba(theme::bg_hover()))
                    .hover(|s| s.bg(rgba(theme::bg_selected())))
                    .text_color(rgb(theme::text()))
                    .child(
                        icon(if copied { "check" } else { "link" }, 16.0)
                            .text_color(rgb(theme::text())),
                    )
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.write_to_clipboard(gpui::ClipboardItem::new_string(link.clone()));
                        this.link_copied = true;
                        cx.notify();
                    }))
                    .e2e("button", "secondary copy-invite-link")
                    .attr("type", "button")
                    .text(label),
            )
            .e2e("div", "invite-link-row")
            .into_any_element()
    }

    /// Invite `user` to the room or server of an Invite dialog (from the
    /// list of friends or a typed ID); the friends' row and a note say how
    /// it went.
    fn invite(&mut self, user: morpheus_core::OwnedUserId, cx: &mut Context<Self>) {
        let RoomDialogKind::Invite(room) = self.kind.clone() else {
            return;
        };
        if self.busy {
            if let Some(picker) = &self.picker {
                picker.update(cx, |picker, cx| picker.set_state(&user, None, cx));
            }
            return;
        }
        if self
            .picker
            .as_ref()
            .is_some_and(|p| p.read(cx).is_member(&user))
        {
            self.note = Some(Note::error(crate::view_model::i18n::tf("%{user} is already in.", &[("user", &user.to_string())])));
            cx.notify();
            return;
        }
        // A server's invite also lands in your DM with them as a card.
        let server = self
            .rooms
            .iter()
            .find(|r| r.id == room && r.is_space)
            .map(|r| (r.name.clone(), room.clone()));
        let session = self.session.clone();
        let inviting = self.session.invite(room, user.clone());
        self.run(inviting, cx, move |this, result, cx| {
            let state = match &result {
                Ok(()) => RowState::Done,
                Err(err) => RowState::Failed(err.clone()),
            };
            if let Some(picker) = &this.picker {
                picker.update(cx, |picker, cx| picker.set_state(&user, Some(state), cx));
            }
            if let (Ok(()), Some((name, space))) = (&result, server) {
                let dm_user = user.clone();
                let home = session.user_id().map(|u| u.server_name().to_string());
                cx.spawn(async move |_, _| {
                    let sent = async {
                        let dm = session.dm_with(dm_user).await?;
                        let link = crate::call_invite::channel_link(&space, home.as_deref());
                        session.say(dm, format!("Join me in [{name}]({link})")).await
                    }
                    .await;
                    if let Err(err) = sent {
                        eprintln!("morpheus-desktop: server invite card not sent: {err}");
                    }
                })
                .detach();
            }
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

    fn submit(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        match self.kind.clone() {
            RoomDialogKind::Invite(_) => match vm::parse_invitee(&self.text(&self.user, cx)) {
                Ok(user) => self.invite(user, cx),
                Err(note) => {
                    self.note = Some(note);
                    cx.notify();
                }
            },
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
                let category = self
                    .category
                    .checked_sub(1)
                    .and_then(|i| self.categories.get(i));
                let parent = vm::room_parent(category.map(|(id, _)| id), space.as_ref());
                let creating = self.session.create_channel(name, topic, parent, self.voice);
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
                let saving = self
                    .session
                    .set_room_profile(targets, name, self.avatar.clone());
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
            RoomDialogKind::LockSet(room) => {
                let (password, again) = (self.text(&self.name, cx), self.text(&self.topic, cx));
                if let Some(problem) = vm_locks::password_problem(&password, &again) {
                    self.note = Some(Note::error(problem));
                    cx.notify();
                    return;
                }
                self.busy = true;
                cx.notify();
                let hashing = cx
                    .background_executor()
                    .spawn(async move { vm_locks::hash_password(&password) });
                cx.spawn(async move |this, cx| {
                    let hashed = hashing.await;
                    this.update(cx, |this, cx| {
                        this.busy = false;
                        match hashed {
                            Ok(hash) => cx.emit(Done::LockSet { room, hash }),
                            Err(err) => this.note = Some(Note::error(err)),
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
            }
            RoomDialogKind::LockRemove(room) => {
                let password = self.text(&self.name, cx);
                let Some(hash) = crate::locks::hash_of(&room) else {
                    cx.emit(Done::LockRemoved(room));
                    return;
                };
                self.busy = true;
                cx.notify();
                let checking = cx
                    .background_executor()
                    .spawn(async move { vm_locks::verify_password(&hash, &password) });
                cx.spawn(async move |this, cx| {
                    let right = checking.await;
                    this.update(cx, |this, cx| {
                        this.busy = false;
                        if right {
                            cx.emit(Done::LockRemoved(room));
                        } else {
                            this.note = Some(Note::error("That is not the password."));
                        }
                        cx.notify();
                    })
                    .ok();
                })
                .detach();
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

    /// "Change icon…" and "Choose avatar…": pick a picture, then crop it.
    fn pick_picture(&mut self, cx: &mut Context<Self>) {
        self.note = None;
        // Tests cannot drive the system's file dialog: they name the file.
        #[cfg(feature = "e2e")]
        if let Some(path) = crate::e2e::take_picked() {
            self.picture_chosen(path, None, cx);
            return;
        }
        let picking = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Choose a picture".into()),
            filters: crate::view_model::room_dialogs::picture_filters(),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = picking.await else {
                return;
            };
            let Some(path) = paths.into_iter().next() else {
                return;
            };
            this.update(cx, |this, cx| this.picture_chosen(path, None, cx))
                .ok();
        })
        .detach();
    }

    /// A file chosen for a picture, or (with `emoji`) for the emoji of that
    /// name: it opens in the crop view, or a note says it is not a picture.
    fn picture_chosen(&mut self, path: PathBuf, emoji: Option<String>, cx: &mut Context<Self>) {
        let bytes = std::fs::read(&path).ok();
        let Some(picked) = bytes.and_then(crate::crop_view::picked_from) else {
            let note = Note::error(vm::NOT_A_PICTURE);
            if emoji.is_some() {
                self.emoji_note = Some(note);
            } else {
                self.note = Some(note);
            }
            cx.notify();
            return;
        };
        let target = match emoji {
            Some(code) => CropFor::Emoji { code },
            None => CropFor::Picture {
                stem: path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("picture")
                    .to_owned(),
            },
        };
        self.cropping = Some((picked, target));
        self.note = None;
        self.emoji_note = None;
        cx.notify();
    }

    /// A file dropped on the dialog: for a picture target it opens in the crop
    /// view, or a note says it is not a picture (#268). False when this
    /// dialog takes no picture.
    pub fn dropped(&mut self, path: PathBuf, cx: &mut Context<Self>) -> bool {
        let takes = matches!(
            self.kind,
            RoomDialogKind::Edit(_) | RoomDialogKind::EditProfile | RoomDialogKind::RoomProfile(_)
        );
        if takes && self.cropping.is_none() && !self.busy {
            self.picture_chosen(path, None, cx);
        }
        takes
    }

    fn picked_mut(&mut self) -> Option<&mut crate::crop_view::Picked> {
        self.cropping.as_mut().map(|(picked, _)| picked)
    }

    /// "Use picture": cut the square and set it where it was chosen for.
    fn use_cropped(&mut self, cx: &mut Context<Self>) {
        if self.busy {
            return;
        }
        let Some((picked, target)) = self.cropping.take() else {
            return;
        };
        let rect = picked.crop.source_rect();
        let bytes = picked.image.bytes.clone();
        let side = match target {
            CropFor::Picture { .. } => crate::view_model::crop::OUTPUT_SIDE,
            CropFor::Emoji { .. } => crate::view_model::crop::EMOJI_SIDE,
        };
        let cut = cx.background_spawn(async move { crate::crop_view::cropped_png(&bytes, rect, side) });
        self.busy = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let png = cut.await;
            this.update(cx, |this, cx| {
                this.busy = false;
                match (png, target) {
                    (Err(err), CropFor::Emoji { .. }) => this.emoji_note = Some(Note::error(err)),
                    (Err(err), _) => this.note = Some(Note::error(err)),
                    (Ok(png), CropFor::Emoji { code }) => this.add_cropped_emoji(code, png, cx),
                    (Ok(png), CropFor::Picture { stem }) => this.set_cropped_picture(&stem, png, cx),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The cropped emoji goes up under its name.
    fn add_cropped_emoji(&mut self, code: String, png: Vec<u8>, cx: &mut Context<Self>) {
        let (RoomDialogKind::Edit(room), Some(pack)) = (self.kind.clone(), self.emoji_pack.clone())
        else {
            return;
        };
        let adding = self
            .session
            .add_emote(Some(room), pack.state_key, code, "image/png".to_owned(), png);
        self.run(adding, cx, |this, result, cx| match result {
            Ok(()) => {
                if let Some(name) = &this.emoji_name {
                    name.update(cx, |name, cx| name.set_value("", cx));
                }
                this.emoji_changed(cx);
            }
            Err(err) => this.emoji_note = Some(Note::error(err)),
        });
    }

    /// The cropped picture is the profile picture, the room's or server's
    /// icon, or (per-server profile) kept until Save.
    fn set_cropped_picture(&mut self, stem: &str, png: Vec<u8>, cx: &mut Context<Self>) {
        let dir = std::env::temp_dir().join(format!(
            "morpheus-crop-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or_default()
        ));
        let path = dir.join(format!("{stem}.png"));
        if let Err(err) = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, png)) {
            self.note = Some(Note::error(err.to_string()));
            return;
        }
        let cleanup = {
            let (dir, path) = (dir.clone(), path.clone());
            move || {
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_dir(&dir);
            }
        };
        match self.kind.clone() {
            RoomDialogKind::RoomProfile(_) => self.avatar = Some(path),
            RoomDialogKind::EditProfile => {
                let setting = self.session.set_avatar(path);
                self.run(setting, cx, move |this, result, cx| {
                    cleanup();
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
                let setting = self.session.set_room_avatar(room, path);
                self.run(setting, cx, move |this, result, _| {
                    cleanup();
                    this.note = Some(match result {
                        Ok(()) => Note::ok("Icon updated."),
                        Err(err) => Note::error(err),
                    });
                });
            }
            _ => cleanup(),
        }
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
    let color = if danger { theme::on_danger() } else { theme::text() };
    let el = div()
        .id(ElementId::from(SharedString::from(id.to_owned())))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(6.0))
        .cursor_pointer()
        .rounded(px(if danger { 6.0 } else { theme::RADIUS }))
        .when(danger, |d| {
            d.bg(rgb(theme::danger())).px(px(14.0)).py(px(8.0))
        })
        .when(!danger, |d| d.px(px(6.0)).py(px(1.0)))
        .when(busy, |d| d.opacity(0.6))
        .text_color(rgb(color))
        .children(glyph.map(|glyph| icon(glyph, 16.0).text_color(rgb(color))))
        .child(label.to_owned())
        .tabbable()
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
        .border_color(rgb(if checked {
            theme::accent()
        } else {
            theme::text_faint()
        }))
        .when(checked, |d| {
            d.child(div().size(px(8.0)).rounded_full().bg(rgb(theme::accent())))
        })
        .e2e("input", "")
        .attr("type", "radio")
        .attr("name", group);
    let circle = if checked {
        circle.attr("checked", "true")
    } else {
        circle
    };
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

    fn submit_button(
        &self,
        glyph: Option<&'static str>,
        label: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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

    /// Create room: Text or Voice, each a card with its icon (Discord's two
    /// options). The chosen one is a tinted card with the text unchanged
    /// (readable) and a filled radio.
    fn channel_types(&self, cx: &mut Context<Self>) -> AnyElement {
        let option = |voice: bool,
                      key: &'static str,
                      glyph: &'static str,
                      title: &'static str,
                      about: &'static str| {
            let selected = self.voice == voice;
            div()
                .id(ElementId::from(SharedString::from(format!(
                    "channel-type-{key}"
                ))))
                .flex()
                .items_center()
                .gap(px(12.0))
                .px(px(12.0))
                .py(px(10.0))
                .rounded(px(theme::RADIUS))
                .border_1()
                .border_color(rgb(if selected {
                    theme::accent()
                } else {
                    theme::bg_elevated()
                }))
                .bg(theme::fill_input())
                .when(selected, |d| d.bg(rgba(theme::bg_selected())))
                .text_color(rgb(theme::text()))
                .cursor_pointer()
                .child(icon(glyph, 22.0).text_color(rgb(theme::text_muted())))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_col()
                        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(title))
                        .child(
                            div()
                                .text_size(px(0.8 * theme::REM))
                                .text_color(rgb(theme::text_muted()))
                                .child(about),
                        ),
                )
                .child(
                    div()
                        .size(px(16.0))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .border_1()
                        .border_color(rgb(if selected {
                            theme::accent()
                        } else {
                            theme::text_faint()
                        }))
                        .when(selected, |d| {
                            d.child(div().size(px(8.0)).rounded_full().bg(rgb(theme::accent())))
                        }),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.voice = voice;
                    cx.notify();
                }))
                .e2e(
                    "button",
                    if selected {
                        "channel-type selected"
                    } else {
                        "channel-type"
                    },
                )
                .attr("data-value", key)
                .text(title)
        };
        div()
            .flex()
            .flex_col()
            .gap(px(6.0))
            .child(
                div()
                    .text_size(px(0.75 * theme::REM))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(rgb(theme::text_muted()))
                    .child(t("CHANNEL TYPE")),
            )
            .child(option(
                false,
                "text",
                "hash",
                "Text",
                "Send messages, pictures and files",
            ))
            .child(option(
                true,
                "voice",
                "volume-2",
                "Voice",
                "Hang out with voice, video and screen share",
            ))
            .into_any_element()
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
            .border_color(theme::fill_elevated())
            .bg(theme::fill_input())
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
                .bg(theme::fill_input())
                .border_1()
                .border_color(theme::fill_elevated())
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
            .child(t("Category"))
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
                .child(div().child(t("Encryption")).e2e("span", "").text(t("Encryption")))
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
                parts.push(
                    actions(vec![
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
                    .into_any_element(),
                );
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
            .bg(theme::fill_elevated())
            .children(parts)
            .e2e("section", "encryption-setting")
            .into_any_element()
    }

    /// The dialog's contents and its class.
    /// The crop step of a chosen picture: frame and zoom, then "Use picture".
    fn crop_body(&mut self, cx: &mut Context<Self>) -> (&'static str, Vec<AnyElement>) {
        let mut parts: Vec<AnyElement> = vec![heading("Crop picture").into_any_element()];
        if let Some((picked, _)) = &self.cropping {
            parts.push(crate::crop_view::crop_view(picked, Self::picked_mut, cx));
        }
        parts.extend(self.note.as_ref().map(note_el));
        parts.push(
            actions(vec![
                button(
                    "use-picture",
                    Some("check"),
                    "Use picture",
                    "primary use-picture",
                    false,
                    self.busy,
                    cx.listener(|this, _, _, cx| this.use_cropped(cx)),
                ),
                button(
                    "cancel-crop",
                    None,
                    "Cancel",
                    "secondary cancel-crop",
                    false,
                    false,
                    cx.listener(|this, _, _, cx| {
                        this.cropping = None;
                        cx.notify();
                    }),
                ),
            ])
            .into_any_element(),
        );
        ("crop-dialog", parts)
    }

    fn body(&mut self, cx: &mut Context<Self>) -> (&'static str, Vec<AnyElement>) {
        if self.cropping.is_some() {
            return self.crop_body(cx);
        }
        let mut parts: Vec<AnyElement> = Vec::new();
        let note = self.note.as_ref().map(note_el);
        match self.kind.clone() {
            RoomDialogKind::Invite(_) => {
                parts.push(heading("Invite people").into_any_element());
                if let Some(picker) = &self.picker {
                    parts.push(picker.clone().into_any_element());
                }
                parts.extend(note);
                parts.push(
                    actions(vec![
                        self.submit_button(Some("user-plus"), "Invite", cx),
                        self.close_button("Close", cx),
                    ])
                    .into_any_element(),
                );
                parts.push(self.invite_link_row(cx));
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
                parts.extend(self.permissions_section(cx));
                parts.extend(self.emoji_section(cx));
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
                parts.push(
                    muted(&vm::create_room_scope(space_name.as_deref()), "muted")
                        .into_any_element(),
                );
                parts.push(self.channel_types(cx));
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
                if !is_category {
                    parts.push(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.0))
                            .text_color(rgb(theme::text_muted()))
                            .child(t("Have an invite or address?"))
                            .child(button(
                                "join-server",
                                Some("log-in"),
                                "Join a server",
                                "secondary join-server-button",
                                false,
                                false,
                                cx.listener(|_, _, _, cx| cx.emit(Done::JoinByAddress)),
                            ))
                            .into_any_element(),
                    );
                }
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
                parts.push(
                    muted(
                        &crate::view_model::nicknames::nickname_blurb(&current),
                        "muted",
                    )
                    .into_any_element(),
                );
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
            RoomDialogKind::LockSet(_) => {
                parts.push(heading("Lock with a password").into_any_element());
                parts.push(muted(vm_locks::LOCK_BLURB, "muted").into_any_element());
                if let Some(input) = &self.name {
                    parts.push(field("Password", input));
                }
                if let Some(input) = &self.topic {
                    parts.push(field("Password again", input));
                }
                parts.extend(note);
                parts.push(
                    actions(vec![
                        self.submit_button(Some("lock"), "Lock", cx),
                        self.close_button("Cancel", cx),
                    ])
                    .into_any_element(),
                );
                ("lock-dialog", parts)
            }
            RoomDialogKind::LockRemove(_) => {
                parts.push(heading("Remove lock").into_any_element());
                parts.push(muted(vm_locks::FORGOT, "muted forgot").into_any_element());
                if let Some(input) = &self.name {
                    parts.push(field("Password", input));
                }
                parts.extend(note);
                parts.push(
                    actions(vec![
                        self.submit_button(Some("lock-open"), "Remove lock", cx),
                        self.close_button("Cancel", cx),
                    ])
                    .into_any_element(),
                );
                ("lock-dialog", parts)
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
        #[cfg(feature = "e2e")]
        crate::e2e::on_dialog_drop(cx.weak_entity());
        let leave = matches!(self.kind, RoomDialogKind::LeaveSpace(_));
        let boxed = dialog_box()
            .id("room-dialog")
            .track_focus(&self.focus)
            .on_drop::<gpui::ExternalPaths>(cx.listener(|this, paths: &gpui::ExternalPaths, _, cx| {
                if let Some(path) = paths.paths().first() {
                    this.dropped(path.clone(), cx);
                }
            }))
            .on_key_down(cx.listener(|_, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.emit(Closed);
                }
            }))
            .child(
                // Taller than the window (Edit room with its permissions and emoji):
                // the contents scroll, under the close button.
                div()
                    .id("room-dialog-body")
                    .flex()
                    .flex_col()
                    .gap(px(12.0))
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(parts)
                    .e2e("div", "room-dialog-body")
                    .scrollable(std::rc::Rc::new(crate::e2e::HandleProbe(self.scroll.clone()))),
            )
            .e2e(
                if leave { "div" } else { "form" },
                &format!("message-dialog room-dialog {class}"),
            )
            .focusable(&self.focus);
        let this = cx.weak_entity();
        overlay(window, cx, boxed.into_any_element(), move |_, cx| {
            this.update(cx, |_, cx| cx.emit(Closed)).ok();
        })
    }
}
