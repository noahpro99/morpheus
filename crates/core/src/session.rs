use std::{path::Path, sync::Arc};

use matrix_sdk::{
    Client, ThreadingSupport, authentication::matrix::MatrixSession, store::RoomLoadSettings,
};
use matrix_sdk_ui::sync_service::SyncService;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{Core, CoreFuture, Error, OwnedUserId, Result};

const SESSION_FILE: &str = "session.json";
const STORE_DIR: &str = "store";
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

/// What we persist between launches. Tokens are stored in plain JSON for
/// now; moving them into the OS keyring is tracked in PLAN.md.
#[derive(Serialize, Deserialize)]
struct StoredSession {
    homeserver: String,
    session: MatrixSession,
}

impl Core {
    /// Restore the previous session from disk, if there is one.
    pub fn restore_session(&self) -> CoreFuture<Option<Session>> {
        let core = self.clone();
        self.run(async move {
            let path = core.data_dir().join(SESSION_FILE);
            let Ok(raw) = tokio::fs::read(&path).await else {
                return Ok(None);
            };
            let stored: StoredSession = serde_json::from_slice(&raw)?;

            let client = build_client(core.data_dir(), &stored.homeserver).await?;
            client
                .matrix_auth()
                .restore_session(stored.session, RoomLoadSettings::default())
                .await?;
            info!(user = %client.user_id().map(ToString::to_string).unwrap_or_default(), "restored session");

            Session::start(core, client).await.map(Some)
        })
    }

    /// Log in with a password and persist the resulting session.
    pub fn login(&self, request: LoginRequest) -> CoreFuture<Session> {
        let core = self.clone();
        self.run(async move {
            let client = build_client(core.data_dir(), &request.homeserver).await?;
            client
                .matrix_auth()
                .login_username(&request.username, &request.password)
                .initial_device_display_name(DEVICE_NAME)
                .await?;

            let session = client.matrix_auth().session().ok_or(Error::NoSession)?;
            let stored = StoredSession {
                homeserver: request.homeserver,
                session,
            };
            tokio::fs::write(
                core.data_dir().join(SESSION_FILE),
                serde_json::to_vec_pretty(&stored)?,
            )
            .await?;
            info!(user = %request.username, "logged in");

            Session::start(core, client).await
        })
    }
}

async fn build_client(data_dir: &Path, homeserver: &str) -> Result<Client> {
    Ok(Client::builder()
        .homeserver_url(homeserver)
        .sqlite_store(data_dir.join(STORE_DIR), None)
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
            let _ = tokio::fs::remove_file(core.data_dir().join(SESSION_FILE)).await;
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
