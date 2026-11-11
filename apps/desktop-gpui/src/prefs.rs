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
}

impl Prefs {
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
        Self {
            hide_muted: value
                .get("hide_muted")
                .and_then(|v| v.as_bool())
                .unwrap_or(false),
            rail,
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
}
