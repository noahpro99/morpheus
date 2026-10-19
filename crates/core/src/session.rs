use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use matrix_sdk::{
    Client, ThreadingSupport,
    ruma::api::client::{
        session::get_login_types::v3::LoginType,
        uiaa::{EmailUserIdentifier, UserIdentifier},
    },
    store::RoomLoadSettings,
};
use matrix_sdk_ui::sync_service::SyncService;
use tracing::info;

use crate::{
    Core, CoreFuture, Error, OwnedUserId, Result,
    session_store::{self, STORE_DIR, StoredSession},
};

/// Throwaway store used only to query a server before logging in.
const PROBE_STORE_DIR: &str = "probe-store";
const DEVICE_NAME: &str = "Morpheus";

/// Credentials for a password login.
#[derive(Debug, Clone)]
pub struct LoginRequest {
    pub homeserver: String,
    pub username: String,
    pub password: String,
}

/// A logged-in user with a running sync loop.
#[derive(Clone)]
pub struct Session {
    pub(crate) core: Core,
    pub(crate) client: Client,
    pub(crate) sync: Arc<SyncService>,
    /// The account's folder (see `accounts.rs`).
    pub(crate) dir: PathBuf,
}

/// Which sign-in methods a homeserver offers.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LoginFlows {
    pub password: bool,
    pub sso: bool,
}

impl Core {
    /// Restore the previous session, if there is one. Credentials come from
    /// the session store (OS keyring, or an encrypted file); a plaintext
    /// `session.json` from older builds is migrated in on the way.
    pub fn restore_session(&self) -> CoreFuture<Option<Session>> {
        let core = self.clone();
        self.run(async move {
            let blocking = core.clone();
            let (dir, stored) =
                tokio::task::spawn_blocking(move || -> Result<(PathBuf, Option<StoredSession>)> {
                    let kind = blocking.session_store_kind();
                    // Older builds kept a plaintext session in the data dir.
                    let root = blocking.data_dir();
                    session_store::migrate_legacy_session(
                        root,
                        session_store::resolve(root, kind).as_ref(),
                    )?;
                    let dir = blocking.active_account_dir();
                    let stored = session_store::resolve(&dir, kind).load()?;
                    Ok((dir, stored))
                })
                .await??;
            let Some(stored) = stored else {
                return Ok(None);
            };

            let client =
                build_client(&dir, &stored.homeserver, Some(&stored.store_passphrase)).await?;
            client
                .matrix_auth()
                .restore_session(stored.session, RoomLoadSettings::default())
                .await?;
            info!(user = %client.user_id().map(ToString::to_string).unwrap_or_default(), "restored session");

            Session::start(core, client, dir).await.map(Some)
        })
    }

    /// Ask a homeserver which login methods it supports. Resolves the
    /// server name through `.well-known` like a login would.
    pub fn login_flows(&self, homeserver: String) -> CoreFuture<LoginFlows> {
        let core = self.clone();
        self.run(async move {
            let client = tls_roots(Client::builder())
                .server_name_or_homeserver_url(&homeserver)
                .sqlite_store(core.data_dir().join(PROBE_STORE_DIR), None)
                .build()
                .await?;
            let types = client.matrix_auth().get_login_types().await?;
            let mut flows = LoginFlows::default();
            for flow in types.flows {
                match flow {
                    LoginType::Password(_) => flows.password = true,
                    LoginType::Sso(_) => flows.sso = true,
                    _ => {}
                }
            }
            Ok(flows)
        })
    }

    /// Log in with a password and persist the resulting session.
    ///
    /// `username` may be a localpart, a full Matrix ID, or the account's
    /// email address.
    pub fn login(&self, request: LoginRequest) -> CoreFuture<Session> {
        let core = self.clone();
        self.run(async move {
            let dir = new_account_dir(&core).await?;
            let (client, passphrase) = fresh_client(&dir, &request.homeserver).await?;
            let auth = client.matrix_auth();
            let login = if is_email(&request.username) {
                let email =
                    UserIdentifier::Email(EmailUserIdentifier::new(request.username.clone()));
                auth.login_identifier(email, &request.password)
            } else {
                auth.login_username(&request.username, &request.password)
            };
            login.initial_device_display_name(DEVICE_NAME).await?;
            info!(user = %request.username, "logged in with password");

            persist_and_start(core, client, request.homeserver, passphrase, dir).await
        })
    }

