//! Custom emoji, as MSC2545 image packs (the `im.ponies.*` events Element,
//! Cinny and FluffyChat read and write): a pack is a room's
//! `im.ponies.room_emotes` state event (one per state key) or your own
//! `im.ponies.user_emotes` account data; `im.ponies.emote_rooms` lists the
//! rooms whose packs you turned on everywhere. A pack's `images` map a
//! shortcode to an `mxc://` picture. Sent, an emoji is an
//! `<img data-mx-emoticon src="mxc://…" alt=":code:">` in the formatted body
//! with `:code:` in the plain one, and a reaction is keyed by the URI.

use std::collections::HashSet;

use matrix_sdk::ruma::events::{
    AnyGlobalAccountDataEventContent, GlobalAccountDataEventType, StateEventType,
};
use matrix_sdk::ruma::serde::Raw;
use serde_json::{Value, json};

use crate::{CoreFuture, Error, MediaRef, OwnedRoomId, Session};

fn mxc_ref(uri: String) -> MediaRef {
    MediaRef::from_mxc_uri(&uri)
}

pub const USER_PACK: &str = "im.ponies.user_emotes";
pub const ROOM_PACK: &str = "im.ponies.room_emotes";
pub const EMOTE_ROOMS: &str = "im.ponies.emote_rooms";

/// One custom emoji: `:shortcode:` and its picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Emote {
    pub shortcode: String,
    /// `mxc://…`
    pub url: String,
}

/// A pack of custom emoji and where it lives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmotePack {
    /// Your own pack when `None`, else the room that holds it.
    pub room_id: Option<OwnedRoomId>,
    /// The state key of the pack in its room (empty for the usual one).
    pub state_key: String,
    pub name: String,
    pub avatar: Option<MediaRef>,
    pub emotes: Vec<Emote>,
    /// The signed-in user may change it (the room's power levels allow the
    /// state event; always for your own pack).
    pub editable: bool,
}

