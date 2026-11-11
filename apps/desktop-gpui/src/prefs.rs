//! The layout kept in account data (`dev.morpheus.ui`): the rail's order and
//! folders, and hiding muted channels. Read as `crates/ui`'s `Prefs` does.

use morpheus_core::OwnedRoomId;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RailEntry {
    Space(OwnedRoomId),
    Folder {
        id: String,
        name: String,
        spaces: Vec<OwnedRoomId>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Prefs {
    pub hide_muted: bool,
    /// Spaces and folders of spaces in the rail's order; spaces missing here
    /// come after, in the order the server lists them.
    pub rail: Vec<RailEntry>,
    /// What else the account keeps there (the theme): written back as it was.
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl Prefs {
    /// Takes a space out of wherever it sits in the rail (dropping folders
    /// left empty).
    pub fn detach(&mut self, space: &OwnedRoomId) {
        self.rail
            .retain(|entry| !matches!(entry, RailEntry::Space(id) if id == space));
        for entry in &mut self.rail {
            if let RailEntry::Folder { spaces, .. } = entry {
                spaces.retain(|id| id != space);
            }
        }
        self.rail
            .retain(|entry| !matches!(entry, RailEntry::Folder { spaces, .. } if spaces.is_empty()));
    }

    /// Puts a space in an existing folder.
    pub fn move_to_folder(&mut self, space: &OwnedRoomId, folder: &str) {
        self.detach(space);
        for entry in &mut self.rail {
            if let RailEntry::Folder { id, spaces, .. } = entry
                && id == folder
            {
                spaces.push(space.clone());
            }
        }
    }

    /// Puts a space in a folder of its own, named "New folder"; `now_ms`
    /// makes its id unique.
    pub fn move_to_new_folder(&mut self, space: &OwnedRoomId, now_ms: i64) {
        self.detach(space);
        let n = self
            .rail
            .iter()
            .filter(|entry| matches!(entry, RailEntry::Folder { .. }))
            .count();
        self.rail.push(RailEntry::Folder {
            id: format!("folder-{now_ms}-{n}"),
            name: "New folder".into(),
            spaces: vec![space.clone()],
        });
    }

    /// Takes a space out of its folder, to the end of the rail.
    pub fn move_out_of_folders(&mut self, space: &OwnedRoomId) {
        self.detach(space);
        self.rail.push(RailEntry::Space(space.clone()));
    }

    /// The folders a space can be moved to: id, name and whether it is in it.
    pub fn folder_choices(&self, space: &OwnedRoomId) -> Vec<(String, String, bool)> {
        self.rail
            .iter()
            .filter_map(|entry| match entry {
                RailEntry::Folder { id, name, spaces } => {
                    Some((id.clone(), name.clone(), spaces.contains(space)))
                }
                RailEntry::Space(_) => None,
            })
            .collect()
    }

    /// As kept in the account: what was read, with the settings changed here.
    /// The colour theme chosen (kept with the rest of the account's prefs).
    pub fn theme(&self) -> crate::view_model::settings::Theme {
        self.extra
            .get("theme")
            .and_then(|v| v.as_str())
            .map(crate::view_model::settings::Theme::from_key)
            .unwrap_or_default()
    }

    pub fn set_theme(&mut self, theme: crate::view_model::settings::Theme) {
        self.extra.insert("theme".into(), theme.key().into());
    }

    pub fn to_json(&self) -> serde_json::Value {
        let rail: Vec<serde_json::Value> = self
            .rail
            .iter()
            .map(|entry| match entry {
                RailEntry::Space(id) => serde_json::json!({ "space": id }),
                RailEntry::Folder { id, name, spaces } => {
                    serde_json::json!({ "folder": id, "name": name, "spaces": spaces })
                }
            })
            .collect();
        let mut object = self.extra.clone();
        object.insert("hide_muted".into(), self.hide_muted.into());
        object.insert("rail".into(), rail.into());
        serde_json::Value::Object(object)
    }

    pub fn from_json(value: &serde_json::Value) -> Self {
        let rail = value
            .get("rail")
            .and_then(|v| v.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        if let Some(space) = item.get("space").and_then(|v| v.as_str()) {
                            return space.parse().ok().map(RailEntry::Space);
                        }
                        let id = item.get("folder")?.as_str()?.to_owned();
                        let name = item
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Folder")
                            .to_owned();
                        let spaces = item
                            .get("spaces")
                            .and_then(|v| v.as_array())
                            .map(|s| s.iter().filter_map(|v| v.as_str()?.parse().ok()).collect())
                            .unwrap_or_default();
                        Some(RailEntry::Folder { id, name, spaces })
                    })
                    .collect()
            })
            .unwrap_or_default();
        let extra = value
            .as_object()
            .map(|object| {
                object
                    .iter()
                    .filter(|(key, _)| !matches!(key.as_str(), "hide_muted" | "rail"))
                    .map(|(key, value)| (key.clone(), value.clone()))
                    .collect()
            })
            .unwrap_or_default();
        Self {
            hide_muted: value
                .get("hide_muted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            rail,
            extra,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_spaces_and_folders() {
        let json = serde_json::json!({
            "hide_muted": true,
            "rail": [
                { "space": "!a:localhost" },
                { "folder": "f1", "name": "Work", "spaces": ["!b:localhost", "!c:localhost"] },
            ],
        });
        let prefs = Prefs::from_json(&json);
        assert!(prefs.hide_muted);
        assert_eq!(prefs.rail.len(), 2);
        assert!(
            matches!(&prefs.rail[1], RailEntry::Folder { name, spaces, .. } if name == "Work" && spaces.len() == 2)
        );
        assert_eq!(Prefs::from_json(&serde_json::json!({})), Prefs::default());
    }

    #[test]
    fn spaces_move_between_folders_and_empty_folders_go() {
        let id = |s: &str| OwnedRoomId::try_from(s).expect("room id");
        let (a, b) = (id("!a:x"), id("!b:x"));
        let mut prefs = Prefs {
            rail: vec![RailEntry::Space(a.clone()), RailEntry::Space(b.clone())],
            ..Prefs::default()
        };
        prefs.move_to_new_folder(&a, 7);
        assert_eq!(
            prefs.rail,
            [
                RailEntry::Space(b.clone()),
                RailEntry::Folder {
                    id: "folder-7-0".into(),
                    name: "New folder".into(),
                    spaces: vec![a.clone()],
                },
            ]
        );
        assert_eq!(
            prefs.folder_choices(&a),
            [("folder-7-0".to_owned(), "New folder".to_owned(), true)]
        );
        prefs.move_to_folder(&b, "folder-7-0");
        assert_eq!(prefs.rail.len(), 1, "b joined the folder");
        prefs.move_out_of_folders(&a);
        assert_eq!(
            prefs.rail,
            [
                RailEntry::Folder {
                    id: "folder-7-0".into(),
                    name: "New folder".into(),
                    spaces: vec![b.clone()],
                },
                RailEntry::Space(a.clone()),
            ]
        );
        prefs.move_out_of_folders(&b);
        assert_eq!(
            prefs.rail,
            [RailEntry::Space(a), RailEntry::Space(b)],
            "the folder left empty is gone"
        );
    }

    #[test]
    fn saving_keeps_what_else_the_account_has_there() {
        let json = serde_json::json!({
            "hide_muted": false,
            "theme": "midnight",
            "rail": [{ "space": "!a:localhost" }],
        });
        let mut prefs = Prefs::from_json(&json);
        prefs.hide_muted = true;
        let saved = prefs.to_json();
        assert_eq!(saved["theme"], "midnight");
        assert_eq!(saved["hide_muted"], true);
        assert_eq!(saved["rail"], json["rail"]);
        assert_eq!(Prefs::from_json(&saved), prefs);
    }
}
