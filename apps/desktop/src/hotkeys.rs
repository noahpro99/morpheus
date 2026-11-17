//! Keyboard shortcuts (`crates/ui/src/components/hotkeys.rs`). The shell's
//! root listens in the capture phase, so a combo works whatever has focus
//! (also in a text box, as in the Dioxus app); which combo is which action
//! is `view_model::hotkeys`.

use gpui::{Context, KeyDownEvent, Window};
use morpheus_core::MessageId;

use crate::{
    dialogs::Dialog,
    model::{Scope, rail_scopes, rooms_in_scope},
    shell::Shell,
    view_model::{
        hotkeys::{self, Hotkey},
        room_dialogs::RoomDialogKind,
    },
};

impl Shell {
    /// Whether something modal has the keys: a dialog, a menu, a card, the
    /// emoji picker or the settings pages.
    pub(crate) fn modal_open(&self, cx: &gpui::App) -> bool {
        self.dialog.is_some()
            || self.menu.is_some()
            || self.calls.screen_popover.is_some()
            || self.calls.noise_card.is_some()
            || self.card.is_some()
            || self.picker.is_some()
            || self.switcher.is_some()
            || self.settings.as_ref().is_some_and(|s| s.read(cx).is_open())
    }

    /// Whether a message box has something open that Escape closes first
    /// (an edit or reply banner, suggestions, the emoji panel).
    pub(crate) fn composer_busy(&self, cx: &gpui::App) -> bool {
        let room = self
            .composer
            .as_ref()
            .is_some_and(|(_, c)| c.read(cx).busy());
        let thread = self
            .thread
            .as_ref()
            .is_some_and(|(_, panel)| panel.read(cx).composer_busy(cx));
        room || thread
    }

    /// Whether a message box (the room's or the thread's) takes Escape in
    /// vim mode, or Ctrl+R (as redo).
    fn vim_input_wants(&self, window: &Window, cx: &gpui::App, ctrl_r: bool) -> bool {
        if !crate::vim::enabled() {
            return false;
        }
        let asks = |composer: &gpui::Entity<crate::composer::Composer>| {
            let composer = composer.read(cx);
            if ctrl_r {
                composer.vim_wants_ctrl_r(window, cx)
            } else {
                composer.vim_wants_escape(window, cx)
            }
        };
        self.composer.as_ref().is_some_and(|(_, c)| asks(c))
            || self
                .thread
                .as_ref()
                .is_some_and(|(_, panel)| asks(panel.read(cx).composer()))
    }

    /// Whether vim mode's timeline keys apply: no box has the keyboard and
    /// nothing is open over the chat.
    fn vim_nav_ready(&self, window: &Window, cx: &gpui::App) -> bool {
        crate::vim::enabled()
            && !self.modal_open(cx)
            && !self.inbox_open
            && !self.chat_covered()
            && self.timeline.is_some()
            && self
                .root_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
    }

