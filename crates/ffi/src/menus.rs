//! The long-press sheet's items: the same lists the desktop's right-click menus show (`view_model::menu`,
//! `room_menu`), built from the room list this account has, and what choosing an item does. The last list
//! built is kept here, so an item is named by its place in it (`"3"`, or `"2.1"` in a submenu).

use morpheus_core::{MessageId, MessageKind, OwnedRoomId, RoomSummary};
use morpheus_view_model::{
    i18n,
    menu::{MenuAction, MenuItem, forward_targets, message_menu},
    room_menu::{DmState, available_invite_targets, dm_menu, room_menu, space_menu},
};

use crate::{FfiError, Morpheus, timeline::{MessageBody, MessageItem}};

/// One row of the sheet.
#[derive(Clone, Debug, uniffi::Record)]
pub struct MenuRow {
    /// Its place in the list: `"3"`, or `"2.1"` for the second row of the third row's submenu.
    pub path: String,
    /// The label in English, whatever the language: what the rows are named by in tests and tags.
    pub id: String,
    /// Translated.
    pub label: String,
    /// A Lucide glyph name (`"pin"`), as the desktop draws in front of the label.
    pub icon: Option<String>,
    pub danger: bool,
    /// Shown with a check mark (the current choice).
    pub checked: bool,
    /// The children are short labels (emoji) to lay out in a grid.
    pub grid: bool,
    /// A name whose letter stands in front of the label (rooms, servers).
    pub avatar: Option<String>,
    /// A row that only opens its children.
    pub children: Vec<MenuRow>,
}

/// What the screen does after an item was chosen (the rest was done here).
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum MenuOutcome {
    Done,
    /// Put this text on the clipboard.
    Copy { text: String },
    Reply,
    Edit,
    Thread,
    /// Toggle this reaction on the message.
    React { key: String },
    Delete,
    /// Open this room (a direct message was made or found).
    OpenRoom { room_id: String },
    /// Add `@name ` to the message being written.
    Mention { name: String },
    /// Another account is the signed-in one now.
    Switched,
    /// Sign in another account without signing out of this one.
    AddAccount,
    /// Sign out (the screen asks first).
    SignOut,
    /// A page of the phone for this item: `kind` is `invite`, `create`, `edit`, `privacy`, `overview` or `rename-folder`; `room_id` the
    /// room, server or folder it is about (the server to make a room in, for `create`, empty for none), `extra` its name where that matters.
    Page { kind: String, room_id: String, extra: String },
    /// The reactions of the message the sheet was opened on.
    ViewReactions,
    /// Report the message the sheet was opened on.
    Report,
    /// The picture of the message, to put on the clipboard: its bytes and type.
    CopyImage { bytes: Vec<u8>, mime: String },
    /// Show this theme code in Settings > Appearance, ready to be used.
    ShowTheme { code: String },
    /// The desktop's dialog or screen for this item is not on the phone yet: its label.
    NotYet { label: String },
}

pub(crate) struct Stored {
    pub items: Vec<MenuItem>,
    pub room: Option<OwnedRoomId>,
    pub event_id: Option<String>,
    /// The picture of an image message.
    pub media: Option<morpheus_core::MediaRef>,
}

pub fn rows_of(items: &[MenuItem], prefix: &str) -> Vec<MenuRow> {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let path = if prefix.is_empty() { i.to_string() } else { format!("{prefix}.{i}") };
            MenuRow {
                id: item.label.clone(),
                label: i18n::t(&item.label),
                icon: item.glyph().map(str::to_owned),
                danger: item.danger,
                checked: item.checked,
                grid: item.grid,
                avatar: item.avatar.clone(),
                children: rows_of(&item.children, &path),
                path,
            }
        })
        .collect()
}

fn at<'a>(items: &'a [MenuItem], path: &str) -> Option<&'a MenuItem> {
    let mut list = items;
    let mut found = None;
    for part in path.split('.') {
        let item = list.get(part.parse::<usize>().ok()?)?;
        list = &item.children;
        found = Some(item);
    }
    found
}

