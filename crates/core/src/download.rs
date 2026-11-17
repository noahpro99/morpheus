//! A file fetched and checked: streamed to a `.part` next to its place with
//! progress, kept only if its length and SHA-256 are the expected ones, and
//! asked for again (after a pause) when it came partial or corrupt or the
//! server or network failed for a moment. Used for the low-delay noise
//! model (#37).

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use sha2::{Digest as _, Sha256};

use crate::{Core, CoreFuture, Error};

/// How far a download is, shared with whoever shows it.
#[derive(Debug, Default)]
pub struct DownloadProgress {
    pub done: AtomicU64,
    pub total: AtomicU64,
}

/// The pauses before each new try after the first.
const BACKOFF: [Duration; 2] = [Duration::from_secs(1), Duration::from_secs(3)];

/// Whether another try could help: not for a refusal (a 4xx other than 429).
fn worth_retrying(err: &Failure) -> bool {
    match err {
        Failure::Status(code) => *code >= 500 || *code == 429,
        _ => true,
    }
}

#[derive(Debug)]
enum Failure {
    Status(u16),
    Network(String),
    Wrong(String),
}

impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failure::Status(code) => write!(f, "the server answered {code}"),
            Failure::Network(why) => write!(f, "the download failed: {why}"),
            Failure::Wrong(why) => write!(f, "{why}"),
        }
    }
}

async fn once(
    http: &matrix_sdk::reqwest::Client,
    url: &str,
    part: &Path,
    sha256: &str,
    len: u64,
    progress: &DownloadProgress,
) -> Result<(), Failure> {
    let net = |err: &dyn std::fmt::Display| Failure::Network(err.to_string());
    let mut response = http
        .get(url)
        .header("User-Agent", "Morpheus")
        .send()
        .await
        .map_err(|e| net(&e))?;
    if !response.status().is_success() {
        return Err(Failure::Status(response.status().as_u16()));
    }
    progress.done.store(0, Ordering::Relaxed);
    progress
        .total
        .store(response.content_length().unwrap_or(len), Ordering::Relaxed);
    let mut file = std::fs::File::create(part).map_err(|e| net(&e))?;
    let mut hasher = Sha256::new();
    let mut got = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(|e| net(&e))? {
        std::io::Write::write_all(&mut file, &chunk).map_err(|e| net(&e))?;
        hasher.update(&chunk);
        got += chunk.len() as u64;
        progress.done.store(got, Ordering::Relaxed);
    }
    drop(file);
    let hash: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if got != len || hash != sha256 {
        return Err(Failure::Wrong(format!(
            "the file came partial or damaged ({got} of {len} bytes); it was not used"
        )));
    }
    Ok(())
}

/// Fetch `url` into `dest`, kept only if it is `len` bytes with `sha256`.
pub(crate) async fn fetch_verified(
    http: matrix_sdk::reqwest::Client,
    url: String,
    dest: PathBuf,
    sha256: String,
    len: u64,
    progress: Arc<DownloadProgress>,
    backoff: &[Duration],
) -> Result<(), Error> {
    let failed = |err: &dyn std::fmt::Display| Error::Other(format!("download: {err}"));
    if let Some(folder) = dest.parent() {
        std::fs::create_dir_all(folder).map_err(|e| failed(&e))?;
    }
    let part = dest.with_extension("part");
    let mut tries = std::iter::once(Duration::ZERO).chain(backoff.iter().copied());
    let mut last = None;
    while let Some(wait) = tries.next() {
        if !wait.is_zero() {
            tokio::time::sleep(wait).await;
        }
        match once(&http, &url, &part, &sha256, len, &progress).await {
            Ok(()) => {
                std::fs::rename(&part, &dest).map_err(|e| failed(&e))?;
                return Ok(());
            }
            Err(err) => {
                let _ = std::fs::remove_file(&part);
                let retry = worth_retrying(&err);
                last = Some(err);
                if !retry {
                    break;
                }
            }
        }
    }
    Err(failed(&last.map_or_else(|| "nothing was tried".to_owned(), |e| e.to_string())))
}

fn client() -> matrix_sdk::reqwest::Client {
    matrix_sdk::reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .build()
        .unwrap_or_default()
}

impl Core {
    /// Download `url` to `dest` (its folder made), checked against `sha256`
    /// and `len`, retried on a failure that may pass; `progress` follows it.
    pub fn download_verified(
        &self,
        url: String,
        dest: PathBuf,
        sha256: String,
        len: u64,
        progress: Arc<DownloadProgress>,
    ) -> CoreFuture<()> {
        self.run(fetch_verified(client(), url, dest, sha256, len, progress, &BACKOFF))
    }
}

