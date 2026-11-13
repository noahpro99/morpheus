//! The updater's self-test (`scripts/update-self-test.sh`, run by CI on each
//! platform): the app, started with `MORPHEUS_UPDATE_SELF_TEST=1`, opens no
//! window and signs in nowhere. It pretends to be an old version, asks the
//! feed in `MORPHEUS_UPDATE_FEED` for an update, installs it exactly as the
//! Update button does and ends; the program it started (the new one, told
//! `MORPHEUS_UPDATED_FROM`) cleans up what the update left, writes the file
//! named by `MORPHEUS_UPDATE_SELF_TEST_MARKER` and ends.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use morpheus_core::{Core, UpdateStatus};

/// Whether this run is the self-test; nothing else turns it on.
pub fn requested() -> bool {
    std::env::var("MORPHEUS_UPDATE_SELF_TEST").is_ok_and(|v| v == "1")
}

/// What the old version calls itself, so any release is newer.
const PRETEND_VERSION: &str = "0.0.1";

/// Run the self-test; the process exit code is its result.
pub fn run(data_dir: PathBuf) -> ! {
    let result = if std::env::var_os("MORPHEUS_UPDATED_FROM").is_some() {
        after_update()
    } else {
        update(data_dir)
    };
    match result {
        Ok(()) => std::process::exit(0),
        Err(err) => {
            eprintln!("morpheus-desktop: update self-test failed: {err}");
            std::process::exit(1);
        }
    }
}

/// The old version: find the release and put it in place.
fn update(data_dir: PathBuf) -> Result<(), String> {
    let core = Core::new(data_dir).map_err(|err| err.to_string())?;
    let status = futures::executor::block_on(core.check_for_update(PRETEND_VERSION))
        .map_err(|err| err.to_string())?;
    let UpdateStatus::Available { version, download: Some(download), .. } = status else {
        return Err(format!("no update offered for this platform: {status:?}"));
    };
    eprintln!("update self-test: {PRETEND_VERSION} sees {version}, installing {download}");
    futures::executor::block_on(core.install_update(download)).map_err(|err| err.to_string())?;
    eprintln!("update self-test: installed, the new version is starting");
    Ok(())
}

/// The new version: the old one is going; clean up once it has let go of its
/// files (Windows keeps a running program's file), then say so.
fn after_update() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    // What the update replaces: the app's folder, or a Mac bundle's.
    let bundle = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app"));
    let dir = match (cfg!(target_os = "macos"), bundle) {
        (true, Some(bundle)) => bundle.parent(),
        _ => exe.parent(),
    }
    .ok_or("no folder")?
    .to_owned();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        morpheus_core::clean_up_after_update();
        let left = std::fs::read_dir(&dir)
            .map_err(|err| err.to_string())?
            .flatten()
            .any(|e| e.file_name().to_string_lossy().ends_with(".old"));
        if !left {
            break;
        }
        if Instant::now() > deadline {
            return Err("the old version's files are still there".into());
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    let marker = std::env::var_os("MORPHEUS_UPDATE_SELF_TEST_MARKER").ok_or("no marker path")?;
    let from = std::env::var("MORPHEUS_UPDATED_FROM").unwrap_or_default();
    std::fs::write(&marker, format!("updated from process {from}; now {}\n", exe.display()))
        .map_err(|err| err.to_string())
}
