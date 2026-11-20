//! The signed-in shell (`crates/ui/src/components/shell.rs`): the rail, the
//! sidebar with its footer, and the chat area, over the rooms the core syncs.

use crate::view_model::i18n::t;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use gpui::{
    Context, Entity, FocusHandle, IntoElement, Render, ScrollHandle, Window, div, prelude::*, px,
    rgb,
};
use morpheus_core::{
    MediaRef, Message, MessageId, OwnedEventId, OwnedRoomId, OwnedUserId, Profile, RoomSummary,
    Session, SpaceRoom, TimelineHandle, VectorDiff,
};

use crate::{
    chat,
    composer::Composer,
    dialogs::{self, Dialog, PendingUndo},
    e2e::E2eExt as _,
    emoji_picker::EmojiPicker,
    friends::{FriendsEvent, FriendsPage},
    input::{Submit, TextInput},
    media::{self, Avatars},
    members::{MemberEvent, MembersView},
    menu::{self, OpenMenu},
    model::{Scope, rooms_in_scope},
    prefs::Prefs,
    profile::ProfileCard,
    rail, room_list,
    search::{SearchEvent, SearchPanel},
    settings::{
        DevicesChosen, InputVolumeChanged, KeysChanged, MicTest, NoiseChosen, OutputVolumeChanged,
        SettingsView, ThemeChosen,
    },
    theme,
    thread::{ThreadEvent, ThreadPanel},
    timeline::{LinkClicked, TimelineEvent, TimelineView},
    user_bar,
    view_model::keys::{KeyAccess, SettingsFocus, show_backup_prompt},
    view_model::{marks::Marks, nicknames::NickMap, toast::ToastList},
};

/// How often the lock state is re-read.
const KEYS_RECHECK: std::time::Duration = std::time::Duration::from_secs(30);
/// What is kept of rooms just left: the last 8, and at most 3000 entries (plus
/// pictures) between them. A timeline costs about 2-4 KB an entry (message,
/// rows, measured heights) and its pictures, so the budget is some 12 MB at
/// most; a usual room is 40-150 entries. A room further back opens again from
/// the SDK's event cache on disk, as on a first visit (issue #10: it was 20
/// rooms, and their pictures stayed decoded with them).
const TIMELINE_CACHE: crate::view_model::lru::Budget = crate::view_model::lru::Budget {
    max_items: 8,
    max_weight: 3000,
};

/// The shell root's focus handle: holding it is holding nothing in particular.
pub struct RootFocus(pub FocusHandle);

impl gpui::Global for RootFocus {}

/// What the shell asks of the app's root about accounts (the root owns the
/// session and the screens): switch to a stored one, sign another in, or
/// sign this one out.
#[derive(Clone, Debug)]
pub enum AccountEvent {
    /// Ctrl+R: the interface is built afresh for the same account.
    Reload,
    Switch(String),
    Add,
    SignOut,
}

impl gpui::EventEmitter<AccountEvent> for Shell {}

/// Links handed to the browser during a test run (shown as `.link-log`),
/// from the shell or a page of Settings.
static OPENED: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Hands `url` to the browser; test runs only log the hand-off.
pub(crate) fn open_external(url: &str, cx: &mut gpui::App) {
    if std::env::var_os("MORPHEUS_E2E_PORT").is_some() {
        if let Ok(mut opened) = OPENED.lock() {
            opened.push(url.to_owned());
        }
        cx.refresh_windows();
    } else {
        cx.open_url(url);
    }
}

pub struct Shell {
    /// Absent only in widget tests, which feed `rooms` themselves.
    pub session: Option<Session>,
    pub rooms: Vec<RoomSummary>,
    pub scope: Scope,
    /// The last scope other than the DMs, for Ctrl+Alt+Right to go back to.
    pub last_server: Scope,
    /// The room last open in each scope, for Ctrl+Alt+Right to land in.
    pub last_room_in: std::collections::HashMap<Scope, OwnedRoomId>,
    pub selected: Option<OwnedRoomId>,
    pub prefs: Prefs,
    /// Counts theme changes, so only the last one in a moment is recorded.
    pub look_generation: u64,
    pub open_folders: Vec<String>,
    /// Where a server dragged on the rail would land now.
    pub rail_drop: Option<crate::view_model::rail_drag::Drop>,
    /// A channel is dragged over this row, on this edge (the line shows there).
    pub channel_drop: Option<(morpheus_core::OwnedRoomId, crate::view_model::rail_drag::Zone)>,
    /// The rail badge the pointer is over (its pill grows).
    pub rail_hover: Option<String>,
    /// The top-bar button the pointer is over (its glyph brightens; no background).
    pub bar_hover: Option<&'static str>,
    /// Where the open room's name sits in the top bar (its menu opens under it).
    pub title_bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    pub profile: Option<Profile>,
    /// The accounts stored on this device (the account menu lists them).
    pub accounts: Vec<morpheus_core::SavedAccount>,
    /// The rooms the selected server lists, joined or not.
    pub directory: Vec<SpaceRoom>,
    pub avatars: Avatars,
    /// The open room's messages.
    pub timeline: Option<(OwnedRoomId, Entity<TimelineView>)>,
    /// Timelines of rooms just left, kept as they were (rows, measured
    /// heights, place, pictures) and fed what arrives, so going back is
    /// instant: the most recent ones, within a budget.
    pub timeline_cache: crate::view_model::lru::Lru<OwnedRoomId, Entity<TimelineView>>,
    /// What the open room's message box hears from its timeline.
    composer_link: Option<gpui::Subscription>,
    /// The open room's message box.
    pub composer: Option<(OwnedRoomId, Entity<Composer>)>,
    pub typing: Option<(OwnedRoomId, Entity<crate::typing::TypingView>)>,
    /// The sidebar's room list scrolling.
    pub room_scroll: ScrollHandle,
    /// The shortcuts dialog's list.
    pub help_scroll: ScrollHandle,
    /// The settings window, shown or not (absent only in widget tests).
    pub settings: Option<Entity<SettingsView>>,
    /// Whether this device can read encrypted history.
    pub keys: KeyAccess,
    /// "Later" on the Secure Backup offer, until the app next starts.
    pub backup_later: bool,
    /// The right-click menu, if open.
    pub menu: Option<OpenMenu>,
    pub menu_focus: Option<FocusHandle>,
    /// The dialog over the chat, if any.
    pub dialog: Option<Dialog>,
    pub dialog_focus: Option<FocusHandle>,
    pub take_dialog_focus: bool,
    pub toasts: ToastList,
    /// When the "nothing there" shake began (Alt+Shift+Down with nothing unread).
    pub shake_at: Option<std::time::Instant>,
    /// The channel list's width (dragged at its right edge; kept per device).
    pub sidebar_w: f32,
    /// A drag of its edge: where the pointer was, and the width then.
    pub sidebar_drag: Option<(f32, f32)>,
    /// The width drawn this frame: the kept one within this window's limits.
    pub sidebar_shown: f32,
    /// The F3 stats overlay is showing, and its redraw timer runs.
    pub diag_overlay: bool,
    pub diag_timer: bool,
    /// Deletions waiting out their Undo toast, by toast.
    pub pending: HashMap<u32, PendingUndo>,
    /// Rooms just left, still undoable: out of the lists.
    pub hidden_rooms: HashSet<String>,
    /// Private marks: pinned and closed DMs, ignored people, timed mutes.
    pub marks: Marks,
    /// The people you are friends with.
    pub friends: Vec<OwnedUserId>,
    /// The full emoji picker, and the message it reacts to.
    pub picker: Option<(Entity<EmojiPicker>, MessageId)>,
    /// The profile card (or full profile) that is open.
    pub card: Option<Entity<ProfileCard>>,
    /// Where the last menu opened, for what it opens in its place.
    pub menu_at: gpui::Point<gpui::Pixels>,
    /// The open room's member list.
    pub members: Option<(OwnedRoomId, Entity<MembersView>)>,
    /// Whether the member list shows (kept across rooms).
    pub members_open: bool,
    /// The member count the list was last read at.
    pub members_count: u64,
    /// The open room's timeline, to act on its messages.
    pub timeline_handle: Option<TimelineHandle>,
    /// Whether the message menu, picker or bin in use came from the thread
    /// panel's replies rather than the room's timeline.
    pub acting_in_thread: bool,
    /// The root's own focus, so the window's shortcuts have somewhere to
    /// arrive when nothing else is focused.
    pub root_focus: Option<FocusHandle>,
    /// Whether something modal was showing at the last frame: when it goes, its
    /// focus handle is stale and the keys go back to the root.
    pub overlay_was_open: bool,
    /// The open room's pinned messages.
    pub pinned: Vec<OwnedEventId>,
    /// Your nicknames for people.
    pub nicknames: NickMap,
    /// The custom emoji packs in reach of the open room (`custom_emoji`).
    pub emote_packs: Vec<morpheus_core::EmotePack>,
    /// The open thread, in the side column in place of the members.
    pub thread: Option<(OwnedRoomId, Entity<ThreadPanel>)>,
    /// The room search panel, in the side column when no thread is open.
    pub search: Option<(OwnedRoomId, Entity<SearchPanel>)>,
    /// The header's search box (none in widget tests without an account).
    pub search_input: Option<Entity<TextInput>>,
    /// Whether the box has been wide, so a first render does not animate.
    pub search_was_wide: std::cell::Cell<bool>,
    /// Whether the Friends page fills the chat area, and the page itself.
    /// The server whose welcome screen fills the chat area.
    pub welcome: Option<OwnedRoomId>,
    /// The voice channel under the pointer in the channel list (it shows a chat button).
    pub hovered_voice: Option<OwnedRoomId>,
    /// The member list shown on that server's overview.
    pub welcome_members: Option<(OwnedRoomId, Entity<MembersView>)>,
    /// The server overview's scroll position (also the e2e's probe).
    pub welcome_scroll: gpui::ScrollHandle,
    /// Channels of the overview whose description is open (the rest show one line).
    pub welcome_topics_open: std::collections::HashSet<String>,
    /// A room just opened as a channel whose type may not be known yet: if
    /// it turns out to be a space, its overview replaces it (until the person
    /// goes elsewhere).
    pub watch_space: Option<OwnedRoomId>,
    /// Channels joined from the overview: joining, joined or why not.
    pub welcome_join: std::collections::HashMap<OwnedRoomId, crate::welcome::JoinState>,
    pub friends_open: bool,
    /// The Discover page, when it is open over the chat, and where the user was.
    pub discover: Option<Entity<crate::discover::Discover>>,
    pub discover_place: Option<crate::discover::Place>,
    pub friends_page: Option<Entity<FriendsPage>>,
    /// A direct message just made, to open once the room list has it.
    pub pending_dm: Option<OwnedRoomId>,
    /// The inbox panel, shown or not, and where its keys go.
    pub inbox_open: bool,
    /// The first key of a two-key vim command (`g` of `gg`, `d` of `dd`) in
    /// the timeline.
    pub vim_pending: Option<char>,
    /// The "don't ask again during this share" tick of the open-channel question, and
    /// whether it was ticked for the share going on.
    pub ask_tick: bool,
    /// The rail entry to show when the open-channel question is answered Open.
    pub ask_scope: Option<Scope>,
    pub ask_skipped: bool,
    /// The rooms and servers opened through the streamer question during this share.
    pub opened_this_share: crate::view_model::streamer::OpenedThisShare,
    pub inbox_focus: Option<FocusHandle>,
    pub inbox_take_focus: bool,
    /// A channel of the server that you are not in, shown as a join card.
    pub preview: Option<SpaceRoom>,
    /// The join card's Join was pressed.
    pub joining: bool,
    /// Invites accepted here whose join the room list has not shown yet.
    pub joined_here: HashSet<OwnedRoomId>,
    /// Calls: the one this account is in, who is ringing, the voice flags.
    pub calls: crate::call::Calls,
    /// The quick switcher (Ctrl+K), when open.
    pub switcher: Option<Entity<crate::switcher::Switcher>>,
    /// The rooms visited, for the title bar's back and forward.
    pub nav: crate::view_model::nav::Nav,
    /// The room this account was last in, until the list has it; and the account.
    pub pending_place: Option<crate::view_model::last_place::LastPlace>,
    pub place_user: String,
    /// Messages announced since launch (the inbox's Mentions, the test log).
    pub notices: Vec<morpheus_core::Notification>,
    /// Mentions already seen (their room was open): the inbox leaves them out.
    pub seen_mentions: std::collections::HashSet<String>,
    /// A newer release the app found, and what was done about its card.
    pub update: Option<crate::view_model::updates::Offer>,
    pub update_later: bool,
    /// The "What's new" popup is showing.
    pub whats_new: bool,
    pub update_busy: bool,
    /// A locked room that is open: its password box, the last refusal, whether
    /// a check runs, and whether the box should take the focus.
    pub lock_input: Option<(OwnedRoomId, Entity<TextInput>)>,
    pub lock_error: Option<String>,
    pub lock_busy: bool,
    pub lock_take_focus: bool,
}