fn failed(reason: impl Into<String>) -> FfiError {
    FfiError::Failed { reason: reason.into() }
}

/// The core message a [`MessageItem`] came from, as far as the menu looks at it.
fn message_of(item: &MessageItem) -> morpheus_core::Message {
    let kind = match &item.body {
        MessageBody::Image { caption, media, width, height } => MessageKind::Image { body: caption.clone(), media: media.media_ref(), width: *width, height: *height, blurhash: None },
        MessageBody::Text { text, .. } => MessageKind::Text { body: text.clone(), html: None },
        _ => MessageKind::Text { body: String::new(), html: None },
    };
    morpheus_core::Message {
        unique_id: item.key.clone(),
        id: match item.event_id.as_deref().and_then(|id| morpheus_core::OwnedEventId::try_from(id).ok()) {
            Some(id) => MessageId::Event(id),
            None => MessageId::Transaction(morpheus_core::TransactionId::new()),
        },
        sender: morpheus_core::OwnedUserId::try_from(item.sender_id.as_str())
            .unwrap_or_else(|_| morpheus_core::OwnedUserId::try_from("@unknown:localhost").expect("a user id")),
        sender_name: item.sender_name.clone(),
        sender_avatar: None,
        timestamp_ms: item.timestamp_ms,
        is_own: item.is_own,
        is_local_echo: item.sending,
        is_edited: item.edited,
        mentions_me: false,
        can_edit: item.can_edit,
        reactions: item
            .reactions
            .iter()
            .map(|r| morpheus_core::Reaction { key: r.key.clone(), count: r.count as usize, mine: r.mine, senders: Vec::new() })
            .collect(),
        in_reply_to: None,
        thread_root: None,
        thread: None,
        kind,
        state_kind: None,
        state_who: None,
        shield: None,
        read_by: Vec::new(),
    }
}

impl Morpheus {
    fn room_summary(&self, id: &str) -> Option<RoomSummary> {
        self.rooms_now.lock().ok()?.iter().find(|r| r.id.as_str() == id).cloned()
    }

    pub(crate) fn store_menu(&self, items: &[MenuItem]) -> Vec<MenuRow> {
        self.store(items, None, None)
    }

    fn store(&self, items: &[MenuItem], room: Option<OwnedRoomId>, event_id: Option<String>) -> Vec<MenuRow> {
        self.store_with(items, room, event_id, None)
    }

    fn store_with(&self, items: &[MenuItem], room: Option<OwnedRoomId>, event_id: Option<String>, media: Option<morpheus_core::MediaRef>) -> Vec<MenuRow> {
        let rows = rows_of(items, "");
        if let Ok(mut slot) = self.menu.lock() {
            *slot = Some(Stored { items: items.to_vec(), room, event_id, media });
        }
        rows
    }
}

#[uniffi::export]
impl Morpheus {
    /// The menu of a room or DM, as the desktop's right-click menu has it.
    pub async fn room_menu_rows(&self, room_id: String) -> Result<Vec<MenuRow>, FfiError> {
        let session = self.session()?;
        let room = self.room_summary(&room_id).ok_or_else(|| failed("that room is not in the list"))?;
        let items = match room.dm_user.clone().filter(|_| room.is_dm) {
            Some(user) => {
                let layout = crate::sidebar::Layout::load(&session).await;
                let blocked = session.is_blocked(user.clone()).await.unwrap_or(false);
                let mutual = session.mutual_spaces(user.clone()).await.unwrap_or_default();
                let friend = session.friends().await.map(|f| f.contains(&user)).unwrap_or(false);
                let rooms = self.rooms_now.lock().map(|r| r.clone()).unwrap_or_default();
                let state = DmState {
                    pinned: layout.marks.is_pinned(&room),
                    ignored: layout.marks.ignored.contains_key(user.as_str()),
                    blocked,
                    muted: layout.marks.is_muted(&room),
                    friend,
                    invite_to: available_invite_targets(&rooms, None, &mutual),
                    name: room.name.clone(),
                    user,
                };
                dm_menu(&room, &state)
            }
            None => room_menu(&room, session.notify_mode(room.id.clone()).await.ok()),
        };
        let lock = self.lock_state_of(room.id.as_str());
        let items = morpheus_view_model::room_menu::with_lock_items(items, morpheus_view_model::room_menu::lock_items(&room.id, lock.has_lock, lock.locked));
        Ok(self.store(&items, Some(room.id.clone()), None))
    }

