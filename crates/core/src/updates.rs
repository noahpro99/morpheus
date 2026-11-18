//! Updates: compare this build's version with the latest release, and on
//! desktops put the new release in place of this one.
//!
//! The releases are GitHub's (`MORPHEUS_UPDATE_FEED` names another feed,
//! read when checking, else the value the binary was built with): the
//! latest-release document, or a small `{"version": "0.2.0", "url": "…"}`.
//! A release's assets are named `morpheus-<version>-<platform>.<ext>`; the
//! one for this platform is what [`Session::install_update`] installs.

use serde::Deserialize;

use crate::{Core, CoreFuture, Error, Session};

/// Where releases are published.
const RELEASES: &str = "https://api.github.com/repos/noahpro99/morpheus/releases/latest";

/// What a check found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateStatus {
    /// This build has no feed to ask.
    NoFeed,
    UpToDate,
    Available {
        version: String,
        /// The release's page, when the feed says.
        url: Option<String>,
        /// This platform's build of it, when the release has one.
        download: Option<String>,
    },
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// GitHub's latest-release document, or the small feed.
#[derive(Deserialize)]
struct Feed {
    #[serde(alias = "tag_name")]
    version: String,
    // GitHub's document has both `url` (its API address) and `html_url` (the
    // page): read apart, as an alias of one another they were a "duplicate
    // field" and every check of a GitHub release failed.
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    assets: Vec<Asset>,
}

/// The release file the updater installs for `platform`: the zip (the apk on
/// Android), never the .dmg that sits beside it for people.
fn pick_asset(assets: &[Asset], platform: &str) -> Option<String> {
    let ext = if platform.starts_with("android") { ".apk" } else { ".zip" };
    assets
        .iter()
        .find(|asset| asset.name.contains(platform) && asset.name.ends_with(ext))
        .map(|asset| asset.browser_download_url.clone())
}

/// The name part of this platform's release asset.
fn platform() -> Option<&'static str> {
    if cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        Some("windows-x86_64")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("linux-x86_64")
    } else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        Some("macos-arm64")
    } else if cfg!(all(target_os = "macos", target_arch = "x86_64")) {
        Some("macos-x86_64")
    } else if cfg!(all(target_os = "android", target_arch = "aarch64")) {
        Some("android-arm64")
    } else {
        None
    }
}