    /// Log in through the homeserver's single sign-on page. Opens the
    /// system browser and waits for it to redirect back to a loopback port.
    ///
    /// `open_url` shows the sign-in page; `None` uses the desktop's default
    /// opener (Android has none a Rust process can use).
    pub fn login_sso(
        &self,
        homeserver: String,
        open_url: Option<UrlOpener>,
    ) -> CoreFuture<Session> {
        let core = self.clone();
        self.run(async move {
            let dir = new_account_dir(&core).await?;
            let (client, passphrase) = fresh_client(&dir, &homeserver).await?;
            client
                .matrix_auth()
                .login_sso(|url| async move {
                    info!(%url, "opening browser for single sign-on");
                    match &open_url {
                        Some(open) => open(&url),
                        None => open::that_detached(&url),
                    }
                    .map_err(|err| matrix_sdk::Error::UnknownError(Box::new(err)))
                })
                .initial_device_display_name(DEVICE_NAME)
                .await?;
            info!("logged in with single sign-on");

            persist_and_start(core, client, homeserver, passphrase, dir).await
        })
    }
}

fn is_email(username: &str) -> bool {
    !username.starts_with('@') && username.contains('@') && username.contains('.')
}

/// A client for an account that is not logged in yet, with a fresh store
/// passphrase. Wipes any store left behind by a previous account so the SDK
/// does not refuse the new one.
async fn fresh_client(data_dir: &Path, homeserver: &str) -> Result<(Client, String)> {
    let store = data_dir.join(STORE_DIR);
    if tokio::fs::try_exists(&store).await.unwrap_or(false) {
        tokio::fs::remove_dir_all(&store).await?;
    }
    let passphrase = session_store::generate_passphrase();
    let client = build_client(data_dir, homeserver, Some(&passphrase)).await?;
    Ok((client, passphrase))
}

/// Blocking-safe: the folder a new sign-in goes into (see `accounts.rs`).
async fn new_account_dir(core: &Core) -> Result<PathBuf> {
    let core = core.clone();
    tokio::task::spawn_blocking(move || core.free_account_dir()).await?
}

async fn persist_and_start(
    core: Core,
    client: Client,
    homeserver: String,
    store_passphrase: String,
    dir: PathBuf,
) -> Result<Session> {
    let session = client.matrix_auth().session().ok_or(Error::NoSession)?;
    let stored = StoredSession {
        homeserver,
        session,
        store_passphrase,
    };
    let kind = core.session_store_kind();
    let (blocking, account_dir) = (core.clone(), dir.clone());
    tokio::task::spawn_blocking(move || -> Result<()> {
        session_store::resolve(&account_dir, kind).save(&stored)?;
        // The account just signed in is the one to restore next time.
        crate::accounts::write_active(blocking.data_dir(), &blocking.account_key(&account_dir))
    })
    .await??;
    Session::start(core, client, dir).await
}

async fn build_client(
    data_dir: &Path,
    homeserver: &str,
    passphrase: Option<&str>,
) -> Result<Client> {
    Ok(tls_roots(Client::builder())
        // Accepts "matrix.org" or a full URL and follows `.well-known`.
        .server_name_or_homeserver_url(homeserver)
        .sqlite_store(data_dir.join(STORE_DIR), passphrase)
        // Local message search, decrypted messages included: encrypted on disk
        // with the store's passphrase, next to the store.
        .search_index_store(match passphrase {
            Some(secret) => matrix_sdk::search_index::SearchIndexStoreKind::EncryptedDirectory(
                data_dir.join(STORE_DIR).join("search"),
                secret.to_owned(),
            ),
            None => matrix_sdk::search_index::SearchIndexStoreKind::InMemory,
        })
        .with_threading_support(ThreadingSupport::Enabled {
            with_subscriptions: false,
        })
        .build()
        .await?)
}

impl Session {
    async fn start(core: Core, client: Client, dir: PathBuf) -> Result<Self> {
        let sync = SyncService::builder(client.clone()).build().await?;
        sync.start().await;
        Ok(Self {
            core,
            client,
            sync: Arc::new(sync),
            dir,
        })
    }

    pub fn user_id(&self) -> Option<OwnedUserId> {
        self.client.user_id().map(ToOwned::to_owned)
    }

    /// This device's ID.
    pub fn device_id(&self) -> Option<String> {
        self.client.device_id().map(ToString::to_string)
    }

    pub fn homeserver(&self) -> String {
        self.client.homeserver().to_string()
    }

    /// Stop syncing but stay signed in, before switching to another account.
    pub fn suspend(&self) -> CoreFuture<()> {
        let sync = self.sync.clone();
        self.core.run(async move {
            sync.stop().await;
            Ok(())
        })
    }

