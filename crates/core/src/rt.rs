//! The async runtime underneath: Tokio on desktops and phones, the page's own
//! event loop in a browser (apps/web), where nothing is `Send` and there are
//! no threads. Core and UI code use these instead of `tokio::` directly.

pub use std::time::Duration;

/// `Send` where there are threads; nothing in a browser.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSend: Send {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Send + ?Sized> MaybeSend for T {}
#[cfg(target_arch = "wasm32")]
pub trait MaybeSend {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSend for T {}

/// `Sync` where there are threads; nothing in a browser.
#[cfg(not(target_arch = "wasm32"))]
pub trait MaybeSync: Sync {}
#[cfg(not(target_arch = "wasm32"))]
impl<T: Sync + ?Sized> MaybeSync for T {}
#[cfg(target_arch = "wasm32")]
pub trait MaybeSync {}
#[cfg(target_arch = "wasm32")]
impl<T: ?Sized> MaybeSync for T {}

/// A boxed future, `Send` where there are threads.
#[cfg(not(target_arch = "wasm32"))]
pub type BoxFuture<'a, T> = futures_util::future::BoxFuture<'a, T>;
#[cfg(target_arch = "wasm32")]
pub type BoxFuture<'a, T> = futures_util::future::LocalBoxFuture<'a, T>;

/// A boxed stream, `Send` where there are threads.
#[cfg(not(target_arch = "wasm32"))]
pub type BoxStream<'a, T> = futures_util::stream::BoxStream<'a, T>;
#[cfg(target_arch = "wasm32")]
pub type BoxStream<'a, T> = futures_util::stream::LocalBoxStream<'a, T>;

/// Box a future as [`BoxFuture`].
pub fn boxed<'a, F: std::future::Future + MaybeSend + 'a>(fut: F) -> BoxFuture<'a, F::Output> {
    Box::pin(fut)
}

/// Box a stream as [`BoxStream`].
pub fn boxed_stream<'a, S: futures_util::Stream + MaybeSend + 'a>(
    stream: S,
) -> BoxStream<'a, S::Item> {
    Box::pin(stream)
}

#[cfg(not(target_arch = "wasm32"))]
mod imp {
    use std::future::Future;

    pub use std::time::{SystemTime, UNIX_EPOCH};
    pub use tokio::task::{AbortHandle, JoinError, JoinHandle};
    pub use tokio::time::{Instant, error::Elapsed, timeout};

    pub async fn sleep(duration: std::time::Duration) {
        tokio::time::sleep(duration).await;
    }

    /// Run a task in the background (on the current Tokio runtime).
    pub fn spawn<F>(fut: F) -> JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        tokio::spawn(fut)
    }

    /// Run blocking work (disk, the keyring) off the async threads.
    pub async fn spawn_blocking<T, F>(work: F) -> Result<T, JoinError>
    where
        F: FnOnce() -> T + Send + 'static,
        T: Send + 'static,
    {
        tokio::task::spawn_blocking(work).await
    }
}

#[cfg(target_arch = "wasm32")]
mod imp {
    use std::{
        future::Future,
        pin::Pin,
        task::{Context, Poll},
    };

    use futures_util::{
        FutureExt,
        future::{AbortHandle as Abort, Abortable},
    };
    use tokio::sync::oneshot;

    pub use web_time::{Instant, SystemTime, UNIX_EPOCH};

    pub async fn sleep(duration: std::time::Duration) {
        gloo_timers::future::sleep(duration).await;
    }

    /// The task was aborted before it finished.
    #[derive(Debug, thiserror::Error)]
    #[error("task cancelled")]
    pub struct JoinError;

    /// Stops a task started with [`spawn`].
    #[derive(Clone, Debug)]
    pub struct AbortHandle(Abort);

    impl AbortHandle {
        pub fn abort(&self) {
            self.0.abort();
        }
    }

    /// A task on the page's event loop: await it for its result, or abort it.
    pub struct JoinHandle<T> {
        result: oneshot::Receiver<T>,
        abort: AbortHandle,
    }

    impl<T> JoinHandle<T> {
        pub fn abort(&self) {
            self.abort.abort();
        }

        pub fn abort_handle(&self) -> AbortHandle {
            self.abort.clone()
        }
    }

    impl<T> Future for JoinHandle<T> {
        type Output = Result<T, JoinError>;
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            self.result.poll_unpin(cx).map(|r| r.map_err(|_| JoinError))
        }
    }

    pub fn spawn<F>(fut: F) -> JoinHandle<F::Output>
    where
        F: Future + 'static,
        F::Output: 'static,
    {
        let (tx, rx) = oneshot::channel();
        let (abort, registration) = Abort::new_pair();
        wasm_bindgen_futures::spawn_local(async move {
            if let Ok(output) = Abortable::new(fut, registration).await {
                let _ = tx.send(output);
            }
        });
        JoinHandle {
            result: rx,
            abort: AbortHandle(abort),
        }
    }

    /// No threads in a browser: the work runs now.
    pub async fn spawn_blocking<T, F>(work: F) -> Result<T, JoinError>
    where
        F: FnOnce() -> T + 'static,
        T: 'static,
    {
        Ok(work())
    }

    /// The future ran out of time.
    #[derive(Debug, thiserror::Error)]
    #[error("deadline has elapsed")]
    pub struct Elapsed;

    pub async fn timeout<F: Future>(
        duration: std::time::Duration,
        fut: F,
    ) -> Result<F::Output, Elapsed> {
        let fut = std::pin::pin!(fut);
        let timer = std::pin::pin!(sleep(duration));
        match futures_util::future::select(fut, timer).await {
            futures_util::future::Either::Left((output, _)) => Ok(output),
            futures_util::future::Either::Right(_) => Err(Elapsed),
        }
    }
}

pub use imp::*;
