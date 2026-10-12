use std::{
    future::Future,
    path::{Path, PathBuf},
    sync::Arc,
};

use tokio::runtime::Runtime;

use crate::{CoreFuture, Result, SessionStoreKind};

/// Operations slower than this are logged at warn level with their call site.
const SLOW_OP: std::time::Duration = std::time::Duration::from_millis(750);

/// Entry point of the core. Cheap to clone; all clones share one runtime.
#[derive(Clone)]
pub struct Core {
    inner: Arc<Inner>,
}

struct Inner {
    /// `Option` only so `Drop` can take it and shut it down without blocking.
    runtime: Option<Runtime>,
    data_dir: PathBuf,
    /// Where session credentials go; `None` decides at runtime (see
    /// `session_store::resolve`).
    session_store_kind: Option<SessionStoreKind>,
}

impl Core {
    /// Create a core that persists everything under `data_dir`.
    pub fn new(data_dir: impl Into<PathBuf>) -> Result<Self> {
        Self::build(data_dir, None)
    }

    /// Like [`Core::new`] with the session credential store pinned instead
    /// of probing the OS keyring (tests use the encrypted file).
    pub fn with_session_store(data_dir: impl Into<PathBuf>, kind: SessionStoreKind) -> Result<Self> {
        Self::build(data_dir, Some(kind))
    }

    fn build(data_dir: impl Into<PathBuf>, session_store_kind: Option<SessionStoreKind>) -> Result<Self> {
        let data_dir = data_dir.into();
        std::fs::create_dir_all(&data_dir)?;

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("morpheus-core")
            .build()?;

        Ok(Self {
            inner: Arc::new(Inner {
                runtime: Some(runtime),
                data_dir,
                session_store_kind,
            }),
        })
    }

    pub fn data_dir(&self) -> &Path {
        &self.inner.data_dir
    }

    pub(crate) fn session_store_kind(&self) -> Option<SessionStoreKind> {
        self.inner.session_store_kind
    }

    /// Run a fallible future on the core runtime.
    ///
    /// The returned future only awaits a join handle, so it can be polled from
    /// any executor (Dioxus, a test harness, ...).
    #[track_caller]
    pub(crate) fn run<T, F>(&self, fut: F) -> CoreFuture<T>
    where
        T: Send + 'static,
        F: Future<Output = Result<T>> + Send + 'static,
    {
        // Named after the call site so slow operations are easy to attribute.
        let site = std::panic::Location::caller();
        let op = format!(
            "{}:{}",
            site.file().rsplit('/').next().unwrap_or(site.file()),
            site.line()
        );
        let join = self.runtime().spawn(fut);
        Box::pin(async move {
            let started = std::time::Instant::now();
            let result = join.await?;
            let elapsed = started.elapsed();
            if elapsed > SLOW_OP {
                tracing::warn!(%op, ?elapsed, "slow core operation");
            } else {
                tracing::trace!(%op, ?elapsed, "core operation");
            }
            result
        })
    }

    /// Fire-and-forget a task on the core runtime.
    pub(crate) fn spawn<F>(&self, fut: F) -> tokio::task::JoinHandle<F::Output>
    where
        F: Future + Send + 'static,
        F::Output: Send + 'static,
    {
        self.runtime().spawn(fut)
    }

    fn runtime(&self) -> &Runtime {
        self.inner
            .runtime
            .as_ref()
            .unwrap_or_else(|| unreachable!("runtime is only taken in Drop"))
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        // The last `Core` may be dropped from inside another runtime (Dioxus
        // desktop runs on Tokio). A blocking shutdown would panic there.
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

impl PartialEq for Core {
    /// Two handles are equal when they share the same runtime and data directory.
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}
