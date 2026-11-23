//! The rooms this account was last in, newest first (`recent_rooms.json`),
//! which lead the command palette when nothing is typed (#375). A room counts
//! once it was opened by the person (navigating, a notification, a jump, a
//! link), never because it received messages or was kept warm behind the
//! scenes.

use std::path::Path;

/// The file in the data directory: `{user: [room, ...]}`, newest first.
pub const RECENT_ROOMS: &str = "recent_rooms.json";

/// How many rooms lead the palette.
pub const LEAD: usize = 5;

/// How many are kept: more than shown, so leaving or skipping the current room
/// (and rooms since left) still leaves five.
pub const KEEP: usize = 30;

/// `room` was opened: it becomes the newest.
pub fn record(recent: &mut Vec<String>, room: &str) {
    if recent.first().map(String::as_str) == Some(room) {
        return;
    }
    recent.retain(|r| r != room);
    recent.insert(0, room.to_owned());
    recent.truncate(KEEP);
}

/// The order of the palette's rooms when nothing is typed, as indexes into
/// `listed` (the rooms offered, in their usual order): up to [`LEAD`] recent
/// ones first (newest first, skipping `current` and rooms not listed), then
/// the usual list without them, at most `rest` of it.
pub fn palette_order(listed: &[&str], recent: &[String], current: Option<&str>, rest: usize) -> Vec<usize> {
    let mut order: Vec<usize> = Vec::new();
    for room in recent {
        if order.len() == LEAD {
            break;
        }
        if current == Some(room.as_str()) {
            continue;
        }
        if let Some(at) = listed.iter().position(|l| *l == room) {
            if !order.contains(&at) {
                order.push(at);
            }
        }
    }
    let lead = order.len();
    let rest: Vec<usize> = (0..listed.len()).filter(|at| !order[..lead].contains(at)).take(rest).collect();
    order.extend(rest);
    order
}

fn all(data_dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read(data_dir.join(RECENT_ROOMS))
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

/// This account's recent rooms, newest first.
pub fn read(data_dir: &Path, user: &str) -> Vec<String> {
    all(data_dir)
        .get(user)
        .and_then(|v| v.as_array())
        .map(|list| list.iter().filter_map(|r| Some(r.as_str()?.to_owned())).collect())
        .unwrap_or_default()
}

pub fn write(data_dir: &Path, user: &str, recent: &[String]) -> std::io::Result<()> {
    let mut all = all(data_dir);
    all.insert(user.to_owned(), serde_json::json!(recent));
    std::fs::write(data_dir.join(RECENT_ROOMS), serde_json::Value::Object(all).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn opening_a_room_makes_it_the_newest_once() {
        let mut recent = ids(&["b", "a"]);
        record(&mut recent, "a");
        assert_eq!(recent, ids(&["a", "b"]));
        record(&mut recent, "a");
        record(&mut recent, "c");
        assert_eq!(recent, ids(&["c", "a", "b"]));
    }

    #[test]
    fn the_palette_leads_with_the_last_five_opened_then_the_usual_list() {
        let listed = ["r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8"];
        let recent = ids(&["r7", "r3", "r8", "r2", "r6", "r1"]);
        let order = palette_order(&listed, &recent, None, 8);
        let names: Vec<&str> = order.iter().map(|&i| listed[i]).collect();
        assert_eq!(names, ["r7", "r3", "r8", "r2", "r6", "r1", "r4", "r5"]);
    }

    #[test]
    fn the_current_room_and_left_rooms_are_not_among_the_five() {
        let listed = ["r1", "r2", "r3", "r4", "r5", "r6", "r7"];
        // r7 is open; "gone" was left.
        let recent = ids(&["r7", "gone", "r3", "r2", "r6", "r1", "r5", "r4"]);
        let order = palette_order(&listed, &recent, Some("r7"), 10);
        let names: Vec<&str> = order.iter().map(|&i| listed[i]).collect();
        assert_eq!(&names[..5], ["r3", "r2", "r6", "r1", "r5"]);
        assert_eq!(&names[5..], ["r4", "r7"], "the rest, none twice");
    }

    #[test]
    fn with_fewer_than_five_visited_it_shows_what_there_is() {
        let listed = ["r1", "r2", "r3"];
        let order = palette_order(&listed, &ids(&["r2"]), None, 8);
        assert_eq!(order, [1, 0, 2]);
        assert_eq!(palette_order(&listed, &[], None, 8), [0, 1, 2]);
    }

    #[test]
    fn recent_rooms_are_kept_per_account() {
        let dir = std::env::temp_dir().join(format!("morpheus-recent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        assert!(read(&dir, "@a:x").is_empty());
        write(&dir, "@a:x", &ids(&["!1:x", "!2:x"])).expect("write");
        write(&dir, "@b:x", &ids(&["!3:x"])).expect("write");
        assert_eq!(read(&dir, "@a:x"), ids(&["!1:x", "!2:x"]));
        assert_eq!(read(&dir, "@b:x"), ids(&["!3:x"]));
        std::fs::remove_dir_all(&dir).ok();
    }
}
