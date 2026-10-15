use std::{path::Path, sync::Arc};

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
            let data_dir = core.data_dir().to_path_buf();
            let kind = core.session_store_kind();
            let stored = tokio::task::spawn_blocking(move || -> Result<Option<StoredSession>> {
                let store = session_store::resolve(&data_dir, kind);
                session_store::migrate_legacy_session(&data_dir, store.as_ref())?;
                store.load()
            })
            .await??;
            let Some(stored) = stored else {
                return Ok(None);
            };

            let client =
                build_client(core.data_dir(), &stored.homeserver, Some(&stored.store_passphrase)).await?;
            client
                .matrix_auth()
                .restore_session(stored.session, RoomLoadSettings::default())
                .await?;
            info!(user = %client.user_id().map(ToString::to_string).unwrap_or_default(), "restored session");

            Session::start(core, client).await.map(Some)
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
            let (client, passphrase) = fresh_client(core.data_dir(), &request.homeserver).await?;
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

            persist_and_start(core, client, request.homeserver, passphrase).await
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
            let (client, passphrase) = fresh_client(core.data_dir(), &homeserver).await?;
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

            persist_and_start(core, client, homeserver, passphrase).await
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

async fn persist_and_start(
    core: Core,
    client: Client,
    homeserver: String,
    store_passphrase: String,
) -> Result<Session> {
    let session = client.matrix_auth().session().ok_or(Error::NoSession)?;
    let stored = StoredSession {
        homeserver,
        session,
        store_passphrase,
    };
    let data_dir = core.data_dir().to_path_buf();
    let kind = core.session_store_kind();
    tokio::task::spawn_blocking(move || session_store::resolve(&data_dir, kind).save(&stored))
        .await??;
    Session::start(core, client).await
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
        .with_threading_support(ThreadingSupport::Enabled {
            with_subscriptions: false,
        })
        .build()
        .await?)
}

impl Session {
    async fn start(core: Core, client: Client) -> Result<Self> {
        let sync = SyncService::builder(client.clone()).build().await?;
        sync.start().await;
        Ok(Self {
            core,
            client,
            sync: Arc::new(sync),
        })
    }

    pub fn user_id(&self) -> Option<OwnedUserId> {
        self.client.user_id().map(ToOwned::to_owned)
    }

    pub fn homeserver(&self) -> String {
        self.client.homeserver().to_string()
    }

    /// Stop syncing, invalidate the access token and forget the stored session.
    pub fn logout(self) -> CoreFuture<()> {
        let Self { core, client, sync } = self;
        let runner = core.clone();
        runner.run(async move {
            sync.stop().await;
            client.logout().await?;
            let data_dir = core.data_dir().to_path_buf();
            let kind = core.session_store_kind();
            tokio::task::spawn_blocking(move || session_store::resolve(&data_dir, kind).clear())
                .await??;
            // Drop the account's local data too, so another account can sign in cleanly.
            let _ = tokio::fs::remove_dir_all(core.data_dir().join(STORE_DIR)).await;
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