impl crate::Session {
    /// [`Core::download_verified`], for a signed-in app.
    pub fn download_verified(
        &self,
        url: String,
        dest: PathBuf,
        sha256: String,
        len: u64,
        progress: Arc<DownloadProgress>,
    ) -> CoreFuture<()> {
        self.core
            .run(fetch_verified(client(), url, dest, sha256, len, progress, &BACKOFF))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

    /// A server that answers each request with the next of `replies`: a
    /// status line, then the body, whose declared length may be longer than
    /// what is sent (the connection then drops: a partial file).
    async fn serve(replies: Vec<(u16, Vec<u8>, usize)>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            for (status, body, declared) in replies {
                let Ok((mut stream, _)) = listener.accept().await else { return };
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf).await;
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {declared}\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(head.as_bytes()).await;
                let _ = stream.write_all(&body).await;
                let _ = stream.shutdown().await;
            }
        });
        format!("http://{addr}/model")
    }

    fn hash(bytes: &[u8]) -> String {
        Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
    }

    fn dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("morpheus-dl-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    const NONE: [Duration; 2] = [Duration::ZERO, Duration::ZERO];

    #[tokio::test]
    async fn a_good_file_is_kept_with_progress_and_no_part_left() {
        let body = vec![7u8; 100_000];
        let url = serve(vec![(200, body.clone(), body.len())]).await;
        let dest = dir("good").join("m.bin");
        let progress = Arc::new(DownloadProgress::default());
        fetch_verified(
            matrix_sdk::reqwest::Client::new(),
            url,
            dest.clone(),
            hash(&body),
            body.len() as u64,
            progress.clone(),
            &NONE,
        )
        .await
        .expect("downloaded");
        assert_eq!(std::fs::read(&dest).expect("file"), body);
        assert!(!dest.with_extension("part").exists());
        assert_eq!(progress.done.load(Ordering::Relaxed), body.len() as u64);
    }

    #[tokio::test]
    async fn a_partial_then_a_corrupt_then_a_good_file_is_retried() {
        let body = vec![9u8; 50_000];
        let mut bad = body.clone();
        bad[10] ^= 1;
        let url = serve(vec![
            (200, body[..20_000].to_vec(), body.len()),
            (200, bad.clone(), bad.len()),
            (200, body.clone(), body.len()),
        ])
        .await;
        let dest = dir("retry").join("m.bin");
        fetch_verified(
            matrix_sdk::reqwest::Client::new(),
            url,
            dest.clone(),
            hash(&body),
            body.len() as u64,
            Arc::default(),
            &NONE,
        )
        .await
        .expect("the third try is good");
        assert_eq!(std::fs::read(&dest).expect("file"), body);
    }

    #[tokio::test]
    async fn a_file_that_never_checks_out_is_not_kept() {
        let body = vec![1u8; 10_000];
        let url = serve(vec![(200, vec![2u8; 10_000], 10_000); 3]).await;
        let dest = dir("never").join("m.bin");
        let err = fetch_verified(
            matrix_sdk::reqwest::Client::new(),
            url,
            dest.clone(),
            hash(&body),
            body.len() as u64,
            Arc::default(),
            &NONE,
        )
        .await
        .expect_err("damaged");
        assert!(err.to_string().contains("partial or damaged"), "{err}");
        assert!(!dest.exists() && !dest.with_extension("part").exists());
    }

    #[tokio::test]
    async fn a_refusal_is_not_asked_again_but_a_server_error_is() {
        let body = vec![3u8; 1000];
        let url = serve(vec![(404, Vec::new(), 0), (200, body.clone(), body.len())]).await;
        let err = fetch_verified(
            matrix_sdk::reqwest::Client::new(),
            url,
            dir("404").join("m.bin"),
            hash(&body),
            body.len() as u64,
            Arc::default(),
            &NONE,
        )
        .await
        .expect_err("404");
        assert!(err.to_string().contains("404"), "{err}");
        let url = serve(vec![(503, Vec::new(), 0), (200, body.clone(), body.len())]).await;
        fetch_verified(
            matrix_sdk::reqwest::Client::new(),
            url,
            dir("503").join("m.bin"),
            hash(&body),
            body.len() as u64,
            Arc::default(),
            &NONE,
        )
        .await
        .expect("the second try is good");
    }
}
