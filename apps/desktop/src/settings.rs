//! The settings window (`crates/ui/src/components/settings.rs`), with the
//! pages the locked-messages flow needs: My Account, and Security & Privacy
//! (encryption status, the recovery key form, Secure Backup, verifying with
//! another device, the device list). Its logic is in `view_model::keys` and
//! `view_model::verification`; this draws it and runs the core calls.
//!
//! One `SettingsView` lives as long as the shell, shown or not, so a
//! verification started here keeps running (and is still there) after the
//! window closes and opens again.

use std::time::Duration;

use gpui::{
    Context, ElementId, Entity, EventEmitter, FocusHandle, FontWeight, IntoElement, MouseButton,
    Render, ScrollHandle, SharedString, Stateful, Subscription, Window, canvas, div, fill,
    prelude::*, px, relative, rgb, rgba,
};
use morpheus_core::{
    Device, EncryptionStatus, NotifyMode, QrImage, Session, VerificationActions,
    VerificationHandle, VerificationStep,
};

pub(crate) mod pages;
mod video;

use crate::tab::Tabbable as _;
use crate::{
    call_ui::VideoFrame,
    e2e::E2eExt as _,
    icons::icon,
    input::{Changed, Submit, TextInput},
    theme,
    view_model::{
        keys::{
            self, RecoveryForm, RecoveryOffer, SettingsFocus, recovery_key_ready, recovery_offer,
            status_lines,
        },
        settings::{
            Autostart, NotificationContent, SettingEntry, SettingsPage, Theme, find_settings,
            message_sounds_enabled, notification_content, notifications_enabled, read_autostart,
        },
        verification::{self, Action, Kind, Tone, VERIFIED_NOTE_SECS},
    },
};

/// This device's keys may have changed (a recovery or a verification
/// finished): the shell reads them again at once.
pub struct KeysChanged;

impl EventEmitter<KeysChanged> for SettingsView {}

/// A theme was picked: the shell keeps it in the account's prefs.
pub struct ThemeChosen(pub Theme);

impl EventEmitter<ThemeChosen> for SettingsView {}

/// A custom theme colour (0xRRGGBB) was chosen: the shell applies and keeps it.
pub struct CustomColourChosen(pub u32);

impl EventEmitter<CustomColourChosen> for SettingsView {}

/// A custom colour is being dragged to: the shell shows it now and keeps it on release
/// (as [`CustomColourChosen`]).
pub struct CustomColourLive(pub u32);

impl EventEmitter<CustomColourLive> for SettingsView {}

/// A look from the theme history was clicked: the shell applies it.
pub struct HistoryRestore(pub crate::view_model::theme_history::Look);

impl EventEmitter<HistoryRestore> for SettingsView {}

/// A look's pin in the theme history was toggled.
pub struct HistoryPin(pub crate::view_model::theme_history::Look);

impl EventEmitter<HistoryPin> for SettingsView {}

/// A streamer mode choice was kept: the shell draws again.
pub struct StreamerChanged;

impl EventEmitter<StreamerChanged> for SettingsView {}

/// A call device was picked on the Voice & Video page (kept already): the
/// shell makes a call going now switch over.
pub struct DevicesChosen(pub crate::view_model::menu::DeviceKind);

impl EventEmitter<DevicesChosen> for SettingsView {}

/// The signed-in user's name or picture was changed on the Edit Profile page: the
/// shell reads it again and redraws what shows it.
pub struct OwnProfileChanged;

impl EventEmitter<OwnProfileChanged> for SettingsView {}

pub struct InputVolumeChanged(pub f32);

impl EventEmitter<InputVolumeChanged> for SettingsView {}

pub struct OutputVolumeChanged(pub f32);

impl EventEmitter<OutputVolumeChanged> for SettingsView {}

/// A noise suppression mode was picked on the Voice & Video page.
pub struct NoiseChosen(pub morpheus_call::NoiseSuppression);

impl EventEmitter<NoiseChosen> for SettingsView {}

/// A noise suppression model was picked on the Voice & Video page.
pub struct NoiseModelChosen(pub morpheus_call::NoiseModel);

impl EventEmitter<NoiseModelChosen> for SettingsView {}

/// The Voice & Video page's Mic Test was started (true) or stopped; it
/// also stops when the settings close.
pub struct MicTest(pub bool);

impl EventEmitter<MicTest> for SettingsView {}

/// The devices the Voice & Video page lists: (id, name).
pub struct VoiceLists {
    pub microphones: Vec<(String, String)>,
    pub speakers: Vec<(String, String)>,
    pub cameras: Vec<(String, String)>,
}

/// A verification flow as the window shows it: the core's steps, mirrored
/// as they come, and the actions that drive it.
pub struct Flow {
    actions: VerificationActions,
    step: VerificationStep,
    incoming: bool,
}

/// How often and how many times the status is read again while this device is not verified.
const STATUS_REREAD_SECS: u64 = 2;
const STATUS_REREADS: u32 = 15;

