//! Acknowledgements (`crates/ui/src/components/about.rs`): the open-source
//! work Morpheus stands on, from the lockfile.

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The projects a user would recognise, ahead of the full crate list.
pub const HEADLINERS: [(&str, &str); 5] = [
    (
        "Matrix and matrix-rust-sdk",
        "The open protocol and the SDK doing sync, encryption and timelines.",
    ),
    (
        "GPUI",
        "The GPU-drawn UI framework behind every screen of the desktop app.",
    ),
    (
        "Lucide",
        "The icon set used in menus and buttons (ISC licence).",
    ),
    ("Tokio", "The async runtime underneath the core."),
    (
        "Dioxus",
        "The UI framework of the phone and web apps.",
    ),
];

/// Every third-party crate named in a lockfile's text: (name, version).
pub fn crates_in(lock: &'static str) -> Vec<(&'static str, &'static str)> {
    let field = |line: &'static str, key: &str| {
        line.strip_prefix(key)?
            .trim()
            .strip_prefix("= \"")?
            .strip_suffix('"')
    };
    let mut list = Vec::new();
    let mut name = None;
    for line in lock.lines() {
        if let Some(found) = field(line, "name") {
            name = Some(found);
        } else if let (Some(version), Some(found)) = (field(line, "version"), name.take())
            && !found.starts_with("morpheus")
        {
            list.push((found, version));
        }
    }
    list
}

/// The crates of this build, from the workspace's lockfile.
pub fn crates() -> Vec<(&'static str, &'static str)> {
    crates_in(include_str!("../../../Cargo.lock"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crates_come_from_the_lockfile_without_our_own() {
        let lock = "[[package]]\nname = \"morpheus-core\"\nversion = \"0.1.4\"\n\n[[package]]\nname = \"tokio\"\nversion = \"1.40.0\"\nsource = \"registry\"\n";
        assert_eq!(crates_in(lock), [("tokio", "1.40.0")]);
    }

    #[test]
    fn the_real_lockfile_lists_many() {
        assert!(crates().len() > 100);
        assert!(crates().iter().any(|(name, _)| *name == "gpui"));
    }
}
