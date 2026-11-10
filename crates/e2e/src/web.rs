//! Driving the web app (apps/web) in a headless Chromium.
//!
//! The page cannot listen on a port, so it asks instead: a relay serves the
//! built app and, beside it, `/__e2e/next` (the next script to run, waited
//! for) and `/__e2e/answer` (its result). On the driver's side the relay
//! speaks the desktop bridge's line protocol on a port of its own, so the
//! driver talks to a page exactly as it talks to a desktop app.
//!
//! Each app is a Chromium profile under the test's data directory: its own
//! storage (the session, IndexedDB), kept by a relaunch.

use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, anyhow};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    process::{Child, Command},
    sync::Mutex,
};

/// Whether this run drives the web app (`MORPHEUS_E2E_WEB=1`).
pub fn enabled() -> bool {
    std::env::var_os("MORPHEUS_E2E_WEB").is_some()
}

/// The built web app: `MORPHEUS_E2E_WEB_DIR`, else where `just e2e-web-build`
/// puts it.
fn site_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("MORPHEUS_E2E_WEB_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = root.join("target/dx/morpheus-web/debug/web/public");
    if dir.join("index.html").exists() {
        Ok(dir)
    } else {
        Err(anyhow!(
            "no web build at {} (run `just e2e-web-build`)",
            dir.display()
        ))
    }
}

/// Start the page: the relay, then Chromium on it. Returns the browser and
/// the port the driver connects to.
pub async fn start(
    homeserver: &str,
    data_dir: &Path,
    size: (u32, u32),
    log: std::fs::File,
) -> Result<(Child, u16)> {
    let site = site_dir()?;
    let driver = TcpListener::bind(("127.0.0.1", 0)).await?;
    let page = TcpListener::bind(("127.0.0.1", 0)).await?;
    let driver_port = driver.local_addr()?.port();
    let page_port = page.local_addr()?.port();
    tokio::spawn(relay(driver, page, site));

    let chrome = std::env::var("CHROME").unwrap_or_else(|_| "chromium".into());
    // Written afresh by this Chromium (a stale one points at a closed port).
    let _ = std::fs::remove_file(data_dir.join("chrome/DevToolsActivePort"));
    let url = format!(
        "http://127.0.0.1:{page_port}/?e2e=1&homeserver={}",
        encode(homeserver)
    );
    let child = Command::new(&chrome)
        .args([
            "--headless=new",
            "--no-sandbox",
            "--no-first-run",
            "--no-default-browser-check",
            "--disable-background-timer-throttling",
            "--disable-renderer-backgrounding",
            "--autoplay-policy=no-user-gesture-required",
            "--enable-logging=stderr",
            // For screenshots (DevTools; the port is written to the profile).
            "--remote-debugging-port=0",
            "--v=0",
        ])
        .arg(format!(
            "--user-data-dir={}",
            data_dir.join("chrome").display()
        ))
        .arg(format!("--window-size={},{}", size.0, size.1))
        .arg(url)
        .stdout(Stdio::from(log.try_clone()?))
        .stderr(Stdio::from(log))
        .kill_on_drop(true)
        .spawn()
        .with_context(|| format!("starting {chrome}"))?;
    Ok((child, driver_port))
}

fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// Scripts from the driver, waiting for the page to ask for them, and the
/// one handed out and not answered yet: a page that reloads drops the
/// request it was waiting on, so the next page to ask gets that one again.
#[derive(Default)]
struct Queue {
    waiting: std::collections::VecDeque<String>,
    handed_out: Option<String>,
}
type Pending = Arc<(Mutex<Queue>, tokio::sync::Notify)>;
/// Where the page's answers go: the driver's connection.
type Answers = Arc<Mutex<Option<tokio::net::tcp::OwnedWriteHalf>>>;

async fn relay(driver: TcpListener, page: TcpListener, site: PathBuf) {
    let pending: Pending = Arc::default();
    let answers: Answers = Arc::new(Mutex::new(None));
    // The driver: one connection at a time (a relaunch connects anew).
    {
        let (answers, pending) = (answers.clone(), pending.clone());
        tokio::spawn(async move {
            while let Ok((stream, _)) = driver.accept().await {
                let (reader, writer) = stream.into_split();
                *answers.lock().await = Some(writer);
                let mut lines = BufReader::new(reader).lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    pending.0.lock().await.waiting.push_back(line);
                    pending.1.notify_waiters();
                }
            }
        });
    }
    // The page.
    while let Ok((stream, _)) = page.accept().await {
        let (pending, answers, site) = (pending.clone(), answers.clone(), site.clone());
        tokio::spawn(async move {
            let _ = serve(stream, pending, answers, &site).await;
        });
    }
}

