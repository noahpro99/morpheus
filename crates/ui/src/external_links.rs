//! Links handed to the app from outside: a `matrix:` link the desktop opens
//! with Morpheus, or a matrix.to link passed when starting it. The shell
//! takes them as they come (once signed in) and opens them as a clicked link
//! would.

use std::{collections::VecDeque, sync::Mutex};

static PENDING: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());

/// Open `link` in the app, now or once it is signed in.
pub fn open(link: impl Into<String>) {
    if let Ok(mut pending) = PENDING.lock() {
        pending.push_back(link.into());
    }
}

/// The next link waiting, if any.
pub(crate) fn take() -> Option<String> {
    PENDING.lock().ok()?.pop_front()
}

/// The first Matrix link among a program's arguments.
pub fn from_args(args: impl IntoIterator<Item = String>) -> Option<String> {
    args.into_iter()
        .find(|arg| morpheus_core::parse_matrix_link(arg).is_some())
}
