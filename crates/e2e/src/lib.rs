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
    child: Child,
    reader: Lines<BufReader<OwnedReadHalf>>,
    writer: OwnedWriteHalf,
    next_id: u64,
    data_dir: PathBuf,
    log_path: PathBuf,
    homeserver: String,
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
        let data_dir = std::env::temp_dir().join(format!("morpheus-e2e-{}", unique()));
        Self::launch_with_data_dir(homeserver, data_dir).await
    }

    /// Relaunch on the same data directory, e.g. to test session restore.
    pub async fn relaunch(mut self) -> Result<Self> {
        let data_dir = self.data_dir.clone();
        let homeserver = self.homeserver.clone();
        let _ = self.child.kill().await;
        // Keep the data dir: `Drop` would delete it.
        std::mem::forget(self);
        Self::launch_with_data_dir(&homeserver, data_dir).await
    }

    async fn launch_with_data_dir(homeserver: &str, data_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&data_dir)?;
        let port = free_port().await?;
        let bin = binary_path()?;
        let log_path = data_dir.join("app.log");
        let log = std::fs::File::create(&log_path)?;

        let child = Command::new(&bin)
            .env("MORPHEUS_E2E_PORT", port.to_string())
            .env("MORPHEUS_DATA_DIR", &data_dir)
            .env("MORPHEUS_HOMESERVER", homeserver)
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
            child,
            reader: BufReader::new(reader).lines(),
            writer,
            next_id: 1,
            data_dir,
            log_path,
            homeserver: homeserver.to_owned(),
        })
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

    pub async fn click(&mut self, selector: &str) -> Result<()> {
        self.eval(&format!("{}.click();", query(selector)))
            .await
            .map(drop)
    }

    /// Click the first `selector` element whose text contains `needle`.
    pub async fn click_text(&mut self, selector: &str, needle: &str) -> Result<()> {
        let (sel, text) = (js_str(selector), js_str(needle));
        let js = format!(
            "const el = [...document.querySelectorAll({sel})].find(el => el.textContent.includes({text}));
             if (!el) throw new Error('no element matching ' + {sel} + ' with text ' + {text});
             el.click();"
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
        let Some(pid) = self.child.id() else {
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
        let deadline = Instant::now() + timeout;
        loop {
            if self.eval(predicate_js).await?.as_bool().unwrap_or(false) {
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

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
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
