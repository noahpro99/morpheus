# Morpheus 0.2.0

## A new desktop app

The desktop app (Linux, Windows, macOS) is rebuilt on GPUI, the GPU-drawn UI
toolkit behind the Zed editor, instead of a web view. It is a native program
of its own: it draws on the GPU, scrolls smoothly, and needs no
WebKit/WebView2 on your system. Your account, keys, settings and
last-open rooms carry over: it uses the same data folder as 0.1.x, and the
built-in updater takes you from 0.1.x to this one in place.

Phones (Android, iPhone) and the web app are unchanged apart from the version.

### What you get on desktop

- Everything from 0.1.x: rooms, spaces and folders, direct messages, threads,
  replies, reactions, editing and deleting, pins, search, invites, friends,
  encrypted rooms with key backup and verification, moderation tools,
  Matrix links, themes (Dark, Midnight, Light).
- Voice and video calls: voice channels, DM calls, camera and screen sharing
  (quality choices, sounds for joining, camera and screen), a call that keeps
  going when you close the window, ringing for incoming calls.
- Smooth wheel and touchpad scrolling, and Page Up / Page Down that glide.
- Search as in Discord: a box in the top bar that widens in place, Escape
  steps out; Ctrl+K switcher that also matches the nicknames you gave people.
- Picture viewer with zoom around the pointer (wheel or touchpad, up to 8x).
- Colour emoji drawn as one glyph (faces, flags, keycaps, families).
- Camera in the shape your webcam makes; your own self-view is mirrored.
- Tray icon on Linux, Windows and macOS (open, check for updates,
  acknowledgements, quit); the app and any call go on in the tray when the
  window is closed; start hidden at login.
- Clicking a person listed under a voice channel opens their profile.
- Bigger click targets, and the top bar brightens under the pointer instead of
  drawing a box.
- On a Mac: an app menu (About, Hide, Quit), an Edit menu (Cut, Copy, Paste,
  Select All), a Window menu (Minimize, Close), Cmd+W closes the window,
  Cmd+Q quits, a click on the Dock icon brings the window back, and
  `matrix:` links open in Morpheus.
- Windows and Linux releases are zips of one folder; macOS is a zip of
  Morpheus.app (ad-hoc signed, not notarised: the first time, right-click
  and Open). Linux is built on Ubuntu 22.04 and runs on most distributions
  from that one on.

## Known gaps

- Text boxes have no undo/redo yet (so the Mac Edit menu has no Undo/Redo),
  and no input-method (IME) composition for Chinese, Japanese, Korean.
- No video player yet: a video shows its thumbnail in the timeline.
- The Mac and Windows builds of the new app were checked by the release
  workflows (build, start, update); they have had far less daily use than the
  Linux one. Please report anything odd (Settings, Report a bug).
- Windows: the zip built on a machine other than GitHub's compiles the
  program's shaders when it starts (a moment longer to open the first
  window); GitHub's build has them compiled ahead.
- Tray: no unread count on the Linux tray icon.
