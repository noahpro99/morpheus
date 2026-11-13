//! Keyboard shortcuts (`crates/ui/src/components/hotkeys.rs`). The shell's
//! root listens in the capture phase, so a combo works whatever has focus
//! (also in a text box, as in the Dioxus app); which combo is which action
//! is `view_model::hotkeys`.

use gpui::{Context, KeyDownEvent, Window};

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
            || self.card.is_some()
            || self.picker.is_some()
            || self.switcher.is_some()
            || self.settings.as_ref().is_some_and(|s| s.read(cx).is_open())
    }

    /// Whether a message box has something open that Escape closes first
    /// (an edit or reply banner, suggestions, the emoji panel).
    fn composer_busy(&self, cx: &gpui::App) -> bool {
        let room = self.composer.as_ref().is_some_and(|(_, c)| c.read(cx).busy());
        let thread = self
            .thread
            .as_ref()
            .is_some_and(|(_, panel)| panel.read(cx).composer_busy(cx));
        room || thread
    }

    /// The shell's capture-phase key listener. Returns whether it used the
    /// key (the caller then stops it going on).
    pub fn handle_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let key = keystroke.key.as_str();
        let ringing = !self.ringing(crate::call::now_ms()).is_empty();
        if key == "escape" && !m.shift && !m.control && !m.alt {
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
            // Escape in a call with a card focused: back to the grid.
            if self.calls.focus.is_some() && !self.modal_open(cx) && !self.inbox_open && !self.composer_busy(cx) {
                return self.clear_call_focus(cx);
            }
            // Escape closes the thread panel, when nothing nearer has it.
            if self.thread.is_some() && !self.modal_open(cx) && !self.inbox_open && !self.composer_busy(cx) {
                self.close_thread(cx);
                return true;
            }
            // Escape with nothing open, scrolled up the chat: back to the newest message.
            if !self.modal_open(cx) && !self.inbox_open && !self.composer_busy(cx) {
                let away = self.timeline.as_ref().is_some_and(|(_, t)| t.read(cx).away_from_present());
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
        // Typing with nothing focused goes to the message box, as in Discord.
        if !ctrl
            && !m.alt
            && let Some(text) = keystroke.key_char.clone()
            && text.chars().count() == 1
            && !text.chars().all(char::is_control)
            && !self.modal_open(cx)
            && !self.inbox_open
            && self.root_focus.as_ref().is_some_and(|focus| focus.is_focused(window))
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
        let Some(action) = hotkeys::classify(name, ctrl, m.alt, m.shift) else {
            return false;
        };
        // The panels that toggle themselves stay reachable over their own
        // modal; anything else waits until the modal is closed.
        let toggles_help = matches!(action, Hotkey::Shortcuts) && matches!(self.dialog, Some(Dialog::Help));
        let toggles_switcher = matches!(action, Hotkey::Switcher) && self.switcher.is_some();
        if (self.modal_open(cx) && !toggles_help && !toggles_switcher)
            || (self.inbox_open && !matches!(action, Hotkey::Inbox))
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
            Hotkey::History { forward } => self.nav_jump(if forward { 1 } else { -1 }, cx),
            Hotkey::Inbox => self.toggle_inbox(cx),
            Hotkey::Members => self.toggle_members(cx),
            Hotkey::Pins => self.open_pinned(cx),
            Hotkey::Settings => self.open_settings(crate::view_model::keys::SettingsFocus::None, cx),
            Hotkey::Room { forward } => self.step_room(forward, false, cx),
            Hotkey::Unread { forward } => self.step_room(forward, true, cx),
            Hotkey::Scope { forward } => self.step_scope(forward, cx),
            Hotkey::Switcher => self.toggle_switcher(cx),
            Hotkey::LastPlace => self.toggle_last_place(cx),
            Hotkey::Mention { forward } => self.step_mention(forward, cx),
            Hotkey::Reload => cx.emit(crate::shell::AccountEvent::Reload),
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

    /// Alt+Up / Alt+Down: the room before or after the open one in the
    /// sidebar's list (only rooms with something unread, for `unread`). In
    /// the DMs the Friends page is the first entry of that list.
    fn step_room(&mut self, forward: bool, unread: bool, cx: &mut Context<Self>) {
        let list = crate::room_list::sidebar_order(self, &self.scope);
        let friends = self.scope == Scope::Dms && !unread;
        let offset = usize::from(friends);
        let mut candidates: Vec<bool> = if friends { vec![true] } else { Vec::new() };
        candidates.extend(
            list.iter()
                .map(|r| !unread || r.unread_messages > 0 || r.highlight_count > 0 || r.marked_unread),
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
        self.set_scope(scope.clone(), cx);
        if let Some(first) = crate::room_list::sidebar_order(self, &scope).first() {
            self.open_room(first.id.clone(), cx);
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
        self.set_scope(target.clone(), cx);
        if let Some(room) = self.last_room_in.get(&target).cloned()
            && rooms_in_scope(&self.rooms, &target).iter().any(|r| r.id == room)
        {
            self.open_room(room, cx);
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
        if let Some(room) = hotkeys::step_room(&candidates, current, forward).and_then(|i| all.get(i)) {
            let scope = crate::model::scope_for(&self.rooms, room);
            if scope != self.scope {
                self.set_scope(scope, cx);
            }
            self.open_room(room.id.clone(), cx);
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
