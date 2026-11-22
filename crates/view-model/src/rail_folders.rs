//! Server folders in the account's layout (`dev.morpheus.ui`, key `rail`), in
//! the format the desktop keeps: a list of `{"space": id}` and
//! `{"folder": id, "name": name, "spaces": [id...]}`. These work on that
//! JSON directly, so whatever else the account keeps beside the rail (the
//! theme, hidden muted rooms) is left as it is.

use serde_json::{Value, json};

/// A top-level entry of the rail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RailRow {
    Server(String),
    Folder { id: String, name: String, spaces: Vec<String> },
}

impl RailRow {
    /// What the app calls it when it reorders: the space's ID, or `folder:<id>`.
    pub fn key(&self) -> String {
        match self {
            Self::Server(id) => id.clone(),
            Self::Folder { id, .. } => format!("folder:{id}"),
        }
    }
}

fn entries(prefs: &Value) -> Vec<Value> {
    prefs.get("rail").and_then(Value::as_array).cloned().unwrap_or_default()
}

fn set_entries(prefs: &mut Value, rail: Vec<Value>) {
    match prefs.as_object_mut() {
        Some(object) => {
            object.insert("rail".into(), rail.into());
        }
        None => *prefs = json!({ "rail": rail }),
    }
}

/// The rail's entries in order.
pub fn rows(prefs: &Value) -> Vec<RailRow> {
    entries(prefs)
        .iter()
        .filter_map(|item| {
            if let Some(space) = item.get("space").and_then(Value::as_str) {
                return Some(RailRow::Server(space.to_owned()));
            }
            let id = item.get("folder")?.as_str()?.to_owned();
            let name = item.get("name").and_then(Value::as_str).unwrap_or("Folder").to_owned();
            let spaces = item
                .get("spaces")
                .and_then(Value::as_array)
                .map(|s| s.iter().filter_map(|v| v.as_str().map(str::to_owned)).collect())
                .unwrap_or_default();
            Some(RailRow::Folder { id, name, spaces })
        })
        .collect()
}

/// The folders a server can be moved to: id, name and whether it is in it (what `space_menu` takes).
pub fn folder_choices(prefs: &Value, space: &str) -> Vec<(String, String, bool)> {
    rows(prefs)
        .into_iter()
        .filter_map(|row| match row {
            RailRow::Folder { id, name, spaces } => Some((id, name, spaces.iter().any(|s| s == space))),
            RailRow::Server(_) => None,
        })
        .collect()
}

fn from_rows(rows: &[RailRow]) -> Vec<Value> {
    rows.iter()
        .map(|row| match row {
            RailRow::Server(id) => json!({ "space": id }),
            RailRow::Folder { id, name, spaces } => json!({ "folder": id, "name": name, "spaces": spaces }),
        })
        .collect()
}

/// Take a space out of wherever it sits (a folder left empty goes too).
fn detach(rows: &mut Vec<RailRow>, space: &str) {
    rows.retain(|row| !matches!(row, RailRow::Server(id) if id == space));
    for row in rows.iter_mut() {
        if let RailRow::Folder { spaces, .. } = row {
            spaces.retain(|id| id != space);
        }
    }
    rows.retain(|row| !matches!(row, RailRow::Folder { spaces, .. } if spaces.is_empty()));
}

fn edit(prefs: &mut Value, change: impl FnOnce(&mut Vec<RailRow>)) {
    let mut all = rows(prefs);
    change(&mut all);
    set_entries(prefs, from_rows(&all));
}

/// Put the top-level entries in the order of `keys` (a space's ID or `folder:<id>`). Entries not named keep
/// their order after; names that are not in the rail are servers added to it.
pub fn reorder(prefs: &mut Value, keys: &[String]) {
    edit(prefs, |all| {
        let mut ordered: Vec<RailRow> = Vec::new();
        for key in keys {
            if let Some(at) = all.iter().position(|r| &r.key() == key) {
                ordered.push(all.remove(at));
            } else if !key.starts_with("folder:") && !all.iter().any(|r| matches!(r, RailRow::Folder { spaces, .. } if spaces.contains(key))) {
                ordered.push(RailRow::Server(key.clone()));
            }
        }
        ordered.append(all);
        *all = ordered;
    });
}

/// Break a folder up: its servers stay, in its place.
pub fn ungroup(prefs: &mut Value, folder: &str) {
    edit(prefs, |all| {
        if let Some(at) = all.iter().position(|r| matches!(r, RailRow::Folder { id, .. } if id == folder))
            && let RailRow::Folder { spaces, .. } = all.remove(at)
        {
            for (offset, space) in spaces.into_iter().enumerate() {
                all.insert(at + offset, RailRow::Server(space));
            }
        }
    });
}

/// Put a server in an existing folder.
pub fn move_to_folder(prefs: &mut Value, space: &str, folder: &str) {
    edit(prefs, |all| {
        // A folder that is not there is not made: the server stays where it is.
        if !all.iter().any(|r| matches!(r, RailRow::Folder { id, .. } if id == folder)) {
            return;
        }
        detach(all, space);
        for row in all.iter_mut() {
            if let RailRow::Folder { id, spaces, .. } = row
                && id == folder
            {
                spaces.push(space.to_owned());
            }
        }
    });
}

