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
- Videos play in the message, as in Discord: play/pause, a seek bar, time,
  mute and volume, download and a full-window player (Space, arrow keys to
  skip 5 s, Escape). Short silent clips play by themselves on a loop, like
  GIFs; one video plays with sound at a time and one scrolled away pauses.
  Encrypted videos are decrypted in memory. Formats: MP4 and QuickTime
  (.mov) with H.264 video and AAC sound (or no sound), which is what phones
  and most Matrix apps send.
- Colour emoji drawn as one glyph (faces, flags, keycaps, families).
- Camera in the shape your webcam makes; your own self-view is mirrored.
- Tray icon on Linux, Windows and macOS (open, check for updates,
  acknowledgements, quit); the app and any call go on in the tray when the
  window is closed; start hidden at login.
- Clicking a person listed under a voice channel opens their profile.
- Noise suppression for your microphone: Off, Standard (WebRTC's) or Strong
  (DeepFilterNet 3, the default), from a button beside the camera in a call
  or in Settings > Voice & Video, with a mic test that plays you back to
  yourself and shows your level. Echo cancellation stays on.
- Calls: create voice channels; a server with a call going shows a green
  speaker badge; alone in a call, invite friends from a card; a focused
  person fills the stage; a voice channel's chat is a panel beside the call;
  turning on the camera or sharing your screen takes you to the call.
- Every invite screen has a searchable list of your friends.
- Rooms you just left reopen instantly where you were.
- Mentions show as pills, and messages that mention you are highlighted.
- A sound for new messages that would notify you (Settings > Notifications).
- Holding Shift while deleting (bin or menu) skips the question.
- The top bar brightens under the pointer instead of drawing a box.
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
- Videos in WebM (VP8/VP9 with Opus, as some clients record), HEVC (H.265,
  what iPhones record unless set to "Most Compatible"), fragmented MP4 and
  AAC with SBR (HE-AAC) do not play yet: the card says so and offers the
  download.
- The Mac and Windows builds of the new app were checked by the release
  workflows (build, start, update); they have had far less daily use than the
  Linux one. Please report anything odd (Settings, Report a bug).
- Windows: the zip built on a machine other than GitHub's compiles the
  program's shaders when it starts (a moment longer to open the first
  window); GitHub's build has them compiled ahead.
- Tray: no unread count on the Linux tray icon.
- The microphone is now captured by the app itself (for noise suppression);
  on Windows and macOS a chosen microphone or speaker the app can't match
  falls back to the system default.
