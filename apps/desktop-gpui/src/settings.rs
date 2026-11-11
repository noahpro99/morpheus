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
    Device, EncryptionStatus, QrImage, Session, VerificationActions, VerificationHandle,
    VerificationStep,
};

use crate::{
    e2e::E2eExt as _,
    icons::icon,
    input::{Submit, TextInput},
    theme,
    view_model::{
        keys::{
            self, RecoveryForm, RecoveryOffer, SettingsFocus, SettingsPage, recovery_key_ready,
            recovery_offer, status_lines,
        },
        verification::{self, Action, Kind, Tone, VERIFIED_NOTE_SECS},
    },
};

/// This device's keys may have changed (a recovery or a verification
/// finished): the shell reads them again at once.
pub struct KeysChanged;

impl EventEmitter<KeysChanged> for SettingsView {}

/// A verification flow as the window shows it: the core's steps, mirrored
/// as they come, and the actions that drive it.
pub struct Flow {
    actions: VerificationActions,
    step: VerificationStep,
    incoming: bool,
}

pub struct SettingsView {
    session: Option<Session>,
    open: bool,
    page: SettingsPage,
    status: Option<Result<EncryptionStatus, String>>,
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
}

impl SettingsView {
    pub fn new(session: Option<Session>, cx: &mut Context<Self>) -> Self {
        let key_input = cx.new(|cx| {
            TextInput::new("recovery-key", keys::RECOVERY_PLACEHOLDER, true, "", cx)
        });
        cx.subscribe(&key_input, |this, _, _: &Submit, cx| this.submit_key(cx))
            .detach();
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
        }
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
        self.take_focus = true;
        self.focus_key = focus == SettingsFocus::RecoveryKey;
        self.set_up_when_ready = focus == SettingsFocus::SetUpBackup;
        self.refresh(cx);
        cx.notify();
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
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
                eprintln!("morpheus-gpui: verification action: {err}");
            }
        })
        .detach();
    }
}