pub struct SettingsView {
    session: Option<Session>,
    /// The Diagnostics section redraws itself every second while shown.
    diag_timer: bool,
    open: bool,
    page: SettingsPage,
    status: Option<Result<EncryptionStatus, String>>,
    /// Times the status was read again because this device was not verified yet.
    status_rereads: u32,
    devices: Option<Result<Vec<Device>, String>>,
    form: RecoveryForm,
    /// A new recovery key is asked about first: false for a change from this
    /// device, true for a reset of a lost key.
    confirming: Option<bool>,
    password_input: Entity<TextInput>,
    key_input: Entity<TextInput>,
    flow: Option<Entity<Flow>>,
    flow_watch: Option<Subscription>,
    /// The note after a finished flow has been scheduled to go.
    done_handled: bool,
    focus: FocusHandle,
    scroll: ScrollHandle,
    /// Take the keyboard (and the recovery key field, when asked) at the
    /// next frame, once the elements exist.
    take_focus: bool,
    focus_key: bool,
    /// Opened from the Secure Backup offer: set it up once the status is in.
    set_up_when_ready: bool,
    /// The search field, and the setting a search opened (flashing).
    search: Entity<TextInput>,
    highlight: Option<&'static str>,
    /// The defaults for direct messages (0) and group rooms (1), as the
    /// server has them, and what saving them said.
    notify_defaults: [Option<NotifyMode>; 2],
    notify_notes: [Option<Result<&'static str, String>>; 2],
    /// This device's notification settings, and the autostart entry.
    on_here: bool,
    sounds_here: bool,
    content: NotificationContent,
    autostart: Option<Autostart>,
    startup_note: Option<String>,
    /// The theme in the account's prefs (the shell tells us).
    pub theme: Theme,
    /// The custom colour in use, and the box it is typed in.
    pub custom: Option<u32>,
    /// The looks used lately, as shown (the shell tells us).
    pub history: Vec<crate::view_model::theme_history::Entry>,
    /// The custom colour as hue, saturation and value: kept apart so a grey keeps its hue
    /// while the sliders move.
    hsv: (f32, f32, f32),
    /// A slider is being dragged (the colour is shown, not yet kept).
    hsv_dragging: bool,
    colour_input: Entity<TextInput>,
    data_dir: std::path::PathBuf,
    /// The devices to pick from (None while looking), what is chosen, and
    /// which list is open.
    voice: Option<VoiceLists>,
    voice_prefs: crate::view_model::calls::DevicePrefs,
    /// Voice & Video → Screen Share: Discord left out of shared sound.
    leave_out_discord: bool,
    picker_open: Option<&'static str>,
    /// Whether the noise suppression page's Advanced section is open.
    noise_advanced: bool,
    /// Settings' camera preview: what it shows (under which key), its
    /// message, and whether the camera is opening.
    preview: Option<(&'static str, gpui::Entity<VideoFrame>)>,
    preview_message: Option<String>,
    preview_starting: bool,
    preview_watch: u64,
    /// The Edit Profile page: the name box, the picture picked (path and
    /// preview) and not yet set, what the last call said, and a call running.
    name_input: Entity<TextInput>,
    picked: Option<pages::Picked>,
    profile_note: Option<Result<String, String>>,
    profile_busy: bool,
    /// Where the server lets the account be managed (its OIDC account page), if it says.
    manage_url: Option<String>,
    /// Your display name and current picture (the shell keeps them here).
    pub own: (String, Option<std::sync::Arc<gpui::Image>>),
}

impl SettingsView {
    /// The colours changed: the boxes are views of their own and take the new text
    /// colour only when redrawn (a hex box kept the old theme's light text on a light
    /// colour).
    fn redraw_inputs(&self, cx: &mut Context<Self>) {
        for input in [&self.key_input, &self.search, &self.colour_input, &self.name_input] {
            input.update(cx, |_, cx| cx.notify());
        }
    }

    /// Show the custom colour the account has (the shell tells us).
    pub fn set_custom_shown(&mut self, colour: Option<u32>, cx: &mut Context<Self>) {
        self.custom = colour;
        if let Some(colour) = colour {
            self.sync_hsv(colour);
        }
        let text = colour.map(crate::view_model::theme::hex).unwrap_or_default();
        self.colour_input.update(cx, |input, cx| input.set_value(&text, cx));
        self.redraw_inputs(cx);
    }

    /// Keep the sliders' hue and saturation when `colour` is what they already make
    /// (a grey has no hue of its own), else read them from it.
    fn sync_hsv(&mut self, colour: u32) {
        use crate::view_model::theme::{from_hsv, to_hsv};
        let (h, s, v) = self.hsv;
        if from_hsv(h, s, v) != colour {
            self.hsv = to_hsv(colour);
        }
    }

    /// A slider (0 hue, 1 saturation, 2 value) dragged to `fraction` of its track: the
    /// colour shows at once, and is kept when `commit` (the button let go).
    pub(crate) fn drag_hsv(&mut self, which: usize, fraction: Option<f32>, commit: bool, cx: &mut Context<Self>) {
        use crate::view_model::theme::{DARK, from_hsv, hex, to_hsv};
        let fraction = fraction.map(|f| f.clamp(0.0, 1.0));
        let mut hsv = self.hsv;
        match (which, fraction) {
            (0, Some(f)) => hsv.0 = f * 360.0,
            (1, Some(f)) => hsv.1 = f,
            (_, Some(f)) => hsv.2 = f,
            // No position: back to the default theme's value for this slider.
            (0, None) => hsv.0 = to_hsv(DARK.bg_chat).0,
            (1, None) => hsv.1 = to_hsv(DARK.bg_chat).1,
            (_, None) => hsv.2 = to_hsv(DARK.bg_chat).2,
        }
        self.hsv = hsv;
        self.hsv_dragging = !commit;
        let colour = from_hsv(hsv.0, hsv.1, hsv.2);
        self.custom = Some(colour);
        let text = hex(colour);
        self.colour_input.update(cx, |input, cx| input.set_value(&text, cx));
        self.redraw_inputs(cx);
        if commit {
            cx.emit(CustomColourChosen(colour));
        } else {
            cx.emit(CustomColourLive(colour));
        }
        cx.notify();
    }

    /// The button came up after a slider drag: keep the colour shown.
    pub(crate) fn keep_custom(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.hsv_dragging)
            && let Some(colour) = self.custom
        {
            cx.emit(CustomColourChosen(colour));
        }
    }

    /// A colour picked from the grid (or typed): apply it.
    pub(crate) fn pick_colour(&mut self, colour: u32, cx: &mut Context<Self>) {
        self.set_custom_shown(Some(colour), cx);
        cx.emit(CustomColourChosen(colour));
        cx.notify();
    }

    pub fn new(session: Option<Session>, cx: &mut Context<Self>) -> Self {
        let key_input =
            cx.new(|cx| TextInput::new("recovery-key", keys::RECOVERY_PLACEHOLDER, true, "", cx));
        cx.subscribe(&key_input, |this, _, _: &Submit, cx| this.submit_key(cx))
            .detach();
        let password_input =
            cx.new(|cx| TextInput::new("reset-password", "Account password", true, "", cx));
        let search = cx.new(|cx| {
            TextInput::new("settings-search", "Search settings", false, "", cx)
                .with_class("settings-search")
                .compact()
        });
        cx.subscribe(&search, |this, _, _: &Submit, cx| {
            let query = this.search.read(cx).value().to_owned();
            if let Some(entry) = find_settings(&query, true).first() {
                this.show_setting(entry, cx);
            }
        })
        .detach();
        let name_input =
            cx.new(|cx| TextInput::new("displayname", "Display name", false, "", cx).framed());
        cx.subscribe(&name_input, |this, _, _: &Submit, cx| this.save_name(cx))
            .detach();
        let colour_input = cx.new(|cx| TextInput::new("custom-colour", "#rrggbb", false, "", cx).framed());
        cx.subscribe(&colour_input, |this, _, _: &Changed, cx| {
            let typed = this.colour_input.read(cx).value().to_owned();
            if let Some(colour) = crate::view_model::theme::parse_hex(&typed)
                && this.custom != Some(colour)
            {
                this.custom = Some(colour);
                this.sync_hsv(colour);
                cx.emit(CustomColourChosen(colour));
                this.redraw_inputs(cx);
                cx.notify();
            }
        })
        .detach();
        let data_dir = crate::data_dir();
        if let Some(session) = session.clone() {
            // Incoming requests from the user's other devices pop the window.
            let requests = session.verification_requests();
            cx.spawn(async move |this, cx| {
                let Ok(mut requests) = requests.await else {
                    return;
                };
                while let Some(handle) = requests.next().await {
                    let alive = this
                        .update(cx, |this, cx| {
                            this.track(handle, true, cx);
                            this.open(SettingsFocus::None, cx);
                        })
                        .is_ok();
                    if !alive {
                        break;
                    }
                }
            })
            .detach();
        }
        Self {
            session,
            diag_timer: false,
            open: false,
            page: SettingsPage::Account,
            status: None,
            status_rereads: 0,
            devices: None,
            form: RecoveryForm::default(),
            confirming: None,
            password_input,
            key_input,
            flow: None,
            flow_watch: None,
            done_handled: false,
            focus: cx.focus_handle(),
            scroll: ScrollHandle::new(),
            take_focus: false,
            focus_key: false,
            set_up_when_ready: false,
            search,
            highlight: None,
            notify_defaults: [None, None],
            notify_notes: [None, None],
            on_here: true,
            sounds_here: true,
            content: NotificationContent::Full,
            autostart: None,
            startup_note: None,
            theme: Theme::Dark,
            custom: None,
            history: Vec::new(),
            hsv: crate::view_model::theme::to_hsv(crate::view_model::theme::DARK.bg_chat),
            hsv_dragging: false,
            colour_input,
            data_dir,
            voice: None,
            voice_prefs: Default::default(),
            leave_out_discord: true,
            picker_open: None,
            noise_advanced: false,
            preview: None,
            preview_message: None,
            preview_starting: false,
            preview_watch: 0,
            name_input,
            picked: None,
            profile_note: None,
            profile_busy: false,
            manage_url: None,
            own: (String::new(), None),
        }
    }

    /// Whether the settings window is showing.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Show the window, on the page `focus` asks for.
    pub fn open(&mut self, focus: SettingsFocus, cx: &mut Context<Self>) {
        self.open = true;
        self.confirming = None;
        self.form = RecoveryForm::default();
        self.key_input
            .update(cx, |input, cx| input.set_value("", cx));
        self.page = SettingsPage::for_focus(focus).unwrap_or(if self.flow.is_some() {
            SettingsPage::Security
        } else {
            SettingsPage::Account
        });
        self.search.update(cx, |input, cx| input.set_value("", cx));
        self.highlight = None;
        self.on_here = notifications_enabled(&self.data_dir);
        self.sounds_here = message_sounds_enabled(&self.data_dir);
        self.leave_out_discord = crate::view_model::settings::leaves_out_discord(&self.data_dir);
        self.content = notification_content(&self.data_dir);
        self.autostart = read_autostart();
        self.startup_note = None;
        self.load_notify_defaults(cx);
        self.load_own_name(cx);
        self.load_voice(cx);
        self.take_focus = true;
        self.focus_key = focus == SettingsFocus::RecoveryKey;
        self.set_up_when_ready = focus == SettingsFocus::SetUpBackup;
        self.status_rereads = 0;
        self.refresh(cx);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            if morpheus_call::mic_test_running() {
                cx.emit(MicTest(false));
            }
            self.stop_video(cx);
            self.set_up_when_ready = false;
            cx.notify();
        }
    }

