//! Morpheus for native phone apps: the shared core and view-models behind a
//! small UniFFI surface (Kotlin today, Swift later).
//!
//! The shape follows what the screens need, not the core's own API:
//!
//! - [`Morpheus`] is the one object an app makes: it signs in, restores the
//!   session, signs out.
//! - Live data (the sidebar, a room's timeline, the sync state) is pushed to a
//!   listener the app passes in; each call returns a [`TaskHandle`] that stops
//!   it. The UI never polls. Kotlin wraps a listener in a `callbackFlow`
//!   (`apps/android`, docs/ANDROID.md).
//! - Updates are whole snapshots of what the screen lists (the channel list;
//!   the timeline's rows), made by `morpheus-view-model`: the grouping,
//!   scopes and categories are decided there, the same as on the desktop. The
//!   core's incremental diffs are applied here, in Rust.
//! - Pictures are [`MediaHandle`]s the app asks for bytes when it draws them.
//!
//! Async functions are plain `suspend` functions on the Kotlin side. The core
//! runs its own runtime, so they work on any executor; the forwarding tasks
//! of the listeners run on this crate's own small runtime.

mod composer;
mod error;
mod media;
mod people;
mod push;
mod room_info;
mod security;
mod settings;
mod sidebar;
mod timeline;

use std::sync::{Arc, Mutex, OnceLock};

use morpheus_core::{Core, Session};
use tokio::{runtime::Runtime, task::JoinHandle};

pub use error::FfiError;
pub use composer::{CustomEmojiItem, CustomEmojiSection, EmojiSection, SuggestionItem, SuggestionKind, emoji_sections, with_emoji};
pub use media::MediaHandle;
pub use settings::{Appearance, LanguageChoice, NotifyDefaults, PaletteItem, ThemeChoice};
pub use room_info::{MemberItem, MemberRow, ProfileCard, TypingListener};
pub use people::{DirectoryItem, DirectoryPage, FriendItem, InviteItem};
pub use push::{PushNotice, preview_push};
pub use security::{
    ButtonKind, IncomingVerificationListener, PanelButton, PanelCode, PanelEmoji, PanelTone, RecoveryKind,
    SecurityStatus, VerificationAction, VerificationFlow, VerificationListener, VerificationPanel,
};
pub use sidebar::{RoomItem, ScopeItem, ScopeKind, SectionItem, Sidebar, SidebarListener};
pub use timeline::{
    FileKind, MessageBody, MessageItem, ReactionItem, ReplyItem, ThreadChip, RoomTimeline, TimelineListener, TimelineRow,
};

uniffi::setup_scaffolding!();

/// The runtime the listeners' forwarding tasks run on.
pub(crate) fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .thread_name("morpheus-ffi")
            .enable_all()
            .build()
            .expect("a runtime for the forwarding tasks")
    })
}

/// Stops a live feed (a listener's updates) when cancelled or dropped.
#[derive(uniffi::Object)]
pub struct TaskHandle {
    task: Mutex<Option<JoinHandle<()>>>,
}

impl TaskHandle {
    pub(crate) fn new(task: JoinHandle<()>) -> Arc<Self> {
        Arc::new(Self {
            task: Mutex::new(Some(task)),
        })
    }
}

#[uniffi::export]
impl TaskHandle {
    /// Stop the updates. Safe to call more than once.
    pub fn cancel(&self) {
        if let Some(task) = self.task.lock().ok().and_then(|mut task| task.take()) {
            task.abort();
        }
    }
}

impl Drop for TaskHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Who is signed in.
#[derive(Debug, Clone, uniffi::Record)]
pub struct Account {
    pub user_id: String,
    pub homeserver: String,
    pub device_id: String,
}

impl Account {
    fn of(session: &Session) -> Self {
        Self {
            user_id: session.user_id().map(|id| id.to_string()).unwrap_or_default(),
            homeserver: session.homeserver(),
            device_id: session.device_id().unwrap_or_default(),
        }
    }
}

/// Told when the sync state changes (`"running"`, `"offline"`, ...).
#[uniffi::export(with_foreign)]
pub trait SyncListener: Send + Sync {
    fn on_sync_state(&self, state: String);
}

/// Where the session's secrets go: iPhones have the keychain; on Android (and in
/// tests) an encrypted file in the app's private folder, as the Dioxus app had it.
fn store_kind() -> morpheus_core::SessionStoreKind {
    if cfg!(target_os = "ios") {
        morpheus_core::SessionStoreKind::Keyring
    } else {
        morpheus_core::SessionStoreKind::File
    }
}

/// The app's one entry point.
#[derive(uniffi::Object)]
pub struct Morpheus {
    core: Core,
    pub(crate) dir: std::path::PathBuf,
    session: Mutex<Option<Session>>,
}

impl Morpheus {
    pub(crate) fn session(&self) -> Result<Session, FfiError> {
        self.session
            .lock()
            .ok()
            .and_then(|session| session.clone())
            .ok_or(FfiError::SignedOut)
    }

    fn keep(&self, session: Session) -> Account {
        let account = Account::of(&session);
        if let Ok(mut slot) = self.session.lock() {
            *slot = Some(session);
        }
        account
    }
}

#[uniffi::export]
impl Morpheus {
    /// Everything is kept under `data_dir` (the app's private folder).
    #[uniffi::constructor]
    pub fn new(data_dir: String) -> Result<Arc<Self>, FfiError> {
        Ok(Arc::new(Self {
            dir: std::path::PathBuf::from(&data_dir),
            core: Core::with_session_store(data_dir, store_kind())?,
            session: Mutex::new(None),
        }))
    }

    /// The session of the last launch, if there is one (and syncing starts).
    pub async fn restore_session(&self) -> Result<Option<Account>, FfiError> {
        Ok(self.core.restore_session().await?.map(|s| self.keep(s)))
    }

    /// Sign in with a password. `homeserver` is a server name or a URL.
    pub async fn login(
        &self,
        homeserver: String,
        username: String,
        password: String,
    ) -> Result<Account, FfiError> {
        let session = self
            .core
            .login(morpheus_core::LoginRequest {
                homeserver,
                username,
                password,
            })
            .await?;
        Ok(self.keep(session))
    }

    /// Sign out and forget the session and its local data.
    pub async fn logout(&self) -> Result<(), FfiError> {
        let session = self
            .session
            .lock()
            .ok()
            .and_then(|mut slot| slot.take())
            .ok_or(FfiError::SignedOut)?;
        session.logout().await?;
        Ok(())
    }

    /// Who is signed in, if anyone.
    pub fn account(&self) -> Option<Account> {
        let session = self.session.lock().ok()?;
        session.as_ref().map(Account::of)
    }

    /// Tell the listener the sync state now and whenever it changes.
    pub fn watch_sync(&self, listener: Arc<dyn SyncListener>) -> Result<Arc<TaskHandle>, FfiError> {
        let session = self.session()?;
        let task = runtime().spawn(async move {
            let mut last = String::new();
            loop {
                let state = session.sync_state();
                if state != last {
                    listener.on_sync_state(state.clone());
                    last = state;
                }
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            }
        });
        Ok(TaskHandle::new(task))
    }

    /// Stop syncing (the app goes to the background for good) but stay signed in.
    pub async fn suspend(&self) -> Result<(), FfiError> {
        self.session()?.suspend().await?;
        Ok(())
    }
}
