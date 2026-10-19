//! Drive the real Morpheus desktop binary the way Playwright drives a browser.
//!
//! The app is built with the `e2e` feature (`just e2e-build`), which opens a
//! loopback control socket that evaluates JavaScript inside the webview.
//! [`App`] launches the binary with its own data directory, talks to that
//! socket, and offers selector-based helpers. Tests live in `tests/`.

#![allow(clippy::expect_used)]

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    net::{
        TcpListener, TcpStream,
        tcp::{OwnedReadHalf, OwnedWriteHalf},
    },
    process::{Child, Command},
    time::sleep,
};

const LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);
const POLL: Duration = Duration::from_millis(100);

/// Default wait for elements to appear; sync against a local homeserver is fast.
pub const DEFAULT_WAIT: Duration = Duration::from_secs(30);

/// A running instance of the app under test. Killed on drop.
pub struct App {
    /// The desktop process; `None` for an app driven on an Android device.
    child: Option<Child>,
    /// Driving the mobile app on the device `adb` talks to.
    android: bool,
    reader: Lines<BufReader<OwnedReadHalf>>,
    writer: OwnedWriteHalf,
    next_id: u64,
    data_dir: PathBuf,
    download_dir: PathBuf,
    port: u16,
    log_path: PathBuf,
    homeserver: String,
    /// `(what, elapsed)` for every wait, printed when the app is dropped.
    timings: Vec<(String, Duration)>,
}

#[derive(Deserialize)]
struct Response {
    id: u64,
    ok: bool,
    #[serde(default)]
    value: Value,
    #[serde(default)]
    error: Option<String>,
}

impl App {
    /// Launch a fresh app (empty data dir) pointed at `homeserver`.
    pub async fn launch(homeserver: &str) -> Result<Self> {
        if let Some(app) = Self::try_phone().await? {
            return Ok(app);
        }
        let data_dir = std::env::temp_dir().join(format!("morpheus-e2e-{}", unique()));
        Self::launch_with_data_dir(homeserver, data_dir, None, &[]).await
    }

    /// Launch a fresh app with extra environment variables, e.g. to turn on
    /// something test runs leave off.
    pub async fn launch_with_env(homeserver: &str, env: &[(&str, &str)]) -> Result<Self> {
        let data_dir = std::env::temp_dir().join(format!("morpheus-e2e-{}", unique()));
        Self::launch_with_data_dir(homeserver, data_dir, None, env).await
    }

    /// The app's process ID, for finding it on the session bus.
    pub fn pid(&self) -> Option<u32> {
        self.child.as_ref().and_then(Child::id)
    }

    /// Launch with a window of `width` x `height` logical pixels, e.g. a
    /// phone-sized one for the narrow layout.
    pub async fn launch_sized(homeserver: &str, width: u32, height: u32) -> Result<Self> {
        // A phone is phone-sized; a test that asks for a wide window wants a desktop.
        if width < 900
            && let Some(app) = Self::try_phone().await?
        {
            return Ok(app);
        }
        let data_dir = std::env::temp_dir().join(format!("morpheus-e2e-{}", unique()));
        Self::launch_with_data_dir(homeserver, data_dir, Some((width, height)), &[]).await
    }

    /// Relaunch on the same data directory, e.g. to test session restore.
    pub async fn relaunch(mut self) -> Result<Self> {
        if self.android {
            // Same app data; `Drop` stops the app and frees the phone.
            drop(self);
            PHONE_BUSY.store(true, Ordering::SeqCst);
            return Self::start_android(false, &[]).await;
        }
        let data_dir = self.data_dir.clone();
        let homeserver = self.homeserver.clone();
        if let Some(child) = &mut self.child {
            let _ = child.kill().await;
        }
        // Keep the data dir: `Drop` would delete it.
        std::mem::forget(self);
        Self::launch_with_data_dir(&homeserver, data_dir, None, &[]).await
    }