/// A button in the settings window.
fn button(id: impl Into<SharedString>, _label: &str, kind: Kind, _classes: &str) -> Stateful<gpui::Div> {
    let id: SharedString = id.into();
    let (bg, fg) = match kind {
        Kind::Primary => (rgb(theme::ACCENT_STRONG), rgb(theme::TEXT)),
        Kind::Secondary => (rgb(theme::BG_ELEVATED), rgb(theme::TEXT)),
        Kind::Danger => (rgba(0xff6b6b26), rgb(theme::DANGER)),
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
        .text_color(rgb(theme::TEXT_MUTED))
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

const OK: u32 = 0x4fd1a5;
const WARN: u32 = 0xffb020;

fn section(title: &str, anchor: &'static str, children: Vec<gpui::AnyElement>) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .px(px(20.0))
        .py(px(16.0))
        .border_b_1()
        .border_color(rgb(theme::BG_ELEVATED))
        .child(
            div()
                .mb(px(8.0))
                .text_size(px(0.75 * theme::REM))
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(theme::TEXT_MUTED))
                .child(title.to_uppercase())
                .e2e("h3", "")
                .text(title),
        )
        .children(children)
        .e2e("section", "settings-section")
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
            .bg(rgb(theme::BG_ELEVATED));
        column = column.child(note(
            &panel.text,
            match panel.tone {
                Tone::Plain => "",
                Tone::Ok => "ok",
                Tone::Error => "error",
            },
            match panel.tone {
                Tone::Plain => theme::TEXT,
                Tone::Ok => OK,
                Tone::Error => theme::DANGER,
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
                                    .text_color(rgb(theme::TEXT_MUTED))
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
                encryption.push(note(err, "error", theme::DANGER).into_any_element())
            }
            Some(Ok(status)) => encryption.push(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .children(status_lines(status).into_iter().map(|line| {
                        let color = match line.class {
                            "ok" => OK,
                            "warn" => WARN,
                            _ => theme::TEXT_MUTED,
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
                        .bg(rgb(theme::BG_ELEVATED))
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
                                .child(icon("copy", 16.0).text_color(rgb(theme::TEXT)))
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
                            .child(icon("shield", 16.0).text_color(rgb(theme::TEXT)))
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
                            .child(icon("refresh-cw", 16.0).text_color(rgb(theme::TEXT)))
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
                        .child(icon("shield-check", 16.0).text_color(rgb(theme::TEXT)))
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
                            .text_color(rgb(theme::TEXT_MUTED))
                            .child("RECOVERY KEY OR PASSPHRASE")
                            .child(
                                div()
                                    .text_size(px(theme::REM))
                                    .text_color(rgb(theme::TEXT))
                                    .child(self.key_input.clone()),
                            )
                            .e2e("label", "")
                            .text("Recovery key or passphrase"),
                    )
                    .child(
                        div().flex().child(
                            button("restore-backup", "Restore from backup", Kind::Secondary, "")
                                .when(busy, |d| d.opacity(0.6))
                                .child(icon("key", 16.0).text_color(rgb(theme::TEXT)))
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
            Some(Ok(message)) => encryption.push(note(message, "ok", OK).into_any_element()),
            Some(Err(message)) => {
                encryption.push(note(message, "error", theme::DANGER).into_any_element())
            }
            None => {}
        }

        let devices: Vec<gpui::AnyElement> = match &self.devices {
            None => vec![muted("Loading…", "muted").into_any_element()],
            Some(Err(err)) => vec![note(err, "error", theme::DANGER).into_any_element()],
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
            section("Encryption", "encryption", encryption).into_any_element(),
            section("Devices", "sessions", devices).into_any_element(),
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
        vec![section("Account", "account", children).into_any_element()]
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
                .text_color(rgb(theme::TEXT_MUTED))
                .child(device.id.to_string())
                .e2e("span", "device-id muted")
                .text(device.id.to_string()),
        )
        .children(device.is_current.then(|| {
            div()
                .ml_auto()
                .child(tag("this device", "tag", 0x222736ff))
        }))
        .child(if device.verified {
            tag("verified", "tag ok", 0x4fd1a526)
        } else {
            tag("unverified", "tag warn", 0xffb02026)
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
            window.focus(&self.focus);
        }
        if self.focus_key && self.page == SettingsPage::Security && self.flow.is_none() {
            self.focus_key = false;
            self.key_input.read(cx).focus(window);
        }
        let page = self.page;
        let nav = div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .w(px(220.0))
            .flex_none()
            .h_full()
            .px(px(8.0))
            .py(px(12.0))
            .bg(rgb(theme::BG_SIDEBAR))
            .child(
                div()
                    .px(px(10.0))
                    .pt(px(6.0))
                    .pb(px(10.0))
                    .text_size(px(0.75 * theme::REM))
                    .font_weight(FontWeight::BOLD)
                    .text_color(rgb(theme::TEXT_MUTED))
                    .child("SETTINGS")
                    .e2e("h2", "")
                    .text("Settings"),
            )
            .children(SettingsPage::ALL.into_iter().map(|item| {
                let selected = item == page;
                div()
                    .id(ElementId::from(SharedString::from(format!("nav-{}", item.label()))))
                    .flex()
                    .items_center()
                    .gap(px(10.0))
                    .w_full()
                    .px(px(10.0))
                    .py(px(8.0))
                    .rounded(px(theme::RADIUS_SM))
                    .cursor_pointer()
                    .text_color(rgb(if selected { theme::TEXT } else { theme::TEXT_MUTED }))
                    .when(selected, |d| d.bg(rgba(theme::BG_SELECTED)))
                    .hover(|s| s.bg(rgba(theme::BG_HOVER)).text_color(rgb(theme::TEXT)))
                    .child(icon(item.glyph(), 18.0).text_color(rgb(if selected { theme::TEXT } else { theme::TEXT_MUTED })))
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
        };
        let header = div()
            .flex()
            .flex_none()
            .items_center()
            .gap(px(8.0))
            .px(px(20.0))
            .py(px(16.0))
            .border_b_1()
            .border_color(rgb(theme::BG_ELEVATED))
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
                    .text_color(rgb(theme::TEXT_MUTED))
                    .hover(|s| s.bg(rgba(theme::BG_HOVER)))
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
            .bg(rgb(theme::BG_CHAT))
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
