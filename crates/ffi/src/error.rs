/// What went wrong, as the app can show it.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    /// No one is signed in.
    #[error("not signed in")]
    SignedOut,
    /// The core refused or failed; the message is for people.
    #[error("{message}")]
    Failed { message: String },
}

impl From<morpheus_core::Error> for FfiError {
    fn from(err: morpheus_core::Error) -> Self {
        Self::Failed {
            message: err.to_string(),
        }
    }
}