/// One HTTP request from the page.
async fn serve(
    mut stream: TcpStream,
    pending: Pending,
    answers: Answers,
    site: &Path,
) -> Result<()> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        if stream.read(&mut byte).await? == 0 {
            return Ok(());
        }
        head.push(byte[0]);
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    let mut lines = head.lines();
    let first = lines.next().unwrap_or_default();
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or_default().to_owned();
    let target = parts.next().unwrap_or("/").to_owned();
    let length: usize = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.trim().parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body).await?;
    let path = target.split('?').next().unwrap_or("/");

    match (method.as_str(), path) {
        ("GET", "/__e2e/next") => {
            let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
            let next = loop {
                let woken = pending.1.notified();
                {
                    let mut queue = pending.0.lock().await;
                    if let Some(script) = queue.handed_out.clone() {
                        break Some(script);
                    }
                    if let Some(script) = queue.waiting.pop_front() {
                        queue.handed_out = Some(script.clone());
                        break Some(script);
                    }
                }
                if tokio::time::timeout_at(deadline, woken).await.is_err() {
                    break None;
                }
            };
            match next {
                Some(script) => {
                    respond(&mut stream, "200 OK", "application/json", script.as_bytes()).await
                }
                _ => respond(&mut stream, "204 No Content", "text/plain", b"").await,
            }
        }
        ("POST", "/__e2e/answer") => {
            pending.0.lock().await.handed_out = None;
            if let Some(writer) = answers.lock().await.as_mut() {
                let mut line = body;
                line.push(b'\n');
                writer.write_all(&line).await?;
            }
            respond(&mut stream, "200 OK", "text/plain", b"").await
        }
        _ => {
            let relative = path.trim_start_matches('/');
            let mut file = site.join(relative);
            if relative.is_empty() || !file.is_file() {
                file = site.join("index.html");
            }
            let bytes = tokio::fs::read(&file).await.unwrap_or_default();
            respond(&mut stream, "200 OK", mime(&file), &bytes).await
        }
    }
}

async fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await?;
    stream.flush().await?;
    Ok(())
}

fn mime(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
    {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript",
        "wasm" => "application/wasm",
        "css" => "text/css",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "json" => "application/json",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

/// A PNG of the page, through Chromium's DevTools protocol (a headless
/// browser has no window to capture).
pub(crate) async fn screenshot(data_dir: &Path, size: (u32, u32), to: &Path) -> Result<()> {
    use futures_util::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    let active = std::fs::read_to_string(data_dir.join("chrome/DevToolsActivePort"))
        .context("Chromium's DevTools port")?;
    let port: u16 = active
        .lines()
        .next()
        .and_then(|l| l.trim().parse().ok())
        .ok_or_else(|| anyhow!("no DevTools port"))?;
    // The page's own endpoint, from /json/list.
    let mut http = TcpStream::connect(("127.0.0.1", port))
        .await
        .with_context(|| format!("DevTools on port {port} ({active:?})"))?;
    // The Host (with its port) is what the page's endpoint is built from.
    http.write_all(
        format!("GET /json/list HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )
    .await?;
    let mut reply = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(5), http.read_to_end(&mut reply)).await;
    let reply = String::from_utf8_lossy(&reply).into_owned();
    let body = reply.split("\r\n\r\n").nth(1).unwrap_or_default();
    let targets: serde_json::Value =
        serde_json::from_str(body).with_context(|| format!("DevTools' page list: {reply:?}"))?;
    let ws = targets
        .as_array()
        .and_then(|list| list.iter().find(|t| t["type"] == "page"))
        .and_then(|t| t["webSocketDebuggerUrl"].as_str())
        .ok_or_else(|| anyhow!("no page to capture"))?
        // Chromium listens on IPv4; "localhost" may resolve to ::1 first.
        .replace("ws://localhost:", "ws://127.0.0.1:");
    let (mut socket, _) = tokio::time::timeout(
        Duration::from_secs(10),
        tokio_tungstenite::connect_async(ws.as_str()),
    )
    .await
    .context("connecting to the page's DevTools")?
    .with_context(|| format!("the page's DevTools at {ws}"))?;
    // The page at the size asked for (a headless window's includes the
    // browser's own bars), laid out again, then the picture.
    socket
        .send(Message::text(
            serde_json::json!({ "id": 1, "method": "Emulation.setDeviceMetricsOverride",
                "params": { "width": size.0, "height": size.1, "deviceScaleFactor": 1, "mobile": false } })
            .to_string(),
        ))
        .await?;
    tokio::time::sleep(Duration::from_millis(1500)).await;
    socket
        .send(Message::text(
            serde_json::json!({ "id": 2, "method": "Page.captureScreenshot", "params": { "format": "png" } })
                .to_string(),
        ))
        .await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
    while let Ok(Some(message)) = tokio::time::timeout_at(deadline, socket.next()).await {
        let Message::Text(text) = message? else {
            continue;
        };
        let reply: serde_json::Value = serde_json::from_str(&text)?;
        if reply["id"] != 2 {
            continue;
        }
        let data = reply["result"]["data"]
            .as_str()
            .ok_or_else(|| anyhow!("no picture: {reply}"))?;
        let png = base64::Engine::decode(&base64::engine::general_purpose::STANDARD, data)?;
        std::fs::write(to, png)?;
        return Ok(());
    }
    Err(anyhow!("DevTools closed before the picture"))
}
