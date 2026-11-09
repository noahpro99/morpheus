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
