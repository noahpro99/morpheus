// Deeply nested SDK futures trip the default `Send` auto-trait recursion limit.
#![recursion_limit = "256"]

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

mod core;
mod error;
mod media;
mod members;
mod rooms;
mod session;
mod timeline;
mod typing;

pub use core::Core;
pub use error::{Error, Result};

/// A future produced by the core. Boxed so the deeply nested SDK types never
/// leak into dependents, and `Send` so any executor can drive it.
pub type CoreFuture<T> = futures_util::future::BoxFuture<'static, Result<T>>;
pub use media::{Media, MediaRef};
pub use members::{Member, MemberRole};
pub use rooms::{RoomListUpdates, RoomSummary};
pub use session::{LoginRequest, Session};
pub use timeline::{
    Message, MessageId, MessageKind, OpenTimeline, Reaction, ReplyPreview, ThreadSummary,
    TimelineEntry, TimelineHandle, TimelineUpdates,
};
pub use typing::TypingUpdates;

pub use eyeball_im::VectorDiff;
pub use imbl::Vector;
pub use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, UserId};
