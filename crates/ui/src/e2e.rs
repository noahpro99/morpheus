//! Test bridge for end-to-end tests (`e2e` feature only).
//!
//! When `MORPHEUS_E2E_PORT` is set, the app listens on that loopback port for
//! newline-delimited JSON requests `{"id": n, "js": "..."}` and answers each
//! with `{"id": n, "ok": bool, "value": ..., "error": ...}` after evaluating
//! the JavaScript inside the webview. `crates/e2e` wraps this in a
//! Playwright-style driver. Never enable the feature in release builds.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};
use tracing::{info, warn};

const PORT_ENV: &str = "MORPHEUS_E2E_PORT";

#[derive(Deserialize)]
struct Request {
    id: u64,
    js: String,
}

#[derive(Serialize)]
struct Response {
    id: u64,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

/// Renders nothing; runs the bridge for the lifetime of the app.
#[component]
pub fn E2eBridge() -> Element {
    use_future(|| async {
        let Some(port) = std::env::var(PORT_ENV)
            .ok()
            .and_then(|p| p.parse::<u16>().ok())
        else {
            return;
        };
        let listener = match TcpListener::bind(("127.0.0.1", port)).await {
            Ok(listener) => listener,
            Err(err) => {
                warn!(%err, port, "e2e bridge could not bind");
                return;
            }
        };
        info!(port, "e2e bridge listening");

        // Clients are served one at a time; tests are sequential by nature.
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let (reader, mut writer) = stream.into_split();
            let mut lines = BufReader::new(reader).lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let response = match serde_json::from_str::<Request>(&line) {
                    Ok(request) => evaluate(request).await,
                    Err(err) => Response {
                        id: 0,
                        ok: false,
                        value: None,
                        error: Some(format!("bad request: {err}")),
                    },
                };
                let Ok(mut encoded) = serde_json::to_string(&response) else {
                    break;
                };
                encoded.push('\n');
                if writer.write_all(encoded.as_bytes()).await.is_err() {
                    break;
                }
            }
        }
    });

    rsx! {}
}

async fn evaluate(request: Request) -> Response {
    match document::eval(&request.js).await {
        Ok(value) => Response {
            id: request.id,
            ok: true,
            value: Some(value),
            error: None,
        },
        Err(err) => Response {
            id: request.id,
            ok: false,
            value: None,
            error: Some(format!("{err:?}")),
        },
    }
}