    async fn launch_with_data_dir(
        homeserver: &str,
        data_dir: PathBuf,
        size: Option<(u32, u32)>,
        env: &[(&str, &str)],
    ) -> Result<Self> {
        keep_test_windows_unfocused();
        std::fs::create_dir_all(&data_dir)?;
        // A disposable, test-local stand-in for the real Downloads directory
        // (see `MORPHEUS_DOWNLOAD_DIR` in `morpheus-core`), so downloads can
        // be asserted on without touching the machine's real one.
        let download_dir = data_dir.join("downloads");
        std::fs::create_dir_all(&download_dir)?;
        let port = free_port().await?;
        let bin = binary_path()?;
        let log_path = data_dir.join("app.log");
        let log = std::fs::File::create(&log_path)?;

        let mut command = Command::new(&bin);
        command.envs(env.iter().copied());
        if let Some((width, height)) = size {
            command.env("MORPHEUS_E2E_SIZE", format!("{width}x{height}"));
        }
        let child = command
            .env("MORPHEUS_E2E_PORT", port.to_string())
            .env("MORPHEUS_DATA_DIR", &data_dir)
            .env("MORPHEUS_DOWNLOAD_DIR", &download_dir)
            .env("MORPHEUS_HOMESERVER", homeserver)
            // Never touch the developer's real keyring from a test run.
            .env("MORPHEUS_SESSION_STORE", "file")
            .env(
                "RUST_LOG",
                std::env::var("RUST_LOG").unwrap_or_else(|_| "morpheus=debug,warn".into()),
            )
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .kill_on_drop(true)
            .spawn()
            .with_context(|| format!("spawning {}", bin.display()))?;

        let stream = connect(port, &log_path).await?;
        let (reader, writer) = stream.into_split();
        Ok(Self {
            child: Some(child),
            android: false,
            reader: BufReader::new(reader).lines(),
            writer,
            next_id: 1,
            data_dir,
            download_dir,
            log_path,
            port,
            homeserver: homeserver.to_owned(),
            timings: Vec::new(),
        })
    }

    /// Whether the app process is still alive.
    pub fn is_running(&mut self) -> bool {
        if self.android {
            return adb(&["shell", "pidof", ANDROID_PACKAGE])
                .is_some_and(|pid| !pid.trim().is_empty());
        }
        self.child
            .as_mut()
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    /// Show the window again after it was closed (hidden) through the title
    /// bar, the way the tray does: the desktop app listens on the port after
    /// the bridge's for exactly this, since a hidden webview may not answer
    /// through the bridge.
    pub async fn show_window(&mut self) -> Result<()> {
        let _ = tokio::net::TcpStream::connect(("127.0.0.1", self.port + 1))
            .await
            .context("show-window control port")?;
        sleep(Duration::from_millis(800)).await;
        Ok(())
    }

    /// Evaluate JavaScript in the webview. Use `return` to get a value back.
    pub async fn eval(&mut self, js: &str) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut request = json!({ "id": id, "js": js }).to_string();
        request.push('\n');
        self.writer.write_all(request.as_bytes()).await?;

        let line = self
            .reader
            .next_line()
            .await?
            .ok_or_else(|| anyhow!("app closed the bridge"))?;
        let response: Response = serde_json::from_str(&line).context("decoding bridge response")?;
        if response.id != id {
            bail!(
                "bridge answered request {} while waiting for {id}",
                response.id
            );
        }
        if !response.ok {
            bail!(
                "eval failed: {}\n  js: {js}",
                response.error.unwrap_or_default()
            );
        }
        Ok(response.value)
    }

    pub async fn count(&mut self, selector: &str) -> Result<usize> {
        let js = format!(
            "return document.querySelectorAll({}).length;",
            js_str(selector)
        );
        Ok(self.eval(&js).await?.as_u64().unwrap_or(0) as usize)
    }

    /// `textContent` of the first match, if any.
    pub async fn text(&mut self, selector: &str) -> Result<Option<String>> {
        let js = format!(
            "const el = document.querySelector({}); return el ? el.textContent : null;",
            js_str(selector)
        );
        Ok(self.eval(&js).await?.as_str().map(ToOwned::to_owned))
    }

    /// Current `value` of the first matching input.
    pub async fn value(&mut self, selector: &str) -> Result<Option<String>> {
        let js = format!(
            "const el = document.querySelector({}); return el ? el.value : null;",
            js_str(selector)
        );
        Ok(self.eval(&js).await?.as_str().map(ToOwned::to_owned))
    }

    /// Wait until at least one element matches.
    pub async fn wait_for(&mut self, selector: &str, timeout: Duration) -> Result<()> {
        let js = format!(
            "return document.querySelectorAll({}).length > 0;",
            js_str(selector)
        );
        self.wait_until(timeout, &format!("element matching {selector}"), &js)
            .await
    }

    /// Wait until some element matching `selector` contains `needle`.
    pub async fn wait_for_text(
        &mut self,
        selector: &str,
        needle: &str,
        timeout: Duration,
    ) -> Result<()> {
        let js = format!(
            "return [...document.querySelectorAll({})].some(el => el.textContent.includes({}));",
            js_str(selector),
            js_str(needle)
        );
        self.wait_until(timeout, &format!("{selector} containing {needle:?}"), &js)
            .await
    }

