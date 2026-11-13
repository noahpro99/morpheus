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
    Device, EncryptionStatus, NotifyMode, QrImage, Session, VerificationActions, VerificationHandle,
    VerificationStep,
};

mod pages;

use crate::{
    e2e::E2eExt as _,
    icons::icon,
    input::{Submit, TextInput},
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

/// A call device was picked on the Voice & Video page (kept already): the
/// shell makes a call going now switch over.
pub struct DevicesChosen(pub crate::view_model::menu::DeviceKind);

impl EventEmitter<DevicesChosen> for SettingsView {}

/// A noise suppression mode was picked on the Voice & Video page.
pub struct NoiseChosen(pub morpheus_call::NoiseSuppression);

impl EventEmitter<NoiseChosen> for SettingsView {}

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
    open: bool,
    page: SettingsPage,
    status: Option<Result<EncryptionStatus, String>>,
    /// Times the status was read again because this device was not verified yet.
    status_rereads: u32,
    devices: Option<Result<Vec<Device>, String>>,
    form: RecoveryForm,
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
    data_dir: std::path::PathBuf,
    /// The devices to pick from (None while looking), what is chosen, and
    /// which list is open.
    voice: Option<VoiceLists>,
    voice_prefs: crate::view_model::calls::DevicePrefs,
    picker_open: Option<&'static str>,
}

