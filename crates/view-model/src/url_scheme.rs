//! Registering Morpheus as the opener of `matrix:` links, per user and
//! without asking for more than that: a desktop entry on Linux, a registry
//! key on Windows (macOS declares it in the bundle's Info.plist). The
//! decisions are pure functions; [`register`] is the thin shell round them.

use std::path::Path;

/// The desktop entry's file name (also what `xdg-mime` calls the app).
pub const DESKTOP_FILE: &str = "morpheus.desktop";

const MIME: &str = "x-scheme-handler/matrix";

/// The text of the desktop entry that opens `matrix:` links with the app at `exe`.
pub fn desktop_entry(exe: &Path) -> String {
    let path = exe.to_string_lossy();
    let exec = if path.contains(|c: char| c.is_whitespace() || "\"'\\$`".contains(c)) {
        format!("\"{}\"", path.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        path.into_owned()
    };
    format!(
        "[Desktop Entry]\nType=Application\nName=Morpheus\nComment=Matrix chat\nExec={exec} %u\nIcon=morpheus\nTerminal=false\nCategories=Network;InstantMessaging;Chat;\nStartupWMClass=morpheus-desktop\nMimeType={MIME};\n"
    )
}

/// Whether the entry on disk (`existing`) must be written as `wanted`.
pub fn entry_needs_writing(existing: Option<&str>, wanted: &str) -> bool {
    existing != Some(wanted)
}

/// Whether to make Morpheus the default for `matrix:` links, given what
/// `xdg-mime query default` said: only when nothing is set or it is already
/// ours, never taking it from another app (Element, ...).
pub fn should_become_default(current: Option<&str>) -> bool {
    match current.map(str::trim) {
        None | Some("") => true,
        Some(other) => other == DESKTOP_FILE,
    }
}

/// What to do with the Windows `matrix` key, given the command it holds now.
#[derive(Debug, PartialEq, Eq)]
pub enum KeyAction {
    /// Nothing there, or ours but pointing at another place: write it.
    Write,
    /// Right already, or another app's: leave it.
    Leave,
}

/// The key's command is `existing` (`None`: no key); the app is at `exe`.
pub fn key_action(existing: Option<&str>, exe: &Path) -> KeyAction {
    let Some(existing) = existing else {
        return KeyAction::Write;
    };
    if existing == key_command(exe) {
        return KeyAction::Leave;
    }
    // By hand: a Windows path is not split on `\` when this runs elsewhere.
    let ours = |p: &Path| {
        let text = p.to_string_lossy().to_lowercase();
        let name = text.rsplit(['\\', '/']).next().unwrap_or_default().to_owned();
        name == "morpheus-desktop.exe" || name == "morpheus-desktop"
    };
    match crate::settings::command_exe(existing) {
        Some(old) if ours(&old) => KeyAction::Write,
        _ => KeyAction::Leave,
    }
}

/// The command stored in the Windows key for the app at `exe`.
pub fn key_command(exe: &Path) -> String {
    format!("\"{}\" \"%1\"", exe.display())
}

/// Where an exe lives, which decides whether it may own the desktop entry.
#[derive(Debug, PartialEq, Eq)]
pub enum ExeKind {
    /// A debug build or one inside a cargo `target/` dir: never registered.
    Dev,
    /// Under `/nix/store`: the package ships its own entry.
    Store,
    /// Anything else: gets the entry.
    Installed,
}

/// Classify the exe; `debug` is `cfg!(debug_assertions)`.
pub fn exe_kind(exe: &Path, debug: bool) -> ExeKind {
    if debug || exe.components().any(|c| c.as_os_str() == "target") {
        ExeKind::Dev
    } else if exe.starts_with("/nix/store") {
        ExeKind::Store
    } else {
        ExeKind::Installed
    }
}

/// Whether `entry` is one we wrote whose Exec points at a dev or store exe
/// (so it would shadow the right entry or fail to launch).
pub fn entry_is_stale(entry: &str) -> bool {
    let ours = entry.lines().any(|l| l == "Name=Morpheus") && entry.lines().any(|l| l == "Comment=Matrix chat");
    let Some(exec) = entry.lines().find_map(|l| l.strip_prefix("Exec=")) else {
        return false;
    };
    let path = match exec.strip_prefix('"') {
        Some(rest) => rest.split('"').next().unwrap_or_default(),
        None => exec.split_whitespace().next().unwrap_or_default(),
    };
    ours && exe_kind(Path::new(path), false) != ExeKind::Installed
}

/// Linux, by kind of exe: dev builds leave things alone; a store exe leaves
/// the package's own entry in charge and only claims a free scheme; both
/// remove a stale entry of ours. Returns whether it wrote an entry.
pub fn register_for(apps: &Path, exe: &Path, debug: bool, xdg_mime: &str) -> std::io::Result<bool> {
    let kind = exe_kind(exe, debug);
    if kind == ExeKind::Installed {
        return register_linux(apps, exe, xdg_mime);
    }
    let path = apps.join(DESKTOP_FILE);
    if std::fs::read_to_string(&path).is_ok_and(|text| entry_is_stale(&text)) {
        std::fs::remove_file(&path)?;
    }
    if kind == ExeKind::Store {
        claim_if_free(xdg_mime);
    }
    Ok(false)
}

fn claim_if_free(xdg_mime: &str) {
    let current = std::process::Command::new(xdg_mime)
        .args(["query", "default", MIME])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
    if should_become_default(current.as_deref()) && current.as_deref() != Some(DESKTOP_FILE) {
        let _ = std::process::Command::new(xdg_mime)
            .args(["default", DESKTOP_FILE, MIME])
            .status();
    }
}

/// Linux: write the entry under `apps` if it changed, and become the default
/// handler through `xdg_mime` if nobody else is. Returns whether it wrote.
pub fn register_linux(apps: &Path, exe: &Path, xdg_mime: &str) -> std::io::Result<bool> {
    let wanted = desktop_entry(exe);
    let path = apps.join(DESKTOP_FILE);
    let existing = std::fs::read_to_string(&path).ok();
    let write = entry_needs_writing(existing.as_deref(), &wanted);
    if write {
        std::fs::create_dir_all(apps)?;
        std::fs::write(&path, wanted)?;
    }
    let current = std::process::Command::new(xdg_mime)
        .args(["query", "default", MIME])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
    if should_become_default(current.as_deref()) && current.as_deref() != Some(DESKTOP_FILE) {
        let _ = std::process::Command::new(xdg_mime)
            .args(["default", DESKTOP_FILE, MIME])
            .status();
    }
    Ok(write)
}

/// At startup: make `matrix:` links open this app, where the system allows
/// it per user. Test runs and other apps' claims are left alone.
pub fn register() {
    if std::env::var_os("MORPHEUS_E2E_PORT").is_some() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    #[cfg(target_os = "linux")]
    {
        let Some(data) = std::env::var_os("XDG_DATA_HOME")
            .map(std::path::PathBuf::from)
            .filter(|dir| dir.is_absolute())
            .or_else(|| {
                std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".local/share"))
            })
        else {
            return;
        };
        let program = std::env::var("MORPHEUS_XDG_MIME").unwrap_or_else(|_| "xdg-mime".into());
        let _ = register_for(&data.join("applications"), &exe, cfg!(debug_assertions), &program);
    }
    #[cfg(target_os = "windows")]
    {
        const KEY: &str = r"Software\Classes\matrix";
        let existing = windows_registry::CURRENT_USER
            .open(format!(r"{KEY}\shell\open\command"))
            .ok()
            .and_then(|k| k.get_string("").ok());
        if key_action(existing.as_deref(), &exe) == KeyAction::Write
            && let Ok(key) = windows_registry::CURRENT_USER.create(KEY)
        {
            let _ = key.set_string("", "URL:Matrix");
            let _ = key.set_string("URL Protocol", "");
            if let Ok(command) = windows_registry::CURRENT_USER.create(format!(r"{KEY}\shell\open\command")) {
                let _ = command.set_string("", key_command(&exe));
            }
        }
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    let _ = exe;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_opens_links_with_this_exe_and_says_so() {
        let text = desktop_entry(Path::new("/opt/morpheus/morpheus-desktop"));
        assert!(text.contains("Exec=/opt/morpheus/morpheus-desktop %u\n"));
        assert!(text.contains("MimeType=x-scheme-handler/matrix;\n"));
        let spaced = desktop_entry(Path::new("/opt/my apps/morpheus-desktop"));
        assert!(spaced.contains("Exec=\"/opt/my apps/morpheus-desktop\" %u\n"));
    }

    #[test]
    fn the_entry_is_written_only_when_it_changes() {
        assert!(entry_needs_writing(None, "a"));
        assert!(entry_needs_writing(Some("a"), "b"));
        assert!(!entry_needs_writing(Some("a"), "a"));
    }

    #[test]
    fn the_default_is_taken_only_when_free_or_ours() {
        assert!(should_become_default(None));
        assert!(should_become_default(Some("")));
        assert!(should_become_default(Some("morpheus.desktop")));
        assert!(!should_become_default(Some("element-desktop.desktop")));
    }

    #[test]
    fn the_windows_key_is_written_unless_another_app_owns_it() {
        let exe = Path::new(r"C:\Users\a\Morpheus\morpheus-desktop.exe");
        let ours = key_command(exe);
        assert_eq!(ours, r#""C:\Users\a\Morpheus\morpheus-desktop.exe" "%1""#);
        assert_eq!(key_action(None, exe), KeyAction::Write);
        assert_eq!(key_action(Some(&ours), exe), KeyAction::Leave);
        let moved = r#""C:\Old\morpheus-desktop.exe" "%1""#;
        assert_eq!(key_action(Some(moved), exe), KeyAction::Write);
        let element = r#""C:\Program Files\Element\Element.exe" "%1""#;
        assert_eq!(key_action(Some(element), exe), KeyAction::Leave);
    }

    /// A fake `xdg-mime` in a temp dir (answering `query` with `answer` and
    /// logging `default` calls), so nothing touches the real session.
    #[cfg(unix)]
    fn fake_xdg(dir: &Path, answer: &str) -> String {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join("xdg-mime");
        let log = dir.join("calls.log");
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nif [ \"$1\" = query ]; then echo '{answer}'; else echo \"$@\" >> '{}'; fi\n",
                log.display()
            ),
        )
        .expect("script");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        script.display().to_string()
    }

    #[cfg(unix)]
    #[test]
    fn registering_writes_once_and_claims_only_a_free_scheme() {
        let dir = std::env::temp_dir().join(format!("morpheus-scheme-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let exe = Path::new("/opt/morpheus/morpheus-desktop");
        let apps = dir.join("applications");
        // Free: written and made the default.
        let free = fake_xdg(&dir, "");
        assert!(register_linux(&apps, exe, &free).expect("first"));
        assert!(!register_linux(&apps, exe, &free).expect("second"), "unchanged");
        let calls = std::fs::read_to_string(dir.join("calls.log")).expect("log");
        assert!(calls.contains("default morpheus.desktop x-scheme-handler/matrix"));
        // Element's: the entry is still kept right, but the default is left.
        std::fs::remove_file(dir.join("calls.log")).expect("clear");
        let other = fake_xdg(&dir, "element-desktop.desktop");
        let moved = Path::new("/elsewhere/morpheus-desktop");
        assert!(register_linux(&apps, moved, &other).expect("moved"));
        assert!(!dir.join("calls.log").exists(), "took the default from Element");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dev_and_store_exes_are_told_apart() {
        assert_eq!(exe_kind(Path::new("/home/a/repos/morpheus/target/debug/morpheus-desktop"), false), ExeKind::Dev);
        assert_eq!(exe_kind(Path::new("/home/a/m/target/release/morpheus-desktop"), false), ExeKind::Dev);
        assert_eq!(exe_kind(Path::new("/opt/morpheus/morpheus-desktop"), true), ExeKind::Dev);
        assert_eq!(exe_kind(Path::new("/nix/store/abc-morpheus/bin/.morpheus-desktop-wrapped"), false), ExeKind::Store);
        assert_eq!(exe_kind(Path::new("/opt/morpheus/morpheus-desktop"), false), ExeKind::Installed);
        assert_eq!(exe_kind(Path::new("/home/a/.local/bin/morpheus-desktop"), false), ExeKind::Installed);
    }

    #[test]
    fn a_stale_entry_of_ours_is_recognised() {
        let dev = desktop_entry(Path::new("/home/a/repos/morpheus/target/debug/morpheus-desktop"));
        let store = desktop_entry(Path::new("/nix/store/abc-morpheus/bin/morpheus-desktop"));
        let good = desktop_entry(Path::new("/opt/morpheus/morpheus-desktop"));
        assert!(entry_is_stale(&dev));
        assert!(entry_is_stale(&store));
        assert!(!entry_is_stale(&good));
        assert!(!entry_is_stale("[Desktop Entry]\nName=Other\nExec=/x/target/debug/foo\n"));
    }

    #[cfg(unix)]
    #[test]
    fn dev_and_store_exes_write_nothing_and_clear_stale_entries() {
        let dir = std::env::temp_dir().join(format!("morpheus-scheme-skip-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let apps = dir.join("applications");
        let xdg = fake_xdg(&dir, "");
        // Dev: nothing written, nothing claimed.
        let dev = Path::new("/home/a/m/target/debug/morpheus-desktop");
        assert!(!register_for(&apps, dev, false, &xdg).expect("dev"));
        assert!(!apps.join(DESKTOP_FILE).exists());
        assert!(!dir.join("calls.log").exists());
        // Store: nothing written, default claimed for the package's entry.
        let store = Path::new("/nix/store/abc-morpheus/bin/morpheus-desktop");
        assert!(!register_for(&apps, store, false, &xdg).expect("store"));
        assert!(!apps.join(DESKTOP_FILE).exists());
        let calls = std::fs::read_to_string(dir.join("calls.log")).expect("log");
        assert!(calls.contains("default morpheus.desktop x-scheme-handler/matrix"));
        // A stale entry of ours is removed, a good one is kept.
        std::fs::create_dir_all(&apps).expect("apps");
        std::fs::write(apps.join(DESKTOP_FILE), desktop_entry(dev)).expect("stale");
        register_for(&apps, store, false, &xdg).expect("clean");
        assert!(!apps.join(DESKTOP_FILE).exists(), "stale entry kept");
        let good = Path::new("/opt/morpheus/morpheus-desktop");
        assert!(register_for(&apps, good, false, &xdg).expect("good"));
        register_for(&apps, store, false, &xdg).expect("store again");
        assert!(apps.join(DESKTOP_FILE).exists(), "a good entry was removed");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
