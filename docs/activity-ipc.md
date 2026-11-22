# Activity IPC: setting my status from another program

Morpheus can show what you are doing as your status ("Playing Hades",
"Listening to Song - Artist"). Steam is built in. Any other program, such as a
browser extension that reads YouTube Music, can push an activity over a local
socket. Nothing listens on the network.

An activity says what the source is doing; Morpheus sets your status from the
one it picks, and puts your old status back when the activity ends. A status you
type yourself while an activity shows is never replaced. Games come before
videos, videos before music, music before a custom line; the newest wins among
equals. Each source group (Steam, Other apps) has a switch in Settings, My
Account.

## The socket

- Linux: `$XDG_RUNTIME_DIR/morpheus-activity.sock` (else `activity.sock` in
  Morpheus's data folder), mode 0600, so only your user connects.
- macOS: `activity.sock` in the data folder (`~/Library/Application Support/...`).
- Windows: the named pipe `\\.\pipe\morpheus-activity` (local clients only).
- `MORPHEUS_ACTIVITY_SOCK` overrides the path (testing).

## Protocol (version 1)

One JSON object per line (newline-delimited), answered by one JSON line each.
Lines are at most 16 KiB.

```
{"op":"hello","version":1,"client":"my-extension"}
-> {"ok":true,"op":"hello","version":1}

{"op":"set","source":"youtube-music","kind":"listening","name":"Song","details":"Artist","url":"https://music.youtube.com/watch?v=...","ttl_ms":30000}
-> {"ok":true}

{"op":"clear","source":"youtube-music"}
-> {"ok":true}
```

- `hello` is optional; it checks the version. Another version is an error.
- `set` fields: `source` (1 to 64 of letters, digits, `.`, `_`, `-`; your own
  name; one activity per source, a new `set` replaces it), `kind` (`playing`,
  `listening`, `watching`, `custom`; custom shows `name` as it is), `name`,
  optional `details` and `url`, and `ttl_ms`.
- `ttl_ms` is a heartbeat: the activity lapses that long after the last `set`
  (default 60000, from 500 to 86400000). Send `set` again before it runs out
  while the activity goes on; a source that crashes expires by itself.
- `clear` removes the source's activity.
- Errors answer `{"ok":false,"error":"..."}`.

Try it: `echo '{"op":"set","source":"test","kind":"listening","name":"Song"}' | nc -U $XDG_RUNTIME_DIR/morpheus-activity.sock`

## Chrome (native messaging)

Chrome extensions cannot open sockets, but they can start a native host. Morpheus
is its own host: started with `--native-messaging` (Chrome also passes the
extension's origin) it speaks Chrome's framing (a 4-byte little-endian length,
then the JSON) on stdin and stdout, and forwards each message as a line to the
socket above and the reply back. The messages are the same JSON as above.

Host manifest, saved as
`~/.config/google-chrome/NativeMessagingHosts/dev.morpheus.activity.json`
(Chromium: `~/.config/chromium/NativeMessagingHosts/`; macOS:
`~/Library/Application Support/Google/Chrome/NativeMessagingHosts/`; Windows: a
registry key `HKCU\Software\Google\Chrome\NativeMessagingHosts\dev.morpheus.activity`
whose default value is the manifest's path):

```json
{
  "name": "dev.morpheus.activity",
  "description": "Morpheus activity status",
  "path": "/usr/bin/morpheus-desktop-native",
  "type": "stdio",
  "allowed_origins": ["chrome-extension://<your extension id>/"]
}
```

`path` must be an executable that runs `morpheus-desktop --native-messaging`
(a one-line shell script does). In the extension, `chrome.runtime.connectNative(
"dev.morpheus.activity")`, then `port.postMessage({op:"set", ...})` on each track
change and every `ttl_ms / 2` while it plays, and `{op:"clear", ...}` when it
stops. If Morpheus is not running the host answers `{"ok":false,"error":"Morpheus
is not running"}`.