    /// The status and the device list, read again.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let status = session.encryption_status();
        let devices = session.own_devices();
        cx.spawn(async move |this, cx| {
            let status = status.await.map_err(|e| e.to_string());
            this.update(cx, |this, cx| {
                this.status = Some(status);
                if this.set_up_when_ready
                    && matches!(&this.status, Some(Ok(s)) if recovery_offer(Some(s), false) == RecoveryOffer::SetUp)
                {
                    this.set_up_when_ready = false;
                    this.set_up_recovery(cx);
                }
                // A signed-in device is made the account's first verified one in the
                // background (a new account, or an old sign-in as the app starts): the
                // page, opened meanwhile, follows instead of staying "not verified".
                let unverified = matches!(&this.status, Some(Ok(s)) if !s.device_verified);
                if this.open && unverified && this.status_rereads < STATUS_REREADS {
                    this.status_rereads += 1;
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(Duration::from_secs(STATUS_REREAD_SECS))
                            .await;
                        this.update(cx, |this, cx| {
                            if this.open {
                                this.refresh(cx);
                            }
                        })
                        .ok();
                    })
                    .detach();
                }
                cx.notify();
            })
            .ok();
            let devices = devices.await.map_err(|e| e.to_string());
            this.update(cx, |this, cx| {
                this.devices = Some(devices);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn keys_changed(&mut self, cx: &mut Context<Self>) {
        self.refresh(cx);
        cx.emit(KeysChanged);
    }

    /// "Set up recovery": key backup and a recovery key to keep.
    fn set_up_recovery(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        if !self.form.begin() {
            return;
        }
        let making = session.enable_recovery();
        cx.spawn(async move |this, cx| {
            let made = making.await;
            this.update(cx, |this, cx| {
                match made {
                    Ok(key) => {
                        this.confirming = None;
                        this.form.made(key);
                        this.keys_changed(cx);
                    }
                    Err(err) => this.form.failed(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// "Change recovery key" and "Reset recovery key" ask first (the old key
    /// stops working).
    fn ask_new_key(&mut self, reset: bool, cx: &mut Context<Self>) {
        if self.form.busy {
            return;
        }
        self.confirming = Some(reset);
        self.form.message = None;
        self.password_input
            .update(cx, |input, cx| input.set_value("", cx));
        cx.notify();
    }

    fn cancel_new_key(&mut self, cx: &mut Context<Self>) {
        self.confirming = None;
        cx.notify();
    }

    /// Confirmed: a new key; the old one stops working.
    fn change_key(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let Some(reset) = self.confirming else {
            return;
        };
        let password = self.password_input.read(cx).value().to_owned();
        if !self.form.begin() {
            return;
        }
        let making = if reset {
            session.reset_lost_recovery((!password.is_empty()).then_some(password))
        } else {
            session.change_recovery_key()
        };
        cx.spawn(async move |this, cx| {
            let made = making.await;
            this.update(cx, |this, cx| {
                match made {
                    Ok(key) => {
                        this.form.made(key);
                        this.refresh(cx);
                    }
                    Err(err) => this.form.failed(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// "Restore from backup": the typed recovery key.
    fn submit_key(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        let typed = self.key_input.read(cx).value().to_owned();
        if !self.form.begin_submit(&typed) {
            return;
        }
        let restoring = session.recover(typed);
        cx.spawn(async move |this, cx| {
            let result = restoring.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(()) => {
                        this.key_input
                            .update(cx, |input, cx| input.set_value("", cx));
                        this.form.restored();
                        this.keys_changed(cx);
                    }
                    Err(err) => this.form.failed(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// "Verify with another signed-in device".
    fn start_verification(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        if !self.form.begin() {
            return;
        }
        let requesting = session.request_self_verification();
        cx.spawn(async move |this, cx| {
            let result = requesting.await;
            this.update(cx, |this, cx| {
                match result {
                    Ok(handle) => {
                        this.form.busy = false;
                        this.track(handle, false, cx);
                    }
                    Err(err) => this.form.failed(err.to_string()),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Mirror a flow's steps from now on.
    fn track(&mut self, mut handle: VerificationHandle, incoming: bool, cx: &mut Context<Self>) {
        let actions = handle.actions();
        let flow = cx.new(|_| Flow {
            actions,
            step: VerificationStep::Requested,
            incoming,
        });
        let weak = flow.downgrade();
        cx.spawn(async move |_, cx| {
            while let Some(step) = handle.next().await {
                if weak
                    .update(cx, |flow, cx| {
                        flow.step = step;
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        self.flow_watch = Some(cx.observe(&flow, |this, flow, cx| {
            let done = verification::is_done(&flow.read(cx).step);
            if done && !this.done_handled {
                this.done_handled = true;
                this.keys_changed(cx);
                // The success shows for a moment, then makes way for the
                // settings (which now list the device as verified).
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(Duration::from_secs(VERIFIED_NOTE_SECS))
                        .await;
                    this.update(cx, |this, cx| {
                        if this
                            .flow
                            .as_ref()
                            .is_some_and(|f| verification::is_done(&f.read(cx).step))
                        {
                            this.end_flow(cx);
                        }
                    })
                    .ok();
                })
                .detach();
            }
            cx.notify();
        }));
        self.done_handled = false;
        self.flow = Some(flow);
        self.page = SettingsPage::Security;
        cx.notify();
    }

    fn end_flow(&mut self, cx: &mut Context<Self>) {
        self.flow = None;
        self.flow_watch = None;
        // The other device's trust has settled by now.
        self.refresh(cx);
        cx.notify();
    }

    /// A button of the verification panel.
    fn flow_action(&mut self, action: Action, cx: &mut Context<Self>) {
        let Some(flow) = self.flow.clone() else {
            return;
        };
        if action == Action::Dismiss {
            self.end_flow(cx);
            return;
        }
        let actions = flow.read(cx).actions.clone();
        let running = match action {
            Action::Accept => actions.accept(),
            Action::Cancel => actions.cancel(),
            Action::UseEmoji => actions.use_emoji(),
            Action::ConfirmScanned => actions.confirm_scanned(),
            Action::ConfirmMatch => actions.confirm_match(),
            Action::Mismatch => actions.mismatch(),
            Action::Dismiss => return,
        };
        cx.spawn(async move |_, _| {
            if let Err(err) = running.await {
                eprintln!("morpheus-desktop: verification action: {err}");
            }
        })
        .detach();
    }
}

/// A button in the settings window.
fn button(
    id: impl Into<SharedString>,
    _label: &str,
    kind: Kind,
    _classes: &str,
) -> Stateful<gpui::Div> {
    let id: SharedString = id.into();
    let (bg, fg) = match kind {
        Kind::Primary => (rgb(theme::accent_solid()), rgb(theme::on_accent_solid())),
        Kind::Secondary => (rgb(theme::bg_elevated()), rgb(theme::text())),
        Kind::Danger => (
            rgba(theme::alpha(theme::danger(), 0x26)),
            rgb(theme::danger()),
        ),
    };
    div()
        .id(ElementId::from(id))
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.0))
        .px(px(14.0))
        .py(px(10.0))
        .rounded(px(theme::RADIUS))
        .font_weight(FontWeight::SEMIBOLD)
        .cursor_pointer()
        .bg(bg)
        .text_color(fg)
        .tabbable()
}

fn kind_class(kind: Kind) -> &'static str {
    match kind {
        Kind::Primary => "primary",
        Kind::Secondary => "secondary",
        Kind::Danger => "danger",
    }
}

fn muted(text: &str, class: &str) -> impl IntoElement {
    div()
        .text_color(rgb(theme::text_muted()))
        .child(text.to_owned())
        .e2e("p", class)
        .text(text)
        .attr("data-color", format!("{:06x}", theme::text_muted()))
        .attr("data-bg", format!("{:06x}", theme::bg_chat()))
}

fn note(text: &str, class: &'static str, color: u32) -> impl IntoElement {
    div()
        .text_color(rgb(color))
        .child(text.to_owned())
        .e2e("p", class)
        .text(text)
}

fn section(
    title: &str,
    anchor: &'static str,
    children: Vec<gpui::AnyElement>,
    highlighted: bool,
) -> impl IntoElement {
    section_classed(title, anchor, children, highlighted, "")
}

/// [`section`] with more classes for the test bridge (`voice-devices`).
fn section_classed(
    title: &str,
    anchor: &'static str,
    children: Vec<gpui::AnyElement>,
    highlighted: bool,
    extra: &str,
) -> impl IntoElement {
    let classes = match (highlighted, extra.is_empty()) {
        (true, true) => "settings-section setting-highlight".to_owned(),
        (true, false) => format!("settings-section setting-highlight {extra}"),
        (false, true) => "settings-section".to_owned(),
        (false, false) => format!("settings-section {extra}"),
    };
    div()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .px(px(20.0))
        .py(px(16.0))
        .border_b_1()
        .border_color(rgb(theme::bg_elevated()))
        .when(highlighted, |d| d.bg(rgba(theme::accent_tint())))
        .child(
            div()
                .mb(px(8.0))
                .text_size(px(0.75 * theme::REM))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(theme::text_muted()))
                .child(title.to_uppercase())
                .e2e("h3", "")
                .text(title)
                .attr("data-color", format!("{:06x}", theme::text_muted()))
                .attr("data-bg", format!("{:06x}", theme::bg_chat())),
        )
        .children(children)
        .e2e("section", &classes)
        .attr("data-setting", anchor)
}

/// A verification code drawn as dark modules on white.
fn qr_el(code: &QrImage) -> impl IntoElement {
    let (size, modules) = verification::qr_modules(code);
    #[allow(unused_mut)]
    let mut shown = div()
        .size(px(240.0))
        .my(px(8.0))
        .mx_auto()
        .rounded(px(theme::RADIUS))
        .overflow_hidden()
        .bg(rgb(0xffffff)) // fixed: a QR code is black on white
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let cell = f32::from(bounds.size.width) / size as f32;
                    for (index, dark) in modules.iter().enumerate() {
                        if !dark {
                            continue;
                        }
                        let (x, y) = (index % size, index / size);
                        window.paint_quad(fill(
                            gpui::Bounds::new(
                                gpui::point(
                                    bounds.origin.x + px(x as f32 * cell),
                                    bounds.origin.y + px(y as f32 * cell),
                                ),
                                gpui::size(px(cell.ceil()), px(cell.ceil())),
                            ),
                            rgb(0x000000), // fixed: a QR code is black on white
                        ));
                    }
                },
            )
            .size_full(),
        );
    shown
        .e2e("div", "verification-qr")
        .attr("value", qr_data(code))
}

/// What the code holds (its bytes, comma separated), for test runs to scan it.
fn qr_data(code: &QrImage) -> String {
    code.data
        .iter()
        .map(|byte| byte.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

impl SettingsView {
    fn flow_panel(&self, flow: &Entity<Flow>, cx: &mut Context<Self>) -> gpui::AnyElement {
        let f = flow.read(cx);
        let panel = verification::panel(&f.step, f.incoming);
        let mut column = div()
            .flex()
            .flex_col()
            .gap(px(8.0))
            .p(px(12.0))
            .rounded(px(theme::RADIUS))
            .bg(rgb(theme::bg_elevated()));
        column = column.child(note(
            &panel.text,
            match panel.tone {
                Tone::Plain => "",
                Tone::Ok => "ok",
                Tone::Error => "error",
            },
            match panel.tone {
                Tone::Plain => theme::text(),
                Tone::Ok => theme::ok(),
                Tone::Error => theme::danger(),
            },
        ));
        if let Some(code) = &panel.qr {
            column = column.child(qr_el(code));
        }
        if !panel.emojis.is_empty() {
            column = column.child(
                div()
                    .flex()
                    .flex_wrap()
                    .my(px(12.0))
                    .children(
                        panel
                            .emojis
                            .iter()
                            .enumerate()
                            .map(|(index, (symbol, name))| {
                                div()
                                    .w(relative(0.25))
                                    .flex()
                                    .flex_col()
                                    .items_center()
                                    .gap(px(4.0))
                                    .py(px(4.0))
                                    .child(
                                        div()
                                            .text_size(px(2.0 * theme::REM))
                                            .child(symbol.clone())
                                            .e2e("span", "emoji")
                                            .text(symbol.clone()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(0.75 * theme::REM))
                                            .text_color(rgb(theme::text_muted()))
                                            .child(name.clone())
                                            .e2e("span", "emoji-name")
                                            .text(name.clone()),
                                    )
                                    .e2e("li", "")
                                    .attr("data-index", index.to_string())
                            }),
                    )
                    .e2e("ul", "emoji-grid"),
            );
        }
        if !panel.buttons.is_empty() {
            column = column.child(div().flex().gap(px(8.0)).children(panel.buttons.iter().map(
                |b| {
                    let action = b.action;
                    button(format!("flow-{}", b.label), b.label, b.kind, b.class)
                        .child(b.label)
                        .on_click(cx.listener(move |this, _, _, cx| this.flow_action(action, cx)))
                        .e2e("button", &format!("{} {}", kind_class(b.kind), b.class))
                        .text(b.label)
                },
            )));
        }
        column.e2e("div", "verification").into_any_element()
    }

    fn security_page(&mut self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let busy = self.form.busy;
        let mut encryption: Vec<gpui::AnyElement> = Vec::new();
        match &self.status {
            None => encryption.push(muted("Checking…", "muted").into_any_element()),
            Some(Err(err)) => {
                encryption.push(note(err, "error", theme::danger()).into_any_element())
            }
            Some(Ok(status)) => encryption.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .children(status_lines(status).into_iter().map(|line| {
                        let color = match line.class {
                            "ok" => theme::ok(),
                            "warn" => theme::warn(),
                            _ => theme::text_muted(),
                        };
                        let text = format!("● {}", line.text);
                        div()
                            .text_color(rgb(color))
                            .child(text.clone())
                            .e2e("li", line.class)
                            .text(text)
                    }))
                    .e2e("ul", "status-list")
                    .into_any_element(),
            ),
        }

        let offer = recovery_offer(
            self.status.as_ref().and_then(|s| s.as_ref().ok()),
            self.form.new_key.is_some(),
        );
        let offer = if self.confirming.is_some() && offer != RecoveryOffer::NewKey {
            RecoveryOffer::None
        } else {
            offer
        };
        match offer {
            RecoveryOffer::NewKey => {
                let key = self.form.new_key.clone().unwrap_or_default();
                let copied = key.clone();
                encryption.push(
                    div()
                        .flex()
                        .flex_col()
                        .items_start()
                        .gap(px(8.0))
                        .my(px(12.0))
                        .p(px(12.0))
                        .rounded(px(theme::RADIUS_LG))
                        .bg(rgb(theme::bg_elevated()))
                        .child(
                            div()
                                .font_weight(FontWeight::BOLD)
                                .child("Your recovery key")
                                .e2e("strong", "")
                                .text("Your recovery key"),
                        )
                        .child(muted(keys::RECOVERY_SAVE_NOTE, "muted"))
                        .child(
                            div()
                                .font_family(theme::MONO)
                                .text_size(px(0.95 * theme::REM))
                                .child(key.clone())
                                .e2e("code", "")
                                .text(key),
                        )
                        .child(
                            button("copy-key", "Copy", Kind::Secondary, "")
                                .child(icon("copy", 16.0).text_color(rgb(theme::text())))
                                .child("Copy")
                                .on_click(move |_, _, cx| {
                                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                        copied.clone(),
                                    ))
                                })
                                .e2e("button", "secondary")
                                .text("Copy"),
                        )
                        .e2e("div", "recovery-key-new")
                        .into_any_element(),
                );
            }
            RecoveryOffer::SetUp => encryption.push(
                div()
                    .flex()
                    .my(px(12.0))
                    .child(
                        button("setup-recovery", "Set up recovery", Kind::Primary, "")
                            .when(busy, |d| d.opacity(0.6))
                            .child(icon("shield", 16.0).text_color(rgb(theme::on_accent_solid())))
                            .child("Set up recovery")
                            .on_click(cx.listener(|this, _, _, cx| this.set_up_recovery(cx)))
                            .e2e("button", "primary setup-recovery")
                            .text("Set up recovery"),
                    )
                    .e2e("div", "settings-actions")
                    .into_any_element(),
            ),
            RecoveryOffer::Change => encryption.push(
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(8.0))
                    .my(px(12.0))
                    .child(
                        button("change-key", "Change recovery key", Kind::Secondary, "")
                            .when(busy, |d| d.opacity(0.6))
                            .child(icon("refresh-cw", 16.0).text_color(rgb(theme::text())))
                            .child("Change recovery key")
                            .on_click(cx.listener(|this, _, _, cx| this.ask_new_key(false, cx)))
                            .e2e("button", "secondary change-recovery-key")
                            .text("Change recovery key"),
                    )
                    .child(muted(keys::RECOVERY_CHANGE_NOTE, "muted"))
                    .e2e("div", "settings-actions")
                    .into_any_element(),
            ),
            RecoveryOffer::Reset => encryption.push(
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(8.0))
                    .my(px(12.0))
                    .child(
                        button("reset-key", "Reset recovery key", Kind::Secondary, "")
                            .when(busy, |d| d.opacity(0.6))
                            .child(icon("refresh-cw", 16.0).text_color(rgb(theme::text())))
                            .child("Reset recovery key")
                            .on_click(cx.listener(|this, _, _, cx| this.ask_new_key(true, cx)))
                            .e2e("button", "secondary reset-recovery-key")
                            .text("Reset recovery key"),
                    )
                    .child(muted(keys::RECOVERY_RESET_NOTE, "muted"))
                    .e2e("div", "settings-actions")
                    .into_any_element(),
            ),
            RecoveryOffer::None => {}
        }
        if let Some(reset) = self.confirming.filter(|_| self.form.new_key.is_none()) {
            let warning = if reset {
                keys::CONFIRM_RESET_WARNING
            } else {
                keys::CONFIRM_CHANGE_WARNING
            };
            encryption.push(
                div()
                    .flex()
                    .flex_col()
                    .items_start()
                    .gap(px(8.0))
                    .my(px(12.0))
                    .p(px(12.0))
                    .rounded(px(theme::RADIUS_LG))
                    .bg(rgb(theme::bg_elevated()))
                    .child(muted(warning, "muted"))
                    .when(reset, |d| {
                        d.child(
                            div()
                                .w_full()
                                .text_size(px(theme::REM))
                                .text_color(rgb(theme::text()))
                                .child(self.password_input.clone()),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .gap(px(8.0))
                            .child(
                                button("confirm-new-key", "Create new key", Kind::Primary, "")
                                    .when(busy, |d| d.opacity(0.6))
                                    .child("Create new key")
                                    .on_click(cx.listener(|this, _, _, cx| this.change_key(cx)))
                                    .e2e("button", "primary confirm-new-key")
                                    .text("Create new key"),
                            )
                            .child(
                                button("cancel-new-key", "Cancel", Kind::Secondary, "")
                                    .child("Cancel")
                                    .on_click(
                                        cx.listener(|this, _, _, cx| this.cancel_new_key(cx)),
                                    )
                                    .e2e("button", "secondary cancel-key-reset")
                                    .text("Cancel"),
                            ),
                    )
                    .e2e("div", "confirm-key-reset")
                    .into_any_element(),
            );
        }


        if let Some(flow) = self.flow.clone() {
            encryption.push(self.flow_panel(&flow, cx));
        } else {
            let typed = self.key_input.read(cx).value().to_owned();
            let _ = recovery_key_ready(&typed);
            encryption.push(
                div()
                    .flex()
                    .my(px(12.0))
                    .child(
                        button(
                            "verify-device",
                            "Verify with another signed-in device",
                            Kind::Primary,
                            "",
                        )
                        .when(busy, |d| d.opacity(0.6))
                        .child(icon("shield-check", 16.0).text_color(rgb(theme::on_accent_solid())))
                        .child("Verify with another signed-in device")
                        .on_click(cx.listener(|this, _, _, cx| this.start_verification(cx)))
                        .e2e("button", "primary")
                        .text("Verify with another signed-in device"),
                    )
                    .e2e("div", "settings-actions")
                    .into_any_element(),
            );
            encryption.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.0))
                            .text_size(px(0.75 * theme::REM))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(theme::text_muted()))
                            .child("RECOVERY KEY OR PASSPHRASE")
                            .child(
                                div()
                                    .text_size(px(theme::REM))
                                    .text_color(rgb(theme::text()))
                                    .child(self.key_input.clone()),
                            )
                            .e2e("label", "")
                            .text("Recovery key or passphrase"),
                    )
                    .child(
                        div().flex().child(
                            button("restore-backup", "Restore from backup", Kind::Secondary, "")
                                .when(busy, |d| d.opacity(0.6))
                                .child(icon("key", 16.0).text_color(rgb(theme::text())))
                                .child("Restore from backup")
                                .on_click(cx.listener(|this, _, _, cx| this.submit_key(cx)))
                                .e2e("button", "secondary")
                                .attr("type", "submit")
                                .text("Restore from backup"),
                        ),
                    )
                    .e2e("form", "recovery-form")
                    .into_any_element(),
            );
        }
        match &self.form.message {
            Some(Ok(message)) => {
                encryption.push(note(message, "ok", theme::ok()).into_any_element())
            }
            Some(Err(message)) => {
                encryption.push(note(message, "error", theme::danger()).into_any_element())
            }
            None => {}
        }

        let devices: Vec<gpui::AnyElement> = match &self.devices {
            None => vec![
                // Waiting shows the shape of what is coming, not words.
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .children([60.0, 85.0, 45.0].map(|width| {
                        div()
                            .h(px(14.0))
                            .w(gpui::relative(width / 100.0))
                            .rounded(px(7.0))
                            .bg(rgb(theme::bg_elevated()))
                            .e2e("div", "skeleton skeleton-line")
                    }))
                    .e2e("div", "skeleton-block")
                    .into_any_element(),
            ],
            Some(Err(err)) => vec![note(err, "error", theme::danger()).into_any_element()],
            Some(Ok(list)) => vec![
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .children(list.iter().map(device_row))
                    .e2e("ul", "device-list")
                    .into_any_element(),
            ],
        };
        vec![
            section(
                "Encryption",
                "encryption",
                encryption,
                self.hl("encryption"),
            )
            .into_any_element(),
            section("Devices", "sessions", devices, self.hl("sessions")).into_any_element(),
            self.locked_chats_section(cx),
        ]
    }

    fn account_page(&self, cx: &mut Context<Self>) -> Vec<gpui::AnyElement> {
        let user = self
            .session
            .as_ref()
            .and_then(|s| s.user_id())
            .map(|id| id.to_string())
            .unwrap_or_default();
        let mut children: Vec<gpui::AnyElement> = vec![
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(crate::streamer::id(&user))
                .e2e("p", "settings-user")
                .text(crate::streamer::id(&user))
                .into_any_element(),
        ];
        if let Some(Ok(status)) = &self.status {
            let device = status
                .device_id
                .as_ref()
                .map(|d| d.to_string())
                .unwrap_or_default();
            children.push(
                muted(
                    &format!("This device: {}", crate::streamer::id(&device)),
                    "muted",
                )
                .into_any_element(),
            );
        }
        if let Some(url) = self.manage_url.clone() {
            children.push(
                button("manage-account", "Manage account", Kind::Secondary, "")
                    .child(icon("link", 16.0).text_color(rgb(theme::text())))
                    .child("Manage account")
                    .on_click(cx.listener(move |_, _, _, cx| crate::shell::open_external(&url, cx)))
                    .e2e("button", "secondary manage-account")
                    .text("Manage account")
                    .into_any_element(),
            );
        }
        let mut page =
            vec![section("Account", "account", children, self.hl("account")).into_any_element()];
        page.extend(self.edit_profile_page(cx));
        page
    }
}

fn device_row(device: &Device) -> impl IntoElement {
    let tag = |text: &str, class: &str, bg: u32| {
        div()
            .px(px(8.0))
            .py(px(1.0))
            .rounded(px(10.0))
            .bg(rgba(bg))
            .text_size(px(0.7 * theme::REM))
            .font_weight(FontWeight::BOLD)
            .child(text.to_uppercase())
            .e2e("span", class)
            .text(text)
    };
    div()
        .flex()
        .items_center()
        .gap(px(8.0))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(device.name.clone())
                .e2e("span", "device-name")
                .text(device.name.clone()),
        )
        .child(
            div()
                .font_family(theme::MONO)
                .text_size(px(0.75 * theme::REM))
                .text_color(rgb(theme::text_muted()))
                .child(device.id.to_string())
                .e2e("span", "device-id muted")
                .text(device.id.to_string()),
        )
        .children(device.is_current.then(|| {
            div().ml_auto().child(tag(
                "this device",
                "tag",
                theme::alpha(theme::bg_elevated(), 0xff),
            ))
        }))
        .child(if device.verified {
            tag("verified", "tag ok", theme::alpha(theme::ok(), 0x26))
        } else {
            tag("unverified", "tag warn", theme::alpha(theme::warn(), 0x26))
        })
        .e2e("li", "device")
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.open {
            return div().into_any_element();
        }
        if self.take_focus {
            self.take_focus = false;
            if self.focus_key || self.flow.is_some() {
                window.focus(&self.focus);
            } else {
                // Settings opens with the search bar ready to type in.
                self.search.read(cx).focus(window);
            }
        }
        if self.focus_key && self.page == SettingsPage::Security && self.flow.is_none() {
            self.focus_key = false;
            self.key_input.read(cx).focus(window);
        }
        let page = self.page;
        let query = self.search.read(cx).value().to_owned();
        let searching = !query.trim().is_empty();
        let mut results: Vec<gpui::AnyElement> = find_settings(&query, true)
            .into_iter()
            .map(|entry: &'static SettingEntry| {
                div()
                    .id(ElementId::from(SharedString::from(format!(
                        "result-{}",
                        entry.anchor
                    ))))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .w_full()
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(px(theme::RADIUS_SM))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(theme::bg_hover())))
                    .child(icon(entry.page.glyph(), 18.0).text_color(rgb(theme::text())))
                    .child(div().child(entry.label).e2e("span", "").text(entry.label))
                    .child(
                        div()
                            .ml_auto()
                            .text_size(px(0.75 * theme::REM))
                            .text_color(rgb(theme::text_muted()))
                            .child(entry.page.label())
                            .e2e("span", "settings-result-page muted")
                            .text(entry.page.label()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.show_setting(entry, cx)))
                    .e2e("button", "settings-result")
                    .into_any_element()
            })
            .collect();
        if searching && results.is_empty() {
            results
                .push(muted("No setting matches.", "muted settings-no-result").into_any_element());
        }
        let nav = div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .w(px(220.0))
            .flex_none()
            .h_full()
            .px(px(8.0))
            .py(px(12.0))
            .rounded_tl(px(theme::RADIUS_XL))
            .rounded_bl(px(theme::RADIUS_XL))
            .bg(rgb(theme::bg_sidebar()))
            .child({
                let (name, image) = self.own.clone();
                let name = if name.is_empty() {
                    self.session
                        .as_ref()
                        .and_then(|s| s.user_id())
                        .map(|id| id.localpart().to_owned())
                        .unwrap_or_default()
                } else {
                    name
                };
                let name = crate::streamer::name(&name);
                div()
                    .id("settings-profile")
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .px(px(10.0))
                    .pt(px(6.0))
                    .pb(px(10.0))
                    .rounded(px(theme::RADIUS_SM))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(theme::bg_hover())))
                    .child(crate::media::avatar(image, &name, 40.0, None))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(name.clone())
                                    .e2e("span", "settings-profile-name")
                                    .text(name.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(4.0))
                                    .text_size(px(0.75 * theme::REM))
                                    .text_color(rgb(theme::text_muted()))
                                    .child(
                                        icon("pencil", 12.0).text_color(rgb(theme::text_muted())),
                                    )
                                    .child("Edit profile")
                                    .e2e("span", "settings-profile-edit")
                                    .text("Edit profile"),
                            ),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.stop_video(cx);
                        this.page = SettingsPage::Account;
                        cx.notify();
                    }))
                    .e2e("button", "settings-profile")
            })
            .child(div().mb(px(8.0)).child(self.search.clone()))
            .children(results)
            .children(
                SettingsPage::listed(true)
                    .into_iter()
                    .filter(|item| !searching && *item != SettingsPage::Account)
                    .map(|item| {
                        let selected = item == page;
                        div()
                            .id(ElementId::from(SharedString::from(format!(
                                "nav-{}",
                                item.label()
                            ))))
                            .flex()
                            .items_center()
                            .gap(px(10.0))
                            .w_full()
                            .px(px(10.0))
                            .py(px(6.5))
                            .rounded(px(theme::RADIUS_SM))
                            .cursor_pointer()
                            .text_color(rgb(if selected {
                                theme::text()
                            } else {
                                theme::text_muted()
                            }))
                            .when(selected, |d| d.bg(rgba(theme::bg_selected())))
                            .hover(|s| s.bg(rgba(theme::bg_hover())).text_color(rgb(theme::text())))
                            .child(icon(item.glyph(), 18.0).text_color(rgb(if selected {
                                theme::text()
                            } else {
                                theme::text_muted()
                            })))
                            .child(item.label())
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if item != SettingsPage::VoiceVideo {
                                    this.stop_video(cx);
                                }
                                this.page = item;
                                cx.notify();
                            }))
                            .e2e(
                                "button",
                                if selected {
                                    "settings-nav-item selected"
                                } else {
                                    "settings-nav-item"
                                },
                            )
                            .text(item.label())
                    }),
            )
            .e2e("nav", "settings-nav");

        let body = match page {
            SettingsPage::Account => self.account_page(cx),
            SettingsPage::Security => self.security_page(cx),
            SettingsPage::Notifications => self.notifications_page(cx),
            SettingsPage::Appearance => self.appearance_page(cx),
            SettingsPage::VoiceVideo => self.voice_page(cx),
            SettingsPage::StreamerMode => self.streamer_page(cx),
            SettingsPage::Startup => self.startup_page(cx),
            SettingsPage::Help => self.help_page(window, cx),
        };
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .px(px(20.0))
            .py(px(16.0))
            .border_b_1()
            .border_color(rgb(theme::bg_elevated()))
            .child(
                div()
                    .flex_1()
                    .text_size(px(1.1 * theme::REM))
                    .font_weight(FontWeight::BOLD)
                    .child(page.label())
                    .e2e("h2", "")
                    .text(page.label()),
            )
            .child(
                div()
                    .id("settings-close")
                    .px(px(8.0))
                    .py(px(4.0))
                    .rounded(px(theme::RADIUS_SM))
                    .cursor_pointer()
                    .text_color(rgb(theme::text_muted()))
                    .hover(|s| s.bg(rgba(theme::bg_hover())))
                    .child("✕")
                    .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
                    .e2e("button", "ghost")
                    .attr("title", "Close")
                    .text("✕"),
            )
            .e2e("header", "settings-header");
        let content = div()
            .id("settings-page")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            // A click elsewhere on the page closes an open device list.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.picker_open.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .child(header)
            .children(body)
            .e2e("section", "settings-page")
            .scrollable(std::rc::Rc::new(crate::e2e::HandleProbe(
                self.scroll.clone(),
            )));
        let panel = div()
            .id("settings")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.close(cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .flex()
            .w(relative(0.7))
            .h(relative(0.8))
            .max_h(px(760.0))
            .overflow_hidden()
            .rounded(px(theme::RADIUS_XL))
            .bg(rgb(theme::bg_chat()))
            .shadow_lg()
            .child(nav)
            .child(content)
            .e2e("div", "settings");
        div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgba(0x0000008c))
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close(cx)),
            )
            .child(panel)
            .e2e("div", "overlay settings-overlay")
            .into_any_element()
    }
}