impl Shell {
    pub fn new(session: Session, cx: &mut Context<Self>) -> Self {
        let mut shell = Self::bare(Some(session.clone()));
        shell.wait_for_last_place(cx);
        shell.start_update_checks(cx);
        shell.start_patch_notes(&session, cx);
        shell.start_calls(&session, cx);
        shell.start_profile_sync(&session, cx);
        shell.make_search_box(cx);
        shell.start_lock_clock(cx);
        let subscribed = session.room_list();
        cx.spawn(async move |this, cx| {
            let Ok(mut updates) = subscribed.await else {
                return;
            };
            while let Some(diffs) = updates.next().await {
                let alive = this
                    .update(cx, |this, cx| {
                        for diff in diffs {
                            apply(&mut this.rooms, diff);
                        }
                        crate::model::adopt_children(&mut this.rooms);
                        this.forget_confirmed_leaves();
                        this.share_servers_with_card(cx);
                        this.keep_joined_here();
                        this.reopen_closed(cx);
                        this.open_pending_dm(cx);
                        this.redirect_space_selection(cx);
                        this.restore_place(cx);
                        this.sync_members(cx);
                        this.refresh_switcher(cx);
                        crate::badge::set(crate::badge::count_of(&this.rooms), cx);
                        cx.notify();
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
        let profile = session.own_profile();
        cx.spawn(async move |this, cx| {
            if let Ok(profile) = profile.await {
                this.update(cx, |this, cx| {
                    this.profile = Some(profile);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        let prefs = session.ui_prefs();
        cx.spawn(async move |this, cx| {
            if let Ok(value) = prefs.await {
                this.update(cx, |this, cx| {
                    this.prefs = Prefs::from_json(&value);
                    let theme = this.prefs.theme();
                    crate::theme::set(theme);
                    crate::theme::set_custom(this.prefs.custom_colour());
                    crate::theme::set_glass(this.prefs.glass());
                    crate::theme::sync_windows(cx);
                    cx.refresh_windows();
                    // What the account has been using counts as used.
                    this.record_look(cx);
                    if let Some(settings) = &this.settings {
                        let custom = this.prefs.custom_colour();
                        settings.update(cx, |settings, cx| {
                            settings.glass = this.prefs.glass();
                        settings.theme = theme;
                            settings.set_custom_shown(custom, cx);
                        });
                    }
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        let nicknames = session.nicknames();
        cx.spawn(async move |this, cx| {
            if let Ok(names) = nicknames.await {
                this.update(cx, |this, cx| {
                    this.nicknames = names;
                    this.push_nicknames(cx);
                })
                .ok();
            }
        })
        .detach();
        shell.scope = Scope::Rooms;
        let settings = cx.new(|cx| SettingsView::new(Some(session), cx));
        cx.subscribe(&settings, |this, _, _: &KeysChanged, cx| {
            this.refresh_keys(cx)
        })
        .detach();
        cx.subscribe(
            &settings,
            |_, _, live: &crate::settings::CustomColourLive, cx| {
                crate::theme::set_custom(Some(live.0));
                cx.refresh_windows();
            },
        )
        .detach();
        cx.subscribe(
            &settings,
            |this, _, chosen: &crate::settings::CustomColourChosen, cx| {
                crate::theme::set_custom(Some(chosen.0));
                cx.refresh_windows();
                this.prefs.set_custom_colour(Some(chosen.0));
                this.save_prefs(cx);
                this.look_changed(cx);
            },
        )
        .detach();
        cx.subscribe(
            &settings,
            |this, _, chosen: &crate::settings::GlassChosen, cx| {
                crate::theme::set_glass(chosen.0);
                crate::theme::sync_windows(cx);
                cx.refresh_windows();
                this.prefs.set_glass(chosen.0);
                this.save_prefs(cx);
                cx.notify();
            },
        )
        .detach();
        cx.subscribe(
            &settings,
            |this, _, imported: &crate::settings::ThemeImported, cx| this.apply_shared_theme(imported.0, cx),
        )
        .detach();
        cx.subscribe(&settings, |this, _, chosen: &ThemeChosen, cx| {
            this.choose_theme(chosen.0, cx)
        })
        .detach();
        cx.subscribe(
            &settings,
            |this, _, restore: &crate::settings::HistoryRestore, cx| {
                let look = restore.0;
                crate::theme::set(look.theme);
                crate::theme::set_custom(look.colour);
                cx.refresh_windows();
                this.prefs.set_theme(look.theme);
                this.prefs.set_custom_colour(look.colour);
                this.save_prefs(cx);
                if let Some(settings) = &this.settings {
                    settings.update(cx, |settings, cx| {
                        settings.theme = look.theme;
                        settings.set_custom_shown(look.colour, cx);
                    });
                }
                this.look_changed(cx);
            },
        )
        .detach();
        cx.subscribe(
            &settings,
            |this, _, pin: &crate::settings::HistoryPin, cx| {
                let mut history = this.prefs.theme_history();
                if history.toggle_pin(pin.0) {
                    this.prefs.set_theme_history(&history);
                    this.save_prefs(cx);
                    this.show_history(cx);
                }
            },
        )
        .detach();
        cx.subscribe(
            &settings,
            |this, _, _: &crate::settings::OwnProfileChanged, cx| this.reload_own_profile(cx),
        )
        .detach();
        cx.subscribe(
            &settings,
            |this, _, _: &crate::settings::StreamerChanged, cx| this.streamer_changed(cx),
        )
        .detach();
        cx.subscribe(&settings, |this, _, chosen: &DevicesChosen, cx| {
            this.calls.devices = crate::view_model::calls::read_device_prefs(&crate::data_dir());
            if chosen.0 != crate::view_model::menu::DeviceKind::Camera {
                this.use_devices(cx);
            }
        })
        .detach();
        cx.subscribe(&settings, |this, _, vol: &InputVolumeChanged, cx| {
            this.set_input_volume(vol.0, cx);
        })
        .detach();
        cx.subscribe(&settings, |this, _, vol: &OutputVolumeChanged, cx| {
            this.set_output_volume(vol.0, cx);
        })
        .detach();
        cx.subscribe(&settings, |this, _, chosen: &NoiseChosen, cx| {
            this.choose_noise(chosen.0, cx)
        })
        .detach();
        cx.subscribe(&settings, |this, _, chosen: &super::settings::NoiseModelChosen, cx| {
            this.choose_noise_model(chosen.0, cx)
        })
        .detach();
        cx.subscribe(&settings, |this, _, test: &MicTest, cx| {
            this.set_mic_test(test.0, cx)
        })
        .detach();
        shell.settings = Some(settings);
        shell.refresh_keys(cx);
        shell.load_marks(cx);
        // Keys can arrive from elsewhere: look again now and then.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(KEYS_RECHECK).await;
                if this.update(cx, |this, cx| this.refresh_keys(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        // The call timers count: draw again each second while a call is going.
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_secs(1)).await;
                let alive = this.update(cx, |this, cx| {
                    if this.rooms.iter().any(|r| !r.call_memberships.is_empty() && !r.call_participants.is_empty()) {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    break;
                }
            }
        })
        .detach();
        shell
    }

    /// A shell with no account behind it, for widget tests.
    pub fn bare(session: Option<Session>) -> Self {
        Self {
            session,
            rooms: Vec::new(),
            scope: Scope::default(),
            last_server: Scope::Rooms,
            last_room_in: Default::default(),
            selected: None,
            prefs: Prefs::default(),
            look_generation: 0,
            open_folders: Vec::new(),
            rail_drop: None,
            channel_drop: None,
            rail_hover: None,
            bar_hover: None,
            title_bounds: Default::default(),
            profile: None,
            accounts: Vec::new(),
            directory: Vec::new(),
            avatars: Avatars::default(),
            timeline: None,
            timeline_cache: crate::view_model::lru::Lru::new(TIMELINE_CACHE),
            composer_link: None,
            composer: None,
            typing: None,
            room_scroll: ScrollHandle::new(),
            help_scroll: ScrollHandle::new(),
            settings: None,
            keys: KeyAccess::default(),
            backup_later: false,
            menu: None,
            menu_focus: None,
            dialog: None,
            dialog_focus: None,
            take_dialog_focus: false,
            toasts: ToastList::default(),
            shake_at: None,
            sidebar_w: crate::view_model::sidebar::load(&crate::data_dir())
                .map_or(theme::SIDEBAR_W, |w| w.clamp(crate::view_model::sidebar::MIN_WIDTH, crate::view_model::sidebar::MAX_WIDTH)),
            sidebar_drag: None,
            sidebar_shown: theme::SIDEBAR_W,
            diag_overlay: false,
            diag_timer: false,
            pending: HashMap::new(),
            hidden_rooms: HashSet::new(),
            marks: Marks::default(),
            friends: Vec::new(),
            picker: None,
            card: None,
            menu_at: gpui::Point::default(),
            members: None,
            members_open: true,
            welcome_members: None,
            hovered_voice: None,
            welcome_scroll: gpui::ScrollHandle::new(),
            welcome_topics_open: Default::default(),
            watch_space: None,
            members_count: 0,
            timeline_handle: None,
            acting_in_thread: false,
            root_focus: None,
            overlay_was_open: false,
            pinned: Vec::new(),
            nicknames: NickMap::new(),
            emote_packs: Vec::new(),
            thread: None,
            search: None,
            search_input: None,
            search_was_wide: std::cell::Cell::new(false),
            friends_open: false,
            discover: None,
            discover_place: None,
            welcome: None,
            welcome_join: std::collections::HashMap::new(),
            friends_page: None,
            pending_dm: None,
            inbox_open: false,
            vim_pending: None,
            ask_tick: false,
            ask_scope: None,
            ask_skipped: false,
            opened_this_share: Default::default(),
            inbox_focus: None,
            inbox_take_focus: false,
            preview: None,
            joining: false,
            joined_here: HashSet::new(),
            calls: crate::call::Calls::default(),
            switcher: None,
            nav: Default::default(),
            pending_place: None,
            place_user: String::new(),
            notices: crate::notify::log(),
            seen_mentions: Default::default(),
            update: None,
            update_later: false,
            whats_new: false,
            update_busy: false,
            lock_input: None,
            lock_error: None,
            lock_busy: false,
            lock_take_focus: false,
        }
    }

    /// Makes the header's search box: Enter searches, Escape or an emptied
    /// box closes the results.
    fn make_search_box(&mut self, cx: &mut Context<Self>) {
        let input = cx.new(|cx| {
            TextInput::new(
                "search",
                crate::view_model::search::PLACEHOLDER,
                false,
                "",
                cx,
            )
            .bare("input", "search-input")
            .slim()
        });
        cx.subscribe(&input, |this: &mut Self, _, _: &Submit, cx| {
            this.run_search(cx)
        })
        .detach();
        cx.subscribe(
            &input,
            |this: &mut Self, input, _: &crate::input::Changed, cx| {
                // Emptied by hand: nothing left to show results for.
                if input.read(cx).value().is_empty() && this.search.is_some() {
                    this.search = None;
                    cx.notify();
                }
            },
        )
        .detach();
        self.search_input = Some(input);
    }

    /// Enter in the search box: searches the open room and shows the results
    /// in the side column.
    pub fn run_search(&mut self, cx: &mut Context<Self>) {
        let (Some(room), Some(input)) = (self.selected.clone(), self.search_input.clone()) else {
            return;
        };
        let text = input.read(cx).value().to_owned();
        if text.trim().is_empty() {
            return;
        }
        let panel = match &self.search {
            Some((id, panel)) if id == &room => panel.clone(),
            _ => {
                let panel = cx.new(|_| SearchPanel::new(self.session.clone(), room.clone()));
                cx.subscribe(
                    &panel,
                    |this: &mut Self, _, event: &SearchEvent, cx| match event {
                        SearchEvent::Jump(event_id) => {
                            if let Some((_, timeline)) = &this.timeline {
                                timeline.update(cx, |timeline, cx| {
                                    timeline.jump_to(event_id.clone(), cx)
                                });
                            }
                        }
                    },
                )
                .detach();
                self.search = Some((room, panel.clone()));
                panel
            }
        };
        panel.update(cx, |panel, cx| panel.run(text, cx));
        cx.notify();
    }

    /// Clears the box and closes the results: the member list is back.
    pub fn clear_search(&mut self, cx: &mut Context<Self>) {
        if let Some(input) = &self.search_input {
            input.update(cx, |input, cx| input.set_value("", cx));
        }
        self.search = None;
        cx.notify();
    }

    /// Escape's part in searching; whether it used the key.
    pub fn escape_search(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(input) = self.search_input.clone() else {
            return false;
        };
        if input.read(cx).is_focused(window) {
            if self.search.is_some() {
                self.search = None;
            } else {
                self.clear_search(cx);
                if let Some((_, composer)) = &self.composer {
                    composer.update(cx, |composer, cx| composer.focus_input(window, cx));
                }
            }
            cx.notify();
            return true;
        }
        if self.search.is_some() && !self.composer_busy(cx) {
            self.search = None;
            cx.notify();
            return true;
        }
        false
    }

    /// Ctrl+F: the caret goes to the search box.
    pub fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(input) = &self.search_input {
            input.read(cx).focus(window);
            cx.notify();
        }
    }

    /// Opens a message's thread in the side column.
    pub fn open_thread(&mut self, root: Message, cx: &mut Context<Self>) {
        let Some(room) = self.selected.clone() else {
            return;
        };
        if !crate::view_model::threads::can_open(&root) {
            return;
        }
        let session = self.session.clone();
        let nicknames = self.nicknames.clone();
        let MessageId::Event(root_id) = root.id.clone() else {
            return;
        };
        let room_id = room.clone();
        let panel = cx.new(|cx| ThreadPanel::new(session, room_id, root, root_id, &nicknames, cx));
        let emotes = crate::custom_emoji::all(&self.emote_packs);
        panel.update(cx, |panel, cx| panel.set_emotes(emotes, cx));
        cx.subscribe(
            &panel,
            |this: &mut Self, _, event: &ThreadEvent, cx| match event {
                ThreadEvent::Close => this.close_thread(cx),
                ThreadEvent::Sender(message, at) => {
                    let seen = Some((message.sender_name.clone(), message.sender_avatar.clone()));
                    this.open_profile(message.sender.clone(), Some(*at), seen, cx)
                }
                ThreadEvent::SenderMenu(message, at) => this.open_sender_menu(message, *at, cx),
                ThreadEvent::Menu(message, at) => {
                    this.acting_in_thread = true;
                    this.open_message_menu(message, *at, cx)
                }
                ThreadEvent::Picker(message, at) => {
                    this.acting_in_thread = true;
                    this.open_picker(message.id.clone(), *at, cx)
                }
                ThreadEvent::Delete(message, skip) => {
                    this.acting_in_thread = true;
                    this.delete_message((**message).clone(), *skip, cx)
                }
                ThreadEvent::Failed(what, err) => this.fail(what, err, cx),
                ThreadEvent::Link(url) => this.link_clicked(url, cx),
            },
        )
        .detach();
        self.thread = Some((room, panel));
        cx.notify();
    }

    /// Closes the thread panel; the member list comes back.
    pub fn close_thread(&mut self, cx: &mut Context<Self>) {
        if self.thread.take().is_some() {
            cx.notify();
        }
    }

    /// The Friends page takes the chat area (the top of the DMs).
    pub fn open_friends(&mut self, cx: &mut Context<Self>) {
        if self.friends_page.is_none() {
            let (session, friends, names) = (
                self.session.clone(),
                self.friends.clone(),
                self.nicknames.clone(),
            );
            let page = cx.new(|cx| FriendsPage::new(session, friends, names, cx));
            cx.subscribe(
                &page,
                |this: &mut Self, _, event: &FriendsEvent, cx| match event {
                    FriendsEvent::OpenDm(id) => this.open_dm_when_listed(id.clone(), cx),
                    FriendsEvent::Remove(user) => this.set_friend(user.clone(), false, cx),
                    FriendsEvent::Add(user) => this.set_friend(user.clone(), true, cx),
                    FriendsEvent::Failed(what, err) => this.fail(what, err, cx),
                },
            )
            .detach();
            self.friends_page = Some(page);
        } else if !self.friends_open
            && let Some(page) = &self.friends_page
        {
            // Back after leaving: the rows removed before are gone.
            page.update(cx, |page, cx| page.clear_removed(cx));
        }
        self.friends_open = true;
        cx.notify();
    }

    /// A space is never a channel: when the room shown turns out to be one
    /// (its type is known only after the list has had it a while), its
    /// overview takes its place.
    fn redirect_space_selection(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.clone() else {
            return;
        };
        if self.watch_space.as_ref() != Some(&id) {
            return;
        }
        if self.welcome.as_ref() != Some(&id)
            && self.rooms.iter().any(|room| room.id == id && room.is_space && !room.invited)
        {
            self.show_welcome(id, cx);
        }
    }

    /// A direct message that was just made is opened once the room list has
    /// it (before that it would have no name).
    fn open_dm_when_listed(&mut self, id: OwnedRoomId, cx: &mut Context<Self>) {
        if self.rooms.iter().any(|room| room.id == id) {
            self.open_room_in(Scope::Dms, id, cx);
        } else {
            self.set_scope(Scope::Dms, cx);
            self.pending_dm = Some(id);
        }
    }

    /// A room just accepted shows as joined even while the list still says
    /// invited; once the list agrees, it is left to it.
    fn keep_joined_here(&mut self) {
        let joined = &mut self.joined_here;
        for room in &mut self.rooms {
            if joined.contains(&room.id) {
                if room.invited {
                    room.invited = false;
                } else {
                    joined.remove(&room.id);
                }
            }
        }
    }
    /// Opens the DM or joined channel waiting for the room list, if it has
    /// come; and the open room that was only an invite or a join card, once
    /// it is joined.
    pub fn open_pending_dm(&mut self, cx: &mut Context<Self>) {
        let listed = |this: &Self, id: &OwnedRoomId| {
            this.rooms
                .iter()
                .any(|room| &room.id == id && !room.invited)
        };
        if let Some(id) = self.pending_dm.clone()
            && listed(self, &id)
        {
            self.pending_dm = None;
            self.open_room_in_scope(id, cx);
            return;
        }
        if let Some(id) = self.selected.clone()
            && self.session.is_some()
            && self.timeline.is_none()
            && !self.friends_open
            && listed(self, &id)
        {
            self.open_room(id, cx);
        }
    }

    /// Opens Settings, on the page `focus` asks for.
    pub fn open_settings(&mut self, focus: SettingsFocus, cx: &mut Context<Self>) {
        if let Some(settings) = &self.settings {
            settings.update(cx, |settings, cx| settings.open(focus, cx));
        }
    }

    /// Reads whether this device can read encrypted history, and tells the
    /// open timeline (its locked rows and banner follow).
    pub fn refresh_keys(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let status = session.encryption_status();
        cx.spawn(async move |this, cx| {
            if let Ok(status) = status.await {
                this.update(cx, |this, cx| {
                    let keys = KeyAccess::from_status(&status);
                    if keys != this.keys {
                        this.keys = keys;
                        if let Some((_, timeline)) = &this.timeline {
                            timeline.update(cx, |timeline, cx| timeline.set_keys(keys, cx));
                        }
                        cx.notify();
                    }
                })
                .ok();
            }
        })
        .detach();
    }

    /// Whether the open room's chat is out of sight: a voice channel's page
    /// with its chat closed. Nothing is read, and notices still come, then.
    pub fn chat_covered(&self) -> bool {
        self.discover.is_some()
            || !self.calls.voice_chat && self.current().is_some_and(|r| r.is_voice && !r.is_dm)
    }

    /// The open room is being read (its timeline sends the read receipt):
    /// its counts and mentions go from the lists at once, not when the
    /// server's answer comes round.
    fn see_open_room(&mut self, cx: &mut Context<Self>) {
        let Some(room) = self.selected.clone() else {
            return;
        };
        let reads = self
            .timeline
            .as_ref()
            .is_some_and(|(id, timeline)| *id == room && timeline.read(cx).reads());
        if !reads {
            return;
        }
        if let Some(summary) = self.rooms.iter_mut().find(|r| r.id == room)
            && (summary.unread_messages > 0 || summary.highlight_count > 0)
        {
            summary.unread_messages = 0;
            summary.highlight_count = 0;
        }
        for notice in self
            .notices
            .iter()
            .filter(|n| n.room_id == room && n.is_mention)
        {
            self.seen_mentions.insert(notice.event_id.clone());
        }
    }

    /// The open room, if it is one we have joined.
    pub fn current(&self) -> Option<&RoomSummary> {
        let id = self.selected.as_ref()?;
        self.rooms.iter().find(|r| &r.id == id)
    }

    /// What a rail badge does: list another scope.
    pub fn set_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        self.watch_space = None;
        if self
            .welcome
            .as_ref()
            .map(|w| Scope::Space(w.clone()))
            .as_ref()
            != Some(&scope)
        {
            self.welcome = None;
        }
        if scope != Scope::Dms {
            self.last_server = scope.clone();
        }
        self.scope = scope.clone();
        self.directory.clear();
        if let (Some(session), Scope::Space(id)) = (&self.session, &scope) {
            let listing = session.space_rooms(id.clone());
            cx.spawn(async move |this, cx| {
                let list = listing.await.unwrap_or_default();
                this.update(cx, |this, cx| {
                    if this.scope == scope {
                        this.directory = list;
                        cx.notify();
                    }
                })
                .ok();
            })
            .detach();
        }
        cx.notify();
    }

    /// Picking a rail entry (badge, DMs, Rooms, favourites): its list, with
    /// the room last open there open again (as Discord does), if it is still
    /// in it. A server with none remembered opens its top channel, the first
    /// the sidebar lists (not a voice channel: a click on that joins its call);
    /// the other lists only change.
    pub fn pick_scope(&mut self, scope: Scope, cx: &mut Context<Self>) {
        let remembered = self.last_room_in.get(&scope).cloned().filter(|room| {
            crate::model::rooms_in_scope(&self.rooms, &scope)
                .iter()
                .any(|r| &r.id == room)
        });
        // A server never opened before welcomes you first.
        if let Scope::Space(space) = &scope
            && self.rooms.iter().any(|r| &r.id == space)
            && !self.is_welcomed(space)
        {
            if remembered.is_none() {
                return self.open_welcome(space.clone(), cx);
            }
            self.mark_welcomed(space);
        }
        let target = remembered.or_else(|| match scope {
            Scope::Space(_) => crate::room_list::sidebar_order(self, &scope)
                .into_iter()
                .find(|r| !r.is_voice && !r.is_space && !r.invited)
                .map(|r| r.id),
            _ => None,
        });
        match target {
            Some(room) if self.selected.as_ref() != Some(&room) || self.welcome.is_some() => {
                self.open_room_in(scope, room, cx)
            }
            _ => self.set_scope(scope, cx),
        }
    }

    /// A member list of `room` whose clicks open profiles and menus.
    pub fn new_members_view(&mut self, room: OwnedRoomId, cx: &mut Context<Self>) -> Entity<MembersView> {
        let session = self.session.clone();
        let of = room.clone();
        let members = cx.new(|cx| MembersView::new(session, room, cx));
        cx.subscribe(
            &members,
            move |this: &mut Self, _, event: &MemberEvent, cx| match event {
                MemberEvent::Context(member, permissions, at) => {
                    this.open_member_menu_in(of.clone(), member, *permissions, *at, cx)
                }
                MemberEvent::Open(member, at) => {
                    let seen = Some((member.name.clone(), member.avatar.clone()));
                    this.open_profile(member.user_id.clone(), Some(*at), seen, cx)
                }
            },
        )
        .detach();
        members
    }

    /// Show a room now, whatever streamer mode would ask (`open_room` asks).
    pub fn open_room_now(&mut self, id: OwnedRoomId, cx: &mut Context<Self>) {
        // A space is a server, never a channel: its overview opens.
        if self.rooms.iter().any(|room| room.id == id && room.is_space && !room.invited) {
            return self.show_welcome(id, cx);
        }
        self.watch_space = Some(id.clone());
        if self.last_room_in.get(&self.scope) != Some(&id) {
            self.last_room_in.insert(self.scope.clone(), id.clone());
            self.save_last_rooms();
        }
        self.nav.record(id.clone());
        // A voice channel (or a DM) is where a call is likely next: have its
        // token ready.
        self.prepare_call(Some(id.clone()), cx);
        // Remembered below, once the room is set.
        // Going to a room leaves the Friends page.
        self.welcome = None;
        self.friends_open = false;
        // An invite (or a channel not joined) has no timeline: its card shows.
        self.preview = None;
        if self.rooms.iter().any(|room| room.id == id && room.invited) {
            self.stash_timeline(cx);
            self.composer = None;
            self.typing = None;
            self.timeline_handle = None;
            self.members = None;
            self.thread = None;
            self.selected = Some(id);
            self.save_place();
            cx.notify();
            return;
        }
        if crate::locks::is_locked(&id) {
            // A locked room has no timeline: a lock screen shows in its place.
            self.stash_timeline(cx);
            drop(self.timeline_cache.take(&id));
            self.composer = None;
            self.typing = None;
            self.timeline_handle = None;
            self.members = None;
            self.thread = None;
            self.search = None;
            self.pinned.clear();
            self.selected = Some(id.clone());
            self.show_lock_screen(&id, cx);
            self.save_place();
            cx.notify();
            return;
        }
        self.lock_input = None;
        if self.session.is_some() && self.timeline.as_ref().is_none_or(|(room, _)| room != &id) {
            self.stash_timeline(cx);
            // A room left, or one this account is not in any more, is not reused.
            let rooms: Vec<OwnedRoomId> = self.rooms.iter().map(|r| r.id.clone()).collect();
            drop(self.timeline_cache.retain(|kept| rooms.contains(kept)));
            let kept = self.timeline_cache.take(&id);
            let fresh = kept.is_none();
            let session = self.session.clone();
            let room = id.clone();
            let timeline = match kept {
                Some(kept) => kept,
                None => self.new_timeline_view(&id, cx),
            };
            let (name, is_dm) = self
                .rooms
                .iter()
                .find(|r| r.id == id)
                .map(|r| (r.name.clone(), r.is_dm))
                .unwrap_or_default();
            timeline.update(cx, |t, cx| t.set_direct(is_dm, cx));
            let encrypted = self.rooms.iter().any(|r| r.id == id && r.is_encrypted);
            timeline.update(cx, |t, cx| t.set_encrypted(encrypted, cx));
            let composer =
                cx.new(|cx| Composer::new(self.session.clone(), room.clone(), name, is_dm, cx));
            {
                let timeline = timeline.clone();
                cx.subscribe(&composer, move |_, _, event: &crate::composer::EditLast, cx| {
                    timeline.update(cx, |t, cx| t.start_edit(event.0.clone(), cx))
                })
                .detach();
            }
            let target = composer.clone();
            self.composer_link = Some(cx.subscribe(
                &timeline,
                move |_, _, event: &TimelineEvent, cx| {
                    target.update(cx, |composer, cx| match event {
                        TimelineEvent::Opened(handle) => composer.opened(handle.clone(), cx),
                        TimelineEvent::Reply(message) => composer.reply_to(message, cx),
                        TimelineEvent::LastOwn(message) => composer.set_last_own(message.clone()),
                        TimelineEvent::OpenSettings(_)
                        | TimelineEvent::Menu(..)
                        | TimelineEvent::Picker(..)
                        | TimelineEvent::Delete(..)
                        | TimelineEvent::Sender(..)
                        | TimelineEvent::SenderMenu(..)
                        | TimelineEvent::Thread(..)
                        | TimelineEvent::OpenImage(..)
                        | TimelineEvent::Expand(..)
                        | TimelineEvent::FullscreenVideo(..)
                        | TimelineEvent::Failed(..) => {}
                    })
                },
            ));
            // (a fresh timeline's subscriptions are in `new_timeline_view`)
            let keys = self.keys;
            timeline.update(cx, |timeline, cx| timeline.set_keys(keys, cx));
            let resumed = (!fresh).then(|| timeline.clone());
            self.timeline = Some((id.clone(), timeline));
            self.composer = Some((id.clone(), composer));
            let session = self.session.clone();
            let room = id.clone();
            self.typing = Some((
                id.clone(),
                cx.new(|cx| crate::typing::TypingView::new(session, room, cx)),
            ));
            self.timeline_handle = None;
            self.pinned.clear();
            // A room kept in the background: its composer hears of the open timeline
            // now, and the room is read again.
            if let Some(timeline) = resumed {
                timeline.update(cx, |timeline, cx| timeline.resume(cx));
            }
            // A thread belongs to the room it was opened from.
            self.thread = None;
        }
        if self.session.is_some() && self.members.as_ref().is_none_or(|(room, _)| room != &id) {
            let members = self.new_members_view(id.clone(), cx);
            self.members_count = self
                .rooms
                .iter()
                .find(|r| r.id == id)
                .map_or(0, |r| r.member_count);
            self.members = Some((id.clone(), members));
        }
        self.selected = Some(id.clone());
        self.save_place();
        // A search is of one room: another room starts with an empty box.
        if self.search.as_ref().is_some_and(|(room, _)| room != &id) {
            self.clear_search(cx);
        }
        self.refresh_emotes(cx);
        self.push_nicknames(cx);
        self.refresh_pinned(cx);
        cx.notify();
    }

    /// A timeline view for `id`, wired to the shell (clicks, menus, settings).
    fn new_timeline_view(&mut self, id: &OwnedRoomId, cx: &mut Context<Self>) -> Entity<TimelineView> {
        let session = self.session.clone();
        let timeline = cx.new(|cx| TimelineView::new(session, id.clone(), cx));
        cx.subscribe(&timeline, |this: &mut Self, _, event: &LinkClicked, cx| {
            this.link_clicked(&event.0, cx)
        })
        .detach();
            cx.subscribe(
                &timeline,
                |this: &mut Self, from, event: &TimelineEvent, cx| match event {
                    // A timeline kept in the background says nothing to the shell.
                    _ if this.timeline.as_ref().is_none_or(|(_, open)| *open != from) => {}
                    TimelineEvent::Opened(handle) => {
                        this.timeline_handle = Some(handle.clone())
                    }
                    TimelineEvent::Menu(message, at) => {
                        this.acting_in_thread = false;
                        this.open_message_menu(message, *at, cx)
                    }
                    TimelineEvent::Picker(message, at) => {
                        this.acting_in_thread = false;
                        this.open_picker(message.id.clone(), *at, cx)
                    }
                    TimelineEvent::Delete(message, skip) => {
                        this.acting_in_thread = false;
                        this.delete_message((**message).clone(), *skip, cx)
                    }
                    TimelineEvent::Failed(what, err) => this.fail(what, err, cx),
                    TimelineEvent::Thread(message) => this.open_thread((**message).clone(), cx),
                    TimelineEvent::OpenImage(images, index) => {
                        this.open_lightbox(images.clone(), *index, cx)
                    }
                    TimelineEvent::Expand(message) => this.dialog_open(
                        crate::dialogs::Dialog::LongMessage(Box::new(
                            crate::view_model::rows::MessageRow::new((**message).clone()),
                        )),
                        cx,
                    ),
                    TimelineEvent::FullscreenVideo(player) => {
                        this.open_video(player.clone(), cx)
                    }
                    TimelineEvent::SenderMenu(message, at) => {
                        this.open_sender_menu(message, *at, cx)
                    }
                    TimelineEvent::Sender(message, at) => {
                        let seen =
                            Some((message.sender_name.clone(), message.sender_avatar.clone()));
                        this.open_profile(message.sender.clone(), Some(*at), seen, cx)
                    }
                    _ => {}
                },
            )
            .detach();
            cx.subscribe(
                &timeline,
                |this: &mut Self, _, event: &TimelineEvent, cx| {
                    if let TimelineEvent::OpenSettings(focus) = event {
                        this.open_settings(*focus, cx);
                    }
                },
            )
            .detach();
        timeline
    }

    /// A voice channel was joined: its chat loads in the background, a screen
    /// of messages (past the hidden call events), so that opening the panel
    /// shows it at once.
    pub(crate) fn warm_timeline(&mut self, id: OwnedRoomId, cx: &mut Context<Self>) {
        const PREFILL: usize = 30;
        if self.session.is_none() {
            return;
        }
        if let Some((room, timeline)) = &self.timeline
            && room == &id
        {
            timeline.update(cx, |t, cx| t.prefill(PREFILL, cx));
            return;
        }
        let timeline = match self.timeline_cache.take(&id) {
            Some(kept) => kept,
            None => {
                let fresh = self.new_timeline_view(&id, cx);
                // Out of sight, as a room just left is.
                fresh.update(cx, |t, _| t.leave());
                fresh
            }
        };
        timeline.update(cx, |t, cx| t.prefill(PREFILL, cx));
        let weight = timeline.update(cx, |t, _| t.weight());
        drop(self.timeline_cache.insert(id, timeline, weight));
    }

    /// Puts the open room's timeline away in the cache of rooms just left
    /// (it is read up to its newest message, then reads nothing); what has to
    /// go for the budget is dropped.
    pub(crate) fn stash_timeline(&mut self, cx: &mut Context<Self>) {
        let Some((room, timeline)) = self.timeline.take() else {
            return;
        };
        let weight = timeline.update(cx, |timeline, _| {
            timeline.leave();
            timeline.weight()
        });
        drop(self.timeline_cache.insert(room, timeline, weight));
    }

    /// Tells the memory report how many timelines are kept.
    pub(crate) fn note_timelines(&self) {
        use std::sync::atomic::Ordering;
        crate::memory::TIMELINES.store(
            self.timeline_cache.len() + usize::from(self.timeline.is_some()),
            Ordering::Relaxed,
        );
        crate::memory::TIMELINE_WEIGHT.store(self.timeline_cache.weight(), Ordering::Relaxed);
    }

    /// The channel list's width now, kept within the limits for this window.
    pub fn sidebar_width(&self, window: &Window) -> f32 {
        crate::view_model::sidebar::clamp_width(
            self.sidebar_w,
            f32::from(window.viewport_size().width),
            theme::RAIL_W,
        )
    }

    pub fn save_sidebar_width(&self) {
        if let Err(err) = crate::view_model::sidebar::save(&crate::data_dir(), self.sidebar_w) {
            eprintln!("morpheus-desktop: saving the channel list's width: {err}");
        }
    }

    /// A link in a message: Matrix links open in the app,
    /// others in the browser; test runs only log the hand-off.
    pub(crate) fn link_clicked(&mut self, url: &str, cx: &mut Context<Self>) {
        if morpheus_core::parse_matrix_link(url).is_some() {
            self.open_link(url, cx);
            return;
        }
        open_external(url, cx);
    }

    /// Tells the timelines which message has its right-click menu open, so it
    /// stays highlighted while it is.
    fn mark_menu_message(&mut self, cx: &mut Context<Self>) {
        let open = self.menu.as_ref().and_then(|menu| match &menu.target {
            crate::view_model::menu::MenuTarget::Message { message, .. } => {
                Some(message.unique_id.clone())
            }
            _ => None,
        });
        if let Some((_, timeline)) = &self.timeline {
            timeline.update(cx, |timeline, cx| {
                timeline.set_menu_message(open.clone(), cx)
            });
        }
        if let Some((_, panel)) = &self.thread {
            let (timeline, _) = panel.read(cx).timeline();
            timeline.update(cx, |timeline, cx| timeline.set_menu_message(open, cx));
        }
    }

    /// Starts fetching every picture the frame will draw.
    fn fetch_avatars(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let mut wanted: Vec<(MediaRef, u32)> = Vec::new();
        for room in &self.rooms {
            let size = if room.is_space { 48 } else { 32 };
            wanted.extend(room.avatar.clone().map(|m| (m, size)));
        }
        // Invites: the sidebar row, and the card for the one open.
        for room in self.rooms.iter().filter(|r| r.invited) {
            wanted.extend(
                room.avatar
                    .clone()
                    .into_iter()
                    .flat_map(|m| [(m.clone(), 24), (m, 72)]),
            );
        }
        // The overview's big icon.
        wanted.extend(
            self.welcome
                .as_ref()
                .and_then(|id| self.rooms.iter().find(|r| &r.id == id))
                .and_then(|r| r.avatar.clone())
                .map(|m| (m, 96)),
        );
        if let Some(room) = &self.preview {
            wanted.extend(room.avatar.clone().map(|m| (m, 72)));
        }
        wanted.extend(
            self.profile
                .as_ref()
                .and_then(|p| p.avatar.clone())
                .map(|m| (m, 34)),
        );
        wanted.extend(
            self.profile
                .as_ref()
                .and_then(|p| p.avatar.clone())
                .map(|m| (m, 72)),
        );
        wanted.extend(
            self.rooms
                .iter()
                .filter(|r| r.is_space)
                .filter_map(|r| r.avatar.clone())
                .flat_map(|m| [(m.clone(), 40), (m, 18)]),
        );
        wanted.extend(
            self.current()
                .and_then(|r| r.avatar.clone())
                .map(|m| (m, 24)),
        );
        for person in self.calls.people.values() {
            wanted.extend(
                person
                    .avatar
                    .clone()
                    .into_iter()
                    .flat_map(|m| [(m.clone(), 20), (m, 72)]),
            );
        }
        // A DM's picture is the other person's: theirs falls back to their
        // other picture when this one is gone.
        let mut people: HashMap<String, (OwnedUserId, Option<OwnedRoomId>)> = self
            .rooms
            .iter()
            .filter_map(|r| {
                Some((
                    r.avatar.as_ref()?.key().to_owned(),
                    (r.dm_user.clone()?, Some(r.id.clone())),
                ))
            })
            .collect();
        // People in the call, likewise.
        let call_room = self.calls.state.as_ref().map(|state| state.room_id.clone());
        for (user, person) in &self.calls.people {
            if let Some(media) = &person.avatar {
                people
                    .entry(media.key().to_owned())
                    .or_insert_with(|| (user.clone(), call_room.clone()));
            }
        }
        for (media, size) in wanted {
            if !self.avatars.begin(&media, size) {
                continue;
            }
            let person = people.get(media.key()).cloned();
            let fetching = media::fetch_picture(&session, person, &media, size * 2, size * 2);
            cx.spawn(async move |this, cx| {
                let image = media::fetched(cx, fetching, (size * 2, size * 2), true).await;
                this.update(cx, |this, cx| {
                    this.avatars.finish(&media, size, image);
                    // An open palette shows the picture that has just come.
                    this.refresh_switcher(cx);
                    cx.notify();
                })
                .ok();
            })
            .detach();
        }
    }

    /// The picture for `media` at `size`, if fetched.
    pub fn picture(&self, media: &Option<MediaRef>, size: u32) -> Option<Arc<gpui::Image>> {
        media.as_ref().and_then(|m| self.avatars.get(m, size))
    }
}

/// The Secure Backup offer: a card in the corner while the account has no
/// key backup (a new account's first sign-in), until set up or "Later".
fn backup_prompt(cx: &mut Context<Shell>) -> impl IntoElement {
    let action = |id: &'static str, label: &'static str, primary: bool| {
        div()
            .id(id)
            .px(px(12.0))
            .py(px(8.0))
            .rounded(px(theme::RADIUS))
            .cursor_pointer()
            .bg(if primary {
                rgb(theme::accent())
            } else {
                gpui::rgba(theme::bg_hover()).into()
            })
            .text_color(if primary {
                rgb(theme::on_accent())
            } else {
                rgb(theme::text())
            })
            .child(label)
    };
    div()
        .absolute()
        // Top left of the chat, under the title bar: not the top right (the
        // window's and pages' own buttons, a voice chat's X) nor the bottom left (the
        // call panel above the user bar).
        .left(px(theme::RAIL_W + 280.0))
        .top(px(48.0))
        .w(px(360.0))
        .flex()
        .gap(px(12.0))
        .px(px(16.0))
        .py(px(14.0))
        .rounded(px(10.0))
        .bg(theme::fill_elevated())
        .shadow_md()
        .occlude()
        .child(
            div()
                .mt(px(2.0))
                .text_color(rgb(theme::accent()))
                .child(crate::icons::icon("key", 18.0).text_color(rgb(theme::accent()))),
        )
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
                        .font_weight(gpui::FontWeight::BOLD)
                        .child(t("Set up Secure Backup"))
                        .e2e("strong", "")
                        .text(t("Set up Secure Backup")),
                )
                .child(
                    div()
                        .text_color(rgb(theme::text_muted()))
                        .child(t("Keep your encrypted messages if you lose this device: you'll get a recovery key to save."))
                        .e2e("span", "")
                        .text(t("Keep your encrypted messages if you lose this device: you'll get a recovery key to save.")),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(8.0))
                        .justify_end()
                        .mt(px(6.0))
                        .child(
                            action("backup-later", "Later", false)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.backup_later = true;
                                    cx.notify();
                                }))
                                .e2e("button", "secondary")
                                .text(t("Later")),
                        )
                        .child(
                            action("backup-set-up", "Set up", true)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.open_settings(SettingsFocus::SetUpBackup, cx)
                                }))
                                .e2e("button", "primary")
                                .text(t("Set up")),
                        )
                        .e2e("div", "backup-prompt-actions"),
                )
                .e2e("div", "backup-prompt-text"),
        )
        .e2e("div", "backup-prompt")
        .attr("role", "status")
}

pub use morpheus_view_model::diffs::apply;

impl Render for Shell {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Going anywhere else (a server, a notification, a hotkey) ends Discover.
        if self.discover.is_some() && self.discover_place.as_ref() != Some(&self.place()) {
            self.discover = None;
            self.discover_place = None;
        }
        self.sidebar_shown = self.sidebar_width(window);
        crate::timeline::THREAD_PANEL_OPEN.store(self.thread.is_some(), std::sync::atomic::Ordering::Relaxed);
        // The shake of "nothing there": an offset while it lasts.
        let shake = match self.shake_at {
            Some(at) if at.elapsed() < std::time::Duration::from_millis(crate::view_model::hotkeys::SHAKE_MS) => {
                window.request_animation_frame();
                Some(crate::view_model::hotkeys::shake_offset(
                    at.elapsed().as_secs_f32() * 1000.0 / crate::view_model::hotkeys::SHAKE_MS as f32,
                ))
            }
            _ => {
                self.shake_at = None;
                None
            }
        };
        crate::pictures::drop_orphans(window);
        crate::media::sweep(window, cx);
        self.note_timelines();
        crate::memory::log_now_and_then(window, cx);
        crate::diagnostics::note_rooms(self.rooms.len());
        let covered = self.chat_covered();
        crate::notify::set_looking(
            window.is_window_active(),
            self.selected.clone().filter(|_| !covered),
        );
        self.see_open_room(cx);
        self.sync_streamer(cx);
        if let Some((_, timeline)) = &self.timeline {
            timeline.update(cx, |timeline, cx| timeline.set_covered(covered, cx));
        }
        self.fetch_avatars(cx);
        self.mark_menu_message(cx);
        self.fetch_people(cx);
        self.sync_video_frames(cx);
        if std::mem::take(&mut self.lock_take_focus)
            && let Some((_, input)) = &self.lock_input
        {
            input.read(cx).focus(window);
        }
        let root_focus = self
            .root_focus
            .get_or_insert_with(|| {
                let handle = cx.focus_handle();
                cx.set_global(RootFocus(handle.clone()));
                handle
            })
            .clone();
        // Nothing else focused (or a closed dialog's handle left over): the
        // shortcuts still arrive here.
        let overlay = self.modal_open(cx) || self.inbox_open;
        if (self.overlay_was_open && !overlay) || !root_focus.contains_focused(window, cx) {
            window.focus(&root_focus);
        }
        self.overlay_was_open = overlay;
        let menu = menu::render(self, window, cx);
        let screen_quality = crate::screen_quality::render(self, cx);
        let noise_card = crate::noise::render(self, cx);
        // Your own share's preview rests while the window is in the background.
        let background = match crate::call_ui::FORCE_BACKGROUND.load(std::sync::atomic::Ordering::Relaxed) {
            1 => true,
            2 => false,
            _ => !window.is_window_active() && std::env::var_os("MORPHEUS_E2E_PORT").is_none(),
        };
        if crate::call_ui::set_background(background) {
            cx.notify();
        }
        crate::awake::set_in_call(self.calls.state.is_some());
        self.keep_mic_fresh(cx);
        if self.calls.audio_cache.is_none() && !self.calls.audio_reading && self.session.is_some() {
            self.refresh_audio_cache(cx);
        }
        let dialog = dialogs::render_dialog(self, window, cx);
        let whats_new = self.whats_new_popup(window, cx);
        let inbox = crate::invites::inbox(self, window, cx);
        let toasts = dialogs::render_toasts(self, cx);
        let diagnostics = self.diagnostics_overlay(window, cx);
        let incoming = crate::call_ui::incoming_calls(self, cx);
        let mini_feed = self.mini_call_feed(cx);
        let mini_call = crate::mini_call::render(self, mini_feed, window, cx);
        // The Rooms scope is empty of rooms at first; nothing else to wait for.
        let _ = rooms_in_scope;
        let left = div()
            .flex()
            .flex_col()
            .w(px(theme::RAIL_W + self.sidebar_shown))
            .flex_none()
            .h_full()
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .bg(theme::fill_rail())
                    .child(rail::render(self, cx))
                    .child(room_list::render(self, cx)),
            )
            .child(user_bar::render(self, cx));
        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme::fill_backdrop())
            .track_focus(&root_focus)
            // A click on nothing in particular leaves the focus where it is (the
            // root would otherwise take it from a text box). On Windows a press
            // on the bar's caption still drags: vendor/gpui leaves the caption
            // to the system whatever the app did with the press.
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(|this, _, window, cx| {
                    window.prevent_default();
                    // A click that got here landed on nothing that takes the
                    // caret (a text box would have, first): the search box lets go.
                    if let Some(input) = &this.search_input
                        && input.read(cx).is_focused(window)
                        && let Some(root) = &this.root_focus
                    {
                        window.focus(root);
                        cx.notify();
                    }
                }),
            )
            .on_mouse_move(
                cx.listener(|this, event: &gpui::MouseMoveEvent, window, cx| {
                    if let Some((x0, start)) = this.sidebar_drag {
                        let moved = f32::from(event.position.x) - x0;
                        this.sidebar_w = crate::view_model::sidebar::dragged_width(
                            start,
                            moved,
                            f32::from(window.viewport_size().width),
                            theme::RAIL_W,
                        );
                        cx.notify();
                    }
                    if let Some((y0, start)) = this.calls.stage_drag {
                        let chat = f32::from(window.viewport_size().height) - 32.0 - 48.0;
                        let moved = f32::from(event.position.y) - y0;
                        this.calls.stage_height = Some(
                            crate::view_model::calls::dragged_stage_height(start, moved, chat),
                        );
                        cx.notify();
                    }
                }),
            )
            .on_mouse_up(
                gpui::MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.calls.stage_drag.take().is_some() {
                        cx.notify();
                    }
                    if this.sidebar_drag.take().is_some() {
                        this.save_sidebar_width();
                        cx.notify();
                    }
                }),
            )
            // The mouse's back and forward buttons walk the history as Alt+Left/Right do.
            // A window-level listener, not the root's: Settings, dialogs and menus
            // occlude the root, which would never see the press.
            .child({
                let shell = cx.entity();
                gpui::canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        let shell = shell.clone();
                        window.on_mouse_event(move |event: &gpui::MouseDownEvent, phase, window, cx| {
                            if phase != gpui::DispatchPhase::Capture {
                                return;
                            }
                            match event.button {
                                gpui::MouseButton::Navigate(gpui::NavigationDirection::Back) => {
                                    shell.update(cx, |this, cx| this.go_back(window, cx));
                                    cx.stop_propagation();
                                }
                                gpui::MouseButton::Navigate(gpui::NavigationDirection::Forward) => {
                                    shell.update(cx, |this, cx| {
                                        if !this.modal_open(cx) && !this.inbox_open {
                                            this.nav_jump(1, cx);
                                        }
                                    });
                                    cx.stop_propagation();
                                }
                                _ => {}
                            }
                        });
                    },
                )
                .size_0()
                .absolute()
            })
            .capture_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, window, cx| {
                if this.handle_key(event, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .size_0()
                    .children(
                        OPENED
                            .lock()
                            .map(|opened| opened.clone())
                            .unwrap_or_default()
                            .into_iter()
                            .map(|url| div().size_0().e2e("li", "").text(url)),
                    )
                    .e2e("ul", "link-log"),
            )
            .child(
                div()
                    .size_0()
                    .children(
                        self.calls
                            .sounds
                            .iter()
                            .map(|sound| div().size_0().e2e("li", "").text(*sound)),
                    )
                    .e2e("ul", "sound-log"),
            )
            .child(
                div()
                    .size_0()
                    .children(self.notices.iter().map(|notice| {
                        let line = if crate::locks::is_locked(&notice.room_id) {
                            crate::view_model::locks::NOTIFICATION.to_owned()
                        } else if crate::streamer::notifications_hidden() {
                            "Someone sent a message".to_owned()
                        } else {
                            crate::view_model::notices::log_line(notice)
                        };
                        div()
                            .size_0()
                            .e2e("li", "")
                            .attr("data-room", notice.room_id.to_string())
                            .attr(
                                "data-mention",
                                if notice.is_mention { "true" } else { "false" },
                            )
                            .text(line)
                    }))
                    .e2e("ul", "notification-log"),
            )
            .child(crate::titlebar::render(self, window, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .relative()
                    .left(px(shake.unwrap_or(0.0)))
                    .when_some(self.discover.clone(), |d, page| {
                        // Everything right of the rail is Discover's, above the
                        // user bar, which stays where it is on other pages.
                        let rail_col = div()
                            .w(px(theme::RAIL_W))
                            .flex_none()
                            .h_full()
                            .bg(theme::fill_rail())
                            .child(rail::render(self, cx));
                        // The sidebar column stays under Discover: its colour and width, so
                        // the user bar below it is part of the layout as on every page.
                        let side = div()
                            .w(px(self.sidebar_shown))
                            .flex_none()
                            .h_full()
                            .bg(theme::fill_sidebar())
                            .px(px(16.0))
                            .py(px(14.0))
                            .flex()
                            .flex_col()
                            .gap(px(4.0))
                            .child(
                                div()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .child(t("Discover"))
                                    .e2e("h3", "")
                                    .text(t("Discover")),
                            )
                            .child(
                                div()
                                    .text_size(px(0.85 * theme::REM))
                                    .text_color(rgb(theme::text_muted()))
                                    .child(t("Public servers and rooms"))
                                    .e2e("p", "muted"),
                            )
                            .e2e("div", "discover-sidebar");
                        let bar = user_bar::render(self, cx);
                        d.child(
                            div()
                                .flex()
                                .flex_col()
                                .size_full()
                                .child(
                                    div()
                                        .flex()
                                        .flex_1()
                                        .min_h_0()
                                        .child(rail_col)
                                        .child(side)
                                        .child(page),
                                )
                                .child(
                                    div()
                                        .w(px(theme::RAIL_W + self.sidebar_shown))
                                        .flex_none()
                                        .child(bar),
                                ),
                        )
                    })
                    .when(self.discover.is_none(), |d| {
                        d.child(left).child(chat::render(self, window, cx))
                    })
                    .e2e("div", "shell-body")
                    .attr("data-shaking", if shake.is_some() { "true" } else { "false" })
                    .attr("data-held-pictures", crate::media::held().to_string()),
            )
            .when(show_backup_prompt(self.keys, self.backup_later), |d| {
                d.child(backup_prompt(cx))
            })
            // Over the app (the key backup prompt too), under its dialogs,
            // menus and Settings.
            .children(mini_call)
            .children(self.settings.clone().inspect(|settings| {
                let own = match &self.profile {
                    Some(p) => (
                        p.name.clone(),
                        p.avatar.as_ref().and_then(|m| self.avatars.get(m, 72)),
                    ),
                    None => (String::new(), None),
                };
                settings.update(cx, |settings, _| settings.own = own);
            }))
            .children(inbox)
            .children(incoming)
            .children(self.switcher.clone())
            .children(dialog)
            .children(whats_new)
            .children(self.card.clone())
            .children(self.picker.as_ref().map(|(picker, _)| picker.clone()))
            .children(menu)
            .children(screen_quality)
            .children(noise_card)
            .children(toasts)
            .children(diagnostics)
            .e2e(
                "div",
                if self.members_open {
                    "shell"
                } else {
                    "shell no-side"
                },
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::tests::room;

    #[test]
    fn diffs_apply_in_order() {
        let mut rooms = vec![room("!a:x", "a")];
        apply(
            &mut rooms,
            VectorDiff::PushBack {
                value: room("!b:x", "b"),
            },
        );
        apply(
            &mut rooms,
            VectorDiff::Insert {
                index: 0,
                value: room("!c:x", "c"),
            },
        );
        apply(&mut rooms, VectorDiff::Remove { index: 1 });
        let names: Vec<_> = rooms.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["c", "b"]);
        apply(&mut rooms, VectorDiff::Clear);
        assert!(rooms.is_empty());
    }
}

/// Widget tests: the shell drawn on gpui's test platform, headless, with
/// rooms fed in by hand. They read the frame's tagged tree and click at
/// element centres, so hit-testing is covered too.
#[cfg(all(test, feature = "e2e"))]
mod widget_tests {
    use super::*;
    use crate::e2e::{self, query};
    use crate::model::tests::room;
    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point};

    /// The window's root: the shell in a fresh tree per frame.
    struct Harness(Entity<Shell>);

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            e2e::frame(div().size_full().child(self.0.clone()))
        }
    }

    fn open(
        cx: &mut TestAppContext,
        rooms: Vec<RoomSummary>,
    ) -> (Entity<Shell>, &mut VisualTestContext) {
        let (harness, cx) = cx.add_window_view(|_, cx| Harness(cx.new(|_| Shell::bare(None))));
        let shell = harness.read_with(cx, |h, _| h.0.clone());
        shell.update(cx, |s, cx| {
            s.rooms = rooms;
            cx.notify();
        });
        cx.run_until_parked();
        cx.draw(
            point(px(0.0), px(0.0)),
            gpui::size(px(1100.0), px(720.0)),
            |_, _| div(),
        );
        cx.run_until_parked();
        (shell, cx)
    }

    fn click(cx: &mut VisualTestContext, selector: &str) {
        let (_, (x, y)) = query(selector)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no {selector}"));
        cx.simulate_click(point(px(x), px(y)), Modifiers::none());
        cx.run_until_parked();
    }

    fn texts(selector: &str) -> Vec<String> {
        query(selector).into_iter().map(|(t, _)| t).collect()
    }

    fn seeded() -> Vec<RoomSummary> {
        let mut space = room("!s:x", "Engineering");
        space.is_space = true;
        let mut dev = room("!dev:x", "dev");
        dev.parent_spaces = vec![space.id.clone()];
        let mut bob = room("!bob:x", "bob");
        bob.is_dm = true;
        let mut random = room("!r:x", "random");
        random.unread_messages = 2;
        random.topic = Some("anything goes".into());
        vec![room("!g:x", "general"), random, dev, space, bob]
    }

    #[gpui::test]
    fn the_rail_selects_what_the_sidebar_lists(cx: &mut TestAppContext) {
        let (_shell, cx) = open(cx, seeded());
        assert_eq!(texts(".sidebar-header h2"), ["Rooms"]);
        assert_eq!(texts(".room .room-name"), ["general", "random"]);
        assert_eq!(query(".rail-item.selected[title=\"Rooms\"]").len(), 1);
        assert_eq!(query(".rail-item.selected").len(), 1);

        click(cx, ".rail-item[title=\"Engineering\"]");
        assert_eq!(texts(".sidebar-header h2"), ["Engineering"]);
        assert_eq!(texts(".room .room-name"), ["dev"]);
        assert_eq!(query(".rail-item.selected[title=\"Engineering\"]").len(), 1);

        click(cx, ".rail-item[title=\"Direct messages\"]");
        assert!(texts(".sidebar-header h2").is_empty());
        assert_eq!(query(".sidebar-header .palette-button").len(), 1);
        let bounds = crate::e2e::query_bounds(".sidebar-header .palette-button").unwrap();
        assert!(
            bounds.w > 200.0,
            "palette button should stretch across the sidebar header, got w={}",
            bounds.w
        );
        assert_eq!(texts(".room.dm .room-name"), ["bob"]);
        assert!(
            query(".create-room-button").is_empty(),
            "no create button in DMs"
        );
    }

    #[gpui::test]
    fn opening_a_room_puts_its_name_in_the_header(cx: &mut TestAppContext) {
        let (shell, cx) = open(cx, seeded());
        assert_eq!(texts(".empty-state h2"), ["Pick a room"]);
        assert!(query(".chat-header").is_empty());

        click(cx, ".room[data-room=\"!r:x\"]");
        assert_eq!(texts(".chat-header h2"), ["random"]);
        assert_eq!(texts(".chat-header .topic"), ["anything goes"]);
        assert_eq!(query(".room.selected").len(), 1);
        assert!(query(".empty-state").is_empty());
        shell.read_with(cx, |s, _| {
            assert_eq!(s.current().map(|r| r.name.as_str()), Some("random"))
        });
    }

    #[gpui::test]
    fn rooms_arriving_later_appear_and_unread_ones_are_marked(cx: &mut TestAppContext) {
        let (shell, cx) = open(cx, seeded());
        assert_eq!(query(".room.unread").len(), 1);
        shell.update(cx, |s, cx| {
            s.rooms.push(room("!n:x", "new-room"));
            cx.notify();
        });
        cx.run_until_parked();
        cx.draw(
            point(px(0.0), px(0.0)),
            gpui::size(px(1100.0), px(720.0)),
            |_, _| div(),
        );
        assert!(texts(".room .room-name").contains(&"new-room".to_owned()));
    }

    #[gpui::test]
    fn the_footer_shows_a_name_not_an_id(cx: &mut TestAppContext) {
        let (shell, _cx) = open(cx, Vec::new());
        assert_eq!(texts(".sidebar-footer .profile-chip .user"), [""]);
        shell.read_with(_cx, |s, _| assert!(s.profile.is_none()));
        assert_eq!(texts(".sidebar .room-list .muted"), ["No rooms here."]);
    }
}