    /// Open a room by name. Rooms inside a space are hidden in Home, so if
    /// the row is not visible this clicks through the space badges to find it.
    pub async fn open_room(&mut self, name: &str) -> Result<()> {
        let visible = format!(
            "return [...document.querySelectorAll('.room')].some(el => el.textContent.includes({}));",
            js_str(name)
        );
        if !self.eval(&visible).await?.as_bool().unwrap_or(false) {
            let badges = self.count(".rail-item").await?;
            let mut found = false;
            for index in 0..badges {
                self.eval(&format!(
                    "document.querySelectorAll('.rail-item')[{index}].click();"
                ))
                .await?;
                if self
                    .wait_until(Duration::from_secs(3), "room in space", &visible)
                    .await
                    .is_ok()
                {
                    found = true;
                    break;
                }
            }
            if !found {
                bail!("no room named {name:?} in any space");
            }
        }
        // The list can re-render between the check and the click (the room
        // list service resets it while it settles), so retry the click.
        let click = format!(
            "const el = [...document.querySelectorAll('.room')].find(el => el.textContent.includes({})); \
             if (!el) return false; el.click(); return true;",
            js_str(name)
        );
        self.wait_until(DEFAULT_WAIT, "room row to click", &click)
            .await?;
        self.wait_for_text(".chat-header h2", name, DEFAULT_WAIT)
            .await
    }

    /// Click an element with a real-looking pointer position (its centre), so
    /// handlers that anchor popups to the click behave as they do for a mouse.
    pub async fn click(&mut self, selector: &str) -> Result<()> {
        let js = format!(
            "const el = {}; const r = el.getBoundingClientRect(); \
             el.dispatchEvent(new MouseEvent('click', {{ bubbles: true, cancelable: true, button: 0, \
             clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 }}));",
            query(selector)
        );
        self.eval(&js).await.map(drop)
    }

    /// Scroll the timeline to its top repeatedly (revealing windowed rows and
    /// paginating) until `predicate_js` (a JS body returning a boolean) holds.
    pub async fn reveal(
        &mut self,
        what: &str,
        predicate_js: &str,
        timeout: Duration,
    ) -> Result<()> {
        let js = format!(
            "if ((() => {{ {predicate_js} }})()) return true; \
             const t = document.querySelector('.timeline'); if (t) t.scrollTop = -t.scrollHeight; \
             document.querySelector('.load-older')?.click(); return false;"
        );
        self.wait_until(timeout, &format!("{what} after scrolling up"), &js)
            .await
    }

    /// [`Self::reveal`] for an element matching `selector`.
    pub async fn reveal_element(&mut self, selector: &str, timeout: Duration) -> Result<()> {
        let predicate = format!(
            "return document.querySelector({}) !== null;",
            js_str(selector)
        );
        self.reveal(&format!("element matching {selector}"), &predicate, timeout)
            .await
    }

    /// [`Self::reveal`] for an element matching `selector` whose text contains `text`.
    pub async fn reveal_text(
        &mut self,
        selector: &str,
        text: &str,
        timeout: Duration,
    ) -> Result<()> {
        let predicate = format!(
            "return [...document.querySelectorAll({})].some(el => el.textContent.includes({}));",
            js_str(selector),
            js_str(text)
        );
        self.reveal(
            &format!("{selector} containing {text:?}"),
            &predicate,
            timeout,
        )
        .await
    }

    /// [`Self::reveal`] for a message whose body contains `text`.
    pub async fn reveal_message(&mut self, text: &str, timeout: Duration) -> Result<()> {
        let predicate = format!(
            "return [...document.querySelectorAll('.message .body')].some(el => el.textContent.includes({}));",
            js_str(text)
        );
        self.reveal(&format!("message {text:?}"), &predicate, timeout)
            .await
    }

    /// Right-click an element: a bubbling `contextmenu` event at its centre.
    pub async fn right_click(&mut self, selector: &str) -> Result<()> {
        let js = format!(
            "const el = {}; const r = el.getBoundingClientRect(); \
             el.dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, button: 2, \
             clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 }}));",
            query(selector)
        );
        self.eval(&js).await.map(drop)
    }

    /// Click the first `selector` element whose text contains `needle`.
    pub async fn click_text(&mut self, selector: &str, needle: &str) -> Result<()> {
        let (sel, text) = (js_str(selector), js_str(needle));
        let js = format!(
            "const el = [...document.querySelectorAll({sel})].find(el => el.textContent.includes({text}));
             if (!el) throw new Error('no element matching ' + {sel} + ' with text ' + {text});
             const r = el.getBoundingClientRect(); \
             el.dispatchEvent(new MouseEvent('click', {{ bubbles: true, cancelable: true, button: 0, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 }}));"
        );
        self.eval(&js).await.map(drop)
    }