    /// Stop syncing, invalidate the access token and forget the stored
    /// session. Another stored account, if any, becomes the active one.
    pub fn logout(self) -> CoreFuture<()> {
        let Self {
            core,
            client,
            sync,
            dir,
        } = self;
        let runner = core.clone();
        runner.run(async move {
            sync.stop().await;
            client.logout().await?;
            let kind = core.session_store_kind();
            let account_dir = dir.clone();
            tokio::task::spawn_blocking(move || session_store::resolve(&account_dir, kind).clear())
                .await??;
            // Drop the account's local data too, so another account can sign in cleanly.
            let _ = tokio::fs::remove_dir_all(dir.join(STORE_DIR)).await;
            if dir != core.data_dir() {
                let _ = tokio::fs::remove_dir_all(&dir).await;
            }
            tokio::task::spawn_blocking(move || {
                let next = core.active_account_dir();
                crate::accounts::write_active(core.data_dir(), &core.account_key(&next))
            })
            .await??;
            Ok(())
        })
    }
}

impl PartialEq for Session {
    /// Two handles are "the same session" when they are the same user on the same device.
    fn eq(&self, other: &Self) -> bool {
        self.client.session_meta() == other.client.session_meta()
    }
}

/// How server certificates are checked. The default is the platform's own
/// verifier, which on Android needs a JNI and Kotlin setup the app does not
/// ship: every HTTPS request panicked ("expect rustls-platform-verifier to be
/// installed"). Android builds check against the Mozilla root list instead,
/// so certificates only the phone's own store trusts (a company CA) are not
/// accepted there.
fn tls_roots(builder: matrix_sdk::ClientBuilder) -> matrix_sdk::ClientBuilder {
    #[cfg(target_os = "android")]
    {
        let roots = webpki_root_certs::TLS_SERVER_ROOT_CERTS
            .iter()
            .filter_map(|der| matrix_sdk::reqwest::Certificate::from_der(der).ok())
            .collect();
        builder
            .disable_built_in_root_certificates()
            .add_root_certificates(roots)
    }
    #[cfg(not(target_os = "android"))]
    builder
}

/// Shows a URL in the user's browser. Platforms supply one where the
/// desktop opener does not work (Android, through an intent).
pub type UrlOpener = std::sync::Arc<dyn Fn(&str) -> std::io::Result<()> + Send + Sync>;

/// What a push notification can show for one message: who, where, what.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotificationPreview {
    pub sender: String,
    pub room: String,
    pub body: String,
    pub is_dm: bool,
}

impl Core {
    /// Fetch and decrypt one pushed message for its notification, without
    /// starting the app's sync: for a push that arrives while the app is
    /// closed. Uses the SDK's notification client, which takes a cross-process
    /// lock on the account's stores. `None` when there is nothing to show
    /// (filtered out by push rules, redacted, not found).
    pub fn preview_notification(
        &self,
        room_id: String,
        event_id: String,
    ) -> CoreFuture<Option<NotificationPreview>> {
        let core = self.clone();
        self.run(async move {
            use matrix_sdk::ruma::events::{
                AnySyncMessageLikeEvent, AnySyncTimelineEvent, SyncMessageLikeEvent,
            };
            use matrix_sdk_ui::notification_client::{
                NotificationClient, NotificationEvent, NotificationProcessSetup, NotificationStatus,
            };
            let room_id: crate::OwnedRoomId = room_id
                .parse()
                .map_err(|_| Error::Other(format!("not a room id: {room_id}")))?;
            let event_id: matrix_sdk::ruma::OwnedEventId = event_id
                .parse()
                .map_err(|_| Error::Other(format!("not an event id: {event_id}")))?;

            let blocking = core.clone();
            let (dir, stored) =
                tokio::task::spawn_blocking(move || -> Result<(PathBuf, Option<StoredSession>)> {
                    let dir = blocking.active_account_dir();
                    let stored =
                        session_store::resolve(&dir, blocking.session_store_kind()).load()?;
                    Ok((dir, stored))
                })
                .await??;
            let Some(stored) = stored else {
                return Ok(None);
            };
            let client =
                build_client(&dir, &stored.homeserver, Some(&stored.store_passphrase)).await?;
            client
                .matrix_auth()
                .restore_session(stored.session, RoomLoadSettings::default())
                .await?;
            let notifications =
                NotificationClient::new(client, NotificationProcessSetup::MultipleProcesses)
                    .await
                    .map_err(|err| Error::Other(format!("notification client: {err}")))?;
            let status = notifications
                .get_notification(&room_id, &event_id)
                .await
                .map_err(|err| Error::Other(format!("fetching the notification: {err}")))?;
            let NotificationStatus::Event(item) = status else {
                return Ok(None);
            };
            let body = match &item.event {
                NotificationEvent::Timeline(event) => match event.as_ref() {
                    AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
                        SyncMessageLikeEvent::Original(message),
                    )) => message.content.body().to_owned(),
                    _ => "New message".to_owned(),
                },
                NotificationEvent::Invite(_) => "Invited you".to_owned(),
            };
            Ok(Some(NotificationPreview {
                sender: item.sender_display_name.clone().unwrap_or_default(),
                room: item.room_computed_display_name.clone(),
                body,
                is_dm: item.is_direct_message_room,
            }))
        })
    }
}
