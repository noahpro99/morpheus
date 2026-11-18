//! `Morpheus.exe` on Windows: the program the taskbar pin, the Start menu
//! shortcut, the `matrix:` handler and start on login point at. It runs
//! `app\morpheus-desktop.exe` with its own arguments and ends. An update
//! replaces what is in `app\` and never this file, so a pin to it survives
//! updates (a pin that followed the running program's rename to `.old` broke).
//!
//! The first time it runs from somewhere other than the stable install
//! folder (`%LOCALAPPDATA%\Programs\Morpheus`, typically from an unpacked
//! download), it copies itself and `app\` there and starts that copy. The app
//! makes the Start menu shortcut from there.
//!
//! No dependencies and no window (the Windows subsystem): nothing here can
//! show an error, so a failure is a nonzero exit code.
//! `MORPHEUS_LAUNCHER_WAIT=1` makes it wait for the app and end with the
//! app's exit code (the updater's self-test uses it).

#![cfg_attr(windows, windows_subsystem = "windows")]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

const APP_DIR: &str = "app";
const APP_EXE: &str = "morpheus-desktop.exe";
const LAUNCHER: &str = "Morpheus.exe";

/// The app of the install whose launcher is in `root`.
fn app_exe(root: &Path) -> PathBuf {
    root.join(APP_DIR).join(APP_EXE)
}

/// The stable install folder, under the user's local app data.
fn stable_dir(local_app_data: &Path) -> PathBuf {
    local_app_data.join("Programs").join("Morpheus")
}