    /// Replace an input's value and fire `input` so the UI sees it.
    pub async fn type_into(&mut self, selector: &str, text: &str) -> Result<()> {
        let js = format!(
            "const el = {}; el.focus(); el.value = {}; el.dispatchEvent(new Event('input', {{ bubbles: true }}));",
            query(selector),
            js_str(text)
        );
        self.eval(&js).await.map(drop)
    }

    /// Dispatch a `keydown` for `key` (e.g. "Enter") on the element.
    pub async fn press(&mut self, selector: &str, key: &str) -> Result<()> {
        let js = format!(
            "{}.dispatchEvent(new KeyboardEvent('keydown', {{ key: {}, bubbles: true, cancelable: true }}));",
            query(selector),
            js_str(key)
        );
        self.eval(&js).await.map(drop)
    }

    /// Dispatch a key combo with any of Ctrl, Alt and Shift held.
    pub async fn key_combo(
        &mut self,
        selector: &str,
        key: &str,
        ctrl: bool,
        alt: bool,
        shift: bool,
    ) -> Result<()> {
        let js = format!(
            "{}.dispatchEvent(new KeyboardEvent('keydown', {{ key: {}, ctrlKey: {ctrl}, altKey: {alt}, shiftKey: {shift}, bubbles: true, cancelable: true }}));",
            query(selector),
            js_str(key)
        );
        self.eval(&js).await.map(drop)
    }

    /// Dispatch a keyboard shortcut (e.g. Ctrl+K) on an element.
    pub async fn shortcut(&mut self, selector: &str, key: &str, ctrl: bool) -> Result<()> {
        let js = format!(
            "{}.dispatchEvent(new KeyboardEvent('keydown', {{ key: {}, ctrlKey: {ctrl}, bubbles: true, cancelable: true }}));",
            query(selector),
            js_str(key)
        );
        self.eval(&js).await.map(drop)
    }

    pub async fn submit(&mut self, form_selector: &str) -> Result<()> {
        self.eval(&format!("{}.requestSubmit();", query(form_selector)))
            .await
            .map(drop)
    }

    /// Screenshot the app window into `target/e2e/<name>.png`.
    ///
    /// Best effort: works on Hyprland with `grim` available and returns
    /// `None` elsewhere so tests stay portable.
    pub async fn screenshot(&mut self, name: &str) -> Result<Option<PathBuf>> {
        if self.android {
            return android_screenshot(name);
        }
        let Some(pid) = self.child.as_ref().and_then(Child::id) else {
            return Ok(None);
        };
        let Ok(clients) = std::process::Command::new("hyprctl")
            .args(["clients", "-j"])
            .output()
        else {
            return Ok(None);
        };
        let clients: Value = serde_json::from_slice(&clients.stdout)?;
        let Some(window) = clients.as_array().and_then(|list| {
            list.iter()
                .find(|c| c["pid"].as_u64() == Some(u64::from(pid)))
        }) else {
            return Ok(None);
        };
        let geometry = format!(
            "{},{} {}x{}",
            window["at"][0], window["at"][1], window["size"][0], window["size"][1]
        );

        let dir = workspace_root()?.join("target/e2e");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{name}.png"));
        let status = std::process::Command::new("grim")
            .args(["-g", &geometry])
            .arg(&path)
            .status();
        match status {
            Ok(status) if status.success() => Ok(Some(path)),
            _ => Ok(None),
        }
    }

    /// Last lines of the app's log, for failure messages.
    pub fn log_tail(&self, lines: usize) -> String {
        let log = std::fs::read_to_string(&self.log_path).unwrap_or_default();
        let all: Vec<_> = log.lines().collect();
        all[all.len().saturating_sub(lines)..].join("\n")
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The disposable stand-in for the Downloads directory (`MORPHEUS_DOWNLOAD_DIR`)
    /// that this app instance was launched with.
    pub fn download_dir(&self) -> &Path {
        &self.download_dir
    }

    /// Wait until some element matching `selector` has exactly this trimmed text.
    pub async fn wait_for_exact_text(
        &mut self,
        selector: &str,
        text: &str,
        timeout: Duration,
    ) -> Result<()> {
        let js = format!(
            "return [...document.querySelectorAll({})].some(el => el.textContent.trim() === {});",
            js_str(selector),
            js_str(text)
        );
        self.wait_until(timeout, &format!("{selector} with text {text:?}"), &js)
            .await
    }

    /// Wait until a JavaScript predicate (it must `return` a boolean) holds.
    pub async fn wait_for_js(&mut self, predicate_js: &str, timeout: Duration) -> Result<()> {
        self.wait_until(timeout, predicate_js, predicate_js).await
    }

    /// Poll a JavaScript predicate (it must `return` a boolean) until it holds.
    async fn wait_until(
        &mut self,
        timeout: Duration,
        what: &str,
        predicate_js: &str,
    ) -> Result<()> {
        let started = Instant::now();
        let deadline = started + timeout;
        loop {
            if self.eval(predicate_js).await?.as_bool().unwrap_or(false) {
                self.timings.push((what.to_owned(), started.elapsed()));
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "timed out after {timeout:?} waiting for {what}\n--- app log ---\n{}",
                    self.log_tail(30)
                );
            }
            sleep(POLL).await;
        }
    }
}

