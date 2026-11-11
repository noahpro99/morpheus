//! Verification with another device as a state machine with no UI toolkit
//! in it: the core's steps (`morpheus_core::VerificationStep`) become what
//! the panel says and which buttons it offers, and each button names the
//! action it takes (`crates/ui/src/components/settings.rs`,
//! `VerificationPanel`).

use morpheus_core::{QrImage, VerificationStep};

/// How long a finished verification shows its success before the panel
/// steps aside.
pub const VERIFIED_NOTE_SECS: u64 = 3;

/// What a button does to the flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Accept,
    /// Decline an incoming request, cancel one we made, or say "No" to a
    /// scanned code.
    Cancel,
    UseEmoji,
    ConfirmScanned,
    ConfirmMatch,
    Mismatch,
    /// Close a cancelled flow's panel (nothing for the core to do).
    Dismiss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Primary,
    Secondary,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Button {
    pub label: &'static str,
    pub kind: Kind,
    pub action: Action,
    /// A class tests find it by, besides its kind.
    pub class: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Plain,
    Ok,
    Error,
}

/// What the panel shows for one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Panel {
    pub text: String,
    pub tone: Tone,
    /// Seven emoji and their names, to compare with the other device.
    pub emojis: Vec<(String, String)>,
    pub qr: Option<QrImage>,
    pub buttons: Vec<Button>,
}

fn button(label: &'static str, kind: Kind, action: Action) -> Button {
    Button {
        label,
        kind,
        action,
        class: "",
    }
}

/// The panel for a flow at `step`; `incoming` when the other device asked.
pub fn panel(step: &VerificationStep, incoming: bool) -> Panel {
    let mut panel = Panel {
        text: String::new(),
        tone: Tone::Plain,
        emojis: Vec::new(),
        qr: None,
        buttons: Vec::new(),
    };
    match step {
        VerificationStep::Requested if incoming => {
            panel.text = "Another one of your devices wants to verify this one.".into();
            panel.buttons = vec![
                button("Accept", Kind::Primary, Action::Accept),
                button("Decline", Kind::Secondary, Action::Cancel),
            ];
        }
        VerificationStep::Requested => {
            panel.text = "Waiting for your other device to accept…".into();
            panel.buttons = vec![button("Cancel", Kind::Secondary, Action::Cancel)];
        }
        VerificationStep::Ready => panel.text = "Connecting…".into(),
        VerificationStep::ShowQr(code) => {
            panel.text = "Scan this code with your other device.".into();
            panel.qr = Some(code.clone());
            panel.buttons = vec![
                Button {
                    class: "use-emoji",
                    ..button("Compare emoji instead", Kind::Secondary, Action::UseEmoji)
                },
                button("Cancel", Kind::Secondary, Action::Cancel),
            ];
        }
        VerificationStep::QrScanned => {
            panel.text =
                "Your other device scanned the code. Does it say it verified this one?".into();
            panel.buttons = vec![
                Button {
                    class: "confirm-scanned",
                    ..button("Yes", Kind::Primary, Action::ConfirmScanned)
                },
                button("No", Kind::Danger, Action::Cancel),
            ];
        }
        VerificationStep::Reciprocated => {
            panel.text = "Scanned. Confirm on your other device…".into()
        }
        VerificationStep::Emojis(emojis) => {
            panel.text =
                "Compare these with your other device. They must match in the same order.".into();
            panel.emojis = emojis.clone();
            panel.buttons = vec![
                button("They match", Kind::Primary, Action::ConfirmMatch),
                button("They don't match", Kind::Danger, Action::Mismatch),
            ];
        }
        VerificationStep::Confirmed => {
            panel.text = "Waiting for your other device to confirm…".into()
        }
        // Nothing to dismiss: the panel steps aside by itself.
        VerificationStep::Done => {
            panel.text = "Verified. Encrypted messages will now sync between your devices.".into();
            panel.tone = Tone::Ok;
        }
        VerificationStep::Cancelled(reason) => {
            panel.text = format!("Verification cancelled: {reason}");
            panel.tone = Tone::Error;
            panel.buttons = vec![button("Close", Kind::Secondary, Action::Dismiss)];
        }
    }
    panel
}