/// Whether the app is running on NixOS or from a Nix store.
pub(crate) fn is_nixos() -> bool {
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("MORPHEUS_UPDATE_SELF_TEST").is_some() {
            return false;
        }
        std::path::Path::new("/etc/NIXOS").exists()
            || std::env::current_exe().is_ok_and(|p| p.starts_with("/nix/store"))
            || std::fs::read_to_string("/etc/os-release")
                .is_ok_and(|s| s.lines().any(|l| l.trim() == "ID=nixos"))
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Whether this build replaces itself with a downloaded release: desktops,
/// when the app's folder is the app's to change. An install it can't write
/// to (Nix's store, NixOS, a package manager's files) and phones are updated by
/// whatever installed them.
pub fn installs_updates() -> bool {
    if is_nixos() {
        return false;
    }
    #[cfg(any(target_os = "windows", target_os = "linux", target_os = "macos"))]
    {
        swap::can_replace()
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        false
    }
}

fn feed_url() -> Option<String> {
    if let Ok(url) = std::env::var("MORPHEUS_UPDATE_FEED") {
        return Some(url).filter(|url| !url.trim().is_empty());
    }
    // Test runs never ask the real releases.
    if std::env::var_os("MORPHEUS_E2E_PORT").is_some() {
        return None;
    }
    Some(
        option_env!("MORPHEUS_UPDATE_FEED")
            .unwrap_or(RELEASES)
            .to_owned(),
    )
    .filter(|url| !url.trim().is_empty())
}

/// Dotted numeric versions, compared field by field; a missing field is 0
/// and anything after a `-` or `+` (pre-release, build) is ignored.
fn is_newer(candidate: &str, current: &str) -> bool {
    fn fields(version: &str) -> Vec<u64> {
        version
            .trim()
            .trim_start_matches('v')
            .split(['-', '+'])
            .next()
            .unwrap_or_default()
            .split('.')
            .map(|field| field.parse().unwrap_or(0))
            .collect()
    }
    let (a, b) = (fields(candidate), fields(current));
    let len = a.len().max(b.len());
    let at = |v: &[u64], i: usize| v.get(i).copied().unwrap_or(0);
    (0..len)
        .map(|i| at(&a, i).cmp(&at(&b, i)))
        .find(|order| order.is_ne())
        .is_some_and(|order| order.is_gt())
}

/// Ask the release feed whether something newer than `current` exists.
async fn check(
    http: matrix_sdk::reqwest::Client,
    current: &'static str,
) -> Result<UpdateStatus, Error> {
    let Some(url) = feed_url() else {
        return Ok(UpdateStatus::NoFeed);
    };
    let failed = |err: &dyn std::fmt::Display| Error::Other(format!("update feed: {err}"));
    let mut request = http
        .get(&url)
        // GitHub turns away requests that don't say who they are.
        .header("User-Agent", "Morpheus")
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(15));
    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let response = request.send().await.map_err(|err| failed(&err))?;
    if (response.status() == matrix_sdk::reqwest::StatusCode::FORBIDDEN
        || response.status() == matrix_sdk::reqwest::StatusCode::TOO_MANY_REQUESTS)
        && url == RELEASES
    {
        return check_github_redirect(&http, current).await;
    }
    let response = response.error_for_status().map_err(|err| failed(&err))?;
    let body = response.text().await.map_err(|err| failed(&err))?;
    let feed: Feed = serde_json::from_str(&body).map_err(|err| failed(&err))?;
    if !is_newer(&feed.version, current) {
        return Ok(UpdateStatus::UpToDate);
    }
    let download = platform().and_then(|platform| pick_asset(&feed.assets, platform));
    Ok(UpdateStatus::Available {
        version: feed.version.trim_start_matches('v').to_owned(),
        url: feed.html_url.or(feed.url),
        download,
    })
}

async fn check_github_redirect(
    http: &matrix_sdk::reqwest::Client,
    current: &'static str,
) -> Result<UpdateStatus, Error> {
    let failed = |err: &dyn std::fmt::Display| Error::Other(format!("update feed: {err}"));
    let res = http
        .get("https://github.com/noahpro99/morpheus/releases/latest")
        .header("User-Agent", "Morpheus")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|err| failed(&err))?;
    let final_url = res.url().to_string();
    let tag = final_url.rsplit('/').next().unwrap_or_default();
    if tag.is_empty() || tag == "latest" {
        return Err(Error::Other("could not resolve latest release tag from GitHub redirect".into()));
    }
    if !is_newer(tag, current) {
        return Ok(UpdateStatus::UpToDate);
    }
    let version = tag.trim_start_matches('v').to_owned();
    let download = platform().map(|plat| {
        format!("https://github.com/noahpro99/morpheus/releases/download/{tag}/morpheus-{version}-{plat}.zip")
    });
    Ok(UpdateStatus::Available {
        version,
        url: Some(final_url),
        download,
    })
}

/// Download the release at `download` and put it in place of the running app.
async fn install(http: matrix_sdk::reqwest::Client, download: String) -> Result<(), Error> {
    let failed = |err: &dyn std::fmt::Display| Error::Other(format!("update: {err}"));
    let bytes = http
        .get(&download)
        .header("User-Agent", "Morpheus")
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|err| failed(&err))?
        .bytes()
        .await
        .map_err(|err| failed(&err))?;
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    let installed = tokio::task::spawn_blocking(move || swap::install(&bytes))
        .await
        .map_err(|err| failed(&err))?
        .map_err(|err| failed(&err));
    // Phones install through the system, never in place.
    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
    let installed = {
        drop(bytes);
        Err(failed(&"this device installs updates itself"))
    };
    installed
}