impl App {
    /// Every wait so far, as `(what, elapsed)`, in order.
    pub fn timings(&self) -> &[(String, Duration)] {
        &self.timings
    }

    /// Print the slowest waits of this session to stderr.
    pub fn report_timings(&self, label: &str) {
        let mut sorted: Vec<_> = self.timings.iter().collect();
        sorted.sort_by_key(|entry| std::cmp::Reverse(entry.1));
        eprintln!("[timing] {label}: {} waits, slowest:", self.timings.len());
        for (what, elapsed) in sorted.iter().take(8) {
            eprintln!("[timing]   {elapsed:>8.3?}  {what}");
        }
    }
}

impl Drop for App {
    fn drop(&mut self) {
        if std::env::var_os("MORPHEUS_E2E_TIMINGS").is_some() {
            self.report_timings("app");
        }
        if let Some(child) = &mut self.child {
            let _ = child.start_kill();
        }
        if self.android {
            let _ = adb(&["shell", "am", "force-stop", ANDROID_PACKAGE]);
            let _ = adb(&["forward", "--remove", &format!("tcp:{}", self.port)]);
            PHONE_BUSY.store(false, Ordering::SeqCst);
        }
        if std::env::var_os("MORPHEUS_E2E_KEEP").is_none() {
            let _ = std::fs::remove_dir_all(&self.data_dir);
        }
    }
}

/// `document.querySelector(sel)` that throws a useful error when missing.
fn query(selector: &str) -> String {
    let sel = js_str(selector);
    format!(
        "(() => {{ const el = document.querySelector({sel}); if (!el) throw new Error('no element matching ' + {sel}); return el; }})()"
    )
}

fn js_str(s: &str) -> String {
    serde_json::to_string(s).expect("strings always serialise")
}

fn unique() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default()
}

async fn free_port() -> Result<u16> {
    Ok(TcpListener::bind(("127.0.0.1", 0))
        .await?
        .local_addr()?
        .port())
}

fn workspace_root() -> Result<PathBuf> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()?)
}

/// `MORPHEUS_E2E_BIN`, or the output of `dx build --package morpheus-desktop --features e2e`.
fn binary_path() -> Result<PathBuf> {
    if let Some(bin) = std::env::var_os("MORPHEUS_E2E_BIN") {
        return Ok(PathBuf::from(bin));
    }
    let bin = workspace_root()?.join("target/dx/morpheus-desktop/debug/linux/app/morpheus-desktop");
    if !bin.exists() {
        bail!(
            "app binary not found at {}; run `just e2e-build` first",
            bin.display()
        );
    }
    Ok(bin)
}

async fn connect(port: u16, log_path: &Path) -> Result<TcpStream> {
    let deadline = Instant::now() + LAUNCH_TIMEOUT;
    loop {
        if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)).await {
            return Ok(stream);
        }
        if Instant::now() >= deadline {
            let log = std::fs::read_to_string(log_path).unwrap_or_default();
            bail!(
                "app did not open its e2e bridge within {LAUNCH_TIMEOUT:?}\n--- app log ---\n{log}"
            );
        }
        sleep(POLL).await;
    }
}

/// On Hyprland, stop test windows from grabbing keyboard focus: a run opens
/// many of them and keystrokes meant for the terminal otherwise land in the
/// app (breaking logins). Applied once per process; a no-op elsewhere.
fn keep_test_windows_unfocused() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_none() {
            return;
        }
        for rule in [
            "noinitialfocus, title:^(Morpheus \\(test\\))$",
            "nofocus, title:^(Morpheus \\(test\\))$",
        ] {
            let _ = std::process::Command::new("hyprctl")
                .args(["keyword", "windowrulev2", rule])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
    });
}

/// Whether a test currently holds the phone.
static PHONE_BUSY: AtomicBool = AtomicBool::new(false);

/// The mobile app's package and activity (see `apps/mobile`).
pub const ANDROID_PACKAGE: &str = "dev.morpheus.mobile";
const ANDROID_ACTIVITY: &str = "dev.morpheus.mobile/dev.dioxus.main.MainActivity";
/// Where the mobile app's bridge listens on the device (`crates/ui/src/e2e.rs`).
const ANDROID_BRIDGE_PORT: u16 = 47100;

