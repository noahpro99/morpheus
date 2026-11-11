use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
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
use tracing::{info, warn};

use crate::{
    Core, CoreFuture, Error, OwnedUserId, Result,
    session_store::{self, STORE_DIR, StoredSession},
};

/// Throwaway store used only to query a server before logging in.
const PROBE_STORE_DIR: &str = "probe-store";
/// What other apps' device lists call a sign-in: the app and the system
/// it runs on.
pub(crate) fn device_name() -> &'static str {
    if cfg!(target_os = "android") {
        "Morpheus Android"
    } else if cfg!(target_os = "ios") {
        "Morpheus iOS"
    } else if cfg!(target_os = "macos") {
        "Morpheus macOS"
    } else if cfg!(target_os = "windows") {
        "Morpheus Windows"
    } else {
        "Morpheus Linux"
    }
}

/// The name devices signed in before got (now `device_name`).
const OLD_DEVICE_NAME: &str = "Morpheus";

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
    /// Syncing is wanted: false once suspended or signed out, so the
    /// watchdog (`keep_syncing`) leaves a deliberate stop alone.
    pub(crate) syncing: Arc<AtomicBool>,
    /// The account's folder (see `accounts.rs`).
    pub(crate) dir: PathBuf,
    /// This device can scan QR codes (a camera the app can read): verification
    /// offers to scan the other device's code. See `set_can_scan_qr`.
    pub(crate) can_scan_qr: Arc<std::sync::atomic::AtomicBool>,
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
                crate::rt::spawn_blocking(move || -> Result<(PathBuf, Option<StoredSession>)> {
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
                match build_client(&dir, &stored.homeserver, Some(&stored.store_passphrase)).await {
                    Ok(client) => client,
                    Err(err) if err.to_string().contains("store cipher") => {
                        return Err(set_aside(&core, &dir, &stored, &err).await);
                    }
                    Err(err) => return Err(err),
                };
            client
                .matrix_auth()
                .restore_session(stored.session, RoomLoadSettings::default())
                .await?;
            info!(user = %client.user_id().map(ToString::to_string).unwrap_or_default(), "restored session");

            let session = Session::start(core.clone(), client.clone(), dir).await?;
            // Signed in before the app did this at sign-in: an account still
            // without a cross-signing identity gets it from this device now.
            let renamer = client.clone();
            core.spawn(async move { first_device(&client, None).await });
            core.spawn(async move { rename_old_device(&renamer).await });
            Ok(Some(session))
        })
    }

    /// Ask a homeserver which login methods it supports. Resolves the
    /// server name through `.well-known` like a login would.
    pub fn login_flows(&self, homeserver: String) -> CoreFuture<LoginFlows> {
        let core = self.clone();
        self.run(async move {
            let client = tls_roots(Client::builder())
                .server_name_or_homeserver_url(&homeserver)
                .pipe_store(&core.data_dir().join(PROBE_STORE_DIR))
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
            if let Err(err) = login.initial_device_display_name(device_name()).await {
                abandon(&core, client, &dir).await;
                return Err(err.into());
            }
            info!(user = %request.username, "logged in with password");

            let session = persist_and_start(
                core.clone(),
                client.clone(),
                request.homeserver,
                passphrase,
                dir,
            )
            .await?;
            // A brand-new account: this device becomes its first verified one.
            let password = request.password;
            core.spawn(async move { first_device(&client, Some(password)).await });
            Ok(session)
        })
    }

    /// Finish a single sign-on that came back to the page with a login
    /// token (a browser's SSO is a redirect away and back; see
    /// [`Core::login_sso`] there).
    pub fn login_with_token(&self, homeserver: String, token: String) -> CoreFuture<Session> {
        let core = self.clone();
        self.run(async move {
            let dir = new_account_dir(&core).await?;
            let (client, passphrase) = fresh_client(&dir, &homeserver).await?;
            let signed_in = client
                .matrix_auth()
                .login_token(&token)
                .initial_device_display_name(device_name())
                .await;
            if let Err(err) = signed_in {
                abandon(&core, client, &dir).await;
                return Err(err.into());
            }
            info!("logged in with a single sign-on token");
            let session =
                persist_and_start(core.clone(), client.clone(), homeserver, passphrase, dir)
                    .await?;
            core.spawn(async move { first_device(&client, None).await });
            Ok(session)
        })
    }

    /// In a browser: off to the homeserver's sign-on page, which sends the
    /// page back with `?loginToken=` (and the homeserver, kept here, in
    /// `?sso_homeserver=`) for [`Core::login_with_token`]. Never resolves:
    /// the page goes.
    #[cfg(target_arch = "wasm32")]
    pub fn login_sso(
        &self,
        homeserver: String,
        _open_url: Option<UrlOpener>,
    ) -> CoreFuture<Session> {
        let core = self.clone();
        self.run(async move {
            let (client, _) =
                fresh_client(&core.data_dir().join(PROBE_STORE_DIR), &homeserver).await?;
            let window =
                web_sys::window().ok_or_else(|| Error::Io(std::io::Error::other("no window")))?;
            let location = window.location();
            let here = format!(
                "{}{}",
                location.origin().unwrap_or_default(),
                location.pathname().unwrap_or_default()
            );
            let back =
                url::Url::parse_with_params(&here, [("sso_homeserver", homeserver.as_str())])
                    .map_err(|err| Error::Io(std::io::Error::other(err)))?;
            let url = client
                .matrix_auth()
                .get_sso_login_url(back.as_str(), None)
                .await?;
            info!(%url, "off to single sign-on");
            let _ = location.set_href(&url);
            std::future::pending::<Result<Session>>().await
        })
    }

    /// Log in through the homeserver's single sign-on page. Opens the
    /// system browser and waits for it to redirect back to a loopback port.
    #[cfg(not(target_arch = "wasm32"))]
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
            let signed_in = client
                .matrix_auth()
                .login_sso(|url| async move {
                    info!(%url, "opening browser for single sign-on");
                    match &open_url {
                        Some(open) => open(&url),
                        None => open::that_detached(&url),
                    }
                    .map_err(|err| matrix_sdk::Error::UnknownError(Box::new(err)))
                })
                .initial_device_display_name(device_name())
                .await;
            if let Err(err) = signed_in {
                abandon(&core, client, &dir).await;
                return Err(err.into());
            }
            info!("logged in with single sign-on");

            let session =
                persist_and_start(core.clone(), client.clone(), homeserver, passphrase, dir)
                    .await?;
            core.spawn(async move { first_device(&client, None).await });
            Ok(session)
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
    if crate::fs::nonblocking::try_exists(&store)
        .await
        .unwrap_or(false)
    {
        crate::fs::nonblocking::remove_dir_all(&store).await?;
    }
    let passphrase = session_store::generate_passphrase();
    let client = build_client(data_dir, homeserver, Some(&passphrase)).await?;
    Ok((client, passphrase))
}

