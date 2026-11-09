// Deeply nested SDK futures trip the default `Send` auto-trait recursion limit.
#![recursion_limit = "256"]
// A browser has one thread: shared state there is `Arc` without `Send`.
#![cfg_attr(target_arch = "wasm32", allow(clippy::arc_with_non_send_sync))]

//! Morpheus core: everything that talks to Matrix, with no UI dependencies.
//!
//! The core owns its own Tokio runtime so that UI frameworks with their own
//! executors (Dioxus on desktop and mobile) can drive it. Every async method
//! returns a future that is safe to await from *any* executor; the actual work
//! runs on the core runtime.
//!
//! Live data reaches the UI as streams of [`VectorDiff`]s that apply onto a
//! [`Vector`]. Both the room list and a room timeline use the same shape, so
//! UI code has exactly one way of consuming incremental updates.

mod accounts;
mod blur;
mod calls;
mod core;
mod error;
pub mod fs;
mod media;
mod members;
mod messages;
mod notifications;
mod people;
mod profile;
mod push;
mod recovery;
mod report;
mod rooms;
pub mod rt;
mod search;
mod session;
mod session_store;
mod spaces;
mod timeline;
mod typing;
mod updates;
mod verification;

pub use core::Core;
pub use error::{Error, Result};

/// A future produced by the core. Boxed so the deeply nested SDK types never
/// leak into dependents, and `Send` so any executor can drive it.
pub type CoreFuture<T> = rt::BoxFuture<'static, Result<T>>;
pub use accounts::SavedAccount;
pub use blur::placeholder as blur_placeholder;
pub use calls::{
    CallCredentials, CallKey, CallKeyUpdates, CallMedia, CallMember, IncomingCall, RING_FOR_MS,
    RingUpdates, Ringing,
};
pub use media::{Media, MediaRef};
pub use members::{Member, MemberPermissions, MemberRole, ModRoom, Standing};
pub use messages::PinnedMessage;
pub use notifications::{Notification, NotificationUpdates};
pub use people::marks;
pub use profile::Profile;
pub use push::{DEFAULT_PUSH_GATEWAY, PUSH_APP_ID, PushRegistration};
pub use recovery::{EncryptionStatus, RecoveryAvailability};
pub use report::{bug_report_url, device};
pub use rooms::{NotifyMode, RoomListUpdates, RoomSummary, SpaceRoom};
pub use search::SearchHit;
pub use session::{LoginFlows, LoginRequest, NotificationPreview, Session, UrlOpener};
pub use session_store::SessionStoreKind;
pub use spaces::{History, Privacy, RoomProfile};
pub use timeline::{
    Message, MessageId, MessageKind, OpenTimeline, Reaction, ReplyPreview, Shield, ThreadSummary,
    TimelineEntry, TimelineHandle, TimelineUpdates,
};
pub use typing::TypingUpdates;
pub use updates::{UpdateStatus, clean_up_after_update, installs_updates};
pub use verification::{
    Device, QrImage, VerificationActions, VerificationHandle, VerificationStep, VerificationUpdates,
};

pub use eyeball_im::VectorDiff;
pub use imbl::Vector;
pub use matrix_sdk::ruma::{OwnedDeviceId, OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, UserId};