/// Run `adb` with `args`; its stdout when it succeeds.
fn adb(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("adb").args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `adb exec-out screencap` into `target/e2e/<name>.png`.
fn android_screenshot(name: &str) -> Result<Option<PathBuf>> {
    let output = std::process::Command::new("adb")
        .args(["exec-out", "screencap", "-p"])
        .output()?;
    if !output.status.success() || output.stdout.is_empty() {
        return Ok(None);
    }
    let dir = workspace_root()?.join("target/e2e");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{name}.png"));
    std::fs::write(&path, output.stdout)?;
    Ok(Some(path))
}

impl App {
    /// Start the mobile app (built with `--features e2e` and installed, see
    /// `just e2e-android-build`) on the device or emulator `adb` sees, with
    /// its data cleared, and connect to its bridge through `adb forward`.
    /// The app's own default homeserver applies (the host, from an emulator).
    pub async fn launch_android() -> Result<Self> {
        PHONE_BUSY.store(true, Ordering::SeqCst);
        Self::start_android(true, &[]).await
    }

    /// [`Self::launch_android`] with runtime permissions granted before the
    /// app starts (so no permission prompt covers it).
    pub async fn launch_android_granting(permissions: &[&str]) -> Result<Self> {
        PHONE_BUSY.store(true, Ordering::SeqCst);
        Self::start_android(true, permissions).await
    }

    /// Launch on the desktop even in a phone run: for what only the desktop
    /// app has (downloads to disk, the tray, a wide window).
    pub async fn launch_desktop(homeserver: &str) -> Result<Self> {
        let data_dir = std::env::temp_dir().join(format!("morpheus-e2e-{}", unique()));
        Self::launch_with_data_dir(homeserver, data_dir, None, &[]).await
    }

    /// In a phone run (`MORPHEUS_E2E_ANDROID`), the first app a test
    /// launches goes to the phone; later ones (other users) are desktop
    /// windows, as there is only one phone.
    async fn try_phone() -> Result<Option<Self>> {
        if std::env::var_os("MORPHEUS_E2E_ANDROID").is_none()
            || PHONE_BUSY.swap(true, Ordering::SeqCst)
        {
            return Ok(None);
        }
        match Self::start_android(true, &[]).await {
            Ok(app) => Ok(Some(app)),
            Err(err) => {
                PHONE_BUSY.store(false, Ordering::SeqCst);
                Err(err)
            }
        }
    }

    async fn start_android(clear: bool, permissions: &[&str]) -> Result<Self> {
        if clear {
            adb(&["shell", "pm", "clear", ANDROID_PACKAGE])
                .context("clearing the app's data (is it installed, and is a device attached?)")?;
        }
        for permission in permissions {
            adb(&["shell", "pm", "grant", ANDROID_PACKAGE, permission])
                .with_context(|| format!("granting {permission}"))?;
        }
        // The app asks for this on first launch; the system prompt would sit
        // over the page and take the test's taps. Older builds lack it.
        let _ = adb(&[
            "shell",
            "pm",
            "grant",
            ANDROID_PACKAGE,
            "android.permission.POST_NOTIFICATIONS",
        ]);
        adb(&["shell", "am", "start", "-W", "-n", ANDROID_ACTIVITY]).context("starting the app")?;
        let port = free_port().await?;
        adb(&[
            "forward",
            &format!("tcp:{port}"),
            &format!("tcp:{ANDROID_BRIDGE_PORT}"),
        ])
        .context("adb forward")?;

        // A forwarded port accepts even before the app listens, so only a
        // stream that answers an eval counts.
        let deadline = Instant::now() + LAUNCH_TIMEOUT;
        let mut restarted = false;
        loop {
            if let Ok(stream) = TcpStream::connect(("127.0.0.1", port)).await {
                // Logs stay on the device (`adb logcat`); the directory
                // only keeps the driver's bookkeeping consistent.
                let data_dir = std::env::temp_dir().join(format!("morpheus-e2e-{}", unique()));
                let download_dir = data_dir.join("downloads");
                std::fs::create_dir_all(&download_dir)?;
                let log_path = data_dir.join("app.log");
                std::fs::write(&log_path, "")?;
                let (reader, writer) = stream.into_split();
                let mut app = Self {
                    child: None,
                    android: true,
                    reader: BufReader::new(reader).lines(),
                    writer,
                    next_id: 1,
                    data_dir,
                    download_dir,
                    log_path,
                    port,
                    homeserver: String::new(),
                    timings: Vec::new(),
                };
                if tokio::time::timeout(Duration::from_secs(3), app.eval("return 1"))
                    .await
                    .is_ok_and(|answer| answer.is_ok())
                {
                    return Ok(app);
                }
                // Not answering yet; dropping it must not stop the app.
                app.android = false;
            }
            // A cold start occasionally never brings the bridge up; one
            // restart halfway through has always been enough.
            if !restarted && Instant::now() >= deadline - LAUNCH_TIMEOUT / 2 {
                restarted = true;
                let _ = adb(&["shell", "am", "force-stop", ANDROID_PACKAGE]);
                let _ = adb(&["shell", "am", "start", "-W", "-n", ANDROID_ACTIVITY]);
            }
            if Instant::now() >= deadline {
                let log = adb(&["logcat", "-d", "-t", "200"]).unwrap_or_default();
                bail!(
                    "the Android app did not open its e2e bridge within {LAUNCH_TIMEOUT:?}\n--- logcat ---\n{log}"
                );
            }
            sleep(Duration::from_millis(500)).await;
        }
    }
}

impl App {
    /// A real finger tap on the element's centre (Android), so the page sees
    /// a user gesture (Chrome lets back skip history entries made without
    /// one); `click` elsewhere.
    pub async fn tap(&mut self, selector: &str) -> Result<()> {
        if !self.android {
            return self.click(selector).await;
        }
        // Panes slide on a phone: tap where the target has come to rest.
        let where_js = format!(
            "const r = {}.getBoundingClientRect(); return [Math.round(r.left), Math.round(r.top)];",
            query(selector)
        );
        let mut last = self.eval(&where_js).await?;
        for _ in 0..20 {
            sleep(Duration::from_millis(60)).await;
            let now = self.eval(&where_js).await?;
            if now == last {
                break;
            }
            last = now;
        }
        let rect = self
            .eval(&format!(
                "const r = {}.getBoundingClientRect(); return [r.left + r.width / 2, r.top + r.height / 2, devicePixelRatio, innerHeight];",
                query(selector)
            ))
            .await?;
        let [x, y, scale, inner] = rect
            .as_array()
            .and_then(|v| {
                Some([
                    v.first()?.as_f64()?,
                    v.get(1)?.as_f64()?,
                    v.get(2)?.as_f64()?,
                    v.get(3)?.as_f64()?,
                ])
            })
            .ok_or_else(|| anyhow!("no position for {selector}"))?;
        // The page starts below the status bar unless it is drawn edge to edge.
        let (status_bar, screen_height) = screen_metrics();
        let top = if inner * scale + status_bar <= screen_height + 1.0 {
            status_bar
        } else {
            0.0
        };
        let (sx, sy) = ((x * scale).round(), (top + y * scale).round());
        adb(&["shell", "input", "tap", &sx.to_string(), &sy.to_string()])
            .ok_or_else(|| anyhow!("adb tap failed"))?;
        Ok(())
    }
}

/// `(status bar bottom, screen height)` in device pixels, from `dumpsys`.
fn screen_metrics() -> (f64, f64) {
    static METRICS: std::sync::OnceLock<(f64, f64)> = std::sync::OnceLock::new();
    *METRICS.get_or_init(|| {
        let window = adb(&["shell", "dumpsys", "window"]).unwrap_or_default();
        // "InsetsSource id=… type=statusBars frame=[0,0][1080,136] …"
        let status_bar = window
            .lines()
            .find(|line| line.contains("type=statusBars frame="))
            .and_then(|line| line.split("frame=").nth(1))
            .and_then(|frame| frame.split("][").nth(1))
            .and_then(|corner| corner.split([',', ']']).nth(1))
            .and_then(|bottom| bottom.parse().ok())
            .unwrap_or(0.0);
        // "Physical size: 1080x2400"
        let screen_height = adb(&["shell", "wm", "size"])
            .unwrap_or_default()
            .rsplit('x')
            .next()
            .and_then(|h| h.trim().parse().ok())
            .unwrap_or(f64::MAX);
        (status_bar, screen_height)
    })
}

impl App {
    /// [`Self::tap`] on the first `selector` match whose text contains `needle`.
    pub async fn tap_text(&mut self, selector: &str, needle: &str) -> Result<()> {
        self.eval(&format!(
            "document.querySelectorAll('[data-e2e-tap]').forEach(e => e.removeAttribute('data-e2e-tap')); \
             const el = [...document.querySelectorAll({})].find(e => e.textContent.includes({})); \
             if (!el) throw new Error('no element matching ' + {} + ' with text ' + {}); \
             el.setAttribute('data-e2e-tap', ''); return true;",
            js_str(selector),
            js_str(needle),
            js_str(selector),
            js_str(needle)
        ))
        .await?;
        self.tap("[data-e2e-tap]").await
    }
}

impl App {
    /// A real finger swipe on the phone between two page points (CSS pixels),
    /// over `ms` milliseconds.
    pub async fn swipe(&mut self, from: (f64, f64), to: (f64, f64), ms: u32) -> Result<()> {
        if !self.android {
            bail!("swipes need the Android app");
        }
        let metrics = self.eval("return [devicePixelRatio, innerHeight];").await?;
        let [scale, inner] = metrics
            .as_array()
            .and_then(|v| Some([v.first()?.as_f64()?, v.get(1)?.as_f64()?]))
            .ok_or_else(|| anyhow!("no page metrics"))?;
        let (status_bar, screen_height) = screen_metrics();
        let top = if inner * scale + status_bar <= screen_height + 1.0 {
            status_bar
        } else {
            0.0
        };
        let point = |(x, y): (f64, f64)| ((x * scale).round(), (top + y * scale).round());
        let ((x1, y1), (x2, y2)) = (point(from), point(to));
        adb(&[
            "shell",
            "input",
            "swipe",
            &x1.to_string(),
            &y1.to_string(),
            &x2.to_string(),
            &y2.to_string(),
            &ms.to_string(),
        ])
        .ok_or_else(|| anyhow!("adb swipe failed"))?;
        Ok(())
    }

    /// The page's size in CSS pixels.
    pub async fn page_size(&mut self) -> Result<(f64, f64)> {
        let size = self.eval("return [innerWidth, innerHeight];").await?;
        size.as_array()
            .and_then(|v| Some((v.first()?.as_f64()?, v.get(1)?.as_f64()?)))
            .ok_or_else(|| anyhow!("no page size"))
    }
}

impl App {
    /// Screen pixels for a page point (CSS pixels) on the phone.
    async fn screen_point(&mut self, (x, y): (f64, f64)) -> Result<(i64, i64)> {
        let metrics = self.eval("return [devicePixelRatio, innerHeight];").await?;
        let [scale, inner] = metrics
            .as_array()
            .and_then(|v| Some([v.first()?.as_f64()?, v.get(1)?.as_f64()?]))
            .ok_or_else(|| anyhow!("no page metrics"))?;
        let (status_bar, screen_height) = screen_metrics();
        let top = if inner * scale + status_bar <= screen_height + 1.0 {
            status_bar
        } else {
            0.0
        };
        // Whole pixels: `input motionevent` takes integers.
        #[allow(clippy::cast_possible_truncation)]
        Ok(((x * scale).round() as i64, (top + y * scale).round() as i64))
    }

    /// Put a finger down at `from` and drag it to `to` in small steps,
    /// leaving it down: the page is mid-gesture until [`Self::lift`].
    pub async fn drag_and_hold(&mut self, from: (f64, f64), to: (f64, f64)) -> Result<()> {
        if !self.android {
            bail!("drags need the Android app");
        }
        let (x1, y1) = self.screen_point(from).await?;
        let (x2, y2) = self.screen_point(to).await?;
        adb(&[
            "shell",
            "input",
            "motionevent",
            "DOWN",
            &x1.to_string(),
            &y1.to_string(),
        ])
        .ok_or_else(|| anyhow!("touch down failed"))?;
        const STEPS: i64 = 8;
        for step in 1..=STEPS {
            let (x, y) = (x1 + (x2 - x1) * step / STEPS, y1 + (y2 - y1) * step / STEPS);
            adb(&[
                "shell",
                "input",
                "motionevent",
                "MOVE",
                &x.to_string(),
                &y.to_string(),
            ])
            .ok_or_else(|| anyhow!("touch move failed"))?;
        }
        Ok(())
    }

    /// Lift the finger put down by [`Self::drag_and_hold`] at `at`.
    pub async fn lift(&mut self, at: (f64, f64)) -> Result<()> {
        let (x, y) = self.screen_point(at).await?;
        adb(&[
            "shell",
            "input",
            "motionevent",
            "UP",
            &x.to_string(),
            &y.to_string(),
        ])
        .ok_or_else(|| anyhow!("touch up failed"))?;
        Ok(())
    }
}

impl App {
    /// Start the app again on this app's data directory, the way a user
    /// launching it twice would, and wait up to `timeout` for that second
    /// process to exit. `Some(status)` if it exited, `None` if it kept running
    /// (it is killed then).
    pub async fn launch_again(
        &self,
        timeout: Duration,
    ) -> Result<Option<std::process::ExitStatus>> {
        let port = free_port().await?;
        let mut child = Command::new(binary_path()?)
            .env("MORPHEUS_E2E_PORT", port.to_string())
            .env("MORPHEUS_DATA_DIR", &self.data_dir)
            .env("MORPHEUS_HOMESERVER", &self.homeserver)
            .env("MORPHEUS_SESSION_STORE", "file")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        match tokio::time::timeout(timeout, child.wait()).await {
            Ok(status) => Ok(Some(status?)),
            Err(_) => {
                let _ = child.kill().await;
                Ok(None)
            }
        }
    }
}
