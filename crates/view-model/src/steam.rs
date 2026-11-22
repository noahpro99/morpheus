//! A status from the Steam game that is running (#329): reading Steam's
//! `registry.vdf` for the running app id, the app's name from its
//! `appmanifest_<id>.acf`, and when to set and when to restore the status.
//! Polled by the desktop on a background thread.

use std::path::{Path, PathBuf};

/// The id of Steam as an activity source.
pub const SOURCE: &str = "steam";

// ---- Valve's text format --------------------------------------------------------

/// A parsed VDF (KeyValues) file: strings and sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Vdf {
    Str(String),
    Map(Vec<(String, Vdf)>),
}

impl Vdf {
    /// The value under `key` (keys are compared without regard to case, as Steam does).
    pub fn get(&self, key: &str) -> Option<&Vdf> {
        match self {
            Vdf::Map(entries) => entries
                .iter()
                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                .map(|(_, v)| v),
            Vdf::Str(_) => None,
        }
    }

    /// Follows `path` down through sections.
    pub fn path(&self, path: &[&str]) -> Option<&Vdf> {
        path.iter().try_fold(self, |at, key| at.get(key))
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Vdf::Str(s) => Some(s),
            Vdf::Map(_) => None,
        }
    }

    pub fn entries(&self) -> &[(String, Vdf)] {
        match self {
            Vdf::Map(entries) => entries,
            Vdf::Str(_) => &[],
        }
    }
}

#[derive(Debug, PartialEq)]
enum Token {
    Text(String),
    Open,
    Close,
}

fn tokens(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' => out.push(Token::Open),
            '}' => out.push(Token::Close),
            '"' => {
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    match c {
                        '"' => break,
                        '\\' => match chars.next() {
                            Some('n') => s.push('\n'),
                            Some('t') => s.push('\t'),
                            Some(other) => s.push(other),
                            None => break,
                        },
                        other => s.push(other),
                    }
                }
                out.push(Token::Text(s));
            }
            '/' if chars.peek() == Some(&'/') => {
                for c in chars.by_ref() {
                    if c == '\n' {
                        break;
                    }
                }
            }
            c if c.is_whitespace() => {}
            c => {
                // An unquoted word.
                let mut s = String::from(c);
                while let Some(&next) = chars.peek() {
                    if next.is_whitespace() || matches!(next, '{' | '}' | '"') {
                        break;
                    }
                    s.push(next);
                    chars.next();
                }
                out.push(Token::Text(s));
            }
        }
    }
    out
}

/// Reads a VDF file; its top level is a section holding what the file holds.
pub fn parse_vdf(text: &str) -> Vdf {
    let tokens = tokens(text);
    let mut at = 0;
    Vdf::Map(parse_section(&tokens, &mut at))
}

fn parse_section(tokens: &[Token], at: &mut usize) -> Vec<(String, Vdf)> {
    let mut entries = Vec::new();
    while *at < tokens.len() {
        match &tokens[*at] {
            Token::Close => {
                *at += 1;
                return entries;
            }
            Token::Open => *at += 1,
            Token::Text(key) => {
                *at += 1;
                match tokens.get(*at) {
                    Some(Token::Text(value)) => {
                        entries.push((key.clone(), Vdf::Str(value.clone())));
                        *at += 1;
                    }
                    Some(Token::Open) => {
                        *at += 1;
                        let inner = parse_section(tokens, at);
                        entries.push((key.clone(), Vdf::Map(inner)));
                    }
                    _ => {}
                }
            }
        }
    }
    entries
}

// ---- what the files say -----------------------------------------------------------

/// The app id Steam says is running (`RunningAppID` under `HKCU/Software/Valve/Steam`
/// in `registry.vdf`); None for 0, which means nothing is.
pub fn running_app_id(registry_vdf: &str) -> Option<u32> {
    let root = parse_vdf(registry_vdf);
    let steam = root.path(&["Registry", "HKCU", "Software", "Valve", "Steam"])?;
    let id: u32 = steam.get("RunningAppID")?.as_str()?.trim().parse().ok()?;
    (id != 0).then_some(id)
}