/// An account whose local store cannot be decrypted (its passphrase is not
/// the one it was made with): nothing in it can be read, and the device's
/// encryption keys are in it. Move it aside, sign that device out on the
/// server (best effort: its keys are gone), forget its credentials, and ask
/// for a fresh sign-in instead of failing at every start.
async fn set_aside(core: &Core, dir: &Path, stored: &StoredSession, err: &Error) -> Error {
    let user = stored.session.meta.user_id.to_string();
    warn!(%user, %err, "local store unreadable; setting it aside");
    let stamp = crate::rt::SystemTime::now()
        .duration_since(crate::rt::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let _ = crate::fs::nonblocking::rename(
        dir.join(STORE_DIR),
        dir.join(format!("{STORE_DIR}.unreadable-{stamp}")),
    )
    .await;
    if let Ok(client) = tls_roots(Client::builder())
        .server_name_or_homeserver_url(&stored.homeserver)
        .build()
        .await
        && client
            .matrix_auth()
            .restore_session(stored.session.clone(), RoomLoadSettings::default())
            .await
            .is_ok()
    {
        let _ = client.logout().await;
    }
    let (blocking, account_dir) = (core.clone(), dir.to_path_buf());
    let _ = crate::rt::spawn_blocking(move || -> Result<()> {
        session_store::resolve(&account_dir, blocking.session_store_kind()).clear()?;
        let next = blocking.active_account_dir();
        crate::accounts::write_active(blocking.data_dir(), &blocking.account_key(&next))
    })
    .await;
    Error::Other(format!(
        "Morpheus could not open its saved data for {user}, so it was set aside: sign in again."
    ))
}

/// A sign-in that failed: drop the fresh store it made (and its folder, if
/// it was a new one), so the folder can take the next sign-in.
async fn abandon(core: &Core, client: Client, dir: &Path) {
    drop(client);
    let _ = crate::fs::nonblocking::remove_dir_all(dir.join(STORE_DIR)).await;
    if dir != core.data_dir() {
        let _ = crate::fs::nonblocking::remove_dir_all(dir).await;
    }
}

/// Blocking-safe: the folder a new sign-in goes into (see `accounts.rs`).
async fn new_account_dir(core: &Core) -> Result<PathBuf> {
    let core = core.clone();
    crate::rt::spawn_blocking(move || core.free_account_dir()).await?
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
    crate::rt::spawn_blocking(move || -> Result<()> {
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
    Ok(with_store(
        tls_roots(Client::builder())
            // Accepts "matrix.org" or a full URL and follows `.well-known`.
            .server_name_or_homeserver_url(homeserver),
        &data_dir.join(STORE_DIR),
        passphrase,
    )
    .with_threading_support(ThreadingSupport::Enabled {
        with_subscriptions: false,
    })
    // A message this device can't decrypt asks the key backup for its
    // key (as Element X does). The SDK's default never asks, so after
    // the recovery key, history from before the device stayed locked.
    // Backups and cross-signing are still set up by the app's prompts.
    .with_encryption_settings(matrix_sdk::encryption::EncryptionSettings {
        auto_enable_cross_signing: false,
        backup_download_strategy:
            matrix_sdk::encryption::BackupDownloadStrategy::AfterDecryptionFailure,
        auto_enable_backups: false,
    })
    .build()
    .await?)
}

/// Longest wait between attempts to restart a failed sync.
const SYNC_RETRY_MAX: std::time::Duration = std::time::Duration::from_secs(30);

/// Syncing stops for good on an error the offline mode does not cover (and
/// before it existed, on any lost connection: a phone then showed no new
/// messages until the app was restarted). Start it again, backing off,
/// for as long as the session wants it.
async fn keep_syncing(sync: std::sync::Weak<SyncService>, syncing: Arc<AtomicBool>) {
    use matrix_sdk_ui::sync_service::State;
    let Some(mut states) = sync.upgrade().map(|sync| sync.state()) else {
        return;
    };
    let mut wait = std::time::Duration::from_secs(1);
    while let Some(state) = states.next().await {
        match state {
            State::Running => wait = std::time::Duration::from_secs(1),
            State::Error(_) | State::Terminated if syncing.load(Ordering::Relaxed) => {
                warn!(?state, ?wait, "sync stopped; starting it again");
                crate::rt::sleep(wait).await;
                wait = (wait * 2).min(SYNC_RETRY_MAX);
                let Some(sync) = sync.upgrade() else { return };
                if syncing.load(Ordering::Relaxed) {
                    sync.start().await;
                }
            }
            State::Offline => info!("offline; syncing resumes when the server answers"),
            _ => {}
        }
    }
}

impl Session {
    async fn start(core: Core, client: Client, dir: PathBuf) -> Result<Self> {
        // Offline mode: a failed sync waits for the server to answer again
        // and carries on by itself.
        let sync = Arc::new(
            SyncService::builder(client.clone())
                .with_offline_mode()
                .build()
                .await?,
        );
        sync.start().await;
        let syncing = Arc::new(AtomicBool::new(true));
        core.spawn(keep_syncing(Arc::downgrade(&sync), syncing.clone()));
        Ok(Self {
            core,
            client,
            sync,
            syncing,
            dir,
            can_scan_qr: Arc::default(),
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
        self.syncing.store(false, Ordering::Relaxed);
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
            syncing,
            dir,
            can_scan_qr: _,
        } = self;
        syncing.store(false, Ordering::Relaxed);
        let runner = core.clone();
        runner.run(async move {
            sync.stop().await;
            client.logout().await?;
            let kind = core.session_store_kind();
            let account_dir = dir.clone();
            crate::rt::spawn_blocking(move || session_store::resolve(&account_dir, kind).clear())
                .await??;
            // Drop the account's local data too, so another account can sign in cleanly.
            let _ = crate::fs::nonblocking::remove_dir_all(dir.join(STORE_DIR)).await;
            if dir != core.data_dir() {
                let _ = crate::fs::nonblocking::remove_dir_all(&dir).await;
            }
            crate::rt::spawn_blocking(move || {
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
    /// Just two people in the room: notified like a DM, by name.
    pub is_dm: bool,
    pub is_mention: bool,
    /// A ring: someone calling (MSC4075).
    pub is_call: bool,
    /// The sender's picture, for the notification's icon.
    pub sender_avatar: Option<crate::Media>,
    /// The photo, when the message is one.
    pub image: Option<crate::Media>,
}

impl Core {
    /// Decline the ring `event_id` in `room_id` with the app closed (the
    /// phone's Decline, the process started only by the ring's push): the
    /// saved session signs in for this one encrypted send. `false` when
    /// nobody is signed in or the room isn't known here.
    pub fn decline_ring(&self, room_id: String, event_id: String) -> CoreFuture<bool> {
        let core = self.clone();
        self.run(async move {
            use matrix_sdk::ruma::events::rtc::decline::RtcDeclineEventContent;
            let room_id: crate::OwnedRoomId = room_id
                .parse()
                .map_err(|_| Error::Other(format!("not a room id: {room_id}")))?;
            let event_id: matrix_sdk::ruma::OwnedEventId = event_id
                .parse()
                .map_err(|_| Error::Other(format!("not an event id: {event_id}")))?;
            let blocking = core.clone();
            let (dir, stored) =
                crate::rt::spawn_blocking(move || -> Result<(PathBuf, Option<StoredSession>)> {
                    let dir = blocking.active_account_dir();
                    let stored =
                        session_store::resolve(&dir, blocking.session_store_kind()).load()?;
                    Ok((dir, stored))
                })
                .await??;
            let Some(stored) = stored else {
                return Ok(false);
            };
            let client =
                build_client(&dir, &stored.homeserver, Some(&stored.store_passphrase)).await?;
            client
                .matrix_auth()
                .restore_session(stored.session, RoomLoadSettings::default())
                .await?;
            let Some(room) = client.get_room(&room_id) else {
                return Ok(false);
            };
            room.send(RtcDeclineEventContent::new(event_id)).await?;
            Ok(true)
        })
    }

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
            use matrix_sdk::ruma::events::room::message::MessageType;
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
                crate::rt::spawn_blocking(move || -> Result<(PathBuf, Option<StoredSession>)> {
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
            let notifications = NotificationClient::new(
                client.clone(),
                NotificationProcessSetup::MultipleProcesses,
            )
            .await
            .map_err(|err| Error::Other(format!("notification client: {err}")))?;
            let status = notifications
                .get_notification(&room_id, &event_id)
                .await
                .map_err(|err| Error::Other(format!("fetching the notification: {err}")))?;
            let NotificationStatus::Event(item) = status else {
                return Ok(None);
            };
            let is_call = match &item.event {
                NotificationEvent::Timeline(event) => {
                    crate::calls::is_ring(&event.event_type().to_string())
                }
                NotificationEvent::Invite(_) => false,
            };
            let mut photo = None;
            let body = match &item.event {
                _ if is_call => "Incoming call".to_owned(),
                NotificationEvent::Timeline(event) => match event.as_ref() {
                    AnySyncTimelineEvent::MessageLike(AnySyncMessageLikeEvent::RoomMessage(
                        SyncMessageLikeEvent::Original(message),
                    )) => {
                        if let MessageType::Image(image) = &message.content.msgtype {
                            photo = Some(crate::MediaRef::from_source(image.source.clone()));
                        }
                        message.content.body().to_owned()
                    }
                    _ => "New message".to_owned(),
                },
                NotificationEvent::Invite(_) => "Invited you".to_owned(),
            };
            let avatar = item
                .sender_avatar_url
                .as_deref()
                .map(|url| crate::MediaRef::from_mxc(url.into()));
            let sender_avatar = match avatar {
                Some(media) => crate::media::fetch_thumbnail(&client, media, 96, 96).await,
                None => None,
            };
            let image = match photo {
                Some(media) => crate::media::fetch_thumbnail(&client, media, 512, 512).await,
                None => None,
            };
            Ok(Some(NotificationPreview {
                sender: item.sender_display_name.clone().unwrap_or_default(),
                room: item.room_computed_display_name.clone(),
                body,
                is_dm: item.is_direct_message_room || item.joined_members_count <= 2,
                is_mention: item.has_mention.unwrap_or(false),
                is_call,
                sender_avatar,
                image,
            }))
        })
    }
}

/// A brand-new account's first device becomes its first verified one (see
/// [`crate::recovery::set_up_first_device`]). Test runs can play an app
/// from before this was done at sign-in (`MORPHEUS_E2E_SIGN_IN_UNVERIFIED`).
async fn first_device(client: &Client, password: Option<String>) {
    if password.is_some() && std::env::var_os("MORPHEUS_E2E_SIGN_IN_UNVERIFIED").is_some() {
        return;
    }
    crate::recovery::set_up_first_device(client, password).await;
}

/// A device signed in when the app named every device just "Morpheus":
/// named for its system now, as new sign-ins are.
async fn rename_old_device(client: &Client) {
    let Some(device_id) = client.device_id().map(ToOwned::to_owned) else {
        return;
    };
    let Ok(devices) = client.devices().await else {
        return;
    };
    let old = devices.devices.iter().any(|device| {
        device.device_id == device_id && device.display_name.as_deref() == Some(OLD_DEVICE_NAME)
    });
    if old && let Err(err) = client.rename_device(&device_id, device_name()).await {
        tracing::debug!(%err, "renaming this device failed");
    }
}

/// Where a client keeps its state: SQLite (and an encrypted search index
/// beside it) on desktops and phones, IndexedDB named after the folder in a
/// browser.
#[cfg(not(target_arch = "wasm32"))]
fn with_store(
    builder: matrix_sdk::ClientBuilder,
    store: &Path,
    passphrase: Option<&str>,
) -> matrix_sdk::ClientBuilder {
    builder
        .sqlite_store(store, passphrase)
        // Local message search, decrypted messages included: encrypted on disk
        // with the store's passphrase, next to the store.
        .search_index_store(match passphrase {
            Some(secret) => matrix_sdk::search_index::SearchIndexStoreKind::EncryptedDirectory(
                store.join("search"),
                secret.to_owned(),
            ),
            None => matrix_sdk::search_index::SearchIndexStoreKind::InMemory,
        })
}

#[cfg(target_arch = "wasm32")]
fn with_store(
    builder: matrix_sdk::ClientBuilder,
    store: &Path,
    passphrase: Option<&str>,
) -> matrix_sdk::ClientBuilder {
    // A mark that the folder has a store (`accounts::free_account_dir`
    // looks for one before reusing a folder).
    let _ = crate::fs::write(store.join("indexeddb"), b"");
    builder.indexeddb_store(&store.to_string_lossy(), passphrase)
}

/// A throwaway store, for asking a server what it supports.
trait PipeStore {
    fn pipe_store(self, store: &Path) -> Self;
}

impl PipeStore for matrix_sdk::ClientBuilder {
    fn pipe_store(self, store: &Path) -> Self {
        with_store(self, store, None)
    }
}