/// A client that gives up after `secs`; the browser's fetch has no timeout to set.
fn client_with_timeout(secs: u64) -> Option<matrix_sdk::reqwest::Client> {
    #[cfg(not(target_arch = "wasm32"))]
    return matrix_sdk::reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(secs))
        .build()
        .ok();
    #[cfg(target_arch = "wasm32")]
    {
        let _ = secs;
        None
    }
}

impl Session {
    /// Ask the release feed whether something newer than `current` exists.
    pub fn check_for_update(&self, current: &'static str) -> CoreFuture<UpdateStatus> {
        let http = client_with_timeout(15).unwrap_or_else(|| self.client.http_client().clone());
        self.core.run(check(http, current))
    }

    /// Download the release at `download` (this platform's zip) and put it
    /// in place of the running app, then start it (it waits for this one to
    /// quit). The caller quits once this returns. Desktops only.
    pub fn install_update(&self, download: String) -> CoreFuture<()> {
        let http = client_with_timeout(60).unwrap_or_else(|| self.client.http_client().clone());
        self.core.run(install(http, download))
    }
}

/// The same two steps with no signed-in session (the updater's self-test
/// runs them with no account and no window).
impl Core {
    pub fn check_for_update(&self, current: &'static str) -> CoreFuture<UpdateStatus> {
        self.run(check(matrix_sdk::reqwest::Client::new(), current))
    }

    pub fn install_update(&self, download: String) -> CoreFuture<()> {
        self.run(install(matrix_sdk::reqwest::Client::new(), download))
    }
}

/// Left behind by the last update: the old version's files (which a
/// running program can't always delete) and the unpacked download.
pub fn clean_up_after_update() {
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    swap::clean_up();
}

/// Where a Windows install keeps its parts (the other desktops are one
/// program, or one bundle, and are untouched by this).
///
/// A small launcher, `Morpheus.exe`, is what the taskbar pin, the Start menu
/// shortcut, the registry's `matrix:` handler and start on login all point
/// at; updates never touch it. The app is `app\morpheus-desktop.exe` beside
/// it, and an update swaps only what is in `app\`.
pub mod layout {
    use std::path::{Path, PathBuf};

    /// The launcher's file name, at the top of an install.
    pub const LAUNCHER: &str = "Morpheus.exe";
    /// The folder of the app.
    pub const APP_DIR: &str = "app";
    /// The app's program, in [`APP_DIR`].
    pub const APP_EXE: &str = "morpheus-desktop.exe";

