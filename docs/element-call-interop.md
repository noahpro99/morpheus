# Element Call interop (#82)

What Element Call sends and expects, read from the source at
element-call **v0.28.2** (its `sdk/`), the matrix-js-sdk commit it pins
(`3f05dc4d`, `src/matrixrtc/`), livekit-client **2.18** (`src/e2ee/`) and
lk-jwt-service **v0.10.0** (`src/helper.rs`, `src/handler.rs`), and how
Morpheus lines up. Fixtures of the real formats are in
`crates/core/fixtures/element-call/`; the unit tests in
`crates/core/src/calls.rs` (`element_call_*`) check Morpheus against them.

## Membership (`org.matrix.msc3401.call.member` state event)

Element Call's default mode ("compatibility") writes one state event per
device; the "Matrix 2.0" developer mode writes sticky `m.rtc.member` timeline
events instead (MSC4354), which Morpheus does not read.

- State key `_@user:server_DEVICE_m.call` (no leading `_` in room versions
  `org.matrix.msc3757*`/`msc3779*`). Same as Morpheus.
- Content (`MembershipManager.makeMyMembership`):

  ```json
  {
    "application": "m.call", "call_id": "", "scope": "m.room",
    "device_id": "DEVICE",
    "membershipID": "@user:server:DEVICE",
    "expires": 14400000, "created_ts": 1763200000000,
    "m.call.intent": "audio",
    "focus_active": { "type": "livekit", "focus_selection": "multi_sfu" },
    "foci_preferred": [
      { "livekit_alias": "!room:server", "type": "livekit",
        "livekit_service_url": "https://jwt.example" }
    ]
  }
  ```
- Left: `{}` (via a delayed event, 18 s timeout restarted every 4 s).
- Parsing (`checkSessionsMembershipData`) needs only `device_id`, `call_id`,
  `application` strings and `focus_active.type`. `call_id: ""` means slot
  `m.call#ROOM`. Expired = `created_ts + expires` (default 4 h) passed.