    /// Vim mode with no box focused: j and k move a selection through the
    /// messages, gg and G go to the ends, Ctrl+D and Ctrl+U scroll half a
    /// page, / searches, and on the selected message r replies, e edits, dd
    /// (D without asking) deletes, + reacts, y copies, o or Enter opens its
    /// thread or the box, i goes to the box. Whether it took the key.
    fn vim_nav(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        use crate::{message::RowAction, timeline::VimStep};
        if !self.vim_nav_ready(window, cx) {
            self.vim_pending = None;
            return false;
        }
        let Some((_, timeline)) = self.timeline.clone() else {
            return false;
        };
        let ks = &event.keystroke;
        let m = ks.modifiers;
        if m.control && !m.alt && !m.shift && !m.platform && matches!(ks.key.as_str(), "d" | "u") {
            self.vim_pending = None;
            timeline.update(cx, |timeline, cx| {
                timeline.scroll_half_page(ks.key == "d", cx)
            });
            return true;
        }
        if m.control || m.alt || m.platform {
            return false;
        }
        let typed = ks
            .key_char
            .as_ref()
            .filter(|t| t.chars().count() == 1 && !t.chars().any(char::is_control))
            .and_then(|t| t.chars().next());
        let c = match (typed, ks.key.as_str()) {
            (Some(c), _) => c,
            (None, "enter") if !m.shift => '\n',
            _ => return false,
        };
        let pending = self.vim_pending.take();
        let selected = timeline.read(cx).vim_selection();
        let uid = selected.as_ref().map(|m| m.unique_id.clone());
        let event_id = selected
            .as_ref()
            .is_some_and(|m| matches!(m.id, MessageId::Event(_)));
        let act = |action: RowAction, cx: &mut Context<Self>| {
            if let Some(uid) = &uid {
                timeline.update(cx, |timeline, cx| timeline.row_action(action, uid, cx));
            }
        };
        match (pending, c) {
            (Some('g'), 'g') => timeline.update(cx, |t, cx| t.vim_select(VimStep::Oldest, cx)),
            (Some('d'), 'd') => {
                if selected.as_ref().is_some_and(|m| m.is_own) && event_id {
                    act(RowAction::Delete(false), cx);
                }
            }
            (_, 'g') => self.vim_pending = Some('g'),
            (_, 'd') => self.vim_pending = selected.as_ref().map(|_| 'd'),
            (_, 'j') => timeline.update(cx, |t, cx| t.vim_select(VimStep::Newer, cx)),
            (_, 'k') => timeline.update(cx, |t, cx| t.vim_select(VimStep::Older, cx)),
            (_, 'G') => timeline.update(cx, |t, cx| t.vim_select(VimStep::Newest, cx)),
            (_, '/') => {
                if self.current().is_some() {
                    timeline.update(cx, |t, cx| {
                        t.vim_clear(cx);
                    });
                    self.focus_search(window, cx);
                }
            }
            (_, 'r') if event_id => {
                act(RowAction::Reply, cx);
                timeline.update(cx, |t, cx| {
                    t.vim_clear(cx);
                });
            }
            (_, 'e') if selected.as_ref().is_some_and(|m| m.can_edit) => {
                act(RowAction::Edit, cx);
                timeline.update(cx, |t, cx| {
                    t.vim_clear(cx);
                });
            }
            (_, 'D') if selected.as_ref().is_some_and(|m| m.is_own) && event_id => {
                act(RowAction::Delete(true), cx);
            }
            (_, '+') => {
                // Beside the message when it is measured, else mid-window.
                let at = match timeline.read(cx).vim_selection_bounds() {
                    Some(bounds) => gpui::point(
                        bounds.right() - gpui::px(120.0),
                        bounds.top() + gpui::px(12.0),
                    ),
                    None => {
                        let view = window.viewport_size();
                        gpui::point(view.width / 2.0, view.height / 2.0)
                    }
                };
                act(RowAction::MoreReactions(at), cx);
            }
            (_, 'y') => {
                if let Some(text) = selected
                    .as_ref()
                    .and_then(|m| m.kind.plain_body())
                    .filter(|t| !t.is_empty())
                {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_owned()));
                    self.toast("Copied", cx);
                }
            }
            (_, 'o' | '\n') => {
                if selected.as_ref().is_some_and(|m| m.thread.is_some()) && event_id {
                    act(RowAction::Thread, cx);
                } else if selected.is_some() {
                    self.vim_focus_composer(&timeline, window, cx);
                }
            }
            (_, 'i' | 'a') => self.vim_focus_composer(&timeline, window, cx),
            // Any other letter does nothing (it does not go to the box).
            _ => {}
        }
        true
    }

    /// The caret goes to the message box, in insert mode, and the selection goes.
    fn vim_focus_composer(
        &mut self,
        timeline: &gpui::Entity<crate::timeline::TimelineView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        timeline.update(cx, |t, cx| {
            t.vim_clear(cx);
        });
        if let Some((_, composer)) = &self.composer {
            composer.update(cx, |composer, cx| composer.focus_input(window, cx));
        }
    }

    /// The shell's capture-phase key listener. Returns whether it used the
    /// key (the caller then stops it going on).
    pub fn handle_key(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let key = keystroke.key.as_str();
        let ringing = !self.ringing(crate::call::now_ms()).is_empty();
        if key == "escape" && !m.shift && !m.control && !m.alt {
            // Vim mode: a message box in insert or visual mode takes the key
            // (to go to normal mode); with none focused it lets go of the
            // selected message.
            if self.vim_input_wants(window, cx, false) {
                return false;
            }
            if self.vim_nav_ready(window, cx) {
                self.vim_pending = None;
                if let Some((_, timeline)) = self.timeline.clone()
                    && timeline.update(cx, |timeline, cx| timeline.vim_clear(cx))
                {
                    return true;
                }
            }
            // Escape closes the noise suppression card.
            if self.calls.noise_card.is_some() {
                self.close_noise_card(cx);
                return true;
            }
            // Escape closes Share screen's question.
            if self.calls.screen_popover.is_some() {
                self.close_screen_quality(cx);
                return true;
            }
            // Esc declines a call ringing (unless a dialog or menu is open to close).
            if ringing && !self.modal_open(cx) && !self.inbox_open {
                if let Some(call) = self.ringing(crate::call::now_ms()).first().cloned() {
                    self.decline_ring(&call, cx);
                }
                return true;
            }
            // Escape and the search, as in Discord: with results open it
            // closes them (the box keeps its text and caret); again, or with
            // none open, it leaves search: box emptied, caret to the message
            // box. Anywhere else it closes an open results panel.
            if !self.modal_open(cx) && !self.inbox_open && self.escape_search(window, cx) {
                return true;
            }
            // Scrolled up, the thread's own "Jump to present" comes before closing it.
            if let Some((_, panel)) = self.thread.clone()
                && !self.modal_open(cx)
                && !self.inbox_open
                && !self.composer_busy(cx)
            {
                let (timeline, _) = panel.read(cx).timeline();
                if timeline.read(cx).away_from_present() {
                    timeline.update(cx, |timeline, cx| timeline.jump_to_present(cx));
                    return true;
                }
            }
            // Escape in a voice channel with its chat side panel open: first
            // leave the message box, then close the chat panel, then (below)
            // un-focus a card.
            if self.calls.voice_chat
                && self.current().is_some_and(|r| r.is_voice && !r.is_dm)
                && !self.modal_open(cx)
                && !self.inbox_open
                && !self.composer_busy(cx)
            {
                if self
                    .composer
                    .as_ref()
                    .is_some_and(|(_, c)| c.read(cx).input_focused(window, cx))
                    && let Some(root) = self.root_focus.clone()
                {
                    window.focus(&root);
                    cx.notify();
                    return true;
                }
                // Scrolled up, the chat's "Jump to present" comes before closing it.
                if let Some((_, timeline)) = self.timeline.clone()
                    && timeline.read(cx).away_from_present()
                {
                    timeline.update(cx, |timeline, cx| timeline.jump_to_present(cx));
                    return true;
                }
                self.calls.voice_chat = false;
                cx.notify();
                return true;
            }
            // Escape in a call with a card focused: back to the grid.
            if self.calls.focus.is_some()
                && !self.modal_open(cx)
                && !self.inbox_open
                && !self.composer_busy(cx)
            {
                return self.clear_call_focus(cx);
            }
            // Escape closes the thread panel, when nothing nearer has it.
            if self.thread.is_some()
                && !self.modal_open(cx)
                && !self.inbox_open
                && !self.composer_busy(cx)
            {
                self.close_thread(cx);
                return true;
            }
            // Escape with nothing open, scrolled up the chat: back to the newest message.
            if !self.modal_open(cx) && !self.inbox_open && !self.composer_busy(cx) {
                let away = self
                    .timeline
                    .as_ref()
                    .is_some_and(|(_, t)| t.read(cx).away_from_present());
                if away && let Some((_, timeline)) = &self.timeline {
                    timeline.update(cx, |timeline, cx| timeline.jump_to_present(cx));
                    return true;
                }
            }
            return false;
        }
        let ctrl = m.control || m.platform;
        // Ctrl+Enter answers a ringing call.
        if key == "enter" && ctrl && !m.alt && !m.shift && ringing && !self.modal_open(cx) {
            if let Some(call) = self.ringing(crate::call::now_ms()).first().cloned() {
                self.answer_call(&call, cx);
            }
            return true;
        }
        // Vim mode: the timeline's keys.
        if crate::vim::enabled() && self.vim_nav(event, window, cx) {
            return true;
        }
        // Typing with nothing focused goes to the message box, as in Discord.
        if !ctrl
            && !m.alt
            && let Some(text) = keystroke.key_char.clone()
            && text.chars().count() == 1
            && !text.chars().all(char::is_control)
            && !self.modal_open(cx)
            && !self.inbox_open
            && !self.chat_covered()
            && self
                .root_focus
                .as_ref()
                .is_some_and(|focus| focus.is_focused(window))
            && let Some((_, composer)) = &self.composer
        {
            composer.update(cx, |composer, cx| composer.type_in(&text, cx));
            return true;
        }
        // DOM names for the arrows, as `classify` has them.
        let name = match key {
            "up" => "ArrowUp",
            "down" => "ArrowDown",
            other => other,
        };
        // Ctrl+R is redo in a message box in vim's normal mode, not a reload.
        if ctrl && !m.alt && !m.shift && key == "r" && self.vim_input_wants(window, cx, true) {
            return false;
        }
        let Some(action) = hotkeys::classify(name, ctrl, m.alt, m.shift) else {
            return false;
        };
        // The panels that toggle themselves stay reachable over their own
        // modal; anything else waits until the modal is closed.
        let toggles_help =
            matches!(action, Hotkey::Shortcuts) && matches!(self.dialog, Some(Dialog::Help));
        let toggles_switcher = matches!(action, Hotkey::Switcher) && self.switcher.is_some();
        let toggles_diagnostics = matches!(action, Hotkey::Diagnostics);
        if (self.modal_open(cx) && !toggles_help && !toggles_switcher && !toggles_diagnostics)
            || (self.inbox_open && !matches!(action, Hotkey::Inbox | Hotkey::Diagnostics))
        {
            return false;
        }
        match action {
            Hotkey::Search => {
                if self.current().is_some() {
                    self.focus_search(window, cx);
                }
            }
            Hotkey::Emoji => {
                if let Some((_, composer)) = &self.composer {
                    composer.update(cx, |composer, cx| composer.toggle_emoji(cx));
                }
            }
            Hotkey::CreateServer => self.open_room_dialog(RoomDialogKind::CreateSpace(None), cx),
            Hotkey::Shortcuts => self.toggle_help(cx),
            Hotkey::Diagnostics => self.toggle_diagnostics(cx),
            Hotkey::History { forward } => self.nav_jump(if forward { 1 } else { -1 }, cx),
            Hotkey::Inbox => self.toggle_inbox(cx),
            Hotkey::Members => self.toggle_members(cx),
            Hotkey::Pins => self.open_pinned(cx),
            Hotkey::Settings => {
                self.open_settings(crate::view_model::keys::SettingsFocus::None, cx)
            }
            Hotkey::Room { forward } => self.step_room(forward, false, cx),
            Hotkey::Unread { forward } => self.step_room(forward, true, cx),
            Hotkey::Scope { forward } => self.step_scope(forward, cx),
            Hotkey::Switcher => self.toggle_switcher(cx),
            Hotkey::LastPlace => self.toggle_last_place(cx),
            Hotkey::Mention { forward } => self.step_mention(forward, cx),
            Hotkey::Reload => cx.emit(crate::shell::AccountEvent::Reload),
            Hotkey::Quit => crate::send_command(crate::Command::Quit),
            Hotkey::CopyLink => self.copy_room_link(cx),
            Hotkey::Page { down } => {
                if let Some((_, timeline)) = &self.timeline {
                    timeline.update(cx, |timeline, cx| timeline.scroll_page(down, cx));
                }
            }
            Hotkey::ToggleMute => self.toggle_mute(cx),
            Hotkey::ToggleDeafen => self.toggle_deafen(cx),
            Hotkey::CurrentCall => {
                if let Some(room) = self.calls.state.as_ref().map(|c| c.room_id.clone()) {
                    self.open_room(room, cx);
                }
            }
            Hotkey::StartCall => {
                if let Some(room) = self.current().filter(|r| r.is_dm || r.is_voice).cloned()
                    && !self.calls.in_call(&room.id)
                {
                    self.join_call(room.id, room.name, cx);
                }
            }
        }
        let _ = window;
        true
    }

    /// A small sideways shake of the app: there is nothing to go to.
    fn shake(&mut self, cx: &mut Context<Self>) {
        self.shake_at = Some(std::time::Instant::now());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(hotkeys::SHAKE_MS + 40))
                .await;
            this.update(cx, |_, cx| cx.notify()).ok();
        })
        .detach();
    }

    /// Alt+Shift+Up / Down: the next room with something unread wherever it
    /// is (any server, the DMs), in the rail's order then the sidebar's,
    /// wrapping; its server opens with it.
    fn step_unread(&mut self, forward: bool, cx: &mut Context<Self>) {
        let mut all: Vec<morpheus_core::RoomSummary> = Vec::new();
        for scope in rail_scopes(&self.prefs, &self.rooms) {
            for room in crate::room_list::sidebar_order(self, &scope) {
                if !all.iter().any(|r| r.id == room.id) {
                    all.push(room);
                }
            }
        }
        let candidates: Vec<bool> = all
            .iter()
            .map(|r| r.unread_messages > 0 || r.highlight_count > 0 || r.marked_unread)
            .collect();
        let current = self
            .selected
            .as_ref()
            .and_then(|id| all.iter().position(|r| &r.id == id));
        if let Some(room) =
            hotkeys::step_room(&candidates, current, forward).and_then(|i| all.get(i))
        {
            let scope = crate::model::scope_for(&self.rooms, room);
            self.open_room_in(scope, room.id.clone(), cx);
        } else {
            self.shake(cx);
        }
    }

    /// Alt+Up / Alt+Down: the room before or after the open one in the
    /// sidebar's list (only rooms with something unread, for `unread`). In
    /// the DMs the Friends page is the first entry of that list.
    fn step_room(&mut self, forward: bool, unread: bool, cx: &mut Context<Self>) {
        if unread {
            return self.step_unread(forward, cx);
        }
        let list = crate::room_list::sidebar_order(self, &self.scope);
        let friends = self.scope == Scope::Dms && !unread;
        let offset = usize::from(friends);
        let mut candidates: Vec<bool> = if friends { vec![true] } else { Vec::new() };
        candidates.extend(
            list.iter().map(|r| {
                !unread || r.unread_messages > 0 || r.highlight_count > 0 || r.marked_unread
            }),
        );
        let current = if friends && self.friends_open {
            Some(0)
        } else {
            self.selected
                .as_ref()
                .and_then(|id| list.iter().position(|r| &r.id == id))
                .map(|i| i + offset)
        };
        match hotkeys::step_room(&candidates, current, forward) {
            Some(0) if friends => self.open_friends(cx),
            Some(i) => {
                if let Some(room) = list.get(i - offset) {
                    self.open_room(room.id.clone(), cx);
                }
            }
            None => {}
        }
    }

    /// Ctrl+Alt+Up / Down: the rail entry before or after the current one,
    /// with its first room (as the sidebar lists them) open.
    fn step_scope(&mut self, forward: bool, cx: &mut Context<Self>) {
        let all: Vec<Scope> = rail_scopes(&self.prefs, &self.rooms);
        let current = all.iter().position(|s| *s == self.scope);
        let Some(next) = hotkeys::step_scope(all.len(), current, forward) else {
            return;
        };
        let scope = all[next].clone();
        match crate::room_list::sidebar_order(self, &scope)
            .first()
            .map(|first| first.id.clone())
        {
            Some(first) => self.open_room_in(scope, first, cx),
            None => self.set_scope(scope, cx),
        }
    }

    /// Ctrl+Alt+Right: between the DMs and the server (or Rooms) you were
    /// last in, each in the very room you were last in there.
    fn toggle_last_place(&mut self, cx: &mut Context<Self>) {
        let target = if self.scope == Scope::Dms {
            let back = self.last_server.clone();
            // A server since left: Rooms instead.
            let gone = matches!(&back, Scope::Space(id) if !self.rooms.iter().any(|r| &r.id == id));
            if gone { Scope::Rooms } else { back }
        } else {
            self.last_server = self.scope.clone();
            Scope::Dms
        };
        let room = self.last_room_in.get(&target).cloned().filter(|room| {
            rooms_in_scope(&self.rooms, &target)
                .iter()
                .any(|r| r.id == *room)
        });
        match room {
            Some(room) => self.open_room_in(target, room, cx),
            None => self.set_scope(target, cx),
        }
    }

    /// Ctrl+Shift+Alt+Up / Down: the next room that mentions you, in any scope.
    fn step_mention(&mut self, forward: bool, cx: &mut Context<Self>) {
        let all: Vec<morpheus_core::RoomSummary> = rail_scopes(&self.prefs, &self.rooms)
            .iter()
            .flat_map(|scope| rooms_in_scope(&self.rooms, scope))
            .collect();
        let candidates: Vec<bool> = all.iter().map(|r| r.highlight_count > 0).collect();
        let current = self
            .selected
            .as_ref()
            .and_then(|id| all.iter().position(|r| &r.id == id));
        if let Some(room) =
            hotkeys::step_room(&candidates, current, forward).and_then(|i| all.get(i))
        {
            let scope = crate::model::scope_for(&self.rooms, room);
            self.open_room_in(scope, room.id.clone(), cx);
        } else {
            self.shake(cx);
        }
    }

    /// Ctrl+Shift+L: the open room's link, copied.
    fn copy_room_link(&mut self, cx: &mut Context<Self>) {
        if let Some(room) = self.selected.clone() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(format!(
                "https://matrix.to/#/{room}"
            )));
            self.toast("Link copied", cx);
        }
    }
}