impl SettingsView {
    pub fn new(session: Option<Session>, cx: &mut Context<Self>) -> Self {
        let key_input = cx.new(|cx| {
            TextInput::new("recovery-key", keys::RECOVERY_PLACEHOLDER, true, "", cx)
        });
        cx.subscribe(&key_input, |this, _, _: &Submit, cx| this.submit_key(cx))
            .detach();
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
            open: false,
            page: SettingsPage::Account,
            status: None,
            status_rereads: 0,
            devices: None,
            form: RecoveryForm::default(),
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
            data_dir,
            voice: None,
            voice_prefs: Default::default(),
            picker_open: None,
        }
    }

    /// Whether the settings window is showing.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Show the window, on the page `focus` asks for.
    pub fn open(&mut self, focus: SettingsFocus, cx: &mut Context<Self>) {
        self.open = true;
        self.form = RecoveryForm::default();
        self.key_input.update(cx, |input, cx| input.set_value("", cx));
        self.page = SettingsPage::for_focus(focus).unwrap_or(if self.flow.is_some() {
            SettingsPage::Security
        } else {
            SettingsPage::Account
        });
        self.search.update(cx, |input, cx| input.set_value("", cx));
        self.highlight = None;
        self.on_here = notifications_enabled(&self.data_dir);
        self.sounds_here = message_sounds_enabled(&self.data_dir);
        self.content = notification_content(&self.data_dir);
        self.autostart = read_autostart();
        self.startup_note = None;
        self.load_notify_defaults(cx);
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

    /// "Change recovery key": a new one; the old one stops working.
    fn change_key(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.session.clone() else {
            return;
        };
        if !self.form.begin() {
            return;
        }
        let making = session.change_recovery_key();
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
                        this.key_input.update(cx, |input, cx| input.set_value("", cx));
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
                        if this.flow.as_ref().is_some_and(|f| verification::is_done(&f.read(cx).step)) {
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
fn button(id: impl Into<SharedString>, _label: &str, kind: Kind, _classes: &str) -> Stateful<gpui::Div> {
    let id: SharedString = id.into();
    let (bg, fg) = match kind {
        Kind::Primary => (rgb(theme::accent_solid()), rgb(0xffffff)),
        Kind::Secondary => (rgb(theme::bg_elevated()), rgb(theme::text())),
        Kind::Danger => (rgba(theme::alpha(theme::danger(), 0x26)), rgb(theme::danger())),
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
                .text(title),
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
        .bg(rgb(0xffffff))
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
                            rgb(0x000000),
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
                    .children(panel.emojis.iter().enumerate().map(|(index, (symbol, name))| {
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
                    }))
                    .e2e("ul", "emoji-grid"),
            );
        }
        if !panel.buttons.is_empty() {
            column = column.child(
                div()
                    .flex()
                    .gap(px(8.0))
                    .children(panel.buttons.iter().map(|b| {
                        let action = b.action;
                        button(format!("flow-{}", b.label), b.label, b.kind, b.class)
                            .child(b.label)
                            .on_click(cx.listener(move |this, _, _, cx| this.flow_action(action, cx)))
                            .e2e("button", &format!("{} {}", kind_class(b.kind), b.class))
                            .text(b.label)
                    })),
            );
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
                            .child(icon("shield", 16.0).text_color(rgb(theme::text())))
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
                            .on_click(cx.listener(|this, _, _, cx| this.change_key(cx)))
                            .e2e("button", "secondary change-recovery-key")
                            .text("Change recovery key"),
                    )
                    .child(muted(keys::RECOVERY_CHANGE_NOTE, "muted"))
                    .e2e("div", "settings-actions")
                    .into_any_element(),
            ),
            RecoveryOffer::None => {}
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
                        .child(icon("shield-check", 16.0).text_color(rgb(theme::text())))
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
            Some(Ok(message)) => encryption.push(note(message, "ok", theme::ok()).into_any_element()),
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
            section("Encryption", "encryption", encryption, self.hl("encryption")).into_any_element(),
            section("Devices", "sessions", devices, self.hl("sessions")).into_any_element(),
        ]
    }

    fn account_page(&self) -> Vec<gpui::AnyElement> {
        let user = self
            .session
            .as_ref()
            .and_then(|s| s.user_id())
            .map(|id| id.to_string())
            .unwrap_or_default();
        let mut children: Vec<gpui::AnyElement> = vec![
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(user.clone())
                .e2e("p", "settings-user")
                .text(user)
                .into_any_element(),
        ];
        if let Some(Ok(status)) = &self.status {
            let device = status
                .device_id
                .as_ref()
                .map(|d| d.to_string())
                .unwrap_or_default();
            children.push(muted(&format!("This device: {device}"), "muted").into_any_element());
        }
        vec![section("Account", "account", children, self.hl("account")).into_any_element()]
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
            div()
                .ml_auto()
                .child(tag("this device", "tag", theme::alpha(theme::bg_elevated(), 0xff)))
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
                    .id(ElementId::from(SharedString::from(format!("result-{}", entry.anchor))))
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
            results.push(muted("No setting matches.", "muted settings-no-result").into_any_element());
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
            .child(
                div()
                    .px(px(10.0))
                    .pt(px(6.0))
                    .pb(px(10.0))
                    .text_size(px(0.75 * theme::REM))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(theme::text_muted()))
                    .child("SETTINGS")
                    .e2e("h2", "")
                    .text("Settings"),
            )
            .child(div().mb(px(8.0)).child(self.search.clone()))
            .children(results)
            .children(SettingsPage::listed(true).into_iter().filter(|_| !searching).map(|item| {
                let selected = item == page;
                div()
                    .id(ElementId::from(SharedString::from(format!("nav-{}", item.label()))))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .w_full()
                    .px(px(10.0))
                    .py(px(6.5))
                    .rounded(px(theme::RADIUS_SM))
                    .cursor_pointer()
                    .text_color(rgb(if selected { theme::text() } else { theme::text_muted() }))
                    .when(selected, |d| d.bg(rgba(theme::bg_selected())))
                    .hover(|s| s.bg(rgba(theme::bg_hover())).text_color(rgb(theme::text())))
                    .child(icon(item.glyph(), 18.0).text_color(rgb(if selected { theme::text() } else { theme::text_muted() })))
                    .child(item.label())
                    .on_click(cx.listener(move |this, _, _, cx| {
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
            }))
            .e2e("nav", "settings-nav");

        let body = match page {
            SettingsPage::Account => self.account_page(),
            SettingsPage::Security => self.security_page(cx),
            SettingsPage::Notifications => self.notifications_page(cx),
            SettingsPage::Appearance => self.appearance_page(cx),
            SettingsPage::VoiceVideo => self.voice_page(cx),
            SettingsPage::Startup => self.startup_page(cx),
            SettingsPage::Help => self.help_page(cx),
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
            .e2e("section", "settings-page");
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