/// Put a server in a folder of its own, named "New folder"; `now_ms` makes its ID unique.
pub fn move_to_new_folder(prefs: &mut Value, space: &str, now_ms: i64) {
    edit(prefs, |all| {
        detach(all, space);
        let n = all.iter().filter(|r| matches!(r, RailRow::Folder { .. })).count();
        all.push(RailRow::Folder { id: format!("folder-{now_ms}-{n}"), name: "New folder".into(), spaces: vec![space.to_owned()] });
    });
}

/// Rename a folder.
pub fn rename_folder(prefs: &mut Value, folder: &str, name: &str) {
    edit(prefs, |all| {
        for row in all.iter_mut() {
            if let RailRow::Folder { id, name: current, .. } = row
                && id == folder
            {
                *current = name.trim().to_owned();
            }
        }
    });
}

/// Take a server out of its folder, to the end of the rail.
pub fn move_out_of_folders(prefs: &mut Value, space: &str) {
    edit(prefs, |all| {
        detach(all, space);
        all.push(RailRow::Server(space.to_owned()));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Value {
        json!({
            "theme": "dark",
            "rail": [
                { "space": "!a:x" },
                { "folder": "f1", "name": "Work", "spaces": ["!b:x", "!c:x"] },
                { "space": "!d:x" },
            ]
        })
    }

    #[test]
    fn the_folders_a_server_can_move_to_say_whether_it_is_in_them() {
        assert_eq!(folder_choices(&sample(), "!b:x"), vec![("f1".to_owned(), "Work".to_owned(), true)]);
        assert_eq!(folder_choices(&sample(), "!a:x"), vec![("f1".to_owned(), "Work".to_owned(), false)]);
    }

    #[test]
    fn ungrouping_leaves_its_servers_in_its_place() {
        let mut prefs = sample();
        ungroup(&mut prefs, "f1");
        assert_eq!(rows(&prefs), vec![RailRow::Server("!a:x".into()), RailRow::Server("!b:x".into()), RailRow::Server("!c:x".into()), RailRow::Server("!d:x".into())]);
    }

    #[test]
    fn a_rail_in_the_desktops_format_is_read() {
        assert_eq!(
            rows(&sample()),
            vec![
                RailRow::Server("!a:x".into()),
                RailRow::Folder { id: "f1".into(), name: "Work".into(), spaces: vec!["!b:x".into(), "!c:x".into()] },
                RailRow::Server("!d:x".into()),
            ]
        );
    }

    #[test]
    fn a_server_moves_into_a_folder_and_what_else_the_account_keeps_stays() {
        let mut prefs = sample();
        move_to_folder(&mut prefs, "!a:x", "f1");
        assert_eq!(prefs["theme"], "dark");
        assert_eq!(prefs["rail"][0], json!({ "folder": "f1", "name": "Work", "spaces": ["!b:x", "!c:x", "!a:x"] }));
        assert_eq!(prefs["rail"][1], json!({ "space": "!d:x" }));
        assert_eq!(prefs["rail"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn a_server_gets_a_folder_of_its_own_which_is_renamed() {
        let mut prefs = sample();
        move_to_new_folder(&mut prefs, "!d:x", 99);
        let last = rows(&prefs).pop().expect("a folder");
        assert_eq!(last, RailRow::Folder { id: "folder-99-1".into(), name: "New folder".into(), spaces: vec!["!d:x".into()] });
        rename_folder(&mut prefs, "folder-99-1", "  Games ");
        assert!(matches!(rows(&prefs).pop(), Some(RailRow::Folder { name, .. }) if name == "Games"));
        assert!(!rows(&prefs).contains(&RailRow::Server("!d:x".into())));
    }

    #[test]
    fn the_last_server_leaving_a_folder_takes_it_away_and_one_out_goes_to_the_end() {
        let mut prefs = sample();
        move_out_of_folders(&mut prefs, "!b:x");
        move_out_of_folders(&mut prefs, "!c:x");
        assert_eq!(
            rows(&prefs),
            vec![RailRow::Server("!a:x".into()), RailRow::Server("!d:x".into()), RailRow::Server("!b:x".into()), RailRow::Server("!c:x".into())]
        );
    }

    #[test]
    fn reordering_keeps_the_folders() {
        let mut prefs = sample();
        reorder(&mut prefs, &["!d:x".into(), "folder:f1".into(), "!a:x".into()]);
        let keys: Vec<String> = rows(&prefs).iter().map(RailRow::key).collect();
        assert_eq!(keys, vec!["!d:x", "folder:f1", "!a:x"]);
        // A server the rail did not list yet is added; one in a folder is not added twice.
        reorder(&mut prefs, &["!e:x".into(), "!b:x".into()]);
        let keys: Vec<String> = rows(&prefs).iter().map(RailRow::key).collect();
        assert_eq!(keys, vec!["!e:x", "!d:x", "folder:f1", "!a:x"]);
    }

    #[test]
    fn moving_to_a_folder_that_is_not_there_changes_nothing() {
        let mut prefs = sample();
        move_to_folder(&mut prefs, "!a:x", "nope");
        assert_eq!(rows(&prefs), rows(&sample()));
    }
}
