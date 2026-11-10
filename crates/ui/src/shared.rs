//! What another app shares to Morpheus (Android's share sheet): text and
//! files, waiting for the shell, which asks where to send them.

use std::{path::PathBuf, sync::Mutex};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Shared {
    pub text: Option<String>,
    pub files: Vec<PathBuf>,
}

static PENDING: Mutex<Option<Shared>> = Mutex::new(None);

/// Offer `shared` to send (replacing one not sent yet).
pub fn offer(shared: Shared) {
    if let Ok(mut pending) = PENDING.lock() {
        *pending = Some(shared);
    }
}

pub(crate) fn take() -> Option<Shared> {
    PENDING.lock().ok()?.take()
}

/// From the note the share activity leaves: `{"text": …, "files": [paths]}`.
pub fn from_json(note: &str) -> Option<Shared> {
    let value: serde_json::Value = serde_json::from_str(note).ok()?;
    let text = value
        .get("text")
        .and_then(|t| t.as_str())
        .map(str::to_owned)
        .filter(|t| !t.trim().is_empty());
    let files: Vec<PathBuf> = value
        .get("files")
        .and_then(|f| f.as_array())
        .map(|files| {
            files
                .iter()
                .filter_map(|f| f.as_str())
                .map(PathBuf::from)
                .collect()
        })
        .unwrap_or_default();
    (text.is_some() || !files.is_empty()).then_some(Shared { text, files })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_share_note() {
        assert_eq!(
            from_json(r#"{"text":"hi","files":["/a/b.png"]}"#),
            Some(Shared {
                text: Some("hi".into()),
                files: vec!["/a/b.png".into()]
            })
        );
        assert_eq!(from_json(r#"{"files":[]}"#), None);
    }
}