/// A pack's name, picture and emoji from its event content. Emoji whose
/// usage leaves out `emoticon` (stickers) are not listed; a pack or an image
/// without a usage is for everything.
pub fn parse_pack(content: &Value) -> (Option<String>, Option<String>, Vec<Emote>) {
    let pack = content.get("pack");
    let name = pack
        .and_then(|p| p.get("display_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .map(str::to_owned);
    let avatar = pack
        .and_then(|p| p.get("avatar_url"))
        .and_then(Value::as_str)
        .filter(|u| u.starts_with("mxc://"))
        .map(str::to_owned);
    let pack_usage = pack.and_then(|p| p.get("usage"));
    let emoticon = |usage: &Value| {
        usage
            .as_array()
            .is_none_or(|list| list.iter().any(|u| u == "emoticon"))
    };
    let mut emotes: Vec<Emote> = content
        .get("images")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(shortcode, image)| {
            let url = image.get("url")?.as_str()?;
            if !url.starts_with("mxc://") || shortcode.trim().is_empty() {
                return None;
            }
            let usage = image.get("usage").or(pack_usage);
            if usage.is_some_and(|u| !emoticon(u)) {
                return None;
            }
            Some(Emote {
                shortcode: shortcode.trim_matches(':').to_owned(),
                url: url.to_owned(),
            })
        })
        .collect();
    emotes.sort_by(|a, b| a.shortcode.to_lowercase().cmp(&b.shortcode.to_lowercase()));
    (name, avatar, emotes)
}

/// `content` (a pack's event content, or nothing) with `shortcode` set to
/// `url`, other fields (other emoji, the pack's name, sticker usage) kept.
pub fn with_emote(content: Option<Value>, shortcode: &str, url: &str) -> Value {
    let mut content = content
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let images = content
        .as_object_mut()
        .map(|o| o.entry("images").or_insert_with(|| json!({})));
    if let Some(Value::Object(images)) = images {
        let mut image = images
            .get(shortcode)
            .filter(|i| i.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        image["url"] = json!(url);
        image
            .as_object_mut()
            .map(|o| o.entry("usage").or_insert_with(|| json!(["emoticon"])));
        images.insert(shortcode.to_owned(), image);
    }
    content
}

/// `content` without the emoji `shortcode`.
pub fn without_emote(content: Option<Value>, shortcode: &str) -> Value {
    let mut content = content
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if let Some(Value::Object(images)) = content.get_mut("images") {
        images.remove(shortcode);
    }
    content
}

/// `content` with the emoji `old` named `new`, its picture and other fields kept.
/// `None` when there is no `old`, or `new` is already another emoji.
pub fn with_renamed_emote(content: Option<Value>, old: &str, new: &str) -> Option<Value> {
    let mut content = content.filter(Value::is_object)?;
    let images = content.get_mut("images")?.as_object_mut()?;
    if old == new {
        return images.contains_key(old).then_some(content);
    }
    if images.contains_key(new) {
        return None;
    }
    let image = images.remove(old)?;
    images.insert(new.to_owned(), image);
    Some(content)
}

/// A shortcode to start from for a picture file (`party parrot.png`: `party_parrot`).
pub fn shortcode_from_file_name(file_name: &str) -> Option<String> {
    let stem = std::path::Path::new(file_name)
        .file_stem()
        .and_then(|s| s.to_str())?;
    clean_shortcode(stem)
}

/// A shortcode as typed in a field: no colons or spaces, only letters,
/// digits, `_`, `-`; `None` when nothing is left.
pub fn clean_shortcode(typed: &str) -> Option<String> {
    let code: String = typed
        .trim()
        .trim_matches(':')
        .chars()
        .map(|c| if c.is_whitespace() { '_' } else { c })
        .filter(|c| c.is_alphanumeric() || matches!(c, '_' | '-'))
        .collect();
    (!code.is_empty()).then_some(code)
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// The picture tag for an emoji.
pub fn img_tag(shortcode: &str, url: &str) -> String {
    format!(
        "<img data-mx-emoticon src=\"{url}\" alt=\":{shortcode}:\" title=\":{shortcode}:\" height=\"32\" />"
    )
}

/// `html` with each `:code:` of `emotes` (code, URI) in its text (not in
/// code, not in a tag) an image tag.
pub fn put_emotes_in_html(html: &str, emotes: &[(String, String)]) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let mut code_depth = 0usize;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix('<') {
            let end = after.find('>').map_or(rest.len(), |i| i + 2);
            let tag = &rest[..end];
            let lower = tag.to_lowercase();
            let name = lower
                .trim_start_matches('<')
                .trim_start_matches('/')
                .split(|c: char| !c.is_ascii_alphanumeric())
                .next()
                .unwrap_or("");
            if matches!(name, "code" | "pre") {
                if lower.starts_with("</") {
                    code_depth = code_depth.saturating_sub(1);
                } else if !lower.ends_with("/>") {
                    code_depth += 1;
                }
            }
            out.push_str(tag);
            rest = &rest[end..];
            continue;
        }
        let end = rest.find('<').unwrap_or(rest.len());
        let text = &rest[..end];
        if code_depth == 0 {
            out.push_str(&replace_codes(text, emotes));
        } else {
            out.push_str(text);
        }
        rest = &rest[end..];
    }
    out
}

fn replace_codes(text: &str, emotes: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut at = 0;
    while at < text.len() {
        let Some(colon) = text[at..].find(':').map(|i| i + at) else {
            break;
        };
        let found = emotes.iter().find(|(code, _)| {
            text[colon + 1..].starts_with(code.as_str())
                && text[colon + 1 + code.len()..].starts_with(':')
        });
        match found {
            Some((code, url)) => {
                out.push_str(&text[at..colon]);
                out.push_str(&img_tag(code, url));
                at = colon + code.len() + 2;
            }
            None => {
                out.push_str(&text[at..=colon]);
                at = colon + 1;
            }
        }
    }
    out.push_str(&text[at..]);
    out
}

/// The emoji of `emotes` that the plain text `body` names as `:code:`.
pub fn used_in(body: &str, emotes: &[(String, String)]) -> Vec<(String, String)> {
    emotes
        .iter()
        .filter(|(code, _)| body.contains(&format!(":{code}:")))
        .cloned()
        .collect()
}

/// A message's content for markdown `body`, with `:code:` of `emotes` as
/// pictures in its formatted body.
pub fn message_content(
    body: &str,
    emotes: &[(String, String)],
) -> matrix_sdk::ruma::events::room::message::RoomMessageEventContentWithoutRelation {
    use matrix_sdk::ruma::events::room::message::{
        FormattedBody, MessageType, RoomMessageEventContentWithoutRelation,
    };
    let mut content = RoomMessageEventContentWithoutRelation::text_markdown(body);
    let used = used_in(body, emotes);
    if used.is_empty() {
        return content;
    }
    if let MessageType::Text(text) = &mut content.msgtype {
        let html = text
            .formatted
            .as_ref()
            .map(|f| f.body.clone())
            .unwrap_or_else(|| escape(body).replace('\n', "<br>"));
        let replaced = put_emotes_in_html(&html, &used);
        if replaced != html {
            text.formatted = Some(FormattedBody::html(replaced));
        }
    }
    content
}

fn json_of(raw: &matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState) -> Option<Value> {
    use matrix_sdk::deserialized_responses::RawAnySyncOrStrippedState as Raws;
    let text = match raw {
        Raws::Sync(raw) => raw.json().get(),
        Raws::Stripped(raw) => raw.json().get(),
    };
    serde_json::from_str(text).ok()
}

/// The packs `room` holds (every state key), from the local copy.
async fn room_packs(room: &matrix_sdk::Room) -> Vec<EmotePack> {
    let events = room
        .get_state_events(StateEventType::from(ROOM_PACK))
        .await
        .unwrap_or_default();
    let editable = room_can_edit(room).await;
    let mut packs = Vec::new();
    let mut found: Vec<(String, Value)> = Vec::new();
    for event in events {
        if let Some(event) = json_of(&event)
            && let Some(content) = event.get("content")
        {
            let key = event
                .get("state_key")
                .and_then(Value::as_str)
                .unwrap_or_default();
            found.push((key.to_owned(), content.clone()));
        }
    }
    if found.is_empty()
        && let Some(content) = fetch_pack(room, "").await
    {
        found.push((String::new(), content));
    }
    for (state_key, content) in found {
        let content = &content;
        let (name, avatar, emotes) = parse_pack(content);
        if emotes.is_empty() && name.is_none() {
            continue;
        }
        let name = match name {
            Some(name) => name,
            None => room
                .cached_display_name()
                .map(|n| n.to_string())
                .unwrap_or_else(|| room.room_id().to_string()),
        };
        packs.push(EmotePack {
            room_id: Some(room.room_id().to_owned()),
            state_key,
            name,
            avatar: avatar.map(mxc_ref),
            emotes,
            editable,
        });
    }
    packs
}

/// Whether the signed-in user may change a pack's state event in `room`.
async fn room_can_edit(room: &matrix_sdk::Room) -> bool {
    match room.get_member(room.own_user_id()).await {
        Ok(Some(me)) => me.can_send_state(StateEventType::from(ROOM_PACK)),
        _ => false,
    }
}

impl Session {
    /// The packs usable in `current` (when in a room): your own, those of
    /// the rooms you turned on (`im.ponies.emote_rooms`), the room's own and
    /// those of every space you are in. Packs without emoji are left out
    /// unless the signed-in user can add to them.
    pub fn emote_packs(&self, current: Option<OwnedRoomId>) -> CoreFuture<Vec<EmotePack>> {
        let client = self.client.clone();
        self.core.run(async move {
            let mut packs = Vec::new();
            // Your own.
            let own = crate::people::read_account_data(&client, USER_PACK)
                .await
                .ok()
                .flatten()
                .and_then(|raw| raw.deserialize_as::<Value>().ok());
            if let Some(content) = own {
                let (name, avatar, emotes) = parse_pack(&content);
                if !emotes.is_empty() {
                    packs.push(EmotePack {
                        room_id: None,
                        state_key: String::new(),
                        name: name.unwrap_or_else(|| "My emoji".to_owned()),
                        avatar: avatar.map(mxc_ref),
                        emotes,
                        editable: true,
                    });
                }
            }
            // Rooms to read.
            let mut rooms: Vec<OwnedRoomId> = Vec::new();
            if let Some(current) = current {
                rooms.push(current);
            }
            let enabled = crate::people::read_account_data(&client, EMOTE_ROOMS)
                .await
                .ok()
                .flatten()
                .and_then(|raw| raw.deserialize_as::<Value>().ok());
            if let Some(Value::Object(map)) = enabled.as_ref().and_then(|v| v.get("rooms")) {
                for id in map.keys() {
                    if let Ok(id) = OwnedRoomId::try_from(id.as_str()) {
                        rooms.push(id);
                    }
                }
            }
            for room in client.joined_rooms() {
                if room.is_space() {
                    rooms.push(room.room_id().to_owned());
                }
            }
            let mut seen = HashSet::new();
            for id in rooms {
                if !seen.insert(id.clone()) {
                    continue;
                }
                let Some(room) = client.get_room(&id) else {
                    continue;
                };
                for pack in room_packs(&room).await {
                    if !pack.emotes.is_empty() || pack.editable {
                        packs.push(pack);
                    }
                }
            }
            Ok(packs)
        })
    }

    /// Add (or replace) the emoji `shortcode` of a pack with the picture in
    /// `bytes` (`mime`): uploaded, then written to the pack, which is made if
    /// the room has none. `room_id` `None` is your own pack.
    pub fn add_emote(
        &self,
        room_id: Option<OwnedRoomId>,
        state_key: String,
        shortcode: String,
        mime: String,
        bytes: Vec<u8>,
    ) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let shortcode = clean_shortcode(&shortcode)
                .ok_or_else(|| Error::Other("an emoji needs a name".to_owned()))?;
            let mime: mime_guess::Mime = mime
                .parse()
                .map_err(|_| Error::Other("not a picture".to_owned()))?;
            let url = client.media().upload(&mime, bytes, None).await?.content_uri;
            change_pack(&client, room_id, &state_key, |content| {
                with_emote(content, &shortcode, url.as_str())
            })
            .await
        })
    }

    /// The pack the room's own `im.ponies.room_emotes` event (empty state key)
    /// holds, made-up empty when the room has none yet, with whether the
    /// signed-in user may change it: what a manager shows.
    pub fn room_emote_pack(&self, room_id: OwnedRoomId) -> CoreFuture<EmotePack> {
        let client = self.client.clone();
        self.core.run(async move {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| Error::UnknownRoom(room_id.clone()))?;
            let found = room_packs(&room)
                .await
                .into_iter()
                .find(|pack| pack.state_key.is_empty());
            if let Some(pack) = found {
                return Ok(pack);
            }
            Ok(EmotePack {
                room_id: Some(room_id),
                state_key: String::new(),
                name: room
                    .cached_display_name()
                    .map(|n| n.to_string())
                    .unwrap_or_default(),
                avatar: None,
                emotes: Vec::new(),
                editable: room_can_edit(&room).await,
            })
        })
    }

    /// Rename the emoji `old` of a pack to `new` (its picture stays).
    pub fn rename_emote(
        &self,
        room_id: Option<OwnedRoomId>,
        state_key: String,
        old: String,
        new: String,
    ) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            let new = clean_shortcode(&new)
                .ok_or_else(|| Error::Other("an emoji needs a name".to_owned()))?;
            let mut renamed = true;
            change_pack(&client, room_id, &state_key, |content| {
                match with_renamed_emote(content.clone(), &old, &new) {
                    Some(changed) => changed,
                    None => {
                        renamed = false;
                        content.unwrap_or_else(|| json!({}))
                    }
                }
            })
            .await?;
            if renamed {
                Ok(())
            } else {
                Err(Error::Other(format!("there is already an emoji named :{new}:")))
            }
        })
    }

    /// Remove the emoji `shortcode` from a pack.
    pub fn remove_emote(
        &self,
        room_id: Option<OwnedRoomId>,
        state_key: String,
        shortcode: String,
    ) -> CoreFuture<()> {
        let client = self.client.clone();
        self.core.run(async move {
            change_pack(&client, room_id, &state_key, |content| {
                without_emote(content, &shortcode)
            })
            .await
        })
    }
}

