#![allow(clippy::expect_used)]
//! A `wss://` call service (matrix.org's, any real one) needs TLS, and the
//! app carries two rustls crypto backends: connecting must not stop at
//! "could not determine the crypto provider". A local socket that accepts
//! and says nothing stands in for the service, so TLS gets set up and then
//! the handshake fails.

use morpheus_call::{AudioMode, Call};

#[tokio::test]
async fn a_tls_call_service_gets_past_choosing_the_crypto_provider() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listen");
    let port = listener.local_addr().expect("address").port();
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            tokio::spawn(async move {
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                drop(socket);
            });
        }
    });
    let err = match Call::connect(
        &format!("wss://127.0.0.1:{port}"),
        "not-a-token",
        AudioMode::Tone,
    )
    .await
    {
        Ok(_) => panic!("nothing answers there"),
        Err(err) => err.to_string(),
    };
    let lower = err.to_lowercase();
    assert!(
        !lower.contains("crypto") && !lower.contains("provider"),
        "{err}"
    );
}
