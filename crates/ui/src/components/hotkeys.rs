//! Keyboard shortcuts, following Discord's defaults. A window-level listener
//! (JavaScript, so it works whatever has focus) names the pressed combo and
//! the shell acts on it; a few actions belong to a component (the composer,
//! the timeline) and reach it through [`HotkeyRequest`].

use dioxus::prelude::*;

/// Actions a component handles itself. The shell sets one; the component
/// that owns it acts and clears it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hotkey {
    /// Ctrl+Shift+U: pick a file to send.
    Attach,
    /// Up in an empty composer: edit your last message.
    EditLast,
    /// Escape with nothing open: jump to the newest message and mark read.
    JumpToBottom,
}
pub type HotkeyRequest = Signal<Option<Hotkey>>;

/// Installed once by the shell. Combos that the webview would otherwise act
/// on (print, view source) are swallowed here.
pub const LISTENER_JS: &str = r#"
window.addEventListener('keydown', e => {
  const t = e.target;
  const typing = !!t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.isContentEditable);
  const k = e.key, mod = e.ctrlKey || e.metaKey, alt = e.altKey, shift = e.shiftKey;
  const key = k.length === 1 ? k.toLowerCase() : k;
  let name = null;
  if (k === 'Escape' && shift) name = 'mark_scope_read';
  else if (k === 'Escape') name = 'escape';
  else if (mod && !alt && !shift && key === 'k') name = 'switcher';
  else if (mod && !alt && key === '/') name = 'shortcuts';
  else if (alt && !mod && !shift && k === 'ArrowUp') name = 'prev_room';
  else if (alt && !mod && !shift && k === 'ArrowDown') name = 'next_room';
  else if (alt && !mod && shift && k === 'ArrowUp') name = 'prev_unread';
  else if (alt && !mod && shift && k === 'ArrowDown') name = 'next_unread';
  else if (alt && mod && k === 'ArrowUp') name = 'prev_scope';
  else if (alt && mod && k === 'ArrowDown') name = 'next_scope';
  else if (alt && !mod && k === 'ArrowLeft') name = 'back';
  else if (alt && !mod && k === 'ArrowRight') name = 'forward';
  else if (mod && !alt && !shift && key === 'i') name = 'inbox';
  else if (mod && !alt && !shift && key === 'u') name = 'members';
  else if (mod && !alt && !shift && key === 'p') name = 'pins';
  else if (mod && !alt && key === ',') name = 'settings';
  else if (mod && !alt && shift && key === 'u') name = 'attach';
  else if (k === 'ArrowUp' && !mod && !alt && !shift && typing && t.value === '' && t.closest('.composer')) name = 'edit_last';
  if (!name) return;
  if (name !== 'escape' && name !== 'edit_last') e.preventDefault();
  dioxus.send(name);
});
"#;

/// Rows for the shortcuts dialog: (keys, what it does), grouped.
pub const GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Navigation",
        &[
            ("Ctrl+K", "Jump to a room"),
            ("Alt+↑ / Alt+↓", "Previous / next room"),
            ("Alt+Shift+↑ / ↓", "Previous / next unread room"),
            ("Ctrl+Alt+↑ / ↓", "Previous / next server (rail)"),
            ("Alt+← / Alt+→", "Back / forward through visited rooms"),
        ],
    ),
    (
        "Messages",
        &[
            ("Enter", "Send (Shift+Enter for a new line)"),
            ("↑ in an empty box", "Edit your last message"),
            ("Ctrl+Shift+U", "Attach a file"),
            ("Esc", "Close what is open, or jump to the newest message and mark the room read"),
            ("Shift+Esc", "Mark every room in this scope as read"),
            ("Right-click", "Actions for a message, room, or member"),
        ],
    ),
    (
        "Panels",
        &[
            ("Ctrl+I", "Inbox"),
            ("Ctrl+P", "Pinned messages"),
            ("Ctrl+U", "Show or hide the member list"),
            ("Ctrl+,", "Settings"),
            ("Ctrl+/", "This list"),
        ],
    ),
];