/// The library folders `libraryfolders.vdf` lists (old files list them as bare
/// numbered paths, new ones as sections with a `path`).
pub fn library_paths(libraryfolders_vdf: &str) -> Vec<PathBuf> {
    let root = parse_vdf(libraryfolders_vdf);
    let Some(folders) = root.get("libraryfolders") else {
        return Vec::new();
    };
    folders
        .entries()
        .iter()
        .filter(|(key, _)| key.chars().all(|c| c.is_ascii_digit()))
        .filter_map(|(_, value)| match value {
            Vdf::Str(path) => Some(PathBuf::from(path)),
            map => map.get("path").and_then(Vdf::as_str).map(PathBuf::from),
        })
        .collect()
}

/// The app's name from its `appmanifest_<id>.acf`.
pub fn app_name(acf: &str) -> Option<String> {
    let root = parse_vdf(acf);
    let name = root.path(&["AppState", "name"])?.as_str()?.trim();
    (!name.is_empty()).then(|| name.to_owned())
}


/// Where Steam keeps its files on this machine.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SteamDirs {
    /// Files that may hold `RunningAppID`.
    pub registries: Vec<PathBuf>,
    /// Steam's own folders (each has `steamapps`, maybe `libraryfolders.vdf`).
    pub roots: Vec<PathBuf>,
}

impl SteamDirs {
    /// One folder with everything in it (`MORPHEUS_STEAM_DIR`, and Steam's own folder on a Mac).
    pub fn in_folder(root: &Path) -> Self {
        Self {
            registries: vec![root.join("registry.vdf")],
            roots: vec![root.to_path_buf()],
        }
    }

    /// The usual places on this machine, for `home`.
    pub fn standard(home: &Path) -> Self {
        let mut dirs = Self::default();
        if cfg!(target_os = "macos") {
            dirs = Self::in_folder(&home.join("Library/Application Support/Steam"));
        } else if cfg!(target_os = "linux") {
            let flatpak = home.join(".var/app/com.valvesoftware.Steam");
            for base in [home.to_path_buf(), flatpak] {
                dirs.registries.push(base.join(".steam/registry.vdf"));
                dirs.registries.push(base.join(".local/share/Steam/registry.vdf"));
                dirs.roots.push(base.join(".local/share/Steam"));
                dirs.roots.push(base.join(".steam/steam"));
            }
        }
        dirs
    }
}

/// A running Steam game or app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningGame {
    pub id: u32,
    pub name: String,
}

/// The name of app `id`, from the manifests in Steam's folders and the libraries
/// they list.
pub fn name_of(roots: &[PathBuf], id: u32) -> Option<String> {
    let mut libraries: Vec<PathBuf> = Vec::new();
    for root in roots {
        libraries.push(root.clone());
        for sub in ["steamapps/libraryfolders.vdf", "config/libraryfolders.vdf"] {
            if let Ok(text) = std::fs::read_to_string(root.join(sub)) {
                libraries.extend(library_paths(&text));
            }
        }
    }
    libraries.iter().find_map(|library| {
        let acf = library.join(format!("steamapps/appmanifest_{id}.acf"));
        app_name(&std::fs::read_to_string(acf).ok()?)
    })
}

/// The game running now, if any: `app_id` is what the platform's own source (the
/// Windows registry) says, else the registry files are read.
pub fn detect(dirs: &SteamDirs, app_id: Option<u32>) -> Option<RunningGame> {
    let id = app_id.or_else(|| {
        dirs.registries
            .iter()
            .filter_map(|file| std::fs::read_to_string(file).ok())
            .find_map(|text| running_app_id(&text))
    })?;
    let name = name_of(&dirs.roots, id)?;
    Some(RunningGame { id, name })
}