- Morpheus sends `focus_selection: "oldest_membership"` (ruma's only value)
  and no `membershipID`; Element ignores the first and defaults the second to
  `@user:server:DEVICE`. Ruma parses Element's `multi_sfu` fine.

## Focus selection: `multi_sfu`

Element Call no longer joins the oldest membership's focus. Each member
**publishes on its own transport** (`foci_preferred[0]`, from its own
homeserver's `/rtc/transports` or `.well-known` `org.matrix.msc4143.rtc_foci`)
and every client **subscribes to each distinct transport** in the call
(transport identity = the transport JSON with sorted keys).

Morpheus publishes on its homeserver's first service (falling back to the
services in others' memberships), and connects to that one LiveKit room
only. On one homeserver (one SFU) that is the same room as Element's. With
people on different homeservers/SFUs, Morpheus does not see those who
publish elsewhere (Element does see Morpheus, by subscribing to its SFU).
Not fixed: it needs one LiveKit connection per transport.

## JWT service and LiveKit names

- Element's **publisher** connection asks `POST {service}/sfu/get` with
  `{ room, openid_token, device_id }`; a subscriber-only connection to
  someone else's transport asks `/get_token` with
  `{ room_id, slot_id: "m.call#ROOM", openid_token, member: { id, claimed_user_id, claimed_device_id } }`.
- Room alias (both endpoints): `unpadded_b64(sha256(json(["!room", "m.call#room"])))`.
- Identity: `/sfu/get` → `@user:server:DEVICE`; `/get_token` →
  `unpadded_b64(sha256(json([user, device, member.id])))`.
- Element maps a member to LiveKit participants by
  `CallMembership.backendIdentities`: for a `livekit_service_url` transport
  that is `@user:server:DEVICE` only (a transport with a websocket `url`
  adds `unpadded_b64(sha256(json([user, memberId])))`).

**Mismatch (fixed):** Morpheus asked `/get_token` first, so it was in the
LiveKit room as the hashed name, which Element never looks for: Element
users did not see or hear Morpheus, and filed its key under a name nobody
had. Morpheus now asks `/sfu/get` first (as Element's publisher does) and
`/get_token` only where that is missing.

## Tracks

Matched by `Track.Source`: `microphone`, `camera`, `screen_share`,
`screen_share_audio`. Morpheus publishes the same sources. Names are free.

## Media encryption (per-participant keys)

- Keys: 16 random bytes, index 0 first; a new key with `(index + 1) % 256`
  when someone leaves, or when someone joins and the key is over 10 s old;
  sent to everyone, used for sending 1 s later (`useKeyDelay`).
- Transport: **to-device only**, Olm-encrypted, type
  `io.element.call.encryption_keys`, to every device with a membership
  except its own. The room-event transport is gone from matrix-js-sdk;
  keys sent in the clear are dropped. Content (`ToDeviceKeyTransport`):

  ```json
  {
    "room_id": "!room:server",
    "member_id": "@user:server:DEVICE",
    "media_key": { "index": 0, "key": "<base64>" },
    "keys": { "index": 0, "key": "<base64>" },
    "member": { "claimed_device_id": "DEVICE", "id": "@user:server:DEVICE" }
  }
  ```
  Receiving needs `room_id` (equal to the call's), `media_key` or `keys`
  with a non-empty string `key` and a number `index`, and
  `member.claimed_device_id`. A key from a device without a membership is
  held until its membership arrives. A key is applied to each of the
  member's `backendIdentities`.
- LiveKit key provider (`MatrixKeyProvider`): `ratchetWindowSize: 10`,
  `keyringSize: 256`, salt `LKFrameEncryptionKey`, and the raw key imported
  as **HKDF** material (SHA-256, 128 zero bytes of info, AES-GCM-128).

**Mismatches (fixed):**
1. Morpheus sent and read keys only as room events: neither side ever got
   the other's key. It now sends Element's to-device event to every device
   in the call (and still the room event, for Morpheus 0.2.x), and reads
   both.
2. Morpheus's key provider derived with PBKDF2 (LiveKit Rust's default);
   now HKDF, keyring 256, ratchet window 10, as Element. libwebrtc's HKDF
   (`DeriveHkdfSha256FromSecret`) is the same derivation as the browser's.
   Encrypted calls with Morpheus 0.2.3 and older no longer decrypt.
3. Received keys were all set at index 0, so Element's rotated keys
   (index 1, 2, …) replaced the first and its frames named a key Morpheus
   did not have. Keys are now set at their index.

Morpheus never rotates its own key (index 0 for the whole call).

## Voice channel rooms

Morpheus creates voice channels with room type `org.matrix.msc3417.call`
(Element's call rooms, shown by Element Web with the Element Call video
rooms labs flag) and treats `io.element.video` rooms as voice channels too.

## Video codecs (#83)

Element Call publishes VP8 by default. Morpheus publishes H.264 when a
hardware encoder is found (Linux VAAPI/NVENC), else VP8, with simulcast for
cameras and one layer for screens. LiveKit forwards, never transcodes, so a
browser that cannot decode H.264 shows no picture; Chrome, Edge and Safari
decode it, Firefox needs its OpenH264 plugin (normally installed by itself),
and Element X (LiveKit native) decodes it. With encryption, both
libwebrtc and livekit-client leave H.264's NAL headers in the clear the same
way, but this pairing is not covered by any test here.

## Testing by hand with Element

On one homeserver with a LiveKit service (the dev one in `dev/homeserver`
works with Element Web pointed at it, or use matrix.org with app.element.io):

1. In an unencrypted room, start the call in Morpheus, join from Element
   (Web/Desktop with Element Call, or Element X). Each sees the other's
   tile, hears the other, sees speaking; a camera and a screen share show
   on the other side.
2. The same with Element starting and Morpheus joining.
3. Both again in an encrypted room: audio and video decrypt both ways (no
   "waiting for media" / encryption error on Element's tile).
4. With three people (Element, Morpheus, someone else), have the third
   leave: Element rotates its key; Morpheus must keep hearing Element.
5. Morpheus with H.264 (a machine with VAAPI/NVENC) to Element Web in
   Chrome and in Firefox: the camera shows.