/// Read a pack's content, change it, and write it back.
async fn change_pack(
    client: &matrix_sdk::Client,
    room_id: Option<OwnedRoomId>,
    state_key: &str,
    change: impl FnOnce(Option<Value>) -> Value,
) -> crate::Result<()> {
    match room_id {
        None => {
            let current = crate::people::read_account_data(client, USER_PACK)
                .await?
                .and_then(|raw| raw.deserialize_as::<Value>().ok());
            let content = change(current);
            let raw: Raw<AnyGlobalAccountDataEventContent> =
                Raw::from_json(serde_json::value::to_raw_value(&content)?);
            client
                .account()
                .set_account_data_raw(GlobalAccountDataEventType::from(USER_PACK), raw)
                .await?;
        }
        Some(room_id) => {
            let room = client
                .get_room(&room_id)
                .ok_or_else(|| Error::UnknownRoom(room_id.clone()))?;
            let current = fetch_pack(&room, state_key).await;
            let content = change(current);
            room.send_state_event_raw(ROOM_PACK, state_key, content)
                .await?;
        }
    }
    Ok(())
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renaming_keeps_the_picture_and_refuses_a_taken_name() {
        let pack = json!({ "pack": { "display_name": "P" }, "images": { "a": { "url": "mxc://x/a", "usage": ["emoticon"] }, "b": { "url": "mxc://x/b" } } });
        let renamed = with_renamed_emote(Some(pack.clone()), "a", "c").expect("renamed");
        assert_eq!(renamed["images"]["c"]["url"], "mxc://x/a");
        assert!(renamed["images"].get("a").is_none());
        assert_eq!(renamed["pack"]["display_name"], "P");
        assert!(with_renamed_emote(Some(pack.clone()), "a", "b").is_none(), "taken");
        assert!(with_renamed_emote(Some(pack), "zzz", "c").is_none(), "missing");
    }

    #[test]
    fn a_file_name_suggests_a_shortcode() {
        assert_eq!(shortcode_from_file_name("party parrot.png").as_deref(), Some("party_parrot"));
        assert_eq!(shortcode_from_file_name(":wave:.gif").as_deref(), Some("wave"));
        assert_eq!(shortcode_from_file_name("..png"), None);
    }

    #[test]
    fn a_pack_lists_its_emoticons() {
        let content = json!({
            "pack": { "display_name": " Parrots ", "avatar_url": "mxc://x/av" },
            "images": {
                "party": { "url": "mxc://x/a" },
                "Blob": { "url": "mxc://x/b", "usage": ["emoticon", "sticker"] },
                "sticker_only": { "url": "mxc://x/c", "usage": ["sticker"] },
                "web": { "url": "https://x/d" },
            },
        });
        let (name, avatar, emotes) = parse_pack(&content);
        assert_eq!(name.as_deref(), Some("Parrots"));
        assert_eq!(avatar.as_deref(), Some("mxc://x/av"));
        let codes: Vec<&str> = emotes.iter().map(|e| e.shortcode.as_str()).collect();
        assert_eq!(codes, ["Blob", "party"], "sorted; stickers and non-mxc dropped");
    }

    #[test]
    fn a_packs_usage_applies_to_images_without_their_own() {
        let content = json!({
            "pack": { "usage": ["sticker"] },
            "images": { "a": { "url": "mxc://x/a" }, "b": { "url": "mxc://x/b", "usage": ["emoticon"] } },
        });
        let (_, _, emotes) = parse_pack(&content);
        assert_eq!(emotes.len(), 1);
        assert_eq!(emotes[0].shortcode, "b");
    }

    #[test]
    fn editing_keeps_what_else_the_pack_holds() {
        let start = json!({
            "pack": { "display_name": "Mine" },
            "images": { "keep": { "url": "mxc://x/k", "usage": ["sticker"], "info": { "w": 1 } } },
        });
        let added = with_emote(Some(start.clone()), "new", "mxc://x/n");
        assert_eq!(added["pack"]["display_name"], "Mine");
        assert_eq!(added["images"]["keep"], start["images"]["keep"]);
        assert_eq!(added["images"]["new"]["url"], "mxc://x/n");
        assert_eq!(added["images"]["new"]["usage"], json!(["emoticon"]));
        let replaced = with_emote(Some(added), "keep", "mxc://x/other");
        assert_eq!(replaced["images"]["keep"]["url"], "mxc://x/other");
        assert_eq!(replaced["images"]["keep"]["usage"], json!(["sticker"]));
        assert_eq!(replaced["images"]["keep"]["info"], json!({ "w": 1 }));
        let removed = without_emote(Some(replaced), "new");
        assert!(removed["images"].get("new").is_none());
        assert_eq!(with_emote(None, "a", "mxc://x/a")["images"]["a"]["url"], "mxc://x/a");
    }

    #[test]
    fn shortcodes_are_cleaned() {
        assert_eq!(clean_shortcode(" :Party Time: ").as_deref(), Some("Party_Time"));
        assert_eq!(clean_shortcode("a/b?c").as_deref(), Some("abc"));
        assert_eq!(clean_shortcode(" :: "), None);
    }

    #[test]
    fn shortcodes_in_html_become_images_outside_code() {
        let emotes = vec![("party".to_owned(), "mxc://x/a".to_owned())];
        let html = put_emotes_in_html(
            "hi :party: and <code>:party:</code> <a href=\"x:party:\">:party:</a> :nope: 12:30:party:",
            &emotes,
        );
        assert_eq!(html.matches("<img").count(), 3, "{html}");
        assert!(html.contains("<code>:party:</code>"), "{html}");
        assert!(html.contains("data-mx-emoticon src=\"mxc://x/a\" alt=\":party:\""), "{html}");
        assert!(html.contains(":nope:"));
        assert!(html.contains("href=\"x:party:\""), "attributes are left alone: {html}");
    }

    #[test]
    fn a_message_goes_out_with_pictures_and_a_plain_body() {
        let emotes = vec![("party".to_owned(), "mxc://x/a".to_owned())];
        let content = message_content("go :party: *now*\nsecond & line", &emotes);
        let matrix_sdk::ruma::events::room::message::MessageType::Text(text) = content.msgtype
        else {
            panic!("text");
        };
        assert_eq!(text.body, "go :party: *now*\nsecond & line");
        let html = text.formatted.expect("formatted").body;
        assert!(html.contains("<em>now</em>"), "{html}");
        assert!(html.contains("<img data-mx-emoticon"), "{html}");
        // No emoji used: plain markdown handling, untouched.
        let plain = message_content("hello", &emotes);
        let matrix_sdk::ruma::events::room::message::MessageType::Text(text) = plain.msgtype
        else {
            panic!("text");
        };
        assert!(text.formatted.is_none());
        // Without markdown formatting the html is made from the escaped text.
        let escaped = message_content("a < b :party:", &emotes);
        let matrix_sdk::ruma::events::room::message::MessageType::Text(text) = escaped.msgtype
        else {
            panic!("text");
        };
        let html = text.formatted.expect("formatted").body;
        assert!(html.contains("&lt;") && html.contains("<img"), "{html}");
    }
}

/// A pack's content straight from the server (the room list's sync does not
/// ask for these events, so the local copy may not have them); `None` when
/// the room has no such pack.
async fn fetch_pack(room: &matrix_sdk::Room, state_key: &str) -> Option<Value> {
    use matrix_sdk::ruma::api::client::state::get_state_event_for_key;
    let request = get_state_event_for_key::v3::Request::new(
        room.room_id().to_owned(),
        StateEventType::from(ROOM_PACK),
        state_key.to_owned(),
    );
    let response = room.client().send(request).await.ok()?;
    serde_json::from_str(response.event_or_content.get()).ok()
}