/// What the Windows registry says is running, and where Steam is.
#[cfg(target_os = "windows")]
pub fn windows_running() -> (Option<u32>, Option<PathBuf>) {
    let Ok(key) = windows_registry::CURRENT_USER.open("Software\\Valve\\Steam") else {
        return (None, None);
    };
    let id = key.get_u32("RunningAppID").ok().filter(|id| *id != 0);
    let path = key.get_string("SteamPath").ok().map(PathBuf::from);
    (id, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    const REGISTRY: &str = r#""Registry"
{
	"HKCU"
	{
		"Software"
		{
			"Valve"
			{
				"Steam"
				{
					"language"		"english"
					"RunningAppID"		"1245620"
					"SteamPath"		"/home/me/.local/share/Steam"
				}
			}
		}
	}
}
"#;

    const LIBRARIES: &str = r#""libraryfolders"
{
	"0"
	{
		"path"		"/home/me/.local/share/Steam"
		"apps"
		{
			"228980"		"1234"
		}
	}
	"1"
	{
		"path"		"/mnt/games/SteamLibrary"
	}
}
"#;

    const OLD_LIBRARIES: &str = "\"LibraryFolders\"\n{\n\t\"TimeNextStatsReport\"\t\t\"1\"\n\t\"1\"\t\t\"D:\\\\Games\\\\Steam\"\n}\n";

    const ACF: &str = "\"AppState\"\n{\n\t\"appid\"\t\t\"1245620\"\n\t\"name\"\t\t\"ELDEN RING\"\n\t\"installdir\"\t\t\"ELDEN RING\"\n}\n";

    #[test]
    fn the_running_app_id_is_read_from_the_registry() {
        assert_eq!(running_app_id(REGISTRY), Some(1245620));
    }

    #[test]
    fn app_id_zero_and_a_missing_value_mean_nothing_is_running() {
        assert_eq!(running_app_id(&REGISTRY.replace("1245620", "0")), None);
        assert_eq!(running_app_id(&REGISTRY.replace("RunningAppID", "Other")), None);
        assert_eq!(running_app_id("garbage { \""), None);
        assert_eq!(running_app_id(""), None);
    }

    #[test]
    fn library_folders_come_from_new_and_old_files() {
        assert_eq!(
            library_paths(LIBRARIES),
            vec![
                PathBuf::from("/home/me/.local/share/Steam"),
                PathBuf::from("/mnt/games/SteamLibrary")
            ]
        );
        assert_eq!(library_paths(OLD_LIBRARIES), vec![PathBuf::from("D:\\Games\\Steam")]);
    }

    #[test]
    fn the_name_is_read_from_the_manifest() {
        assert_eq!(app_name(ACF).as_deref(), Some("ELDEN RING"));
        assert_eq!(app_name("\"AppState\" { }"), None);
    }

    #[test]
    fn comments_and_escapes_do_not_break_the_parse() {
        let vdf = parse_vdf("// c\n\"a\" { \"b\" \"x \\\"y\\\" z\" } // end\n\"c\" 5");
        assert_eq!(vdf.path(&["a", "b"]).and_then(Vdf::as_str), Some("x \"y\" z"));
        assert_eq!(vdf.get("C").and_then(Vdf::as_str), Some("5"));
    }

    #[test]
    fn a_game_is_found_through_the_registry_and_a_second_library() {
        let dir = std::env::temp_dir().join(format!("morpheus-steam-{}", std::process::id()));
        let library = dir.join("lib2");
        std::fs::create_dir_all(dir.join("steamapps")).expect("dirs");
        std::fs::create_dir_all(library.join("steamapps")).expect("dirs");
        std::fs::write(dir.join("registry.vdf"), REGISTRY).expect("registry");
        std::fs::write(
            dir.join("steamapps/libraryfolders.vdf"),
            LIBRARIES.replace("/mnt/games/SteamLibrary", &library.display().to_string()),
        )
        .expect("libraries");
        std::fs::write(library.join("steamapps/appmanifest_1245620.acf"), ACF).expect("acf");
        let found = detect(&SteamDirs::in_folder(&dir), None);
        assert_eq!(
            found,
            Some(RunningGame { id: 1245620, name: "ELDEN RING".into() })
        );
        std::fs::write(dir.join("registry.vdf"), REGISTRY.replace("1245620", "0")).expect("registry");
        assert_eq!(detect(&SteamDirs::in_folder(&dir), None), None);
        // An app with no manifest has no name to show.
        assert_eq!(detect(&SteamDirs::in_folder(&dir), Some(7)), None);
        std::fs::remove_dir_all(&dir).ok();
    }
}
