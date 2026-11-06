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
  // Escape in the message box closes what the box has open (an edit, a reply,
  // suggestions, the emoji panel) and nothing else; on a phone it would
  // otherwise also step back to the room list.
  const composerEl = t && t.closest ? t.closest('.composer') : null;
  const boxBusy = !!composerEl && !!composerEl.querySelector('.composer-banner, .autocomplete, .emoji-panel');
  if (k === 'Escape' && boxBusy) name = null;
  else if (k === 'Escape' && shift) name = 'mark_scope_read';
  else if (k === 'Escape') name = 'escape';
  else if (mod && !alt && !shift && key === 'k') name = 'switcher';
  else if (mod && !alt && !shift && key === 'r') name = 'reload';
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
  // Typing with nothing focused goes to the message box, as in Discord.
  if (!name && !typing && !mod && !alt && k.length === 1 && !document.querySelector('.overlay, .context-menu, .card-overlay')) {
    const box = document.querySelector('.composer .composer-input:not([disabled])');
    if (box) { box.focus(); }
    return;
  }
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
            (
                "Esc",
                "Close what is open, or jump to the newest message and mark the room read",
            ),
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
            ("Ctrl+R", "Reload the app"),
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
// Phones have no native picker the app can call (`attach.rs`): the attach
// button opens a file input instead, from the tap itself (Android shows its
// picker only for a real tap), and picked files go the way pasted ones do.
(() => {
  if (!window.__morpheusPagePicker) return;
  const input = document.createElement('input');
  input.type = 'file';
  input.multiple = true;
  input.id = 'morpheus-file-input';
  input.hidden = true;
  input.addEventListener('change', () => { if (input.files.length) sendFiles(input.files); input.value = ''; });
  document.body.append(input);
  window.addEventListener('click', e => {
    if (e.target.closest && e.target.closest('.composer .attach-button')) input.click();
  }, true);
})();
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
// Touch screens: press and hold is right-click, with a short buzz. A finger
// that moves is scrolling, not holding. If the webview raises its own
// long-press contextmenu first, that one opens the menu instead.
(() => {
  const HOLD_MS = 450, SLOP_PX = 10;
  let timer = null, start = null, fired = false;
  const cancel = () => { clearTimeout(timer); timer = null; };
  const buzz = () => { try { if (navigator.vibrate) navigator.vibrate(12); } catch (_) {} };
  window.addEventListener('touchstart', e => {
    cancel();
    fired = false;
    start = null;
    // Selectable text keeps the system's own press and hold.
    if (e.target.closest && e.target.closest('.selectable')) return;
    if (e.touches.length !== 1) return;
    const t = e.touches[0];
    start = { x: t.clientX, y: t.clientY, target: e.target };
    timer = setTimeout(() => {
      timer = null;
      fired = true;
      buzz();
      start.target.dispatchEvent(new MouseEvent('contextmenu', {
        bubbles: true, cancelable: true, button: 2, clientX: start.x, clientY: start.y,
      }));
    }, HOLD_MS);
  }, { passive: true, capture: true });
  window.addEventListener('touchmove', e => {
    if (!timer || !start) return;
    const t = e.touches[0];
    if (Math.hypot(t.clientX - start.x, t.clientY - start.y) > SLOP_PX) cancel();
  }, { passive: true, capture: true });
  // Lifting after a hold must not also tap whatever the menu opened over.
  window.addEventListener('touchend', e => { cancel(); if (fired) { e.preventDefault(); fired = false; } }, { capture: true });
  window.addEventListener('touchcancel', cancel, { capture: true });
  window.addEventListener('contextmenu', e => {
    if (!e.isTrusted || !start) return;
    if (fired) { e.preventDefault(); e.stopPropagation(); return; }
    if (timer) { cancel(); fired = true; buzz(); }
  }, true);
})();
// Android's back key: the app's web view goes back in page history when it
// can, else leaves the app. Keep one history entry while there is something
// to back out of (a menu, a dialog, a card, an edit, a pane past the room
// list) and turn going back into Escape, which closes the top-most thing.
// With nothing open there is no entry, so back leaves the app as usual.
(() => {
  const OPEN = '.overlay, .context-menu, .card-overlay, .lightbox-overlay, .composer-banner, .composer .autocomplete, .composer .emoji-panel';
  const backable = () => {
    if (document.querySelector(OPEN)) return true;
    const shell = document.querySelector('.shell');
    return !!shell && shell.classList.contains('narrow') && (shell.dataset.pane === 'chat' || shell.dataset.pane === 'side');
  };
  let guarded = false, unwinding = false, queued = false;
  const sync = () => {
    queued = false;
    if (unwinding) return;
    const want = backable();
    if (want && !guarded) { history.pushState({ morpheusBack: true }, ''); guarded = true; }
    else if (!want && guarded) { guarded = false; unwinding = true; history.back(); }
  };
  const schedule = () => { if (!queued) { queued = true; queueMicrotask(sync); } };
  window.addEventListener('popstate', () => {
    if (unwinding) { unwinding = false; schedule(); return; }
    guarded = false;
    // An edit, reply or suggestion list in the message box goes first, even
    // when the box lost focus (it does when the phone's keyboard hides).
    const busyBox = [...document.querySelectorAll('.composer')].find(c => c.querySelector('.composer-banner, .autocomplete, .emoji-panel'));
    const el = (busyBox && busyBox.querySelector('.composer-input'))
      || (document.activeElement && document.activeElement !== document.body ? document.activeElement : document.body);
    el.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true, cancelable: true }));
    schedule();
  });
  new MutationObserver(schedule).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['data-pane'] });
  schedule();
})();
// Phones: leaving a chat (back to the room list) puts the keyboard away,
// which a focused box would otherwise keep up over the list.
(() => {
  let pane = null;
  new MutationObserver(() => {
    const shell = document.querySelector('.shell.narrow');
    const now = shell ? shell.dataset.pane : null;
    if (now === pane) return;
    pane = now;
    const el = document.activeElement;
    if (now === 'rooms' && el && el.matches('input, textarea') && el.closest('.chat, .side-pane')) el.blur();
  }).observe(document.body, { childList: true, subtree: true, attributes: true, attributeFilter: ['data-pane'] });
})();
// Touch screens cannot hover, but browsers apply :hover to whatever was last
// tapped and leave it there. Where the device cannot hover, drop the :hover
// selectors from the app's styles (as each stylesheet arrives) so a tap never
// leaves a row, button or message looking hovered.
(() => {
  if (!matchMedia('(hover: none)').matches) return;
  const done = new WeakSet();
  const strip = list => {
    for (let i = list.cssRules.length - 1; i >= 0; i--) {
      const rule = list.cssRules[i];
      if (rule.cssRules && !rule.selectorText) { strip(rule); continue; }
      if (!rule.selectorText || !rule.selectorText.includes(':hover')) continue;
      const keep = rule.selectorText.split(',').filter(s => !s.includes(':hover'));
      if (keep.length) rule.selectorText = keep.join(',');
      else list.deleteRule(i);
    }
  };
  const all = () => {
    for (const sheet of document.styleSheets) {
      if (done.has(sheet)) continue;
      try { strip(sheet); done.add(sheet); } catch (_) { /* not loaded yet, or cross-origin */ }
    }
  };
  all();
  document.addEventListener('load', all, true);
  new MutationObserver(all).observe(document.head, { childList: true, subtree: true });
})();
// Swipes on touch screens. A bottom sheet follows the finger down and closes
// past a third of its height (or on a quick flick). On a phone the panes are
// stacked layers (theme.css): a rightward drag on the chat or side pane moves
// it with the finger, revealing the pane underneath, and steps back one pane
// when let go far or fast enough; a leftward drag on the room list pulls the
// last chat back in over it. Mostly-vertical drags stay scrolls.
(() => {
  const SLOP = 12;
  // Something on top (a dialog, menu, card, image) takes the gesture instead.
  const OPEN_FOR_SWIPE = '.overlay, .context-menu, .card-overlay, .lightbox-overlay';
  let g = null;
  const narrowPane = () => {
    const shell = document.querySelector('.shell');
    return shell && shell.classList.contains('narrow') ? shell.dataset.pane : null;
  };
  // Finish the motion, then (if `then` changes the pane) hand the position
  // back to the pane's CSS once the shell has switched, so nothing jumps.
  const settle = (el, transform, then) => {
    el.style.transition = 'transform 140ms ease-out';
    el.style.transform = transform;
    const release = () => {
      el.style.transition = 'none';
      el.style.transform = '';
      requestAnimationFrame(() => { el.style.transition = ''; });
    };
    setTimeout(() => {
      if (!then) { release(); return; }
      const before = narrowPane();
      then();
      const started = Date.now();
      const wait = () => {
        if (narrowPane() !== before || Date.now() - started > 600) release();
        else requestAnimationFrame(wait);
      };
      requestAnimationFrame(wait);
    }, 150);
  };
  window.addEventListener('touchstart', e => {
    g = null;
    if (e.touches.length !== 1) return;
    const t = e.touches[0];
    const start = { x: t.clientX, y: t.clientY, at: Date.now(), d: 0, axis: null };
    // With a sheet up, a drag anywhere moves it: on its items or the dimmed
    // page around it alike.
    // A menu opened over the profile sheet is the one on top.
    const sheet = document.querySelector('.context-menu.sheet') || document.querySelector('.profile-card.sheet');
    if (sheet) {
      g = { kind: 'sheet', el: sheet, base: 0, ...start };
      return;
    }
    // The call screen goes down like a sheet (to its bar); nothing under it
    // moves sideways.
    const call = document.querySelector('.call-screen');
    if (call) {
      g = { kind: 'call', el: call, base: 0, ...start };
      return;
    }
    if (document.querySelector(OPEN_FOR_SWIPE)) return;
    const pane = narrowPane();
    if (pane === 'chat' || pane === 'side') {
      const el = document.querySelector(pane === 'side' ? '.side-pane' : '.chat');
      if (el) g = { kind: 'back', el, ...start };
    } else if (pane === 'rooms') {
      // Only when there is a chat to come back to.
      const chat = document.querySelector('.chat');
      if (chat && chat.querySelector('.chat-header')) g = { kind: 'forward', el: chat, ...start };
    }
  }, { passive: true, capture: true });
  window.addEventListener('touchmove', e => {
    if (!g) return;
    const t = e.touches[0];
    const dx = t.clientX - g.x, dy = t.clientY - g.y;
    if (!g.axis) {
      if (Math.abs(dx) < SLOP && Math.abs(dy) < SLOP) return;
      g.axis = Math.abs(dx) > Math.abs(dy) * 1.5 ? 'x' : 'y';
      // On the chat, leftward pulls in the parked member list instead.
      if (g.axis === 'x' && g.kind === 'back' && dx < 0 && narrowPane() === 'chat') {
        const side = document.querySelector('.side-pane.parked');
        if (side) { g.kind = 'forward'; g.el = side; g.then = 'members'; }
        else { g = null; return; }
      }
    }
    if (g.kind === 'call' && g.axis === 'y') {
      g.d = Math.max(0, dy);
      g.el.style.transition = 'none';
      g.el.style.transform = `translateY(${g.d}px)`;
    } else if (g.kind === 'sheet' && g.axis === 'y') {
      // A scrolled list scrolls back to its top first; the sheet follows
      // the finger from there.
      if (g.el.scrollTop > 0) { g.base = dy; return; }
      g.d = Math.max(0, dy - g.base);
      g.el.style.transition = 'none';
      g.el.style.transform = `translateY(${g.d}px)`;
    } else if (g.kind === 'back' && g.axis === 'x') {
      g.d = Math.max(0, dx);
      g.el.style.transition = 'none';
      g.el.style.transform = `translateX(${g.d}px)`;
    } else if (g.kind === 'forward' && g.axis === 'x') {
      g.d = Math.max(0, -dx);
      g.el.style.transition = 'none';
      g.el.style.transform = `translateX(max(0px, calc(100% - ${g.d}px)))`;
    }
  }, { passive: true, capture: true });
  window.addEventListener('touchend', () => {
    if (!g || g.d <= 0) { g = null; return; }
    const { kind, el, d, at, then: next } = g;
    g = null;
    const fast = d > 40 && d / Math.max(1, Date.now() - at) > 0.6;
    const far = d > (kind === 'sheet' || kind === 'call' ? el.offsetHeight : el.offsetWidth) / 3;
    if (kind === 'call') {
      // Back (Escape) takes a phone's call screen down to its bar.
      if (fast || far) settle(el, 'translateY(100%)', () => dioxus.send('escape'));
      else settle(el, 'translateY(0)');
    } else if (kind === 'sheet') {
      if (fast || far) {
        settle(el, 'translateY(100%)', () => {
          // The profile sheet closes through its own backdrop, menus through theirs.
          const b = el.closest('.card-overlay') || document.querySelector('.sheet-backdrop');
          if (b) b.click();
        });
      } else settle(el, 'translateY(0)');
    } else if (kind === 'back') {
      if (fast || far) settle(el, 'translateX(100%)', () => dioxus.send('escape'));
      else settle(el, 'translateX(0)');
    } else {
      if (fast || far) {
        // Dragged in by hand: unparking it must not play its slide-in
        // animation as well (it ran from the edge a second time).
        if (next === 'members') el.style.animation = 'none';
        settle(el, 'translateX(0)', () => dioxus.send(next || 'chat_front'));
      } else settle(el, 'translateX(100%)');
    }
  }, { capture: true });
  window.addEventListener('touchcancel', () => { if (g) settle(g.el, g.kind === 'forward' ? 'translateX(100%)' : 'none'); g = null; }, { capture: true });
})();
// Far back in a long room, rows come and go below the view as it scrolls
// (timeline.rs keeps only a few hundred in the page), and WebKit does not
// keep the view where it was. So: the message at the top of the view, and
// where it is, noted on every scroll (before the app hears of it); and when
// the timeline says scrolling changed its rows (`data-anchor` goes up, in the
// same update as the rows), that message is put back where it was.
(() => {
  let anchor = null;
  window.addEventListener('scroll', e => {
    const t = e.target;
    if (!t.classList || !t.classList.contains('timeline')) return;
    const top = t.getBoundingClientRect().top;
    // By its event, not the element: the rows may be drawn afresh.
    const m = [...t.querySelectorAll('.message[data-event-id]:not([data-event-id=""])')]
      .find(m => m.getBoundingClientRect().bottom > top);
    anchor = m ? { id: m.dataset.eventId, top: m.getBoundingClientRect().top } : null;
  }, { capture: true, passive: true });
  new MutationObserver(changes => {
    const t = changes.find(c => c.attributeName === 'data-anchor')?.target;
    const m = t && anchor && t.querySelector('.message[data-event-id=' + JSON.stringify(anchor.id) + ']');
    if (m) t.scrollTop += m.getBoundingClientRect().top - anchor.top;
  }).observe(document.documentElement, { subtree: true, attributes: true, attributeFilter: ['data-anchor'] });
})();
// The call stage's height: its bottom edge is dragged. The height is a CSS
// variable on the page (the stage comes and goes with the cameras) and is
// remembered on this device.
(() => {
  const KEY = 'morpheus.callStageHeight';
  const apply = h => {
    const root = document.documentElement.style;
    root.setProperty('--call-stage-h', h + 'px');
    root.setProperty('--call-stage-max', 'none');
    // The tiles fill it, less the stage's padding.
    root.setProperty('--call-tile-h', Math.max(60, h - 16) + 'px');
  };
  try { const kept = parseFloat(localStorage.getItem(KEY)); if (kept > 0) apply(kept); } catch (_) {}
  let drag = null;
  window.addEventListener('mousedown', e => {
    const edge = e.target.closest && e.target.closest('.call-stage-resize');
    const stage = edge && edge.previousElementSibling;
    if (!stage) return;
    e.preventDefault();
    drag = { y: e.clientY, h: stage.getBoundingClientRect().height };
    document.body.classList.add('resizing-stage');
  }, true);
  window.addEventListener('mousemove', e => {
    if (!drag) return;
    const chat = document.querySelector('.chat');
    const most = chat ? chat.clientHeight * 0.85 : 800;
    drag.now = Math.round(Math.min(most, Math.max(80, drag.h + e.clientY - drag.y)));
    apply(drag.now);
  }, true);
  window.addEventListener('mouseup', () => {
    if (!drag) return;
    if (drag.now) { try { localStorage.setItem(KEY, String(drag.now)); } catch (_) {} }
    drag = null;
    document.body.classList.remove('resizing-stage');
  }, true);
})();
// Links. Formatted messages arrive as HTML, where bare URLs are plain text:
// link them (the same rules as `linkify.rs`, which handles plain messages).
// And every web link opens outside the app: the click goes to the shell,
// which hands it to the browser, instead of navigating this page.
(() => {
  const URL_RE = /(^|[^\w/.])((?:https?:\/\/|www\.)[^\s<]+)/gi;
  const trim = link => {
    for (;;) {
      const last = link[link.length - 1];
      const count = c => link.split(c).length - 1;
      const drop = '.,;:!?\'">'.includes(last)
        || (last === ')' && count(')') > count('('))
        || (last === ']' && count(']') > count('['))
        || (last === '}' && count('}') > count('{'));
      if (!last || !drop) return link;
      link = link.slice(0, -1);
    }
  };
  const linkify = root => {
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT, {
      acceptNode: n => n.parentElement.closest('a, code, pre') ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT,
    });
    const nodes = [];
    while (walker.nextNode()) nodes.push(walker.currentNode);
    for (const node of nodes) {
      const text = node.nodeValue;
      URL_RE.lastIndex = 0;
      if (!URL_RE.test(text)) continue;
      URL_RE.lastIndex = 0;
      const frag = document.createDocumentFragment();
      let at = 0, m;
      while ((m = URL_RE.exec(text))) {
        const start = m.index + m[1].length;
        const link = trim(m[2]);
        const minimum = link.toLowerCase().startsWith('www.') ? 4 : link.indexOf('//') + 2;
        if (link.length <= minimum) continue;
        frag.append(text.slice(at, start));
        const a = document.createElement('a');
        a.className = 'link';
        a.href = link.toLowerCase().startsWith('www.') ? 'https://' + link : link;
        a.rel = 'noopener';
        a.textContent = link;
        frag.append(a);
        at = start + link.length;
        URL_RE.lastIndex = at;
      }
      if (at === 0) continue;
      frag.append(text.slice(at));
      node.replaceWith(frag);
    }
  };
  const scan = () => document.querySelectorAll('.body.rich > div').forEach(linkify);
  let queued = false;
  new MutationObserver(() => {
    if (!queued) { queued = true; requestAnimationFrame(() => { queued = false; scan(); }); }
  }).observe(document.body, { childList: true, subtree: true, characterData: true });
  scan();
  window.addEventListener('click', e => {
    const a = e.target.closest && e.target.closest('a[href]');
    if (!a || !a.closest('.shell')) return;
    if (!/^(https?:|mailto:)/i.test(a.href)) return;
    e.preventDefault();
    e.stopPropagation();
    dioxus.send('open:' + a.href);
  }, true);
})();
// Tapping your own pill gives a small buzz on phones, like Discord's.
window.addEventListener('click', e => {
  if (e.target.closest && e.target.closest('.user-pill .profile-chip')) {
    try { if (navigator.vibrate) navigator.vibrate(10); } catch (_) {}
  }
}, true);
// Page errors reach the Rust log; an exception inside the renderer's own
// edit loop would otherwise freeze the UI silently.
window.addEventListener('error', e => dioxus.send('jserr:' + e.message + ' @' + (e.filename || '') + ':' + e.lineno));
window.addEventListener('unhandledrejection', e => dioxus.send('jserr:unhandled rejection: ' + (e.reason && e.reason.message || e.reason)));
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
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data)
        .ok()?;
    Some(DroppedFile {
        name: value.get("name")?.as_str().unwrap_or("file").to_owned(),
        mime: value
            .get("mime")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_owned(),
        bytes,
    })
}
