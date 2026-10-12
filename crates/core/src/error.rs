use crate::OwnedRoomId;

/// Everything that can go wrong inside the core.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("core runtime task failed: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("stored session is unreadable: {0}")]
    SessionFormat(#[from] serde_json::Error),

    #[error("could not build Matrix client: {0}")]
    ClientBuild(#[from] matrix_sdk::ClientBuildError),

    #[error("Matrix error: {0}")]
    Matrix(#[from] matrix_sdk::Error),

    #[error("Matrix HTTP error: {0}")]
    Http(#[from] matrix_sdk::HttpError),

    #[error("recovery failed: {0}")]
    Recovery(#[from] matrix_sdk::encryption::recovery::RecoveryError),

    #[error("sync error: {0}")]
    Sync(#[from] matrix_sdk_ui::sync_service::Error),

    #[error("room list error: {0}")]
    RoomList(#[from] matrix_sdk_ui::room_list_service::Error),

    #[error("timeline error: {0}")]
    Timeline(#[from] matrix_sdk_ui::timeline::Error),

    #[error("login succeeded but the client has no session")]
    NoSession,

    #[error("unknown room {0}")]
    UnknownRoom(OwnedRoomId),

    #[error("OS keyring error: {0}")]
    Keyring(#[from] keyring::Error),

    #[error("could not decrypt the stored session; its key is missing or the file is corrupt")]
    SessionCrypto,

    #[error("no other device of this user was found to verify against")]
    NoOtherDevice,

    #[error("this verification flow has not reached the emoji step yet")]
    VerificationNotReady,

    #[error("only messages can be forwarded")]
    NotForwardable,

    #[error("{0}")]
    Other(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