    /// The menu of a server (a space), as its badge's right-click menu has it.
    pub async fn server_menu_rows(&self, space_id: String) -> Result<Vec<MenuRow>, FfiError> {
        let session = self.session()?;
        let space = self.room_summary(&space_id).ok_or_else(|| failed("that server is not in the list"))?;
        let rooms = self.rooms_now.lock().map(|r| r.clone()).unwrap_or_default();
        let children = morpheus_view_model::scopes::rooms_in_scope(&rooms, &morpheus_view_model::scopes::Scope::Space(space.id.clone()));
        let prefs = session.ui_prefs().await?;
        let folders = morpheus_view_model::rail_folders::folder_choices(&prefs, space.id.as_str());
        let hide_muted = prefs.get("hide_muted").and_then(|v| v.as_bool()).unwrap_or(false);
        let items = space_menu(&space, &children, hide_muted, &folders);
        Ok(self.store(&items, Some(space.id.clone()), None))
    }

    /// The menu of a person in a room's member list, as the desktop's has it (gated by what you may do there).
    pub async fn member_menu_rows(&self, room_id: String, user_id: String) -> Result<Vec<MenuRow>, FfiError> {
        let session = self.session()?;
        let room = OwnedRoomId::try_from(room_id.as_str()).map_err(|_| failed(format!("{room_id} is not a room id")))?;
        let members = session.members(room.clone()).await?;
        let member = members.iter().find(|m| m.user_id.as_str() == user_id).ok_or_else(|| failed("that person is not in the room"))?;
        let permissions = session.my_permissions(room.clone()).await?;
        let mutual = session.mutual_spaces(member.user_id.clone()).await.unwrap_or_default();
        let friend = session.friends().await.map(|f| f.contains(&member.user_id)).unwrap_or(false);
        let rooms = self.rooms_now.lock().map(|r| r.clone()).unwrap_or_default();
        let items = morpheus_view_model::members::member_menu(&morpheus_view_model::members::MemberMenuInput {
            room: &room,
            member,
            permissions,
            own: session.user_id().as_ref() == Some(&member.user_id),
            friend,
            invite_to: available_invite_targets(&rooms, Some(&room), &mutual),
        });
        Ok(self.store(&items, Some(room), None))
    }

    /// The menu of a rail folder: rename it, or break it up.
    pub async fn folder_menu_rows(&self, folder_id: String, name: String) -> Result<Vec<MenuRow>, FfiError> {
        let items = morpheus_view_model::room_menu::folder_menu(&folder_id, &name);
        Ok(self.store(&items, None, None))
    }

    /// The menu of a message, as the desktop's right-click menu has it (with "Select text", as on a phone).
    pub async fn message_menu_rows(&self, room_id: String, message: MessageItem) -> Result<Vec<MenuRow>, FfiError> {
        let session = self.session()?;
        let room = OwnedRoomId::try_from(room_id.as_str()).map_err(|_| failed(format!("{room_id} is not a room id")))?;
        let core = message_of(&message);
        let pinned = match &core.id {
            MessageId::Event(id) => session.pinned_event_ids(room.clone()).await.map(|p| p.contains(id)).unwrap_or(false),
            _ => false,
        };
        let rooms = self.rooms_now.lock().map(|r| r.clone()).unwrap_or_default();
        let targets = forward_targets(&rooms, &room);
        let items = morpheus_view_model::menu::with_copy_image(message_menu(&core, Some(&room), pinned, &targets, true), &core);
        let media = match &core.kind {
            MessageKind::Image { media, .. } => Some(media.clone()),
            _ => None,
        };
        Ok(self.store_with(&items, Some(room), message.event_id, media))
    }