    /// How an update is put in place.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum Kind {
        /// The install has a launcher: swap only the files of `app\`, then
        /// start the launcher.
        AppOnly { root: PathBuf },
        /// The old single-program install gets the release as it is, so the
        /// launcher takes the program's place and `app\` arrives beside it.
        Migrate,
        /// One program (or bundle) replaced with the release's.
        Flat,
        /// An install with a launcher and a release without an `app\`:
        /// nothing is done, rather than break the install.
        Refuse,
    }

    /// The top of an install whose app is `exe` (that is, `exe` sits in
    /// `<root>\app\`), by the names alone.
    pub fn launcher_root(exe: &Path) -> Option<&Path> {
        let dir = exe.parent()?;
        if dir.file_name()? != APP_DIR {
            return None;
        }
        dir.parent()
    }

    /// The launcher of the install `exe` is the app of, when it is there.
    pub fn launcher_for(exe: &Path) -> Option<PathBuf> {
        let launcher = launcher_root(exe)?.join(LAUNCHER);
        launcher.is_file().then_some(launcher)
    }

    /// What starts this app: the launcher of its install, or itself. This
    /// is what the registry and start on login point at.
    pub fn front_exe() -> std::io::Result<PathBuf> {
        let exe = std::env::current_exe()?;
        Ok(launcher_for(&exe).unwrap_or(exe))
    }

    /// The stable install folder under the user's local app data, where the
    /// launcher puts the first run's copy (the launcher crate has its own
    /// copy of this; it has no dependencies).
    pub fn stable_dir(local_app_data: &Path) -> PathBuf {
        local_app_data.join("Programs").join("Morpheus")
    }

    /// The Start menu shortcut, in the user's Roaming app data.
    pub fn start_menu_shortcut(app_data: &Path) -> PathBuf {
        app_data
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("Morpheus.lnk")
    }

    /// Whether `launcher` is the one of the stable install (case ignored, as
    /// Windows does): only that one gets a Start menu shortcut, not an unpacked
    /// download, a development build or the updater's test install.
    pub fn is_stable_launcher(launcher: &Path, local_app_data: &Path) -> bool {
        let fold = |p: &Path| p.to_string_lossy().replace('/', "\\").to_lowercase();
        launcher
            .parent()
            .is_some_and(|dir| fold(dir) == fold(&stable_dir(local_app_data)))
            && launcher.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(LAUNCHER))
    }

    /// Whether an unpacked release has the app in `app\`.
    pub fn release_has_app(top: &Path) -> bool {
        top.join(APP_DIR).join(APP_EXE).is_file()
    }

    /// How to apply a release (`has_app`: it has `app\`) over the app `exe`.
    pub fn kind(exe: &Path, has_app: bool) -> Kind {
        match (launcher_for(exe), has_app) {
            (Some(_), true) => Kind::AppOnly {
                root: launcher_root(exe).map(Path::to_owned).unwrap_or_default(),
            },
            (Some(_), false) => Kind::Refuse,
            (None, true) => Kind::Migrate,
            (None, false) => Kind::Flat,
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn install(dir: &Path) -> PathBuf {
            std::fs::create_dir_all(dir.join(APP_DIR)).unwrap();
            std::fs::write(dir.join(LAUNCHER), b"launcher").unwrap();
            let exe = dir.join(APP_DIR).join(APP_EXE);
            std::fs::write(&exe, b"app").unwrap();
            exe
        }

        #[test]
        fn the_launcher_is_found_beside_the_app_folder() {
            let dir = tempfile::tempdir().unwrap();
            let exe = install(dir.path());
            assert_eq!(launcher_root(&exe), Some(dir.path()));
            assert_eq!(launcher_for(&exe), Some(dir.path().join(LAUNCHER)));
        }

        #[test]
        fn a_program_outside_an_app_folder_is_its_own_front() {
            let dir = tempfile::tempdir().unwrap();
            let exe = dir.path().join("Morpheus.exe");
            std::fs::write(&exe, b"old single program").unwrap();
            assert_eq!(launcher_root(&exe), None);
            assert_eq!(launcher_for(&exe), None);
            // An app folder with no launcher beside it is no install either.
            std::fs::create_dir_all(dir.path().join("x").join(APP_DIR)).unwrap();
            let lone = dir.path().join("x").join(APP_DIR).join(APP_EXE);
            assert_eq!(launcher_for(&lone), None);
        }

        #[test]
        fn a_release_has_an_app_when_its_program_is_in_app() {
            let dir = tempfile::tempdir().unwrap();
            assert!(!release_has_app(dir.path()));
            install(dir.path());
            assert!(release_has_app(dir.path()));
        }

        #[test]
        fn updates_replace_only_the_app_of_an_install_with_a_launcher() {
            let dir = tempfile::tempdir().unwrap();
            let exe = install(dir.path());
            assert_eq!(kind(&exe, true), Kind::AppOnly { root: dir.path().to_owned() });
            assert_eq!(kind(&exe, false), Kind::Refuse);
        }

        #[test]
        fn only_the_stable_installs_launcher_gets_the_start_menu_shortcut() {
            let local = Path::new(r"C:\Users\a\AppData\Local");
            assert_eq!(stable_dir(local), local.join("Programs").join("Morpheus"));
            let stable = stable_dir(local).join(LAUNCHER);
            assert!(is_stable_launcher(&stable, local));
            assert!(!is_stable_launcher(Path::new("/d/Downloads/Morpheus/Morpheus.exe"), local));
            assert!(!is_stable_launcher(&stable_dir(local).join("app").join(APP_EXE), local));
            let roaming = Path::new("/r/AppData/Roaming");
            assert!(start_menu_shortcut(roaming).ends_with("Start Menu/Programs/Morpheus.lnk"));
        }

        #[test]
        fn the_old_single_program_migrates_to_the_launcher_layout() {
            let dir = tempfile::tempdir().unwrap();
            let exe = dir.path().join("Morpheus.exe");
            std::fs::write(&exe, b"old").unwrap();
            assert_eq!(kind(&exe, true), Kind::Migrate);
            // A release of the old shape is a plain swap.
            assert_eq!(kind(&exe, false), Kind::Flat);
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
mod swap {
    use std::io;
    use std::path::{Path, PathBuf};

    const STAGING: &str = ".morpheus-update";
    const OLD: &str = ".old";

    /// What is replaced: the app's folder (its program and assets beside
    /// it), or on a Mac the `.app` bundle in its folder.
    fn place() -> io::Result<(PathBuf, PathBuf)> {
        let exe = std::env::current_exe()?;
        // With a launcher (Windows) the folder is the install's top, and the
        // app is the `app\` in it.
        if let Some(root) = super::layout::launcher_root(&exe)
            && super::layout::launcher_for(&exe).is_some()
        {
            return Ok((exe.clone(), root.to_owned()));
        }
        if cfg!(target_os = "macos")
            && let Some(bundle) = exe
                .ancestors()
                .find(|p| p.extension().is_some_and(|e| e == "app"))
        {
            let dir = bundle.parent().unwrap_or(Path::new("/")).to_owned();
            return Ok((exe.clone(), dir));
        }
        let dir = exe
            .parent()
            .ok_or_else(|| io::Error::other("the app has no folder"))?
            .to_owned();
        Ok((exe, dir))
    }

    /// Move `from` to `to`, keeping what was at `to` as `to.old`.
    fn replace(from: &Path, to: &Path) -> io::Result<()> {
        let mut old = to.as_os_str().to_owned();
        old.push(OLD);
        let old = PathBuf::from(old);
        if old.is_dir() {
            std::fs::remove_dir_all(&old)?;
        } else if old.exists() {
            std::fs::remove_file(&old)?;
        }
        if to.exists() {
            std::fs::rename(to, &old)?;
        }
        if let Err(err) = std::fs::rename(from, to) {
            // Put the old one back rather than leave nothing.
            let _ = std::fs::rename(&old, to);
            return Err(err);
        }
        Ok(())
    }

    pub fn install(zip: &[u8]) -> io::Result<()> {
        if super::is_nixos() {
            return Err(io::Error::other(
                "auto-updates are not supported on NixOS; update via your package manager or Nix flake",
            ));
        }
        let (exe, dir) = place()?;
        let staging = dir.join(STAGING);
        let _ = std::fs::remove_dir_all(&staging);
        std::fs::create_dir_all(&staging)?;
        zip::ZipArchive::new(io::Cursor::new(zip))
            .and_then(|mut archive| archive.extract(&staging))
            .map_err(io::Error::other)?;
        // A release is one folder holding the app; or the app at the top.
        let entries: Vec<PathBuf> = std::fs::read_dir(&staging)?
            .flatten()
            .map(|e| e.path())
            .collect();
        let top = match entries.as_slice() {
            [only] if only.is_dir() && only.extension().is_none_or(|e| e != "app") => only.clone(),
            _ => staging.clone(),
        };
        let release_kind = super::layout::kind(&exe, super::layout::release_has_app(&top));
        let started = if let super::layout::Kind::AppOnly { root } = &release_kind {
            // The launcher is not touched: only what is in `app\`.
            let app = root.join(super::layout::APP_DIR);
            for entry in std::fs::read_dir(top.join(super::layout::APP_DIR))?.flatten() {
                replace(&entry.path(), &app.join(entry.file_name()))?;
            }
            root.join(super::layout::LAUNCHER)
        } else if release_kind == super::layout::Kind::Refuse {
            let _ = std::fs::remove_dir_all(&staging);
            return Err(io::Error::other("the release has no app folder for this install"));
        } else if cfg!(target_os = "macos")
            && let Some(bundle) = exe
                .ancestors()
                .find(|p| p.extension().is_some_and(|e| e == "app"))
        {
            let new_bundle = std::fs::read_dir(&top)?
                .flatten()
                .map(|e| e.path())
                .find(|p| p.extension().is_some_and(|e| e == "app"))
                .ok_or_else(|| io::Error::other("no app in the release"))?;
            let inside = exe
                .strip_prefix(bundle)
                .map_err(io::Error::other)?
                .to_owned();
            replace(&new_bundle, bundle)?;
            bundle.join(inside)
        } else {
            let name = exe
                .file_name()
                .ok_or_else(|| io::Error::other("the app has no name"))?;
            let new_exe = [
                name.to_owned(),
                "Morpheus.exe".into(),
                "Morpheus".into(),
                "morpheus-desktop".into(),
            ]
            .iter()
            .map(|n| top.join(n))
            .find(|p| p.is_file())
            .ok_or_else(|| io::Error::other("no program in the release"))?;
            replace(&new_exe, &exe)?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755))?;
            }
            // Its assets and anything else beside it.
            for entry in std::fs::read_dir(&top)?.flatten() {
                replace(&entry.path(), &dir.join(entry.file_name()))?;
            }
            exe.clone()
        };
        let _ = std::fs::remove_dir_all(&staging);
        tracing::info!(program = %started.display(), "update in place; starting it");
        std::process::Command::new(&started)
            .args(std::env::args_os().skip(1))
            .env("MORPHEUS_UPDATED_FROM", std::process::id().to_string())
            .spawn()?;
        Ok(())
    }

    /// Whether the app's folder takes new files (tried, as permissions
    /// alone don't say: a read-only mount, or Windows' own rules).
    pub fn can_replace() -> bool {
        if super::is_nixos() {
            return false;
        }
        let Ok((_, dir)) = place() else {
            return false;
        };
        let probe = dir.join(format!("{STAGING}-probe-{}", std::process::id()));
        let ok = std::fs::write(&probe, b"").is_ok();
        let _ = std::fs::remove_file(&probe);
        ok
    }

    pub fn clean_up() {
        let Ok((_, dir)) = place() else {
            return;
        };
        let _ = std::fs::remove_dir_all(dir.join(STAGING));
        // With a launcher, the install's top and its `app\` both hold
        // leftovers (the old single program's file, the old app's).
        for folder in [dir.clone(), dir.join(super::layout::APP_DIR)] {
            for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.to_string_lossy().ends_with(OLD) {
                    let _ = if path.is_dir() {
                        std::fs::remove_dir_all(&path)
                    } else {
                        std::fs::remove_file(&path)
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::is_newer;

    #[test]
    #[cfg(target_os = "linux")]
    fn nixos_detection_matches_system() {
        if std::path::Path::new("/etc/NIXOS").exists() {
            assert!(super::is_nixos());
            assert!(!super::installs_updates());
        }
    }

    /// GitHub's real latest-release document (v0.2.1's) reads: its version, its
    /// page and the Windows zip. It has `url` and `html_url` both, which
    /// made every check fail ("duplicate field `url`") before.
    #[test]
    fn githubs_release_document_reads() {
        let body = include_str!("../fixtures/github-latest-release.json");
        let feed: super::Feed = serde_json::from_str(body).expect("GitHub's document reads");
        assert_eq!(feed.version, "v0.2.1");
        assert_eq!(feed.html_url.as_deref(), Some("https://github.com/noahpro99/morpheus/releases/tag/v0.2.1"));
        assert!(feed.assets.iter().any(|a| a.name == "morpheus-0.2.1-windows-x86_64.zip"));
    }

    #[test]
    fn the_zip_is_picked_over_a_dmg_listed_first() {
        let body = include_str!("../fixtures/github-latest-release.json");
        let mut feed: super::Feed = serde_json::from_str(body).unwrap();
        feed.assets.insert(
            0,
            super::Asset {
                name: "morpheus-0.2.1-macos-arm64.dmg".into(),
                browser_download_url: "https://example.test/morpheus-0.2.1-macos-arm64.dmg".into(),
            },
        );
        let url = super::pick_asset(&feed.assets, "macos-arm64").unwrap();
        assert!(url.ends_with(".zip"), "{url}");
        let apk = super::pick_asset(&feed.assets, "android-arm64").unwrap();
        assert!(apk.ends_with(".apk"), "{apk}");
    }

    #[test]
    fn versions_compare_by_field() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.99.99"));
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1", "0.1.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("0.1.0-beta.2", "0.1.0"));
    }

    #[test]
    fn parse_tag_from_github_release_url() {
        let final_url = "https://github.com/noahpro99/morpheus/releases/tag/v0.2.2";
        let tag = final_url.rsplit('/').next().unwrap_or_default();
        assert_eq!(tag, "v0.2.2");
        assert!(is_newer(tag, "0.2.0"));
        assert!(!is_newer(tag, "0.2.2"));
        let version = tag.trim_start_matches('v');
        assert_eq!(version, "0.2.2");
    }
}

/// A release's notes: its markdown body and its page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseNotes {
    pub body: String,
    pub url: Option<String>,
}

#[derive(Deserialize)]
struct ReleaseDoc {
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: Option<String>,
}

/// Where the notes of `version` are: `MORPHEUS_RELEASE_NOTES_URL` (read when
/// asked), else GitHub's release by tag. Test runs never ask GitHub.
fn notes_url(version: &str) -> Option<String> {
    if let Ok(url) = std::env::var("MORPHEUS_RELEASE_NOTES_URL") {
        return Some(url).filter(|url| !url.trim().is_empty());
    }
    if std::env::var_os("MORPHEUS_E2E_PORT").is_some() {
        return None;
    }
    Some(format!(
        "https://api.github.com/repos/noahpro99/morpheus/releases/tags/v{version}"
    ))
}

async fn fetch_notes(
    http: matrix_sdk::reqwest::Client,
    version: String,
) -> Result<ReleaseNotes, Error> {
    let failed = |err: &dyn std::fmt::Display| Error::Other(format!("release notes: {err}"));
    let Some(url) = notes_url(&version) else {
        return Err(failed(&"no release feed"));
    };
    let mut request = http
        .get(&url)
        .header("User-Agent", "Morpheus")
        .header("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10));
    if let Ok(token) = std::env::var("GITHUB_TOKEN").or_else(|_| std::env::var("GH_TOKEN")) {
        request = request.header("Authorization", format!("Bearer {token}"));
    }
    let body = request
        .send()
        .await
        .and_then(|response| response.error_for_status())
        .map_err(|err| failed(&err))?
        .text()
        .await
        .map_err(|err| failed(&err))?;
    let doc: ReleaseDoc = serde_json::from_str(&body).map_err(|err| failed(&err))?;
    let body = doc.body.unwrap_or_default();
    if body.trim().is_empty() {
        return Err(failed(&"the release has no notes"));
    }
    Ok(ReleaseNotes {
        body,
        url: doc.html_url,
    })
}

impl Session {
    /// The GitHub release notes of `version` (short timeout; no network or
    /// no such release is an error).
    pub fn release_notes(&self, version: &str) -> CoreFuture<ReleaseNotes> {
        let http = client_with_timeout(10).unwrap_or_else(|| self.client.http_client().clone());
        self.core.run(fetch_notes(http, version.to_owned()))
    }
}
