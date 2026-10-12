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

/// Dropped and pasted files, read in the page and handed over as
/// `file:{name, mime, data(base64)}` on the same channel as the shortcuts.
/// A `dropping` class on the body drives the "drop to upload" hint.
pub const FILE_LISTENER_JS: &str = r#"
const sendFiles = files => {
  for (const f of files) {
    const reader = new FileReader();
    reader.onload = () => dioxus.send('file:' + JSON.stringify({ name: f.name || 'pasted', mime: f.type || '', data: String(reader.result).split(',')[1] || '' }));
    reader.readAsDataURL(f);
  }
};
const hasFiles = e => !!e.dataTransfer && [...e.dataTransfer.types].includes('Files');
window.addEventListener('dragenter', e => { if (hasFiles(e)) { e.preventDefault(); document.body.classList.add('dropping'); } }, true);
window.addEventListener('dragover', e => { if (hasFiles(e)) { e.preventDefault(); document.body.classList.add('dropping'); } }, true);
window.addEventListener('dragleave', e => { if (!e.relatedTarget || e.relatedTarget === document.documentElement) document.body.classList.remove('dropping'); }, true);
window.addEventListener('drop', e => {
  document.body.classList.remove('dropping');
  if (e.dataTransfer && e.dataTransfer.files.length) { e.preventDefault(); e.stopPropagation(); sendFiles(e.dataTransfer.files); }
}, true);
window.addEventListener('paste', e => {
  const files = e.clipboardData ? [...e.clipboardData.files] : [];
  if (files.length) { e.preventDefault(); sendFiles(files); }
}, true);
// Phone-width windows get a single-column layout (see shell.rs).
// Measured on the shell element, not the window, so a test can box it.
const shellEl = document.querySelector('.shell');
let wasNarrow = null;
const reportWidth = () => {
  const n = (shellEl ? shellEl.clientWidth : innerWidth) <= 640;
  if (n !== wasNarrow) { wasNarrow = n; dioxus.send('narrow:' + (n ? '1' : '0')); }
};
if (shellEl && window.ResizeObserver) new ResizeObserver(reportWidth).observe(shellEl); else window.addEventListener('resize', reportWidth);
reportWidth();
"#;

/// A file dropped on or pasted into the window, waiting for the composer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DroppedFile {
    pub name: String,
    pub mime: String,
    pub bytes: Vec<u8>,
}
/// Files the composer of the open room should send.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Drops(pub Vec<DroppedFile>);
pub type FileDrops = Signal<Drops>;

/// Parse one `file:` message from the listener.
pub fn parse_dropped(payload: &str) -> Option<DroppedFile> {
    use base64::Engine as _;
    let value: serde_json::Value = serde_json::from_str(payload).ok()?;
    let data = value.get("data")?.as_str()?;
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
    Some(DroppedFile {
        name: value.get("name")?.as_str().unwrap_or("file").to_owned(),
        mime: value.get("mime").and_then(|m| m.as_str()).unwrap_or("").to_owned(),
        bytes,
    })
}