/// A folder's name for comparing: case folded (Windows ignores case),
/// `\\?\` prefix and trailing separators dropped.
fn comparable(path: &Path) -> String {
    let text = path.to_string_lossy().replace('/', "\\");
    let text = text.strip_prefix(r"\\?\").unwrap_or(&text);
    text.trim_end_matches('\\').to_lowercase()
}

/// What to start.
#[derive(Debug, PartialEq, Eq)]
enum Plan {
    /// The app beside this launcher.
    Here,
    /// The app of the install already in the stable folder.
    Stable,
    /// Copy this install to the stable folder first, then start that.
    Install,
}

/// Where to start the app from: from here when this is the stable install,
/// when installing is off (tests, the updater's self-test, a development
/// build with no `app\`), or when there is no stable folder; from the stable
/// install when it exists (it keeps itself updated); otherwise install.
fn plan(root: &Path, stable: Option<&Path>, stable_has_app: bool, installing_off: bool) -> Plan {
    let Some(stable) = stable else {
        return Plan::Here;
    };
    if installing_off || comparable(root) == comparable(stable) {
        return Plan::Here;
    }
    if stable_has_app {
        Plan::Stable
    } else {
        Plan::Install
    }
}

/// Copy a folder with everything in it.
fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Put the install at `root` (its launcher `exe` and `app\`) in `stable`: built
/// in a sibling folder and renamed into place, so a failed copy leaves no
/// half install for the next run to take as one.
fn install(exe: &Path, root: &Path, stable: &Path) -> std::io::Result<()> {
    let parent = stable
        .parent()
        .ok_or_else(|| std::io::Error::other("no folder to install in"))?;
    std::fs::create_dir_all(parent)?;
    let partial = parent.join("Morpheus.installing");
    let _ = std::fs::remove_dir_all(&partial);
    let copied = copy_dir(&root.join(APP_DIR), &partial.join(APP_DIR))
        .and_then(|()| std::fs::copy(exe, partial.join(LAUNCHER)).map(|_| ()));
    if let Err(err) = copied {
        let _ = std::fs::remove_dir_all(&partial);
        return Err(err);
    }
    // A folder left by an earlier failed install (no app in it) is replaced.
    let _ = std::fs::remove_dir_all(stable);
    std::fs::rename(&partial, stable)
}

fn run() -> std::io::Result<i32> {
    let exe = std::env::current_exe()?;
    let root = exe
        .parent()
        .ok_or_else(|| std::io::Error::other("the launcher has no folder"))?
        .to_owned();
    let stable = std::env::var_os("LOCALAPPDATA").map(|dir| stable_dir(Path::new(&dir)));
    let off = ["MORPHEUS_NO_INSTALL", "MORPHEUS_UPDATE_SELF_TEST", "MORPHEUS_E2E_PORT"]
        .iter()
        .any(|name| std::env::var_os(name).is_some())
        || !app_exe(&root).is_file();
    let stable_has_app = stable.as_deref().is_some_and(|s| app_exe(s).is_file());
    let app = match plan(&root, stable.as_deref(), stable_has_app, off) {
        Plan::Here => app_exe(&root),
        Plan::Stable => stable.as_deref().map(app_exe).unwrap_or_else(|| app_exe(&root)),
        Plan::Install => match stable.as_deref() {
            // Not installable (a read-only profile?): run from where it is.
            Some(stable) if install(&exe, &root, stable).is_ok() => app_exe(stable),
            _ => app_exe(&root),
        },
    };
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    let mut child = Command::new(&app).args(args).spawn()?;
    if std::env::var_os("MORPHEUS_LAUNCHER_WAIT").is_some() {
        return Ok(child.wait()?.code().unwrap_or(1));
    }
    Ok(0)
}

fn main() {
    match run() {
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("Morpheus: starting the app failed: {err}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_is_in_the_app_folder_beside_the_launcher() {
        assert_eq!(
            app_exe(Path::new("/x/Morpheus")),
            Path::new("/x/Morpheus/app/morpheus-desktop.exe")
        );
    }

    #[test]
    fn the_stable_folder_is_under_programs_in_local_app_data() {
        assert_eq!(
            stable_dir(Path::new("/u/AppData/Local")),
            Path::new("/u/AppData/Local/Programs/Morpheus")
        );
    }

    #[test]
    fn folders_compare_by_case_prefix_and_trailing_slash() {
        let a = comparable(Path::new(r"C:\Users\A\AppData\Local\Programs\Morpheus"));
        let b = comparable(Path::new(r"\\?\c:\users\a\appdata\local\programs\morpheus\"));
        assert_eq!(a, b);
        assert_ne!(a, comparable(Path::new(r"C:\Users\A\Downloads\Morpheus")));
    }

    #[test]
    fn it_starts_from_here_unless_there_is_a_stable_install_to_use_or_make() {
        let download = Path::new("/d/Morpheus");
        let stable = Path::new("/l/Programs/Morpheus");
        // First run from a download: install.
        assert_eq!(plan(download, Some(stable), false, false), Plan::Install);
        // Already installed: use that one (it updates itself).
        assert_eq!(plan(download, Some(stable), true, false), Plan::Stable);
        // This is the stable install.
        assert_eq!(plan(stable, Some(stable), true, false), Plan::Here);
        // Installing switched off, or nowhere to install.
        assert_eq!(plan(download, Some(stable), false, true), Plan::Here);
        assert_eq!(plan(download, None, false, false), Plan::Here);
    }

    #[test]
    fn installing_copies_the_launcher_and_app_and_leaves_no_partial_folder() {
        let tmp = tempdir();
        let root = tmp.join("download");
        std::fs::create_dir_all(root.join("app/fonts")).unwrap();
        std::fs::write(root.join(LAUNCHER), b"launcher").unwrap();
        std::fs::write(root.join("app").join(APP_EXE), b"app").unwrap();
        std::fs::write(root.join("app/fonts/a.ttf"), b"font").unwrap();
        let stable = tmp.join("local/Programs/Morpheus");
        install(&root.join(LAUNCHER), &root, &stable).unwrap();
        assert_eq!(std::fs::read(stable.join(LAUNCHER)).unwrap(), b"launcher");
        assert_eq!(std::fs::read(stable.join("app").join(APP_EXE)).unwrap(), b"app");
        assert_eq!(std::fs::read(stable.join("app/fonts/a.ttf")).unwrap(), b"font");
        assert!(!tmp.join("local/Programs/Morpheus.installing").exists());
        // Missing source: an error, and nothing half-installed.
        let none = tmp.join("empty");
        std::fs::create_dir_all(&none).unwrap();
        let stable2 = tmp.join("local2/Programs/Morpheus");
        assert!(install(&none.join(LAUNCHER), &none, &stable2).is_err());
        assert!(!stable2.exists());
        assert!(!tmp.join("local2/Programs/Morpheus.installing").exists());
    }

    fn tempdir() -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "morpheus-launcher-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