    /// Do what the item at `path` of the last menu built says. What only the screen can do is told back.
    pub async fn run_menu_item(&self, path: String) -> Result<MenuOutcome, FfiError> {
        let (action, label, room, event_id, media) = {
            let slot = self.menu.lock().map_err(|_| failed("the menu is busy"))?;
            let stored = slot.as_ref().ok_or_else(|| failed("no menu is open"))?;
            let item = at(&stored.items, &path).ok_or_else(|| failed("that item is gone"))?;
            (item.action.clone(), item.label.clone(), stored.room.clone(), stored.event_id.clone(), stored.media.clone())
        };
        let Some(action) = action else { return Ok(MenuOutcome::Done) };
        let session = self.session()?;
        let event = || event_id.as_deref().and_then(|id| morpheus_core::OwnedEventId::try_from(id).ok());
        Ok(match action {
            MenuAction::Reply => MenuOutcome::Reply,
            MenuAction::Edit => MenuOutcome::Edit,
            MenuAction::ReplyInThread => MenuOutcome::Thread,
            MenuAction::React(key) => MenuOutcome::React { key },
            MenuAction::RemoveReactions(keys) => {
                // One toggle each: the screen does them, one at a time.
                return Ok(match keys.into_iter().next() {
                    Some(key) => MenuOutcome::React { key },
                    None => MenuOutcome::Done,
                });
            }
            MenuAction::Delete => MenuOutcome::Delete,
            MenuAction::CopyText(text) | MenuAction::CopyLink(text) | MenuAction::CopyId(text) | MenuAction::SelectText(text) => {
                MenuOutcome::Copy { text }
            }
            MenuAction::CopyImage => {
                let media = media.ok_or_else(|| failed("this message has no picture"))?;
                let file = session.media_file(media).await?.ok_or_else(|| failed("could not fetch the picture"))?;
                MenuOutcome::CopyImage { mime: file.mime().to_owned(), bytes: file.bytes }
            }
            MenuAction::ApplyTheme(text) => {
                let theme = morpheus_view_model::theme_share::find_in_message(&text).ok_or_else(|| failed("there is no theme code in this message"))?;
                MenuOutcome::ShowTheme { code: theme.encode() }
            }
            MenuAction::Pin(pin) => {
                if let (Some(room), Some(event)) = (room, event()) {
                    session.pin(room, event, pin).await?;
                }
                MenuOutcome::Done
            }
            MenuAction::MarkUnread => {
                if let Some(room) = room {
                    session.mark_unread(room, true).await?;
                }
                MenuOutcome::Done
            }
            MenuAction::Forward(to) => {
                if let (Some(from), Some(event)) = (room, event()) {
                    session.forward(from, event, to).await?;
                }
                MenuOutcome::Done
            }
            MenuAction::MarkRead(ids) => {
                self.mark_rooms_read(ids.iter().map(ToString::to_string).collect()).await?;
                MenuOutcome::Done
            }
            MenuAction::MarkRoomUnread(id) => {
                session.mark_unread(id, true).await?;
                MenuOutcome::Done
            }
            MenuAction::Notify(ids, mode) => {
                for id in ids {
                    session.set_notify_mode(id, mode).await?;
                }
                MenuOutcome::Done
            }
            MenuAction::Leave(id, _) => {
                session.leave_room(id).await?;
                MenuOutcome::Done
            }
            MenuAction::Favorite(id, on) => {
                session.set_favorite(id, on).await?;
                MenuOutcome::Done
            }
            MenuAction::PinDm(id, on) => {
                self.set_dm_mark(id.to_string(), "pinned".into(), on).await?;
                MenuOutcome::Done
            }
            MenuAction::CloseDm(id) => {
                self.set_dm_mark(id.to_string(), "closed".into(), true).await?;
                MenuOutcome::Done
            }
            MenuAction::Ignore(user, on) => {
                self.set_ignored(user.to_string(), on).await?;
                MenuOutcome::Done
            }
            MenuAction::Block(user, on) => {
                session.block(user, on).await?;
                MenuOutcome::Done
            }
            MenuAction::Mute(id, ms) => {
                self.mute_room(id.to_string(), u64::try_from(ms).unwrap_or(0)).await?;
                MenuOutcome::Done
            }
            MenuAction::Unmute(id) => {
                self.unmute_room(id.to_string()).await?;
                MenuOutcome::Done
            }
            MenuAction::Friend(user, on) => {
                self.set_friend(user.to_string(), on).await?;
                MenuOutcome::Done
            }
            MenuAction::Invite(room, user) => {
                session.invite(room, user).await?;
                MenuOutcome::Done
            }
            MenuAction::ToggleHideMuted => {
                self.change_rail(|prefs| {
                    let on = prefs.get("hide_muted").and_then(|v| v.as_bool()).unwrap_or(false);
                    if let Some(object) = prefs.as_object_mut() {
                        object.insert("hide_muted".into(), (!on).into());
                    }
                })
                .await?;
                MenuOutcome::Done
            }
            MenuAction::LockNow(room) => {
                self.lock_room(room.to_string());
                MenuOutcome::Done
            }
            MenuAction::SwitchAccount(key) => {
                self.switch_account(key).await?;
                MenuOutcome::Switched
            }
            MenuAction::AddAccount => MenuOutcome::AddAccount,
            MenuAction::SignOut => MenuOutcome::SignOut,
            MenuAction::ViewReactions => MenuOutcome::ViewReactions,
            MenuAction::Report => MenuOutcome::Report,
            MenuAction::ServerOverview(space) => MenuOutcome::Page { kind: "overview".into(), room_id: space.to_string(), extra: String::new() },
            MenuAction::CreateChannel(space) => MenuOutcome::Page { kind: "create".into(), room_id: space.to_string(), extra: String::new() },
            MenuAction::Ungroup(folder) => {
                self.ungroup_folder(folder).await?;
                MenuOutcome::Done
            }
            MenuAction::RoomDialog(kind) => {
                use morpheus_view_model::room_dialogs::RoomDialogKind as K;
                let page = |kind: &str, room: String, extra: String| MenuOutcome::Page { kind: kind.into(), room_id: room, extra };
                match kind {
                    K::Invite(room) => page("invite", room.to_string(), String::new()),
                    K::Edit(room) => page("edit", room.to_string(), String::new()),
                    K::Privacy(room) => page("privacy", room.to_string(), String::new()),
                    K::Create => {
                        let parent = room.as_ref().and_then(|id| self.room_summary(id.as_str())).and_then(|r| if r.is_space { Some(r.id) } else { r.parent_spaces.first().cloned() });
                        page("create", parent.map(|p| p.to_string()).unwrap_or_default(), String::new())
                    }
                    K::RenameFolder(id, name) => page("rename-folder", id, name),
                    K::LockSet(room) => page("lock-set", room.to_string(), String::new()),
                    K::LockRemove(room) => page("lock-remove", room.to_string(), String::new()),
                    K::LeaveSpace(space) => page("leave", space.to_string(), String::new()),
                    K::CreateSpace(Some(space)) => page("category", space.to_string(), String::new()),
                    K::CreateSpace(None) => page("create-server", String::new(), String::new()),
                    K::SetStatus => page("status", String::new(), String::new()),
                    K::RoomProfile(space) => page("profile", space.to_string(), String::new()),
                    _ => MenuOutcome::NotYet { label: i18n::t(&label) },
                }
            }
            MenuAction::Kick(room, user) => {
                session.kick(room, user, None).await?;
                MenuOutcome::Done
            }
            MenuAction::Ban(room, user) => {
                session.ban(room, user, None).await?;
                MenuOutcome::Done
            }
            MenuAction::SetRole(room, user, role) => {
                session.set_role(room, user, role).await?;
                MenuOutcome::Done
            }
            MenuAction::Message(user) => MenuOutcome::OpenRoom { room_id: self.start_dm(user.to_string()).await? },
            MenuAction::Mention(name) => MenuOutcome::Mention { name },
            MenuAction::MoveToFolder(id, folder) => {
                self.move_server_to_folder(id.to_string(), Some(folder)).await?;
                MenuOutcome::Done
            }
            MenuAction::MoveToNewFolder(id) => {
                self.move_server_to_folder(id.to_string(), None).await?;
                MenuOutcome::Done
            }
            MenuAction::MoveOutOfFolders(id) => {
                self.move_server_out_of_folder(id.to_string()).await?;
                MenuOutcome::Done
            }
            _ => MenuOutcome::NotYet { label: i18n::t(&label) },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use morpheus_core::NotifyMode;
    use morpheus_view_model::menu::MenuItem;

    fn labels(rows: &[MenuRow]) -> Vec<&str> {
        rows.iter().map(|r| r.label.as_str()).collect()
    }

    #[test]
    fn the_sheet_has_the_rows_of_the_desktop_menu_in_the_same_order() {
        let room = morpheus_core::OwnedRoomId::try_from("!r:x").expect("room");
        let items = vec![
            MenuItem::new("Mark as read", MenuAction::MarkRead(vec![room.clone()])),
            MenuItem::submenu("Notifications", vec![MenuItem::new("All messages", MenuAction::Notify(vec![room.clone()], NotifyMode::AllMessages)).checked(true)]),
            MenuItem::new("Leave room", MenuAction::Leave(room, "r".into())).danger(),
        ];
        let rows = rows_of(&items, "");
        assert_eq!(labels(&rows), ["Mark as read", "Notifications", "Leave room"]);
        assert_eq!(rows[1].children[0].path, "1.0");
        assert!(rows[1].children[0].checked);
        assert!(rows[2].danger);
        assert_eq!(rows[0].icon.as_deref(), Some("check-check"));
        assert_eq!(at(&items, "1.0").map(|i| i.label.as_str()), Some("All messages"));
        assert!(at(&items, "3").is_none());
    }

    #[test]
    fn a_message_menu_on_a_phone_is_the_desktops_with_select_text() {
        let item = MessageItem {
            key: "k".into(),
            event_id: Some("$e:x".into()),
            sender_id: "@bob:x".into(),
            sender_name: "bob".into(),
            sender_avatar: None,
        sender_avatar_key: None,
            time: "12:00".into(),
            timestamp_ms: 0,
            show_header: true,
            is_own: false,
            sending: false,
            edited: false,
            can_edit: false,
            can_delete: false,
            body: MessageBody::Text { text: "hello".into(), segments: Vec::new() },
            reply_to: None,
            reactions: Vec::new(),
            thread: None,
            preview_url: None,
            read_by: Vec::new(),
        };
        let room = morpheus_core::OwnedRoomId::try_from("!r:x").expect("room");
        let core = message_of(&item);
        let desktop = message_menu(&core, Some(&room), false, &[], true);
        let rows = rows_of(&desktop, "");
        let names = labels(&rows);
        for wanted in ["Reply", "Reply in thread", "Add reaction", "Pin message", "Mark unread", "Select text", "Copy text", "Copy message link", "Report message", "Copy message ID"] {
            assert!(names.contains(&wanted), "{wanted} is missing: {names:?}");
        }
        assert!(!names.contains(&"Delete message"), "not someone else's message");
        assert_eq!(rows[names.iter().position(|n| *n == "Add reaction").expect("grid")].grid, true);
    }
}