pub fn is_done(step: &VerificationStep) -> bool {
    matches!(step, VerificationStep::Done)
}

/// A verification code as rows of modules, with the four-module quiet
/// border scanners need: `true` is dark. The drawing is the front end's.
pub fn qr_modules(code: &QrImage) -> (usize, Vec<bool>) {
    const BORDER: usize = 4;
    let size = code.width + 2 * BORDER;
    let mut modules = vec![false; size * size];
    for (index, dark) in code.dark.iter().enumerate() {
        if *dark {
            let (x, y) = (index % code.width + BORDER, index / code.width + BORDER);
            modules[y * size + x] = true;
        }
    }
    (size, modules)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(panel: &Panel) -> Vec<&'static str> {
        panel.buttons.iter().map(|b| b.label).collect()
    }

    #[test]
    fn an_incoming_request_is_accepted_or_declined() {
        let p = panel(&VerificationStep::Requested, true);
        assert!(p.text.contains("wants to verify"));
        assert_eq!(labels(&p), ["Accept", "Decline"]);
        assert_eq!(p.buttons[0].action, Action::Accept);
        assert_eq!(p.buttons[1].action, Action::Cancel);
    }

    #[test]
    fn our_own_request_waits_and_can_be_cancelled() {
        let p = panel(&VerificationStep::Requested, false);
        assert!(p.text.starts_with("Waiting for your other device to accept"));
        assert_eq!(labels(&p), ["Cancel"]);
    }

    #[test]
    fn emoji_are_compared_and_confirmed_or_refused() {
        let emojis = vec![("🐶".to_owned(), "Dog".to_owned()); 7];
        let p = panel(&VerificationStep::Emojis(emojis.clone()), false);
        assert_eq!(p.emojis, emojis);
        assert_eq!(labels(&p), ["They match", "They don't match"]);
        assert_eq!(p.buttons[0].action, Action::ConfirmMatch);
        assert_eq!(p.buttons[1].action, Action::Mismatch);
        assert_eq!(p.buttons[1].kind, Kind::Danger);
    }

    #[test]
    fn a_code_offers_emoji_instead() {
        let code = QrImage {
            width: 1,
            dark: vec![true],
            data: vec![1],
        };
        let p = panel(&VerificationStep::ShowQr(code.clone()), false);
        assert_eq!(p.qr, Some(code));
        assert_eq!(p.buttons[0].class, "use-emoji");
        let p = panel(&VerificationStep::QrScanned, false);
        assert_eq!(p.buttons[0].class, "confirm-scanned");
        assert_eq!(p.buttons[1].action, Action::Cancel);
    }

    #[test]
    fn waiting_steps_have_no_buttons() {
        for step in [
            VerificationStep::Ready,
            VerificationStep::Reciprocated,
            VerificationStep::Confirmed,
        ] {
            assert!(panel(&step, false).buttons.is_empty(), "{step:?}");
        }
    }

    #[test]
    fn done_is_a_note_with_no_buttons_and_cancelled_an_error_with_close() {
        let done = panel(&VerificationStep::Done, false);
        assert_eq!(done.tone, Tone::Ok);
        assert!(done.buttons.is_empty());
        assert!(is_done(&VerificationStep::Done));
        assert!(!is_done(&VerificationStep::Confirmed));
        let cancelled = panel(&VerificationStep::Cancelled("mismatch".into()), false);
        assert_eq!(cancelled.tone, Tone::Error);
        assert_eq!(cancelled.text, "Verification cancelled: mismatch");
        assert_eq!(cancelled.buttons[0].action, Action::Dismiss);
    }

    #[test]
    fn a_code_gets_its_quiet_border() {
        let code = QrImage {
            width: 2,
            dark: vec![true, false, false, true],
            data: vec![],
        };
        let (size, modules) = qr_modules(&code);
        assert_eq!(size, 10);
        assert!(modules[4 * size + 4]);
        assert!(!modules[4 * size + 5]);
        assert!(modules[5 * size + 5]);
        assert_eq!(modules.iter().filter(|m| **m).count(), 2);
    }
}
