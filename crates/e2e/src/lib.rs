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

pub mod web;

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
/// How long one script in the app may take before the app counts as stuck.
const EVAL_TIMEOUT: Duration = Duration::from_secs(60);

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
    /// The window size asked for (logical pixels; desktop only).
    size: (u32, u32),
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
        prune_test_devices(homeserver);
        std::fs::create_dir_all(&data_dir)?;
        // A disposable, test-local stand-in for the real Downloads directory
        // (see `MORPHEUS_DOWNLOAD_DIR` in `morpheus-core`), so downloads can
        // be asserted on without touching the machine's real one.
        let download_dir = data_dir.join("downloads");
        std::fs::create_dir_all(&download_dir)?;
        let port = free_port().await?;
        // A test may run another copy of the app (MORPHEUS_E2E_BIN in `env`).
        let bin = match env.iter().find(|(key, _)| *key == "MORPHEUS_E2E_BIN") {
            Some((_, bin)) => PathBuf::from(bin),
            None => binary_path()?,
        };
        let log_path = data_dir.join("app.log");
        let log = std::fs::File::create(&log_path)?;

        // The web app: a page in a headless Chromium, behind a relay.
        if web::enabled() && !env.iter().any(|(key, _)| *key == "MORPHEUS_E2E_BIN") {
            let size = size.unwrap_or((1280, 800));
            let (child, port) = web::start(homeserver, &data_dir, size, log).await?;
            let stream = connect(port, &log_path).await?;
            let (reader, writer) = stream.into_split();
            // Connected to the relay; the page answers once its app is up.
            return Ok(Self {
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
                size,
            });
        }

        let mut command = Command::new(&bin);
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
            // Calls send a test tone and play nothing: no real microphone or speakers.
            .env("MORPHEUS_CALL_AUDIO", "tone")
            .env(
                "RUST_LOG",
                std::env::var("RUST_LOG").unwrap_or_else(|_| "morpheus=debug,warn".into()),
            )
            // A test's own settings last: they win over the defaults above.
            .envs(env.iter().copied())
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
            size: size.unwrap_or((1280, 800)),
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
    /// The GPUI app (`MORPHEUS_E2E_UI=gpui`) runs none: the test needs porting.
    pub async fn eval(&mut self, js: &str) -> Result<Value> {
        if gpui() {
            bail!("{NEEDS_PORTING}\n  js: {js}");
        }
        self.send(|id| json!({ "id": id, "js": js }), &format!("js: {js}"))
            .await
    }

    /// Send one request to the app's bridge (`build` makes it from its ID)
    /// and wait for the answer; `detail` says what it was, for failures.
    async fn send(&mut self, build: impl FnOnce(u64) -> Value, detail: &str) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut request = build(id).to_string();
        request.push('\n');
        self.writer.write_all(request.as_bytes()).await?;

        // An app that stops answering fails the test instead of hanging it.
        let line = tokio::time::timeout(EVAL_TIMEOUT, self.reader.next_line())
            .await
            .map_err(|_| {
                anyhow!(
                    "the app did not answer for {EVAL_TIMEOUT:?}\n  {detail}\n--- app log ---\n{}",
                    self.log_tail(40)
                )
            })??
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
                "eval failed: {}\n  {detail}",
                response.error.unwrap_or_default()
            );
        }
        Ok(response.value)
    }

    /// A structured request to the GPUI app: operation `op` on the elements
    /// matching `selector`; `extra` holds the operation's other fields
    /// (`contains`, `exact`, `index`, `text`, `key`, `modifiers`).
    async fn op(&mut self, op: &str, selector: &str, extra: Value) -> Result<Value> {
        self.send(
            |id| {
                let mut request = json!({ "id": id, "op": op, "selector": selector });
                if let (Some(request), Some(extra)) = (request.as_object_mut(), extra.as_object()) {
                    request.extend(extra.clone());
                }
                request
            },
            &format!("{op} {selector}"),
        )
        .await
    }

    /// Poll the GPUI app until some element matches `selector` (and `filter`).
    async fn wait_gpui(
        &mut self,
        timeout: Duration,
        what: &str,
        selector: &str,
        filter: Value,
    ) -> Result<()> {
        let started = Instant::now();
        let deadline = started + timeout;
        loop {
            if self.op("count", selector, filter.clone()).await?.as_u64() > Some(0) {
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

    pub async fn count(&mut self, selector: &str) -> Result<usize> {
        if gpui() {
            let count = self.op("count", selector, json!({})).await?;
            return Ok(count.as_u64().unwrap_or(0) as usize);
        }
        let js = format!(
            "return document.querySelectorAll({}).length;",
            js_str(selector)
        );
        Ok(self.eval(&js).await?.as_u64().unwrap_or(0) as usize)
    }

    /// `textContent` of the first match, if any.
    pub async fn text(&mut self, selector: &str) -> Result<Option<String>> {
        if gpui() {
            let text = self.op("text", selector, json!({})).await?;
            return Ok(text.as_str().map(ToOwned::to_owned));
        }
        let js = format!(
            "const el = document.querySelector({}); return el ? el.textContent : null;",
            js_str(selector)
        );
        Ok(self.eval(&js).await?.as_str().map(ToOwned::to_owned))
    }

    /// `textContent` of every match, in document order.
    pub async fn texts(&mut self, selector: &str) -> Result<Vec<String>> {
        let list = if gpui() {
            self.op("texts", selector, json!({})).await?
        } else {
            let js = format!(
                "return [...document.querySelectorAll({})].map(el => el.textContent);",
                js_str(selector)
            );
            self.eval(&js).await?
        };
        Ok(serde_json::from_value(list).unwrap_or_default())
    }

    /// Current `value` of the first matching input.
    pub async fn value(&mut self, selector: &str) -> Result<Option<String>> {
        if gpui() {
            let value = self.op("value", selector, json!({})).await?;
            return Ok(value.as_str().map(ToOwned::to_owned));
        }
        let js = format!(
            "const el = document.querySelector({}); return el ? el.value : null;",
            js_str(selector)
        );
        Ok(self.eval(&js).await?.as_str().map(ToOwned::to_owned))
    }

    /// Wait until at least one element matches.
    pub async fn wait_for(&mut self, selector: &str, timeout: Duration) -> Result<()> {
        if gpui() {
            let what = format!("element matching {selector}");
            return self.wait_gpui(timeout, &what, selector, json!({})).await;
        }
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
        if gpui() {
            let what = format!("{selector} containing {needle:?}");
            return self
                .wait_gpui(timeout, &what, selector, json!({ "contains": needle }))
                .await;
        }
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
        if gpui() {
            return self.open_room_gpui(name).await;
        }
        // An exact name first: "bob" is not "Empty Room (was bob)".
        let click = format!(
            "const want = {}; const rows = [...document.querySelectorAll('.room')]; \
             const el = rows.find(el => el.querySelector('.room-name')?.textContent.trim() === want) \
               || rows.find(el => el.textContent.includes(want)); \
             if (!el) return false; el.click(); return true;",
            js_str(name)
        );
        // The list can change under us: it re-renders while the room list
        // service settles, and right after sign-in the app reopens the last
        // room, switching the sidebar to its space (or the DMs). So: click
        // the row if it is in the list now, else look through the spaces,
        // and again until the deadline.
        let deadline = Instant::now() + DEFAULT_WAIT;
        loop {
            if self.eval(&click).await?.as_bool().unwrap_or(false) {
                break;
            }
            let badges = self.count(".rail-item").await?;
            for index in 0..badges {
                self.eval(&format!(
                    "document.querySelectorAll('.rail-item')[{index}].click();"
                ))
                .await?;
                sleep(Duration::from_millis(300)).await;
                if self.eval(&click).await?.as_bool().unwrap_or(false) {
                    return self
                        .wait_for_text(".chat-header h2", name, DEFAULT_WAIT)
                        .await;
                }
            }
            if Instant::now() >= deadline {
                let rows = self
                    .eval("return [...document.querySelectorAll('.room')].map(r => r.className + ': ' + r.textContent).join(' | ');")
                    .await;
                bail!("no room named {name:?} in any space\nrows: {rows:?}");
            }
        }
        self.wait_for_text(".chat-header h2", name, DEFAULT_WAIT)
            .await
    }

    /// Click an element with a real-looking pointer position (its centre), so
    /// handlers that anchor popups to the click behave as they do for a mouse.
    pub async fn click(&mut self, selector: &str) -> Result<()> {
        if gpui() {
            return self.op("click", selector, json!({})).await.map(drop);
        }
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
             document.querySelectorAll('.timeline').forEach(t => {{ t.scrollTop = -t.scrollHeight; }}); \
             document.querySelectorAll('.load-older').forEach(b => b.click()); return false;"
        );
        self.wait_until(timeout, &format!("{what} after scrolling up"), &js)
            .await
    }

    /// [`Self::reveal`] for an element matching `selector`.
    pub async fn reveal_element(&mut self, selector: &str, timeout: Duration) -> Result<()> {
        if gpui() {
            return self
                .reveal_gpui(&format!("element matching {selector}"), selector, None, timeout)
                .await;
        }
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
        if gpui() {
            return self
                .reveal_gpui(&format!("{selector} containing {text:?}"), selector, Some(text), timeout)
                .await;
        }
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
        if gpui() {
            return self
                .reveal_gpui(&format!("message {text:?}"), ".message .body", Some(text), timeout)
                .await;
        }
        let predicate = format!(
            "return [...document.querySelectorAll('.message .body')].some(el => el.textContent.includes({}));",
            js_str(text)
        );
        self.reveal(&format!("message {text:?}"), &predicate, timeout)
            .await
    }

    /// Scroll the timeline up until an element matching `selector` (whose
    /// text contains `contains`, when given) is on screen. The GPUI app
    /// paints only the rows in view, so this is how tests reach old ones.
    async fn reveal_gpui(
        &mut self,
        what: &str,
        selector: &str,
        contains: Option<&str>,
        timeout: Duration,
    ) -> Result<()> {
        let filter = match contains {
            Some(text) => json!({ "contains": text }),
            None => json!({}),
        };
        let deadline = Instant::now() + timeout;
        loop {
            if self.op("count", selector, filter.clone()).await?.as_u64() > Some(0) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "timed out after {timeout:?} waiting for {what} after scrolling up\n--- app log ---\n{}",
                    self.log_tail(30)
                );
            }
            self.scroll_to(".timeline", ScrollTo::Top).await?;
            sleep(POLL).await;
        }
    }

    /// How far the scrolling element `selector` is scrolled (see
    /// [`ScrollState`]); `None` when nothing matches. On the Dioxus app a
    /// `column-reverse` container (the timeline) reads as scrolled from its
    /// bottom edge too.
    pub async fn scroll_state(&mut self, selector: &str) -> Result<Option<ScrollState>> {
        let value = if gpui() {
            self.op("scroll_state", selector, json!({})).await.or_else(|err| {
                // Nothing there is not an error.
                if err.to_string().contains("no element matching") {
                    Ok(Value::Null)
                } else {
                    Err(err)
                }
            })?
        } else {
            let js = format!(
                "const t = document.querySelector({}); if (!t) return null; \
                 const rev = getComputedStyle(t).flexDirection === 'column-reverse'; \
                 const range = t.scrollHeight - t.clientHeight; \
                 return {{ from_bottom: rev ? -t.scrollTop : range - t.scrollTop, range, viewport: t.clientHeight }};",
                js_str(selector)
            );
            self.eval(&js).await?
        };
        if value.is_null() {
            return Ok(None);
        }
        let number = |key: &str| value[key].as_f64().unwrap_or(0.0);
        Ok(Some(ScrollState {
            from_bottom: number("from_bottom"),
            range: number("range"),
            viewport: number("viewport"),
        }))
    }

    /// Scroll the element `selector` to its top or bottom, or to a distance
    /// from the bottom (clamped to what it can scroll).
    pub async fn scroll_to(&mut self, selector: &str, to: ScrollTo) -> Result<()> {
        if gpui() {
            let extra = match to {
                ScrollTo::Top => json!({ "to": "top" }),
                ScrollTo::Bottom => json!({ "to": "bottom" }),
                ScrollTo::FromBottom(px) => json!({ "from_bottom": px }),
            };
            return self.op("scroll", selector, extra).await.map(drop);
        }
        let wanted = match to {
            ScrollTo::Top => "range".to_owned(),
            ScrollTo::Bottom => "0".to_owned(),
            ScrollTo::FromBottom(px) => px.to_string(),
        };
        let js = format!(
            "const t = document.querySelector({}); if (!t) return false; \
             const rev = getComputedStyle(t).flexDirection === 'column-reverse'; \
             const range = t.scrollHeight - t.clientHeight; \
             const from_bottom = Math.max(0, Math.min({wanted}, range)); \
             t.scrollTop = rev ? -from_bottom : range - from_bottom; return true;",
            js_str(selector)
        );
        self.eval(&js).await.map(drop)
    }

    /// Where the first element matching `selector` is, in window pixels.
    pub async fn bounds(&mut self, selector: &str) -> Result<Option<Bounds>> {
        self.bounds_of(selector, None).await
    }

    /// [`Self::bounds`] of the first match whose text contains `text`.
    pub async fn bounds_containing(
        &mut self,
        selector: &str,
        text: &str,
    ) -> Result<Option<Bounds>> {
        self.bounds_of(selector, Some(text)).await
    }

    async fn bounds_of(&mut self, selector: &str, text: Option<&str>) -> Result<Option<Bounds>> {
        let value = if gpui() {
            let filter = text.map_or(json!({}), |t| json!({ "contains": t }));
            self.op("bounds", selector, filter).await?
        } else {
            let js = format!(
                "const e = [...document.querySelectorAll({})].find(e => {}); if (!e) return null; const r = e.getBoundingClientRect(); \
                 return {{ x: r.left, y: r.top, w: r.width, h: r.height }};",
                js_str(selector),
                text.map_or("true".to_owned(), |t| format!(
                    "e.textContent.includes({})",
                    js_str(t)
                ))
            );
            self.eval(&js).await?
        };
        if value.is_null() {
            return Ok(None);
        }
        let number = |key: &str| value[key].as_f64().unwrap_or(0.0);
        Ok(Some(Bounds {
            x: number("x"),
            y: number("y"),
            w: number("w"),
            h: number("h"),
        }))
    }

    /// [`Self::bounds`] of the first `selector` inside the first `within`
    /// whose text contains `contains`; GPUI only (None on the web page).
    pub async fn bounds_within(
        &mut self,
        within: &str,
        contains: &str,
        selector: &str,
    ) -> Result<Option<Bounds>> {
        if !gpui() {
            return Ok(None);
        }
        let extra = json!({ "within": within, "within_contains": contains });
        let value = self.op("bounds", selector, extra).await?;
        if value.is_null() {
            return Ok(None);
        }
        let number = |key: &str| value[key].as_f64().unwrap_or(0.0);
        Ok(Some(Bounds {
            x: number("x"),
            y: number("y"),
            w: number("w"),
            h: number("h"),
        }))
    }

    /// Right-click an element: a bubbling `contextmenu` event at its centre.
    pub async fn right_click(&mut self, selector: &str) -> Result<()> {
        if gpui() {
            return self.op("right_click", selector, json!({})).await.map(drop);
        }
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
        if gpui() {
            let extra = json!({ "contains": needle });
            return self.op("click", selector, extra).await.map(drop);
        }
        let (sel, text) = (js_str(selector), js_str(needle));
        let js = format!(
            "const el = [...document.querySelectorAll({sel})].find(el => el.textContent.includes({text}));
             if (!el) throw new Error('no element matching ' + {sel} + ' with text ' + {text});
             const r = el.getBoundingClientRect(); \
             el.dispatchEvent(new MouseEvent('click', {{ bubbles: true, cancelable: true, button: 0, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 }}));"
        );
        self.eval(&js).await.map(drop)
    }

    /// Right-click the first `selector` element whose text contains `needle`.
    pub async fn right_click_text(&mut self, selector: &str, needle: &str) -> Result<()> {
        if gpui() {
            let extra = json!({ "contains": needle });
            return self.op("right_click", selector, extra).await.map(drop);
        }
        let (sel, text) = (js_str(selector), js_str(needle));
        let js = format!(
            "const el = [...document.querySelectorAll({sel})].find(el => el.textContent.includes({text}));
             if (!el) throw new Error('no element matching ' + {sel} + ' with text ' + {text});
             const r = el.getBoundingClientRect(); \
             el.dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, button: 2, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 }}));"
        );
        self.eval(&js).await.map(drop)
    }

    /// Leave `selector`'s field, as clicking elsewhere does (the webview
    /// gets a `blur` event). The GPUI app has no such event: fields that
    /// save on leaving (the profile card's note) save as the writing
    /// pauses there instead, so this does nothing for it.
    pub async fn blur(&mut self, selector: &str) -> Result<()> {
        if gpui() {
            return Ok(());
        }
        let js = format!("{}.dispatchEvent(new Event('blur'));", query(selector));
        self.eval(&js).await.map(drop)
    }

    /// Right-click the first `selector` element whose trimmed text is exactly `text`.
    pub async fn right_click_exact(&mut self, selector: &str, text: &str) -> Result<()> {
        if gpui() {
            let extra = json!({ "exact": text });
            return self.op("right_click", selector, extra).await.map(drop);
        }
        let (sel, text) = (js_str(selector), js_str(text));
        let js = format!(
            "const el = [...document.querySelectorAll({sel})].find(el => el.textContent.trim() === {text});
             if (!el) throw new Error('no element matching ' + {sel} + ' with exact text ' + {text});
             const r = el.getBoundingClientRect(); \
             el.dispatchEvent(new MouseEvent('contextmenu', {{ bubbles: true, cancelable: true, button: 2, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 }}));"
        );
        self.eval(&js).await.map(drop)
    }

    /// The text of every `selector` match inside the `within` elements whose
    /// text contains `contains` (a message row's reactions, say).
    pub async fn texts_within(
        &mut self,
        within: &str,
        contains: &str,
        selector: &str,
    ) -> Result<Vec<String>> {
        let list = if gpui() {
            let extra = json!({ "within": within, "within_contains": contains });
            self.op("texts", selector, extra).await?
        } else {
            let js = format!(
                "return [...document.querySelectorAll({})].filter(el => el.textContent.includes({})) \
                 .flatMap(scope => [...scope.querySelectorAll({})].map(el => el.textContent));",
                js_str(within),
                js_str(contains),
                js_str(selector)
            );
            self.eval(&js).await?
        };
        Ok(serde_json::from_value(list).unwrap_or_default())
    }

    /// Wait until a `selector` match inside the `within` element containing
    /// `contains` has text containing `needle`; `None` waits for there to be
    /// no match at all.
    pub async fn wait_for_within(
        &mut self,
        within: &str,
        contains: &str,
        selector: &str,
        needle: Option<&str>,
        timeout: Duration,
    ) -> Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            let found = self.texts_within(within, contains, selector).await?;
            let ok = match needle {
                Some(needle) => found.iter().any(|t| t.contains(needle)),
                None => found.is_empty(),
            };
            if ok {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "timed out after {timeout:?} waiting for {selector} ({needle:?}) inside {within} containing {contains:?} (found {found:?})\n--- app log ---\n{}",
                    self.log_tail(30)
                );
            }
            sleep(POLL).await;
        }
    }

    /// Pick `child` from the open menu's `parent` submenu. On a desktop the
    /// submenu is in the page under its row; a phone's sheet opens it in
    /// place when the row is tapped. The child matches by its whole text or
    /// label, else by containing `child`.
    pub async fn click_submenu(&mut self, parent: &str, child: &str) -> Result<()> {
        if gpui() {
            // A click moves the pointer onto the row first, which opens the
            // flyout (as hovering does), and keeps it open.
            self.op(
                "click",
                ".context-menu > .context-item.has-submenu",
                json!({ "contains": parent }),
            )
            .await?;
            let deadline = Instant::now() + DEFAULT_WAIT;
            loop {
                for filter in ["exact", "contains"] {
                    let extra = json!({
                        filter: child,
                        "within": ".context-menu > .context-item.has-submenu",
                        "within_contains": parent,
                    });
                    if self
                        .op("click", ".context-submenu .context-item", extra)
                        .await
                        .is_ok()
                    {
                        return Ok(());
                    }
                }
                if Instant::now() >= deadline {
                    bail!("no item {child:?} in the {parent:?} submenu");
                }
                sleep(POLL).await;
            }
        }
        let (parent_js, child_js) = (js_str(parent), js_str(child));
        let pick = |items: &str| {
            format!(
                "const items = {items}; \
                 const label = el => (el.querySelector('.menu-label')?.textContent ?? el.textContent).trim(); \
                 const c = items.find(el => el.textContent.trim() === {child_js} || label(el) === {child_js}) \
                   ?? items.find(el => el.textContent.includes({child_js})); \
                 if (!c) return false; c.click(); return true;"
            )
        };
        let sheet = self.count(".context-menu.sheet").await? > 0;
        // Already showing its items (a test read them first).
        let open_already = sheet && self.count(".context-menu.sheet .sheet-back").await? > 0;
        if sheet && !open_already {
            let open = format!(
                "const p = [...document.querySelectorAll('.context-menu.sheet .context-item')] \
                   .find(el => (el.querySelector('.menu-label')?.textContent ?? el.textContent).trim() === {parent_js}); \
                 if (!p) return false; p.click(); return true;"
            );
            self.wait_until(DEFAULT_WAIT, "submenu row", &open).await?;
            self.wait_for(".context-menu.sheet .sheet-back", DEFAULT_WAIT)
                .await?;
        }
        if sheet {
            let js = pick(
                "[...document.querySelectorAll('.context-menu.sheet .sheet-items .context-item')]",
            );
            return self.wait_until(DEFAULT_WAIT, "submenu item", &js).await;
        }
        let js = pick(&format!(
            "(() => {{ const p = [...document.querySelectorAll('.context-item.has-submenu')] \
               .find(el => el.querySelector('.menu-label')?.textContent === {parent_js}); \
               return p ? [...p.querySelectorAll('.context-submenu .context-item')] : []; }})()"
        ));
        self.wait_until(DEFAULT_WAIT, "submenu item", &js).await
    }

    /// Replace an input's value and fire `input` so the UI sees it.
    pub async fn type_into(&mut self, selector: &str, text: &str) -> Result<()> {
        if gpui() {
            let extra = json!({ "text": text });
            return self.op("type", selector, extra).await.map(drop);
        }
        let js = format!(
            "const el = {}; el.focus(); el.value = {}; el.dispatchEvent(new Event('input', {{ bubbles: true }}));",
            query(selector),
            js_str(text)
        );
        self.eval(&js).await.map(drop)
    }

    /// Dispatch a `keydown` for `key` (e.g. "Enter") on the element.
    pub async fn press(&mut self, selector: &str, key: &str) -> Result<()> {
        if gpui() {
            let extra = json!({ "key": key });
            return self.op("press", selector, extra).await.map(drop);
        }
        // On a phone the keyboard's Enter is a new line in the message box;
        // sending is the send button, so "Enter to send" taps it (unless a
        // suggestion list is open, where Enter picks).
        if self.android && key == "Enter" && selector.contains("composer-input") {
            let js = format!(
                "const box = {}; const composer = box.closest('.composer'); \
                 if (composer.querySelector('.autocomplete')) {{ \
                   box.dispatchEvent(new KeyboardEvent('keydown', {{ key: 'Enter', bubbles: true, cancelable: true }})); \
                 }} else {{ composer.querySelector('.send-button').click(); }}",
                query(selector)
            );
            return self.eval(&js).await.map(drop);
        }
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
        if gpui() {
            let modifiers: Vec<&str> = [(ctrl, "ctrl"), (alt, "alt"), (shift, "shift")]
                .into_iter()
                .filter_map(|(held, name)| held.then_some(name))
                .collect();
            let extra = json!({ "key": key, "modifiers": modifiers });
            return self.op("press", selector, extra).await.map(drop);
        }
        let js = format!(
            "{}.dispatchEvent(new KeyboardEvent('keydown', {{ key: {}, ctrlKey: {ctrl}, altKey: {alt}, shiftKey: {shift}, bubbles: true, cancelable: true }}));",
            query(selector),
            js_str(key)
        );
        self.eval(&js).await.map(drop)
    }

    /// Dispatch a keyboard shortcut (e.g. Ctrl+K) on an element.
    pub async fn shortcut(&mut self, selector: &str, key: &str, ctrl: bool) -> Result<()> {
        if gpui() {
            return self.key_combo(selector, key, ctrl, false, false).await;
        }
        let js = format!(
            "{}.dispatchEvent(new KeyboardEvent('keydown', {{ key: {}, ctrlKey: {ctrl}, bubbles: true, cancelable: true }}));",
            query(selector),
            js_str(key)
        );
        self.eval(&js).await.map(drop)
    }

    pub async fn submit(&mut self, form_selector: &str) -> Result<()> {
        if gpui() {
            return self.op("submit", form_selector, json!({})).await.map(drop);
        }
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
        if web::enabled() {
            let dir = workspace_root()?.join("target/e2e");
            std::fs::create_dir_all(&dir)?;
            let path = dir.join(format!("{name}.png"));
            web::screenshot(&self.data_dir, self.size, &path).await?;
            return Ok(Some(path));
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
        // Tiled, the window can be bigger than the size it was opened at,
        // which the web view keeps (in the top-left corner): just that.
        let size = |i: usize| window["size"][i].as_i64().unwrap_or(0);
        let width = size(0).min(i64::from(self.size.0));
        let height = size(1).min(i64::from(self.size.1));
        let geometry = format!("{},{} {width}x{height}", window["at"][0], window["at"][1]);

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

    /// Whether this app runs on the phone (emulator or device), not a desktop window.
    pub fn is_phone(&self) -> bool {
        self.android
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// The disposable stand-in for the Downloads directory (`MORPHEUS_DOWNLOAD_DIR`)
    /// that this app instance was launched with.
    pub fn download_dir(&self) -> &Path {
        &self.download_dir
    }

    /// Share to Morpheus from another app (Android's share sheet): `text`,
    /// and a `content:` URI of a file, as SEND intents.
    pub fn share_from_outside(&self, text: Option<&str>, stream: Option<&str>) -> Result<()> {
        if !self.android {
            bail!("sharing from another app is a phone's");
        }
        let mut args = vec!["shell", "am", "start", "-a", "android.intent.action.SEND"];
        let text_arg;
        let stream_arg;
        match stream {
            Some(uri) => {
                stream_arg = format!("'{uri}'");
                // As the data too: the read grant covers the intent's data URI.
                args.extend([
                    "-t",
                    "image/*",
                    "-d",
                    &stream_arg,
                    "--eu",
                    "android.intent.extra.STREAM",
                    &stream_arg,
                    "--grant-read-uri-permission",
                ]);
            }
            None => args.extend(["-t", "text/plain"]),
        }
        if let Some(text) = text {
            text_arg = format!("'{text}'");
            args.extend(["--es", "android.intent.extra.TEXT", &text_arg]);
        }
        args.extend([
            "-n",
            "dev.morpheus.mobile/dev.morpheus.mobile.ShareActivity",
        ]);
        adb(&args).ok_or_else(|| anyhow!("adb could not share"))?;
        Ok(())
    }

    /// Open `link` as the desktop does (a `matrix:` link, or matrix.to from a
    /// browser): the app is started again with it, on this app's data, and
    /// hands it to this running app. Returns once that second start exits.
    pub fn open_link_from_outside(&self, link: &str) -> Result<()> {
        // A phone: the link as a VIEW intent, as tapping it elsewhere does.
        if self.android {
            adb(&[
                "shell",
                "am",
                "start",
                "-a",
                "android.intent.action.VIEW",
                "-d",
                &format!("'{link}'"),
                ANDROID_PACKAGE,
            ])
            .ok_or_else(|| anyhow::anyhow!("adb could not open {link}"))?;
            return Ok(());
        }
        let status = std::process::Command::new(binary_path()?)
            .arg(link)
            .env("MORPHEUS_DATA_DIR", &self.data_dir)
            .env("MORPHEUS_HOMESERVER", &self.homeserver)
            .env("MORPHEUS_SESSION_STORE", "file")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;
        if !status.success() {
            bail!("the second start exited with {status}");
        }
        Ok(())
    }

    /// Wait until some element matching `selector` has exactly this trimmed text.
    pub async fn wait_for_exact_text(
        &mut self,
        selector: &str,
        text: &str,
        timeout: Duration,
    ) -> Result<()> {
        if gpui() {
            let what = format!("{selector} with text {text:?}");
            return self
                .wait_gpui(timeout, &what, selector, json!({ "exact": text }))
                .await;
        }
        let js = format!(
            "return [...document.querySelectorAll({})].some(el => el.textContent.trim() === {});",
            js_str(selector),
            js_str(text)
        );
        self.wait_until(timeout, &format!("{selector} with text {text:?}"), &js)
            .await
    }

    /// Whether the app under test is the GPUI one.
    pub fn is_gpui(&self) -> bool {
        gpui()
    }

    /// Wait until the first element matching `selector` has this `value`
    /// (an input's text).
    pub async fn wait_for_value(
        &mut self,
        selector: &str,
        expected: &str,
        timeout: Duration,
    ) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let mut seen = None;
        loop {
            seen = self.value(selector).await.unwrap_or(seen);
            if seen.as_deref() == Some(expected) {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "timed out after {timeout:?} waiting for {selector} to have value {expected:?} (last {seen:?})\n--- app log ---\n{}",
                    self.log_tail(30)
                );
            }
            sleep(POLL).await;
        }
    }

    /// Wait until exactly `count` elements match `selector` (0: until it is gone).
    pub async fn wait_for_count(
        &mut self,
        selector: &str,
        count: usize,
        timeout: Duration,
    ) -> Result<()> {
        let deadline = Instant::now() + timeout;
        let mut seen = usize::MAX;
        loop {
            seen = self.count(selector).await.unwrap_or(seen);
            if seen == count {
                return Ok(());
            }
            if Instant::now() >= deadline {
                bail!(
                    "timed out after {timeout:?} waiting for {count} of {selector} (last {seen})\n--- app log ---\n{}",
                    self.log_tail(30)
                );
            }
            sleep(POLL).await;
        }
    }

    /// Click `selector` inside the element matching `within` whose text
    /// contains `contains` (a message's hover action, say). Hovers first.
    pub async fn click_within(
        &mut self,
        within: &str,
        contains: &str,
        selector: &str,
    ) -> Result<()> {
        if gpui() {
            let extra = json!({ "within": within, "within_contains": contains });
            return self.op("click", selector, extra).await.map(drop);
        }
        let js = format!(
            "const scope = [...document.querySelectorAll({})].find(el => el.textContent.includes({})); \
             if (!scope) throw new Error('no ' + {} + ' containing ' + {}); \
             const el = scope.querySelector({}); \
             if (!el) throw new Error('no ' + {} + ' inside it'); \
             el.click();",
            js_str(within),
            js_str(contains),
            js_str(within),
            js_str(contains),
            js_str(selector),
            js_str(selector),
        );
        self.eval(&js).await.map(drop)
    }

    /// Click `selector` inside the message whose body (`.body`) contains
    /// `body`: unlike [`Self::click_within`], not inside a reply that only
    /// quotes it. Hovers first.
    pub async fn click_in_message(&mut self, body: &str, selector: &str) -> Result<()> {
        if gpui() {
            let extra = json!({ "within": ".message", "within_contains": body, "within_in": ".body" });
            return self.op("click", selector, extra).await.map(drop);
        }
        let js = format!(
            "const scope = [...document.querySelectorAll('.message')].find(el => el.querySelector('.body')?.textContent.includes({})); \
             if (!scope) throw new Error('no message with ' + {}); \
             const el = scope.querySelector({}); \
             if (!el) throw new Error('no ' + {} + ' inside it'); \
             el.click();",
            js_str(body),
            js_str(body),
            js_str(selector),
            js_str(selector),
        );
        self.eval(&js).await.map(drop)
    }

    /// A file pasted into the message box (`dropped` false) or dropped on
    /// the chat (`dropped` true), as the clipboard or a drag would deliver it.
    pub async fn attach_file(
        &mut self,
        dropped: bool,
        name: &str,
        mime: &str,
        bytes: &[u8],
    ) -> Result<()> {
        use base64::Engine as _;
        if gpui() {
            let extra = json!({
                "name": name,
                "mime": mime,
                "data": base64::engine::general_purpose::STANDARD.encode(bytes),
                "via": if dropped { "drop" } else { "paste" },
            });
            return self.op("attach_file", "", extra).await.map(drop);
        }
        let list: Vec<String> = bytes.iter().map(|b| b.to_string()).collect();
        let transfer = format!(
            "const dt = new DataTransfer(); dt.items.add(new File([new Uint8Array([{}])], {}, {{ type: {} }}));",
            list.join(","),
            js_str(name),
            js_str(mime)
        );
        let js = if dropped {
            format!(
                "{transfer} const target = document.querySelector('.chat'); \
                 target.dispatchEvent(new DragEvent('dragover', {{ bubbles: true, cancelable: true, dataTransfer: dt }})); \
                 target.dispatchEvent(new DragEvent('drop', {{ bubbles: true, cancelable: true, dataTransfer: dt }}));"
            )
        } else {
            format!(
                "{transfer} document.querySelector('.composer .composer-input').dispatchEvent(new ClipboardEvent('paste', {{ bubbles: true, cancelable: true, clipboardData: dt }}));"
            )
        };
        self.eval(&js).await.map(drop)
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
        if gpui() {
            bail!("{NEEDS_PORTING}\n  waiting for: {what}");
        }
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
    /// [`Self::open_room`] in the GPUI app: click the row whose name is
    /// exactly `name` (else one containing it), looking through the spaces.
    async fn open_room_gpui(&mut self, name: &str) -> Result<()> {
        let deadline = Instant::now() + DEFAULT_WAIT;
        loop {
            for filter in [json!({ "exact": name }), json!({ "contains": name })] {
                if self
                    .op("count", ".room .room-name", filter.clone())
                    .await?
                    .as_u64()
                    > Some(0)
                {
                    self.op("click", ".room .room-name", filter).await?;
                    return self
                        .wait_for_text(".chat-header h2", name, DEFAULT_WAIT)
                        .await;
                }
            }
            let badges = self.count(".rail-item").await?;
            for index in 0..badges {
                self.op("click", ".rail-item", json!({ "index": index }))
                    .await?;
                sleep(Duration::from_millis(300)).await;
                let filter = json!({ "exact": name });
                if self
                    .op("count", ".room .room-name", filter.clone())
                    .await?
                    .as_u64()
                    > Some(0)
                {
                    self.op("click", ".room .room-name", filter).await?;
                    return self
                        .wait_for_text(".chat-header h2", name, DEFAULT_WAIT)
                        .await;
                }
            }
            if Instant::now() >= deadline {
                let rows = self.op("texts", ".room", json!({})).await;
                bail!("no room named {name:?} in any space\nrows: {rows:?}");
            }
            sleep(POLL).await;
        }
    }

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

/// Whether the app under test is the GPUI one (`MORPHEUS_E2E_UI=gpui`): its
/// bridge takes structured requests, not JavaScript.
fn gpui() -> bool {
    std::env::var("MORPHEUS_E2E_UI").as_deref() == Ok("gpui")
}

const NEEDS_PORTING: &str =
    "the GPUI app runs no JavaScript: this test needs porting to selector calls";

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
/// The app the tests run (`just e2e-build`'s).
pub fn app_binary() -> Result<PathBuf> {
    binary_path()
}

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
        // Hyprland 0.55+ with a Lua config refuses `keyword` ("can't work with
        // non-legacy parsers"): the same rule through `eval`, which also
        // opens the windows on a monitor of their own (below).
        let place = test_workspace()
            .map(|id| format!(", workspace = \"{id} silent\""))
            .unwrap_or_default();
        let _ = std::process::Command::new("hyprctl")
            .args([
                "eval",
                &format!(
                    "hl.window_rule({{ name = \"morpheus-test\", match = {{ title = \"^(Morpheus \\\\(test\\\\))$\" }}, no_initial_focus = true, no_focus = true, opacity = \"1.0 override 1.0 override\"{place} }})"
                ),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    });
}

/// Hyprland's name for the monitor test windows open on.
const TEST_MONITOR: &str = "MORPHEUS-TEST";

/// A workspace on a monitor nobody looks at (a Hyprland headless output, far
/// off to one side), for the test windows: they open there instead of on
/// the workspace in use, never resizing the windows being worked in. A
/// hidden workspace would not do: Hyprland stops drawing windows there, and
/// their pages stop answering. Big, so windows side by side keep a desktop
/// width. The workspace's number, once the monitor is set up.
fn test_workspace() -> Option<i64> {
    let hyprctl = |args: &[&str]| -> Option<String> {
        let out = std::process::Command::new("hyprctl")
            .args(args)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let find = || -> Option<i64> {
        let monitors: Value = serde_json::from_str(&hyprctl(&["-j", "monitors", "all"])?).ok()?;
        monitors
            .as_array()?
            .iter()
            .find(|m| m["name"] == TEST_MONITOR)
            .and_then(|m| m["activeWorkspace"]["id"].as_i64())
    };
    if find().is_none() {
        hyprctl(&["output", "create", "headless", TEST_MONITOR])?;
    }
    hyprctl(&[
        "eval",
        &format!(
            "hl.monitor({{ output = \"{TEST_MONITOR}\", mode = \"3840x2160\", position = \"-20000x-20000\", scale = 1 }})"
        ),
    ]);
    let id = find()?;
    hyprctl(&[
        "eval",
        &format!(
            "hl.dispatch(hl.dsp.workspace.rename({{ workspace = \"{id}\", name = \"morpheus-tests\" }}))"
        ),
    ]);
    Some(id)
}

/// Whether a test currently holds the phone.
static PHONE_BUSY: AtomicBool = AtomicBool::new(false);

/// The mobile app's package and activity (see `apps/mobile`).
pub const ANDROID_PACKAGE: &str = "dev.morpheus.mobile";

/// `adb shell <args>` on the phone; its output when it succeeds.
pub fn android_shell(args: &[&str]) -> Option<String> {
    let mut all = vec!["shell"];
    all.extend_from_slice(args);
    adb(&all)
}

/// The phone's Home button: the app goes to the background (it keeps running).
pub fn android_home() {
    let _ = android_shell(&["input", "keyevent", "KEYCODE_HOME"]);
}

/// The calls Android's Telecom knows of for the app, as `dumpsys telecom`
/// lists them: each one's state (RINGING, ACTIVE, …).
pub fn android_telecom_calls() -> Vec<String> {
    let dump = android_shell(&["dumpsys", "telecom"]).unwrap_or_default();
    // "Call TC@1 [...] ... state=RINGING ... dev.morpheus.mobile" lines.
    dump.lines()
        .filter(|line| line.contains(ANDROID_PACKAGE) && line.contains("state"))
        .filter_map(|line| {
            let state = line.split("state").nth(1)?;
            let state: String = state
                .trim_start_matches(['=', ':', ' '])
                .chars()
                .take_while(|c| c.is_ascii_uppercase() || *c == '_')
                .collect();
            (!state.is_empty()).then_some(state)
        })
        .collect()
}

/// Tap what shows `text` on the phone's screen (a notification's button,
/// say), as a person would: found through `uiautomator dump`.
pub fn android_tap_text(text: &str) -> Result<()> {
    for attempt in 0..20 {
        // Not on screen after a while: in the notification shade, then.
        if attempt == 3 || attempt == 12 {
            let _ = android_shell(&["cmd", "statusbar", "expand-notifications"]);
        }
        let _ = android_shell(&["uiautomator", "dump", "/sdcard/ui.xml"]);
        let ui = android_shell(&["cat", "/sdcard/ui.xml"]).unwrap_or_default();
        let wanted = [
            format!("text=\"{text}\""),
            format!("content-desc=\"{text}\""),
        ];
        if let Some(node) = ui
            .split("<node ")
            .find(|node| wanted.iter().any(|w| node.contains(w.as_str())))
            && let Some(bounds) = node
                .split("bounds=\"")
                .nth(1)
                .and_then(|b| b.split('"').next())
        {
            // "[x1,y1][x2,y2]": its middle.
            let numbers: Vec<i64> = bounds
                .split(|c: char| !c.is_ascii_digit())
                .filter_map(|n| n.parse().ok())
                .collect();
            if let [x1, y1, x2, y2] = numbers[..] {
                let (x, y) = ((x1 + x2) / 2, (y1 + y2) / 2);
                android_shell(&["input", "tap", &x.to_string(), &y.to_string()])
                    .ok_or_else(|| anyhow!("tap failed"))?;
                return Ok(());
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    let _ = android_shell(&["uiautomator", "dump", "/sdcard/ui.xml"]);
    let ui = android_shell(&["cat", "/sdcard/ui.xml"]).unwrap_or_default();
    let shown: Vec<&str> = ui
        .split("text=\"")
        .skip(1)
        .filter_map(|t| t.split('"').next())
        .filter(|t| !t.is_empty())
        .collect();
    bail!("nothing showing {text:?} on the phone; it shows {shown:?}")
}

/// A picture in the device's gallery, as another app would share it: its
/// `content:` URI (a small PNG pushed to Pictures and indexed).
pub fn android_gallery_picture() -> Result<String> {
    use base64::Engine as _;
    const NAME: &str = "morpheus-share.png";
    // 16×16, a periwinkle square.
    let png = base64::engine::general_purpose::STANDARD.decode(
        "iVBORw0KGgoAAAANSUhEUgAAABAAAAAQCAIAAACQkWg2AAAAF0lEQVR4nGOsmXOHgRTARJLqUQ2jGgYJAAA5CwIBTrRTaAAAAABJRU5ErkJggg==",
    )?;
    let local = std::env::temp_dir().join(NAME);
    std::fs::write(&local, png)?;
    let remote = format!("/sdcard/Pictures/{NAME}");
    adb(&["push", &local.to_string_lossy(), &remote]).ok_or_else(|| anyhow!("adb push"))?;
    adb(&[
        "shell",
        "am",
        "broadcast",
        "-a",
        "android.intent.action.MEDIA_SCANNER_SCAN_FILE",
        "-d",
        &format!("file://{remote}"),
    ]);
    // Indexed shortly after.
    for _ in 0..20 {
        let rows = adb(&[
            "shell",
            "content",
            "query",
            "--uri",
            "content://media/external/images/media",
            "--projection",
            "_id:_display_name",
        ])
        .unwrap_or_default();
        if let Some(id) = rows
            .lines()
            .filter(|row| row.contains(&format!("_display_name={NAME}")))
            .find_map(|row| {
                row.split("_id=")
                    .nth(1)?
                    .split(',')
                    .next()
                    .map(str::trim)
                    .map(str::to_owned)
            })
        {
            return Ok(format!("content://media/external/images/media/{id}"));
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    bail!("{NAME} never showed up in the gallery")
}
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

    /// [`Self::launch_android`] with the phone's real microphone and speakers
    /// in calls, as a person's phone has (test builds otherwise send a tone).
    pub async fn launch_android_with_real_audio() -> Result<Self> {
        PHONE_BUSY.store(true, Ordering::SeqCst);
        Self::start_android_with(true, &[], true).await
    }

    /// Real microphone and camera (no test tone or pattern), both allowed
    /// before the app starts.
    pub async fn launch_android_with_real_media() -> Result<Self> {
        PHONE_BUSY.store(true, Ordering::SeqCst);
        Self::start_android_with(
            true,
            &[
                "android.permission.CAMERA",
                "android.permission.RECORD_AUDIO",
            ],
            true,
        )
        .await
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
        Self::start_android_with(clear, permissions, false).await
    }

    async fn start_android_with(
        clear: bool,
        permissions: &[&str],
        real_audio: bool,
    ) -> Result<Self> {
        // Before the phone signs in: pruning later would sign it out.
        if let Ok(homeserver) = std::env::var("MORPHEUS_TEST_HOMESERVER") {
            prune_test_devices(&homeserver);
        }
        if clear {
            adb(&["shell", "pm", "clear", ANDROID_PACKAGE])
                .context("clearing the app's data (is it installed, and is a device attached?)")?;
        }
        // The app looks for this in its files before choosing call audio.
        if real_audio {
            // (`adb shell` joins its arguments, so no quoting: two commands.)
            adb(&[
                "shell",
                "run-as",
                ANDROID_PACKAGE,
                "mkdir",
                "-p",
                "files/morpheus",
            ])
            .and_then(|_| {
                adb(&[
                    "shell",
                    "run-as",
                    ANDROID_PACKAGE,
                    "touch",
                    "files/morpheus/real_audio",
                ])
            })
            .context("marking the app for real call audio")?;
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
        let _ = adb(&[
            "shell",
            "pm",
            "grant",
            ANDROID_PACKAGE,
            "android.permission.RECORD_AUDIO",
        ]);
        // Calls: the dev homeserver names its token service and LiveKit at
        // localhost (8080, 7880-7881), which on the phone is the phone.
        for port in ["8080", "7880", "7881"] {
            let _ = adb(&["reverse", &format!("tcp:{port}"), &format!("tcp:{port}")]);
        }
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
                    size: (0, 0),
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
        if gpui() {
            return self.click_text(selector, needle).await;
        }
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
            .env("MORPHEUS_CALL_AUDIO", "tone")
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

/// A fresh account on the dev homeserver for one test (created through the
/// admin API as alice, with `curl`); signs in with `<name>-password`.
pub fn new_account(homeserver: &str, name: &str) -> Result<String> {
    let token = admin_token(homeserver)?;
    let user = format!("{name}-{}", unique());
    curl(&[
        "-X",
        "PUT",
        "-H",
        &format!("Authorization: Bearer {token}"),
        &format!("{homeserver}/_synapse/admin/v2/users/@{user}:localhost"),
        "-d",
        &json!({ "password": format!("{user}-password") }).to_string(),
    ])?;
    let _ = curl(&[
        "-X",
        "POST",
        "-H",
        &format!("Authorization: Bearer {token}"),
        &format!("{homeserver}/_matrix/client/v3/logout"),
    ]);
    Ok(user)
}

/// Put `user` (a local part) into `room` (an id or alias such as
/// `#general:localhost`) through the admin API, no invite needed.
pub fn admin_join(homeserver: &str, user: &str, room: &str) -> Result<()> {
    let token = admin_token(homeserver)?;
    let room = room.replace('#', "%23").replace(':', "%3A");
    curl(&[
        "-X",
        "POST",
        "-H",
        &format!("Authorization: Bearer {token}"),
        &format!("{homeserver}/_synapse/admin/v1/join/{room}"),
        "-d",
        &json!({ "user_id": format!("@{user}:localhost") }).to_string(),
    ])?;
    let _ = curl(&[
        "-X",
        "POST",
        "-H",
        &format!("Authorization: Bearer {token}"),
        &format!("{homeserver}/_matrix/client/v3/logout"),
    ]);
    Ok(())
}

fn curl(args: &[&str]) -> Result<String> {
    let out = std::process::Command::new("curl")
        .arg("-sf")
        .args(args)
        .output()?;
    if !out.status.success() {
        bail!("curl {args:?} failed");
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// A fresh access token for alice, the dev homeserver's admin.
fn admin_token(homeserver: &str) -> Result<String> {
    let login = curl(&[
        "-X",
        "POST",
        &format!("{homeserver}/_matrix/client/v3/login"),
        "-d",
        r#"{"type":"m.login.password","identifier":{"type":"m.id.user","user":"alice"},"password":"alice-password"}"#,
    ])?;
    let token: Value = serde_json::from_str(&login)?;
    Ok(token["access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("no admin token"))?
        .to_owned())
}

/// Every test login leaves a device on the dev homeserver, and a long run
/// piles up dozens per user, which slows key sharing and sync until tests
/// time out. Once per test process (before its first app starts, phone or
/// desktop), remove the
/// seeded users' devices, as `just hs-seed` does. Best effort.
fn prune_test_devices(homeserver: &str) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let curl = |args: &[&str]| -> Option<String> {
            let out = std::process::Command::new("curl")
                .arg("-sf")
                .args(args)
                .output()
                .ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
        };
        let Some(login) = curl(&[
            "-X",
            "POST",
            &format!("{homeserver}/_matrix/client/v3/login"),
            "-d",
            r#"{"type":"m.login.password","identifier":{"type":"m.id.user","user":"alice"},"password":"alice-password"}"#,
        ]) else {
            return;
        };
        let Some(token) = serde_json::from_str::<Value>(&login)
            .ok()
            .and_then(|v| v["access_token"].as_str().map(str::to_owned))
        else {
            return;
        };
        let auth = format!("Authorization: Bearer {token}");
        // alice last: her token is the one doing the deleting.
        for user in ["bob", "carol", "alice"] {
            let Some(list) = curl(&[
                "-H",
                &auth,
                &format!("{homeserver}/_synapse/admin/v2/users/@{user}:localhost/devices"),
            ]) else {
                continue;
            };
            let ids: Vec<Value> = serde_json::from_str::<Value>(&list)
                .ok()
                .and_then(|v| v["devices"].as_array().cloned())
                .unwrap_or_default()
                .iter()
                .filter_map(|d| d.get("device_id").cloned())
                .collect();
            let _ = curl(&[
                "-X",
                "POST",
                "-H",
                &auth,
                &format!("{homeserver}/_synapse/admin/v2/users/@{user}:localhost/delete_devices"),
                "-d",
                &json!({ "devices": ids }).to_string(),
            ]);
        }
    });
}

/// One client-server API call as `user` (a local part, password
/// `<user>-password`): `path` from `/_matrix/client/v3/` on, `body` JSON
/// (`None` for a GET). Returns the response's JSON.
pub fn api_as(
    homeserver: &str,
    user: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
) -> Result<Value> {
    let login = curl(&[
        "-X",
        "POST",
        &format!("{homeserver}/_matrix/client/v3/login"),
        "-d",
        &json!({ "type": "m.login.password", "identifier": { "type": "m.id.user", "user": user }, "password": format!("{user}-password") }).to_string(),
    ])?;
    let login: Value = serde_json::from_str(&login)?;
    let token = login["access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("no token for {user}"))?
        .to_owned();
    let auth = format!("Authorization: Bearer {token}");
    let url = format!("{homeserver}/_matrix/client/v3/{path}");
    let body = body.map(|b| b.to_string());
    let mut args = vec!["-X", method, "-H", &auth, &url];
    if let Some(body) = body.as_deref() {
        args.extend(["-d", body]);
    }
    let out = curl(&args);
    let _ = curl(&[
        "-X",
        "POST",
        "-H",
        &auth,
        &format!("{homeserver}/_matrix/client/v3/logout"),
    ]);
    Ok(serde_json::from_str(&out?).unwrap_or(Value::Null))
}

/// Send `body` into `room_id` as `user` (a local part, password
/// `<user>-password`) straight through the API, no app needed.
pub fn send_as(homeserver: &str, user: &str, room_id: &str, body: &str) -> Result<()> {
    let login = curl(&[
        "-X",
        "POST",
        &format!("{homeserver}/_matrix/client/v3/login"),
        "-d",
        &json!({ "type": "m.login.password", "identifier": { "type": "m.id.user", "user": user }, "password": format!("{user}-password") }).to_string(),
    ])?;
    let login: Value = serde_json::from_str(&login)?;
    let token = login["access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("no token for {user}"))?;
    let room = room_id.replace('!', "%21").replace(':', "%3A");
    curl(&[
        "-X",
        "PUT",
        "-H",
        &format!("Authorization: Bearer {token}"),
        &format!(
            "{homeserver}/_matrix/client/v3/rooms/{room}/send/m.room.message/e2e-{}",
            unique()
        ),
        "-d",
        &json!({ "msgtype": "m.text", "body": body }).to_string(),
    ])?;
    let _ = curl(&[
        "-X",
        "POST",
        "-H",
        &format!("Authorization: Bearer {token}"),
        &format!("{homeserver}/_matrix/client/v3/logout"),
    ]);
    Ok(())
}

/// Send each of `bodies` into `room_id` as `user`, in order, on one login
/// (quicker than [`send_as`] for filling a room with history).
pub fn send_many_as(homeserver: &str, user: &str, room_id: &str, bodies: &[String]) -> Result<()> {
    let login = curl(&[
        "-X",
        "POST",
        &format!("{homeserver}/_matrix/client/v3/login"),
        "-d",
        &json!({ "type": "m.login.password", "identifier": { "type": "m.id.user", "user": user }, "password": format!("{user}-password") }).to_string(),
    ])?;
    let login: Value = serde_json::from_str(&login)?;
    let token = login["access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("no token for {user}"))?;
    let auth = format!("Authorization: Bearer {token}");
    let room = room_id.replace('!', "%21").replace(':', "%3A");
    for body in bodies {
        curl(&[
            "-X",
            "PUT",
            "-H",
            &auth,
            &format!(
                "{homeserver}/_matrix/client/v3/rooms/{room}/send/m.room.message/e2e-{}",
                unique()
            ),
            "-d",
            &json!({ "msgtype": "m.text", "body": body }).to_string(),
        ])?;
    }
    let _ = curl(&[
        "-X",
        "POST",
        "-H",
        &auth,
        &format!("{homeserver}/_matrix/client/v3/logout"),
    ]);
    Ok(())
}

/// Send `png` into `room_id` as an image from `user` (see [`send_as`]),
/// with `blurhash` in its info when given; `body` names it.
pub fn send_image_as(
    homeserver: &str,
    user: &str,
    room_id: &str,
    png: &[u8],
    body: &str,
    blurhash: Option<&str>,
) -> Result<()> {
    let login = curl(&[
        "-X",
        "POST",
        &format!("{homeserver}/_matrix/client/v3/login"),
        "-d",
        &json!({ "type": "m.login.password", "identifier": { "type": "m.id.user", "user": user }, "password": format!("{user}-password") }).to_string(),
    ])?;
    let login: Value = serde_json::from_str(&login)?;
    let token = login["access_token"]
        .as_str()
        .ok_or_else(|| anyhow!("no token for {user}"))?;
    let file = std::env::temp_dir().join(format!("morpheus-e2e-{}.png", unique()));
    std::fs::write(&file, png)?;
    let uploaded = curl(&[
        "-X",
        "POST",
        "-H",
        &format!("Authorization: Bearer {token}"),
        "-H",
        "Content-Type: image/png",
        "--data-binary",
        &format!("@{}", file.display()),
        &format!("{homeserver}/_matrix/media/v3/upload?filename=image.png"),
    ])?;
    let _ = std::fs::remove_file(&file);
    let uploaded: Value = serde_json::from_str(&uploaded)?;
    let mut info = json!({ "mimetype": "image/png", "w": 64, "h": 48, "size": png.len() });
    if let Some(hash) = blurhash {
        info["xyz.amorgan.blurhash"] = json!(hash);
    }
    let room = room_id.replace('!', "%21").replace(':', "%3A");
    curl(&[
        "-X",
        "PUT",
        "-H",
        &format!("Authorization: Bearer {token}"),
        &format!("{homeserver}/_matrix/client/v3/rooms/{room}/send/m.room.message/e2e-{}", unique()),
        "-d",
        &json!({ "msgtype": "m.image", "body": body, "url": uploaded["content_uri"], "info": info }).to_string(),
    ])?;
    let _ = curl(&[
        "-X",
        "POST",
        "-H",
        &format!("Authorization: Bearer {token}"),
        &format!("{homeserver}/_matrix/client/v3/logout"),
    ]);
    Ok(())
}

/// How far a scrolling element is scrolled, in pixels: `from_bottom` (0 at
/// its newest end), the `range` it can scroll, and its `viewport` height.
/// (The GPUI timeline only knows the rows it has measured, so its range
/// grows as one scrolls up.)
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollState {
    pub from_bottom: f64,
    pub range: f64,
    pub viewport: f64,
}

/// Where [`App::scroll_to`] goes.
#[derive(Clone, Copy, Debug)]
pub enum ScrollTo {
    Top,
    Bottom,
    /// This many pixels up from the bottom.
    FromBottom(f64),
}

/// An element's box in window pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Bounds {
    pub fn right(&self) -> f64 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }
    pub fn centre_x(&self) -> f64 {
        self.x + self.w / 2.0
    }
}
