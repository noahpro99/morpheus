//! Calls, what they look like (`crates/ui/src/components/call.rs`, its
//! second half): the people listed under a room's row, the voice panel in
//! the profile card, the call view above a room's chat (cards, controls, the
//! handle that resizes it), a voice channel's page, the rings. Video frames
//! are drawn natively by [`VideoFrame`] from the call crate's raw pixels.

use std::{rc::Rc, sync::Arc, time::Duration};

use crate::tooltip::TipExt as _;
use crate::zoom::ZoomPan;
use gpui::{
    AnyElement, App, Context, Entity, FontWeight, IntoElement, MouseButton, ObjectFit, RenderImage,
    Styled as _, StyledImage as _, Window, canvas, div, img, prelude::*, px, rgb, rgba,
};
use morpheus_core::{OwnedRoomId, OwnedUserId, RoomSummary};

use crate::{
    e2e::E2eExt as _,
    icons::icon,
    media::avatar,
    shell::Shell,
    theme,
    view_model::{
        calls::{
            CARD_RATIO, CallState, Card, VideoFeed, card_avatar_size, card_width, default_stage_height, focus_layout, focused_card,
            is_unwatched, listed, marks, media_of, tiles, toggled_focus,
        },
        nicknames::shown_name,
    },
};

/// A person's shown name: your nickname for them, else their profile's.
pub(crate) fn shown(shell: &Shell, user: &OwnedUserId) -> String {
    crate::streamer::name(&shown_name(
        &shell.nicknames,
        user.as_str(),
        &shell.calls.name_of(user),
    ))
}

fn picture_of(shell: &Shell, user: &OwnedUserId, size: u32) -> Option<Arc<gpui::Image>> {
    shell
        .calls
        .people
        .get(user)
        .and_then(|p| shell.picture(&p.avatar, size))
}

fn black(alpha: f32) -> gpui::Hsla {
    gpui::Hsla {
        h: 0.0,
        s: 0.0,
        l: 0.0,
        a: alpha,
    }
}

/// What someone in a call has on, as the shell knows it.
fn media(shell: &Shell, room: &OwnedRoomId, user: &OwnedUserId) -> morpheus_core::CallMedia {
    let me = shell.session.as_ref().and_then(|s| s.user_id());
    media_of(
        room,
        user,
        me.as_ref(),
        shell.calls.state.as_ref(),
        shell.calls.voice,
        &shell.rooms,
    )
}

/// Marks for someone in a call (muted, deafened, camera, LIVE), as elements
/// tagged the way the Dioxus app's are.
fn mark_elements(media: morpheus_core::CallMedia, video: bool, size: f32) -> Vec<AnyElement> {
    marks(media, video)
        .into_iter()
        .map(|mark| match mark.glyph {
            Some(glyph) => div()
                .flex_none()
                .flex()
                .items_center()
                .child(icon(glyph, size).text_color(rgb(
                    if mark.class.starts_with("voice-state") {
                        theme::danger()
                    } else {
                        theme::text_muted()
                    },
                )))
                .e2e("span", mark.class)
                .attr("title", mark.title)
                .into_any_element(),
            None => div()
                .flex_none()
                .px(px(5.0))
                .py(px(1.0))
                .rounded(px(4.0))
                .bg(rgb(theme::danger()))
                .text_color(rgb(theme::on_danger()))
                .text_size(px(0.65 * theme::REM))
                .font_weight(FontWeight::EXTRA_BOLD)
                .child("LIVE")
                .e2e("span", mark.class)
                .attr("title", mark.title)
                .text("LIVE")
                .into_any_element(),
        })
        .collect()
}

/// The people in a room's call, under its row in the sidebar. Clicking
/// joins, like a Discord voice channel.
pub fn room_call(shell: &Shell, cx: &mut Context<Shell>, room: &RoomSummary) -> Option<AnyElement> {
    let joining = shell
        .calls
        .state
        .as_ref()
        .is_some_and(|c| c.room_id == room.id && !c.connected);
    let in_this = shell.calls.in_call(&room.id);
    let me = shell.session.as_ref().and_then(|s| s.user_id());
    // Just hung up: the server has not said so yet, the list already does.
    let participants = shell.call_shown(room);
    if participants.is_empty() && !in_this {
        return None;
    }
    let speaking = shell
        .calls
        .state
        .as_ref()
        .filter(|c| c.room_id == room.id)
        .map(|c| c.speaking.clone())
        .unwrap_or_default();
    let (id, name) = (room.id.clone(), room.name.clone());
    let rows = listed(&participants, joining, me.as_ref())
        .into_iter()
        .map(|(user, connecting)| {
            let name = shown(shell, &user);
            let speaks = speaking.contains(&user);
            let mut classes = String::from("call-participant");
            if speaks {
                classes.push_str(" speaking");
            }
            if connecting {
                classes.push_str(" connecting");
            }
            let flags = media(shell, &room.id, &user);
            let mut row = div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(6.0))
                .py(px(3.0))
                .rounded(px(theme::RADIUS_SM))
                .text_size(px(0.85 * theme::REM))
                .text_color(rgb(if speaks {
                    theme::text()
                } else {
                    theme::text_muted()
                }))
                .when(connecting, |d| d.opacity(0.45))
                .child(
                    div()
                        .flex_none()
                        .rounded_full()
                        .border_2()
                        .border_color(if speaks {
                            gpui::Hsla::from(rgb(theme::green()))
                        } else {
                            gpui::Hsla::from(gpui::transparent_black())
                        })
                        .child(avatar(picture_of(shell, &user, 20), &name, 20.0, None)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(name.clone())
                        .e2e("span", "call-participant-name")
                        .text(name.clone()),
                );
            for mark in mark_elements(flags, true, 14.0) {
                row = row.child(mark);
            }
            // Beside the row: the card opens to the right of the sidebar.
            let sidebar_shown = shell.sidebar_shown;
            let beside = move |at: gpui::Point<gpui::Pixels>| {
                gpui::point(px(theme::RAIL_W + sidebar_shown - 8.0), at.y)
            };
            let (opened, menu_user, menu_room) = (user.clone(), user.clone(), room.id.clone());
            let (dbl_user, dbl_room) = (user.clone(), room.id.clone());
            let (seen_name, menu_name) = (name.clone(), name.clone());
            row.id(gpui::SharedString::from(format!(
                "call-participant-{}-{user}",
                room.id
            )))
            .cursor_pointer()
            .hover(|s| s.bg(rgba(theme::bg_hover())))
            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                // A double click while in this call: its view, on their
                // screen or card.
                if event.click_count() >= 2 && this.calls.in_call(&dbl_room) {
                    cx.stop_propagation();
                    this.focus_person_in_call(dbl_room.clone(), &dbl_user, cx);
                    return;
                }
                cx.stop_propagation();
                let seen = Some((seen_name.clone(), None));
                this.open_profile(opened.clone(), Some(beside(event.position())), seen, cx)
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.open_user_menu(
                        menu_room.clone(),
                        menu_user.clone(),
                        menu_name.clone(),
                        beside(event.position),
                        cx,
                    )
                }),
            )
            .e2e("div", &classes)
            .attr("data-user", user.to_string())
        })
        .collect::<Vec<_>>();
    let join = cx.listener(move |this, _, _, cx| this.join_call(id.clone(), name.clone(), cx));
    Some(
        div()
            .id(gpui::SharedString::from(format!("room-call-{}", room.id)))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .mt(px(0.0))
            .mb(px(4.0))
            .mr(px(8.0))
            .ml(px(32.0))
            .cursor_pointer()
            .on_click(join)
            .children(rows)
            .e2e("div", "room-call")
            .attr("data-room", room.id.to_string())
            .attr("title", "Join the call")
            .into_any_element(),
    )
}

/// A squircle button of the call (mute, deafen, camera, screen, leave).
struct Control {
    id: &'static str,
    classes: String,
    title: String,
    glyph: &'static str,
    size: f32,
    enabled: bool,
    pending: bool,
}

fn control_button(
    control: Control,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    on_right: Option<Box<dyn Fn(&gpui::MouseDownEvent, &mut Window, &mut App)>>,
) -> AnyElement {
    let Control {
        id,
        classes,
        title,
        glyph,
        size,
        enabled,
        pending,
    } = control;
    let active = classes.split(' ').any(|c| c == "active");
    let hang = classes.split(' ').any(|c| c == "hang-up");
    let mute = classes.split(' ').any(|c| c == "mute-button");
    let (bg, fg) = if hang || (mute && active) {
        (rgb(theme::danger()), rgb(theme::on_danger()))
    } else if active {
        (rgb(theme::text()), rgb(theme::bg_rail()))
    } else {
        (rgb(theme::bg_elevated()), rgb(theme::text()))
    };
    let side = size + 24.0;
    let mut button = div()
        .id(id)
        .flex_none()
        .size(px(side))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_XL))
        .bg(bg)
        .when(!enabled, |d| d.opacity(0.4))
        .when(pending, |d| d.opacity(0.7))
        .cursor_pointer()
        .hover(|s| s.opacity(0.9))
        .child(icon(glyph, size).text_color(fg))
        .on_click(on_click);
    button = crate::press::press(button, id, None);
    if let Some(on_right) = on_right {
        button = button.on_mouse_down(MouseButton::Right, on_right);
    }
    let tagged = button
        .tip(title.to_owned())
        .e2e("button", &classes)
        .attr("title", title)
        .attr(
            "data-bg",
            if hang || (mute && active) {
                "#ff6b6b"
            } else {
                ""
            },
        )
        .attr("data-radius", theme::RADIUS_XL.to_string());
    if enabled {
        tagged.into_any_element()
    } else {
        tagged.attr("disabled", "").into_any_element()
    }
}

/// The call's controls: along the bottom of its view, and (as small ghost
/// buttons) in the profile card's voice panel.
fn stage_controls(shell: &Shell, cx: &mut Context<Shell>, call: &CallState) -> AnyElement {
    let flags = shell.calls.voice;
    let muted = flags.mic_off();
    let mut row = div().flex_none().flex().justify_center().gap(px(12.0));
    row = row.child(control_button(
        Control {
            id: "stage-mute",
            classes: if muted {
                "call-control mute-button active".into()
            } else {
                "call-control mute-button".into()
            },
            title: if muted {
                "Unmute (Ctrl+Shift+M)".into()
            } else {
                "Mute (Ctrl+Shift+M)".into()
            },
            glyph: if muted { "mic-off" } else { "mic" },
            size: 20.0,
            enabled: true,
            pending: false,
        },
        cx.listener(|this, _, _, cx| this.toggle_mute(cx)),
        Some(audio_right_click(false, cx)),
    ));
    row = row.child(control_button(
        Control {
            id: "stage-deafen",
            classes: if flags.deafened {
                "call-control deafen-button active".into()
            } else {
                "call-control deafen-button".into()
            },
            title: flags.deafen_title().into(),
            glyph: if flags.deafened {
                "headphone-off"
            } else {
                "headphones"
            },
            size: 20.0,
            enabled: true,
            pending: false,
        },
        cx.listener(|this, _, _, cx| this.toggle_deafen(cx)),
        Some(audio_right_click(true, cx)),
    ));
    if morpheus_call::can_send_video() {
        let on = !call.camera;
        row = row.child(control_button(
            Control {
                id: "stage-camera",
                classes: if call.camera {
                    "call-control camera-button active".into()
                } else {
                    "call-control camera-button".into()
                },
                title: if call.camera {
                    "Turn off camera".into()
                } else {
                    "Turn on camera".into()
                },
                glyph: if call.camera { "video" } else { "video-off" },
                size: 20.0,
                enabled: call.connected,
                pending: false,
            },
            cx.listener(move |this, _, _, cx| this.set_camera(on, cx)),
            Some(camera_right_click(cx)),
        ));
    }
    if morpheus_call::can_share_screen() {
        let on = !call.screen;
        row = row.child(control_button(
            Control {
                id: "stage-screen",
                classes: screen_classes("call-control", call),
                title: screen_title(call).into(),
                glyph: if call.screen {
                    "screen-share-off"
                } else {
                    "screen-share"
                },
                size: 20.0,
                enabled: call.connected && !call.choosing_screen,
                pending: call.choosing_screen,
            },
            cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                this.press_screen(on, event.position(), cx)
            }),
            None,
        ));
    }
    row = row.child(control_button(
        Control {
            id: "stage-hang-up",
            classes: "call-control hang-up".into(),
            title: "Disconnect".into(),
            glyph: "phone-off",
            size: 20.0,
            enabled: true,
            pending: false,
        },
        cx.listener(|this, _, _, cx| this.hang_up(cx)),
        None,
    ));
    row.e2e("div", "call-controls stage-controls")
        .into_any_element()
}

fn screen_classes(base: &str, call: &CallState) -> String {
    let mut classes = format!("{base} screen-button");
    if call.screen {
        classes.push_str(" active");
    }
    if call.choosing_screen {
        classes.push_str(" pending");
    }
    classes
}

fn screen_title(call: &CallState) -> &'static str {
    if call.screen {
        "Stop sharing your screen"
    } else {
        "Share your screen"
    }
}

/// The mic (or, `speakers`, the headphones) button's right-click: its devices.
fn audio_right_click(
    speakers: bool,
    cx: &mut Context<Shell>,
) -> Box<dyn Fn(&gpui::MouseDownEvent, &mut Window, &mut App)> {
    let view = cx.entity();
    Box::new(move |event, _, cx| {
        let at = event.position;
        view.update(cx, |this, cx| {
            cx.stop_propagation();
            this.open_audio_menu(speakers, false, at, cx);
        });
    })
}

/// The camera button's right-click: the cameras, to pick from.
fn camera_right_click(
    cx: &mut Context<Shell>,
) -> Box<dyn Fn(&gpui::MouseDownEvent, &mut Window, &mut App)> {
    let view = cx.entity();
    Box::new(move |event, _, cx| {
        let at = event.position;
        view.update(cx, |this, cx| {
            cx.stop_propagation();
            this.open_camera_menu(at, cx);
        });
    })
}

/// A ghost icon button (profile card).
fn ghost_button(
    id: &'static str,
    classes: String,
    title: &'static str,
    glyph: &'static str,
    active: bool,
    pending: bool,
    enabled: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
    on_right: Option<Box<dyn Fn(&gpui::MouseDownEvent, &mut Window, &mut App)>>,
) -> AnyElement {
    let hang = classes.split(' ').any(|c| c == "hang-up");
    let mut button = div()
        .id(id)
        .size(px(32.0))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
        .cursor_pointer()
        .when(hang, |d| d.bg(rgb(theme::danger())))
        .when(!hang, |d| d.hover(|s| s.bg(rgba(theme::bg_hover()))))
        .when(hang, |d| d.hover(|s| s.opacity(0.9)))
        .when(!enabled, |d| d.opacity(0.4))
        .when(pending, |d| d.opacity(0.7))
        .child(icon(glyph, 18.0).text_color(if hang {
            rgb(theme::on_danger())
        } else if active {
            rgb(theme::green())
        } else {
            rgb(theme::text_muted())
        }))
        .on_click(on_click);
    if let Some(on_right) = on_right {
        button = button.on_mouse_down(MouseButton::Right, on_right);
    }
    let tagged = button
        .tip(title.to_owned())
        .e2e("button", &classes)
        .attr("title", title)
        .attr("data-bg", if hang { "#ff6b6b" } else { "" });
    if enabled {
        tagged.into_any_element()
    } else {
        tagged.attr("disabled", "").into_any_element()
    }
}

/// Where the voice panel would be, after a join that did not come off: why,
/// a button to try again and one to put it away.
pub fn join_error(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    if shell.calls.state.is_some() {
        return None;
    }
    let failed = shell.calls.join_error.clone()?;
    let (room_id, room_name) = (failed.room_id.clone(), failed.room_name.clone());
    let text = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            div()
                .text_size(px(0.85 * theme::REM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(theme::danger()))
                .child("Could not join")
                .e2e("span", "voice-state failed")
                .text("Could not join"),
        )
        .child(
            div()
                .text_size(px(0.8 * theme::REM))
                .text_color(rgb(theme::text_muted()))
                .child(failed.message.clone())
                .e2e("span", "call-join-error-text")
                .text(failed.message.clone()),
        );
    let panel = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .pt(px(10.0))
        .pb(px(10.0))
        .pl(px(12.0))
        .pr(px(6.0))
        .border_b_1()
        .border_color(rgba(theme::bg_hover()))
        .child(text)
        .child(ghost_button(
            "call-retry-join",
            "ghost retry-join".into(),
            "Try again",
            "refresh-cw",
            false,
            false,
            true,
            cx.listener(move |this, _, _, cx| {
                this.join_call(room_id.clone(), room_name.clone(), cx)
            }),
            None,
        ))
        .child(ghost_button(
            "call-dismiss-join-error",
            "ghost dismiss-join-error".into(),
            "Dismiss",
            "x",
            false,
            false,
            true,
            cx.listener(|this, _, _, cx| {
                this.calls.join_error = None;
                cx.notify();
            }),
            None,
        ));
    Some(
        panel
            .e2e("div", "call-join-error")
            .attr("data-room", failed.room_id.to_string())
            .into_any_element(),
    )
}

/// At the top of the profile card while in a call: which call, and hang up.
pub fn voice_panel(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let call = shell.calls.state.as_ref()?;
    let room_id = call.room_id.clone();
    let heard = crate::view_model::calls::heard_list(call);
    let status = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(
            div()
                .text_size(px(0.85 * theme::REM))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(if call.connected {
                    theme::green()
                } else {
                    theme::text_muted()
                }))
                .truncate()
                .child(if call.connected {
                    "Voice connected"
                } else {
                    "Connecting…"
                })
                .e2e(
                    "span",
                    if call.connected {
                        "voice-state connected"
                    } else {
                        "voice-state"
                    },
                )
                .text(if call.connected {
                    "Voice connected"
                } else {
                    "Connecting…"
                }),
        )
        .child(
            div()
                .id("voice-room")
                .text_size(px(0.8 * theme::REM))
                .text_color(rgb(theme::text_muted()))
                .truncate()
                .cursor_pointer()
                .hover(|s| s.text_color(rgb(theme::text())))
                .child(call.room_name.clone())
                .on_click(cx.listener(move |this, _, _, cx| this.open_room(room_id.clone(), cx)))
                .e2e("button", "voice-room")
                .attr("title", "Go to the room")
                .text(call.room_name.clone()),
        );
    let mut panel = div()
        .flex_none()
        .flex()
        .items_center()
        .gap(px(8.0))
        .pt(px(10.0))
        .pb(px(10.0))
        .pl(px(12.0))
        .pr(px(6.0))
        .border_b_1()
        .border_color(rgba(theme::bg_hover()))
        .child(status);
    panel = panel.child(crate::noise::button(shell, cx));
    if morpheus_call::can_send_video() {
        let on = !call.camera;
        panel = panel.child(ghost_button(
            "voice-camera",
            if call.camera {
                "ghost camera-button active".into()
            } else {
                "ghost camera-button".into()
            },
            if call.camera {
                "Turn off camera"
            } else {
                "Turn on camera"
            },
            if call.camera { "video" } else { "video-off" },
            call.camera,
            false,
            call.connected,
            cx.listener(move |this, _, _, cx| this.set_camera(on, cx)),
            Some(camera_right_click(cx)),
        ));
    }
    if morpheus_call::can_share_screen() {
        let on = !call.screen;
        panel = panel.child(ghost_button(
            "voice-screen",
            screen_classes("ghost", call),
            screen_title(call),
            if call.screen {
                "screen-share-off"
            } else {
                "screen-share"
            },
            call.screen,
            call.choosing_screen,
            call.connected && !call.choosing_screen,
            cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                this.press_screen(on, event.position(), cx)
            }),
            None,
        ));
    }
    panel = panel.child(ghost_button(
        "voice-hang-up",
        "ghost hang-up".into(),
        "Disconnect",
        "phone-off",
        false,
        false,
        true,
        cx.listener(|this, _, _, cx| this.hang_up(cx)),
        None,
    ));
    Some(
        panel
            .e2e("div", "voice-panel")
            .attr("data-heard", heard)
            // This device's own screen's sound, as it goes out.
            .attr("data-screen-audio", sent_screen_audio())
            // What the call's microphone is processed with now.
            .attr(
                "data-noise",
                morpheus_call::applied_noise_suppression()
                    .map_or("", morpheus_call::NoiseSuppression::key),
            )
            .into_any_element(),
    )
}

/// How a card fits the stage.
#[derive(Clone, Copy)]
enum Fit {
    /// One of a grid, this wide (16:9).
    Grid(f32),
    /// The focused card, filling its place: the room (width, height) it has.
    Main(f32, f32),
    /// One of the row under the focused card, this high (16:9).
    Strip(f32),
}

/// The largest `ratio` (width over height) box inside `room`.
pub fn fit_ratio(room: (f32, f32), ratio: f32) -> (f32, f32) {
    if room.0 / room.1 > ratio {
        ((room.1 * ratio).floor(), room.1)
    } else {
        (room.0, (room.0 / ratio).floor())
    }
}

/// A card in its place. A picture's card (`ratio`) takes the picture's shape,
/// as large as the place lets it, so none of it is cropped and there are no
/// bars; a person's fills the place (16:9 in a grid).
fn sized(
    base: gpui::Stateful<gpui::Div>,
    fit: Fit,
    ratio: Option<f32>,
) -> gpui::Stateful<gpui::Div> {
    let room = match fit {
        Fit::Grid(w) => (w, w / CARD_RATIO),
        Fit::Main(w, h) => (w, h),
        Fit::Strip(h) => ((h * CARD_RATIO).floor(), h),
    };
    match (fit, ratio) {
        // A person's card focused: 16:9, as large as the stage lets it.
        (Fit::Main(..), None) => {
            let (w, h) = fit_ratio(room, CARD_RATIO);
            base.w(px(w)).h(px(h)).flex_none()
        }
        (_, None) => base.w(px(room.0)).h(px(room.1)),
        (_, Some(ratio)) => {
            let (w, h) = fit_ratio(room, ratio);
            base.w(px(w)).h(px(h)).flex_none()
        }
    }
}

/// A card's Enlarge (or, focused, Shrink) at its top right, shown on a hover.
fn focus_button(shell: &Shell, cx: &mut Context<Shell>, key: &str, group: &str) -> AnyElement {
    let focused = shell.calls.focus.as_deref() == Some(key);
    let card = key.to_owned();
    div()
        .id(gpui::SharedString::from(format!("enlarge-{key}")))
        .absolute()
        .top(px(8.0))
        .right(px(8.0))
        .size(px(32.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
        .bg(black(0.6))
        .opacity(0.0)
        .group_hover(gpui::SharedString::from(group.to_owned()), |s| {
            s.opacity(1.0)
        })
        .cursor_pointer()
        .child(
            icon(if focused { "minimize-2" } else { "maximize-2" }, 16.0).text_color(rgb(theme::OVERLAY_TEXT)),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.toggle_focus(&card, cx);
        }))
        .tip(if focused { "Shrink" } else { "Enlarge" })
        .e2e("button", "card-enlarge")
        .attr("title", if focused { "Shrink" } else { "Enlarge" })
        .into_any_element()
}

/// What a shared screen's tile shows until its first picture comes: a screen
/// in the middle, pulsing a little.
fn screen_skeleton() -> AnyElement {
    use gpui::{Animation, AnimationExt as _};
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .child(icon("screen-share", 48.0))
                .with_animation(
                    "skeleton-pulse",
                    Animation::new(Duration::from_millis(1400))
                        .repeat()
                        .with_easing(gpui::pulsating_between(0.35, 0.9)),
                    |el, delta| el.opacity(delta),
                )
                .e2e("span", "skeleton-icon"),
        )
        .e2e("div", "screen-skeleton")
        .into_any_element()
}

/// A zoom button at the focused screen tile's bottom right:
/// - When not zoomed (zoom <= 1.001): shows "Zoom", clicking zooms halfway (2.0x).
/// - When zoomed: shows percentage (e.g. "200%"), clicking resets to fit.
fn zoom_control(shell: &Shell, key: &str, zoom: f32) -> AnyElement {
    let frame = shell.calls.frames.get(key).cloned();
    let zoomed = zoom > 1.001;
    let label = if zoomed {
        format!("{:.0}%", zoom * 100.0)
    } else {
        "Zoom".to_owned()
    };
    div()
        .id("screen-zoom-button")
        .absolute()
        .right(px(8.0))
        .bottom(px(8.0))
        .px(px(8.0))
        .py(px(4.0))
        .flex()
        .items_center()
        .gap(px(4.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(black(0.6))
        .hover(|s| s.bg(black(0.8)))
        .text_size(px(0.8 * theme::REM))
        .text_color(rgb(theme::OVERLAY_TEXT))
        .cursor_pointer()
        .when(!zoomed, |d| {
            d.child(icon("search", 13.0).text_color(rgb(theme::OVERLAY_TEXT)))
        })
        .child(label.clone())
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            if let Some(frame) = &frame {
                frame.update(cx, |f, cx| {
                    if f.view.zoomed() {
                        f.reset_zoom(cx);
                    } else {
                        f.zoom_halfway(cx);
                    }
                });
            }
        })
        .e2e(
            "button",
            if zoomed {
                "screen-zoom-button screen-zoom-indicator"
            } else {
                "screen-zoom-button"
            },
        )
        .attr(
            "title",
            if zoomed {
                "Reset zoom (fit to tile)"
            } else {
                "Zoom in halfway"
            },
        )
        .text(label)
        .into_any_element()
}

/// The name label at a card's bottom left.
fn name_label(text: String, screen: bool) -> AnyElement {
    div()
        .absolute()
        .left(px(8.0))
        .bottom(px(8.0))
        .max_w(px(1000.0))
        .px(px(8.0))
        .py(px(2.0))
        .rounded_full()
        .bg(black(0.55))
        .text_color(rgb(theme::OVERLAY_TEXT))
        .text_size(px(0.8 * theme::REM))
        .flex()
        .items_center()
        .gap(px(4.0))
        .truncate()
        // A share: its screen icon, then the name.
        .when(screen, |d| d.child(icon("screen-share", 13.0).text_color(rgb(theme::OVERLAY_TEXT))))
        .child(text.clone())
        .e2e("span", "video-name")
        .text(text)
        .into_any_element()
}

fn card_marks(media: morpheus_core::CallMedia) -> Option<AnyElement> {
    let items = marks(media, false);
    if items.is_empty() {
        return None;
    }
    let discs = items.into_iter().map(|mark| {
        div()
            .size(px(26.0))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(black(0.6))
            .child(icon(mark.glyph.unwrap_or("mic-off"), 14.0).text_color(rgb(theme::danger())))
            .e2e("span", mark.class)
            .attr("title", mark.title)
    });
    Some(
        div()
            .absolute()
            .right(px(8.0))
            .bottom(px(8.0))
            .flex()
            .gap(px(4.0))
            .children(discs)
            .e2e("span", "card-marks")
            .into_any_element(),
    )
}

/// Someone in the call without a camera on: their picture, large.
fn person_card(
    shell: &Shell,
    cx: &mut Context<Shell>,
    room: &OwnedRoomId,
    user: &OwnedUserId,
    speaking: bool,
    fit: Fit,
) -> AnyElement {
    let key = user.to_string();
    let name = shown(shell, user);
    let group = format!("card-{key}");
    let flags = media(shell, room, user);
    let click_key = key.clone();
    let size = card_avatar_size(match fit {
        Fit::Grid(w) => w,
        Fit::Main(w, h) => w.min(h * CARD_RATIO),
        Fit::Strip(h) => h * CARD_RATIO,
    });
    let mut classes = String::from("call-tile");
    if speaking {
        classes.push_str(" speaking");
    }
    // The average colour of their picture (darkened), else the usual one.
    let card_bg = picture_of(shell, user, 72)
        .and_then(|image| crate::media::card_color_of(&image))
        .unwrap_or_else(theme::bg_elevated);
    let mut tile = sized(
        div().id(gpui::SharedString::from(format!("card-{key}"))),
        fit,
        None,
    )
    .group(gpui::SharedString::from(group.clone()))
    .relative()
    .overflow_hidden()
    .flex()
    .items_center()
    .justify_center()
    .rounded(px(theme::RADIUS_LG))
    .bg(rgb(card_bg))
    .border_2()
    .border_color(if speaking {
        gpui::Hsla::from(rgb(theme::green()))
    } else {
        gpui::Hsla::from(gpui::transparent_black())
    })
    .cursor_pointer()
    .on_click(cx.listener(move |this, _, _, cx| this.toggle_focus(&click_key, cx)))
    .on_mouse_down(MouseButton::Right, {
        let (room, user, key) = (room.clone(), user.clone(), key.clone());
        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.open_call_card_menu(room.clone(), user.clone(), key.clone(), false, event.position, cx)
        })
    })
    // The ring is the card's; the picture inside has none of its own.
    .child(avatar(picture_of(shell, user, 72), &name, size, None))
    .child(focus_button(shell, cx, &key, &group))
    .child(name_label(name, false));
    if let Some(marks) = card_marks(flags) {
        tile = tile.child(marks);
    }
    tile.e2e("div", &classes)
        .attr("data-user", user.to_string())
        .attr(
            "data-voice-gain",
            format!("{:.3}", morpheus_call::user_track_gain(user.as_str(), morpheus_call::TrackSource::Microphone)),
        )
        .attr("data-bg", format!("{card_bg:06x}"))
        .into_any_element()
}

/// Someone's video: the latest picture, as it comes.
fn video_card(
    shell: &Shell,
    cx: &mut Context<Shell>,
    room: &OwnedRoomId,
    feed: &VideoFeed,
    speaking: bool,
    fit: Fit,
) -> AnyElement {
    let key = feed.key.clone();
    let name = shown(shell, &feed.user);
    let label = name;
    let group = format!("card-{key}");
    let flags = media(shell, room, &feed.user);
    let click_key = key.clone();
    let own = key == morpheus_call::LOCAL_CAMERA || key == morpheus_call::LOCAL_SCREEN;
    // Someone's share not received yet: a card with a Watch button, no picture.
    let unwatched = is_unwatched(feed);
    let mut classes = String::from(if unwatched { "share-card" } else { "video-tile" });
    if own {
        classes.push_str(" self");
    }
    if feed.screen && !unwatched {
        classes.push_str(" screen");
    }
    // A person's face rings green while they speak, as their avatar card
    // does; a shared screen is not a face.
    let speaking = speaking && !feed.screen;
    if speaking {
        classes.push_str(" speaking");
    }
    // The picture's own shape, once there is a picture.
    let (pw, ph) = shell
        .calls
        .frames
        .get(&key)
        .map_or((0, 0), |frame| frame.read(cx).dim);
    if feed.screen && pw == 0 && !unwatched {
        classes.push_str(" skeleton");
    }
    let ratio = if pw > 0 && ph > 0 {
        pw as f32 / ph as f32
    } else {
        CARD_RATIO
    };
    // A shared screen, focused, zooms and pans (as the picture viewer does).
    let waiting = feed.screen && pw == 0 && !unwatched;
    let zoomable = feed.screen && matches!(fit, Fit::Main(..));
    let frame = shell.calls.frames.get(&key).cloned();
    let view = frame.as_ref().map(|f| f.read(cx).view).unwrap_or_default();
    let mut tile = sized(
        div().id(gpui::SharedString::from(format!("card-{key}"))),
        fit,
        Some(ratio),
    )
    .group(gpui::SharedString::from(group.clone()))
    .relative()
    .overflow_hidden()
    .rounded(px(theme::RADIUS_LG))
    .bg(if waiting {
        rgb(theme::bg_elevated())
    } else if feed.screen {
        rgb(0x000000) // fixed: a shared screen sits on black
    } else {
        rgb(theme::bg_elevated())
    })
    .cursor_pointer();
    tile = tile.on_mouse_down(MouseButton::Right, {
        let (room, user, key) = (room.clone(), feed.user.clone(), key.clone());
        let screen = feed.screen;
        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
            cx.stop_propagation();
            this.open_call_card_menu(room.clone(), user.clone(), key.clone(), screen, event.position, cx)
        })
    });
    tile = match frame.clone() {
        Some(frame) if zoomable => {
            let (down, wheel, up, up_out, click) = (
                frame.clone(),
                frame.clone(),
                frame.clone(),
                frame.clone(),
                frame.clone(),
            );
            tile.when(view.zoomed(), |d| d.cursor_grab())
                .on_scroll_wheel(move |event, _, cx| {
                    wheel.update(cx, |f, cx| f.wheel(event, cx));
                    cx.stop_propagation();
                })
                .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                    down.update(cx, |f, cx| f.press(event, cx));
                })
                .on_mouse_up(MouseButton::Left, move |_, _, cx| {
                    up.update(cx, |f, cx| f.release(cx));
                })
                .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                    up_out.update(cx, |f, cx| f.release(cx));
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    let clicked = click.update(cx, |f, _| f.clicked());
                    if clicked {
                        this.toggle_focus(&click_key, cx);
                    }
                }))
        }
        // Someone's share not watched yet: a click anywhere on the card watches it.
        _ if unwatched => {
            let user = feed.user.clone();
            tile.on_click(cx.listener(move |this, _, _, cx| this.watch_share(&user, true, cx)))
        }
        _ => tile.on_click(cx.listener(move |this, _, _, cx| this.toggle_focus(&click_key, cx))),
    };
    if let Some(frame) = frame {
        tile = tile.child(frame);
    }
    if waiting {
        tile = tile.child(screen_skeleton());
    }
    if zoomable {
        tile = tile.child(zoom_control(shell, &key, view.zoom));
    }
    if unwatched {
        // One recent small picture of the share, dimmed, instead of black (the
        // video itself is not sent until it is watched).
        if let Some((image, bytes, mean)) = shell.calls.share_thumbs.get(&feed.user) {
            tile = tile.child(
                div()
                    .absolute()
                    .inset_0()
                    .child(
                        gpui::img(image.clone())
                            .size_full()
                            .object_fit(gpui::ObjectFit::Cover)
                            .opacity(0.45),
                    )
                    .e2e("div", "share-thumb")
                    .attr("data-bytes", bytes.to_string())
                    .attr("data-mean", format!("{},{},{}", mean.0, mean.1, mean.2)),
            );
        }
        tile = tile.child(watch_overlay(shell, cx, &feed.user));
    }
    if own && feed.screen && in_background() {
        tile = tile.child(
            div()
                .absolute()
                .inset_0()
                .bg(black(0.55))
                .flex()
                .items_center()
                .justify_center()
                .px(px(16.0))
                .text_color(rgb(theme::OVERLAY_TEXT))
                .text_align(gpui::TextAlign::Center)
                .child("Preview paused while Morpheus is in the background")
                .e2e("div", "preview-paused")
                .text("Preview paused while Morpheus is in the background"),
        );
    }
    if own && feed.screen {
        // How many people watch this share (this device's own preview does not
        // count): nothing is sent while it is nobody.
        let n = shell.calls.state.as_ref().map_or(0, |c| c.watchers);
        let text = if n == 0 {
            "Nobody is watching".to_owned()
        } else {
            format!("{n} watching")
        };
        tile = tile.child(
            div()
                .absolute()
                .top(px(8.0))
                .right(px(8.0))
                .px(px(8.0))
                .py(px(2.0))
                .rounded_full()
                .bg(black(0.55))
                .text_color(rgb(theme::OVERLAY_TEXT))
                .text_size(px(0.8 * theme::REM))
                .child(text.clone())
                .e2e("span", "watchers")
                .attr("data-count", n.to_string())
                .text(text),
        );
    }
    tile = tile
        .child(focus_button(shell, cx, &key, &group))
        .child(name_label(label, feed.screen));
    // Someone else's screen with sound: how it sounds here, and its volume.
    let heard = (feed.screen && !own)
        .then(|| shell.calls.screen_audio.get(&feed.user))
        .flatten()
        .map(|identity| morpheus_call::screen_audio(identity));
    let stream_muted = heard.is_some() && shell.calls.stream_muted.contains(&feed.user);
    if stream_muted {
        // A small muted speaker while the stream is muted for you.
        tile = tile.child(
            div()
                .absolute()
                .top(px(8.0))
                .left(px(8.0))
                .p(px(5.0))
                .rounded_full()
                .bg(black(0.6))
                .child(icon("volume-x", 14.0).text_color(rgb(theme::OVERLAY_TEXT)))
                .e2e("div", "stream-muted")
                .attr("title", "Stream muted"),
        );
    }
    let sound = heard.clone().flatten();
    if speaking {
        // Over the picture, following the card's corners.
        tile = tile.child(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .rounded(px(theme::RADIUS_LG))
                .border_2()
                .border_color(gpui::Hsla::from(rgb(theme::green()))),
        );
    }
    if !feed.screen
        && let Some(marks) = card_marks(flags)
    {
        tile = tile.child(marks);
    }
    tile.e2e("div", &classes)
        .attr("data-zoom", format!("{:.4}", view.zoom))
        .attr("data-x", format!("{:.1}", view.offset.0))
        .attr("data-y", format!("{:.1}", view.offset.1))
        .attr("data-video-size", format!("{pw}x{ph}"))
        .attr(
            "data-mirrored",
            if key == morpheus_call::LOCAL_CAMERA {
                "true"
            } else {
                "false"
            },
        )
        .attr(
            "data-camera",
            if own && !feed.screen {
                morpheus_call::camera_in_use().unwrap_or_default()
            } else {
                String::new()
            },
        )
        .attr("data-audio", if heard.is_some() { "on" } else { "" })
        .attr(
            "data-stream-gain",
            if heard.is_some() {
                format!("{:.3}", morpheus_call::user_track_gain(feed.user.as_str(), morpheus_call::TrackSource::ScreenShare))
            } else {
                String::new()
            },
        )
        .attr(
            "data-voice-gain",
            if feed.screen {
                String::new()
            } else {
                format!("{:.3}", morpheus_call::user_track_gain(feed.user.as_str(), morpheus_call::TrackSource::Microphone))
            },
        )
        .attr(
            "data-audio-level",
            sound
                .as_ref()
                .map(|s| format!("{:.4}", s.level))
                .unwrap_or_default(),
        )
        .attr(
            "data-audio-hz",
            sound
                .as_ref()
                .map(|s| format!("{:.0}/{:.0}", s.left_hz, s.right_hz))
                .unwrap_or_default(),
        )
        .attr(
            "data-audio-played",
            sound
                .as_ref()
                .map(|s| format!("{:.4}", s.played))
                .unwrap_or_default(),
        )
        .attr(
            "data-audio-sent",
            if feed.screen && own {
                sent_screen_audio()
            } else {
                String::new()
            },
        )
        .attr(
            "data-source",
            if feed.screen && own {
                morpheus_call::shared_screen_source()
                    .map(|s| s.key())
                    .unwrap_or_default()
            } else {
                String::new()
            },
        )
        .attr(
            "data-sent-bytes",
            if own && feed.screen {
                morpheus_call::sent_bytes("screen").to_string()
            } else {
                String::new()
            },
        )
        .attr("data-feed", key)
        .attr("data-user", feed.user.to_string())
        .into_any_element()
}

/// A share not received: its Watch button, or (asked for) that it is starting.
fn watch_overlay(shell: &Shell, cx: &mut Context<Shell>, user: &OwnedUserId) -> AnyElement {
    let asked = shell
        .calls
        .state
        .as_ref()
        .is_some_and(|c| c.watching.contains(user));
    let inner = if asked {
        div()
            .text_color(rgb(theme::OVERLAY_TEXT))
            .child("Starting the stream...")
            .e2e("span", "watch-starting")
            .text("Starting the stream...")
            .into_any_element()
    } else {
        let user = user.clone();
        div()
            .id(gpui::SharedString::from(format!("watch-{user}")))
            .px(px(16.0))
            .py(px(8.0))
            .rounded(px(theme::RADIUS_XL))
            .bg(rgb(theme::green()))
            .text_color(rgb(theme::on_green()))
            .cursor_pointer()
            .hover(|s| s.opacity(0.9))
            .child("Watch")
            .on_click(cx.listener(move |this, _, _, cx| {
                cx.stop_propagation();
                this.watch_share(&user, true, cx);
            }))
            .e2e("button", "watch-button")
            .text("Watch")
            .into_any_element()
    };
    div()
        .absolute()
        .inset_0()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .items_center()
        .justify_center()
        .child(icon("screen-share", 40.0).text_color(rgb(theme::OVERLAY_TEXT)))
        .child(inner)
        .e2e("div", "watch-overlay")
        .into_any_element()
}

/// How this device's own share's sound goes out ("48000 Hz stereo
/// bitrate=128000 dtx=false"), "none: why" if it could not, else nothing.
pub fn sent_screen_audio() -> String {
    match morpheus_call::sent_screen_audio() {
        Some(Ok(sent)) => sent,
        Some(Err(why)) => format!("none: {why}"),
        None => String::new(),
    }
}

/// The card shown while you are the only one in the call, a person's size:
/// the way to ask someone in (Discord's "Invite to voice channel").
fn invite_card(cx: &mut Context<Shell>, room: &OwnedRoomId, direct: bool, fit: Fit) -> AnyElement {
    let room = room.clone();
    let label = if direct {
        "Invite to call"
    } else {
        "Invite to voice channel"
    };
    // The text goes with the icon when the card has room for it (about 8.5 px a
    // letter, the icon, the gaps and the card's own edge), else only the icon,
    // the text kept as its tooltip.
    let card_w = match fit {
        Fit::Grid(w) | Fit::Main(w, _) => w,
        Fit::Strip(h) => h * CARD_RATIO,
    };
    let needed = label.chars().count() as f32 * 8.5 + 16.0 + 8.0 + 28.0 + 4.0 + 16.0;
    let button = if card_w >= needed {
        div()
            .id("call-invite-button")
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(14.0))
            .py(px(8.0))
            .rounded(px(theme::RADIUS))
            .bg(rgb(theme::green()))
            .text_color(rgb(theme::on_green()))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .cursor_pointer()
            .child(icon("user-plus", 16.0).text_color(rgb(theme::on_green())))
            .child(label)
            .on_click(cx.listener(move |this, _, _, cx| this.open_call_invite(room.clone(), cx)))
            .e2e("button", "primary invite-to-call")
            .attr("title", label)
            .text(label)
            .into_any_element()
    } else {
        control_button(
            Control {
                id: "call-invite-button",
                classes: "call-control invite-to-call".into(),
                title: label.into(),
                glyph: "user-plus",
                size: 20.0,
                enabled: true,
                pending: false,
            },
            cx.listener(move |this, _, _, cx| this.open_call_invite(room.clone(), cx)),
            None,
        )
    };
    sized(div().id("call-invite-card"), fit, None)
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_LG))
        .border_2()
        .border_color(rgba(theme::bg_selected()))
        .child(
            button,
        )
        .e2e("div", "call-invite")
        .into_any_element()
}

/// The cards: in a grid, each its own size (the stage fits them to it), or
/// one focused, large, with the rest in a strip under it.
fn cards(
    shell: &Shell,
    cx: &mut Context<Shell>,
    room: &OwnedRoomId,
    all: Vec<Card>,
    speaking: &[OwnedUserId],
    card_w: f32,
    stage: (f32, f32),
    invite: Option<(&OwnedRoomId, bool)>,
) -> (Vec<AnyElement>, bool) {
    let focused = focused_card(&shell.calls.focus, &all);
    let mut draw = |card: &Card, fit: Fit, cx: &mut Context<Shell>| match card {
        Card::Video(feed) => video_card(shell, cx, room, feed, speaking.contains(&feed.user), fit),
        Card::Person(user) => person_card(shell, cx, room, user, speaking.contains(user), fit),
    };
    let Some(focused) = focused else {
        let mut out: Vec<AnyElement> = all.iter().map(|c| draw(c, Fit::Grid(card_w), cx)).collect();
        if let Some((room, direct)) = invite {
            out.push(invite_card(cx, room, direct, Fit::Grid(card_w)));
        }
        return (out, false);
    };
    let (main, strip): (Vec<&Card>, Vec<&Card>) = all.iter().partition(|c| c.key() == focused);
    let all_in_order: Vec<&Card> = all.iter().collect();
    // The focused card at the top, the row of the others filling the height
    // it leaves (a little kept for rounding).
    let others = strip.len() + usize::from(invite.is_some());
    // The focused card's shape: its picture's, once there is one.
    let ratio = main
        .first()
        .and_then(|card| match card {
            Card::Video(feed) => shell.calls.frames.get(&feed.key),
            Card::Person(_) => None,
        })
        .map(|frame| frame.read(cx).dim)
        .filter(|(w, h)| *w > 0 && *h > 0)
        .map_or(CARD_RATIO, |(w, h)| w as f32 / h as f32);
    let has_others = others > 0;
    let hidden = has_others && shell.calls.strip_hidden;
    // The strip holds every member, the focused one too (greyed), so it never
    // reflows when the focus moves.
    let others = if hidden { 0 } else if has_others { others + 1 } else { 0 };
    let layout = focus_layout(
        others,
        (stage.0 - 1.0).max(40.0),
        (stage.1 - 1.0).max(40.0),
        ratio,
    );
    let main_room = (
        (stage.0 - 1.0).max(40.0),
        if others == 0 {
            (stage.1 - 1.0).max(40.0)
        } else {
            (stage.1 - 1.0 - layout.strip_h - 8.0).max(40.0)
        },
    );
    let strip_h = layout.strip_h;
    // The focused card and the strip (when shown) are one group, centred in the
    // stage: the strip right under the card with the usual gap.
    let (_, card_h) = fit_ratio(main_room, ratio);
    let group_h = card_h + if others > 0 { 8.0 + strip_h } else { 0.0 };
    let pad = ((stage.1 - 1.0 - group_h) / 2.0).max(0.0);
    let mut col = vec![
        div()
            .flex_none()
            .h(px(card_h))
            .flex()
            .justify_center()
            .items_center()
            .children(
                main.into_iter()
                    .map(|c| draw(c, Fit::Main(main_room.0, main_room.1), cx)),
            )
            .e2e("div", "call-focus-main")
            .into_any_element(),
    ];
    if others > 0 {
        col.push(div().flex_none().h(px(8.0)).into_any_element());
        col.push(
            div()
                .flex_none()
                .h(px(strip_h))
                .flex()
                .justify_center()
                .items_center()
                .gap(px(8.0))
                .children(all_in_order.iter().map(|c| {
                    if c.key() == focused {
                        // The person's avatar card, not the picture again (the one picture is
                        // the focused card's above).
                        let user = match c {
                            Card::Video(feed) => feed.user.clone(),
                            Card::Person(user) => user.clone(),
                        };
                        let card = match c {
                            // A share: its icon, not the person's avatar.
                            Card::Video(feed) if feed.screen => {
                                let name = shown(shell, &feed.user);
                                sized(div().id(gpui::SharedString::from(format!("strip-share-{}", feed.user))), Fit::Strip(strip_h), None)
                                    .relative()
                                    .rounded(px(theme::RADIUS_LG))
                                    .bg(rgb(theme::bg_elevated()))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("screen-share", 28.0).text_color(rgb(theme::text_muted())))
                                    .child(name_label(name, true))
                                    .e2e("div", "call-tile share-placeholder")
                                    .attr("data-user", feed.user.to_string())
                                    .into_any_element()
                            }
                            _ => person_card(shell, cx, room, &user, false, Fit::Strip(strip_h)),
                        };
                        let key = focused.to_owned();
                        // Greyed, with a focus icon; a click on it goes through to the card
                        // (which unfocuses).
                        div()
                            .relative()
                            .flex_none()
                            .child(card)
                            .child(
                                div()
                                    .id("strip-focused-mark")
                                    .absolute()
                                    .inset_0()
                                    .occlude()
                                    .cursor_pointer()
                                    .on_click(cx.listener(move |this, _, _, cx| this.toggle_focus(&key, cx)))
                                    .rounded(px(theme::RADIUS_LG))
                                    .bg(gpui::rgba(0x0000008c))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .child(icon("maximize-2", 20.0).text_color(rgb(theme::text())))
                                    .e2e("div", "strip-focused-mark"),
                            )
                            .e2e("div", "strip-focused")
                            .into_any_element()
                    } else {
                        draw(c, Fit::Strip(strip_h), cx)
                    }
                }))
                .children(invite.map(|(room, direct)| invite_card(cx, room, direct, Fit::Strip(strip_h))))
                .e2e("div", "call-strip")
                .into_any_element(),
        );
    }
    if has_others {
        let top = pad + if hidden { card_h - 26.0 } else { card_h - 6.0 };
        let (name, title) = if hidden {
            ("chevron-up", "Show members")
        } else {
            ("chevron-down", "Hide members")
        };
        col.push(
            div()
                .id("call-strip-toggle")
                .occlude()
                .absolute()
                .top(px(top))
                .left(px(stage.0 / 2.0 - 28.0))
                .w(px(56.0))
                .h(px(20.0))
                .flex()
                .items_center()
                .justify_center()
                .gap(px(4.0))
                .rounded(px(theme::RADIUS_XL))
                .bg(rgb(theme::bg_elevated()))
                .cursor_pointer()
                .hover(|s| s.opacity(0.85))
                .child(icon(name, 16.0).text_color(rgb(theme::text())))
                .child(icon("users", 14.0).text_color(rgb(theme::text_muted())))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.calls.strip_hidden = !this.calls.strip_hidden;
                    cx.notify();
                }))
                .tip(title)
                .e2e("button", if hidden { "call-strip-toggle hidden" } else { "call-strip-toggle" })
                .attr("title", title)
                .into_any_element(),
        );
    }
    let out = vec![div().relative().w_full().flex().flex_col().pt(px(pad)).children(col).into_any_element()];
    (out, true)
}

/// The stage: cards, then the call's controls along the bottom.
fn stage_body(
    shell: &Shell,
    cx: &mut Context<Shell>,
    call: &CallState,
    voice_page: bool,
    window: &Window,
) -> AnyElement {
    let me = shell.session.as_ref().and_then(|s| s.user_id());
    let participants = shell
        .rooms
        .iter()
        .find(|r| r.id == call.room_id)
        .map(|r| r.call_participants.clone())
        .unwrap_or_default();
    let mut all = tiles(call, &participants, me.clone());
    // A hidden camera: the person's avatar card instead of their video.
    let all_cards = std::mem::take(&mut all);
    all = all_cards
        .into_iter()
        .map(|card| match card {
            Card::Video(feed) if !feed.screen && shell.calls.hidden_video.contains(&feed.user) => {
                Card::Person(feed.user)
            }
            other => other,
        })
        .collect();
    // Your screen is being started (the picker, the connection): its tile is
    // here already, a skeleton, until the first picture.
    if call.choosing_screen
        && let Some(me) = me.clone()
        && !all.iter().any(|c| c.key() == morpheus_call::LOCAL_SCREEN)
    {
        all.insert(
            0,
            Card::Video(VideoFeed {
                user: me,
                key: morpheus_call::LOCAL_SCREEN.to_owned(),
                screen: true,
            }),
        );
    }
    let count = all.len();
    // Alone in the call: one more card, to invite someone with.
    let alone = me.as_ref().is_some_and(|me| {
        all.iter().all(|card| match card {
            Card::Video(feed) => &feed.user == me,
            Card::Person(user) => user == me,
        })
    });
    let direct = shell.rooms.iter().any(|r| r.id == call.room_id && r.is_dm);
    let invite = alone.then_some((&call.room_id, direct));
    // The stage's inner size as last painted: cards are as big as lets all fit.
    let measured = shell.calls.stage_measure.get();
    let card_w = card_width(count + usize::from(alone), measured.0, measured.1);
    let (children, focused) = cards(
        shell,
        cx,
        &call.room_id,
        all,
        &call.speaking,
        card_w,
        measured,
        invite,
    );
    let view = cx.entity();
    let measure = shell.calls.stage_measure.clone();
    let probe = canvas(
        move |bounds, _, cx| {
            let now = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let before = measure.get();
            if (now.0 - before.0).abs() > 0.5 || (now.1 - before.1).abs() > 0.5 {
                measure.set(now);
                let view = view.clone();
                cx.defer(move |cx| view.update(cx, |_, cx| cx.notify()));
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full();
    let height = shell
        .calls
        .stage_height
        .unwrap_or_else(|| default_stage_height(f32::from(window.viewport_size().height)));
    let mut stage = div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(8.0))
        .px(px(if voice_page { 16.0 } else { 16.0 }))
        .pt(px(if voice_page { 16.0 } else { 8.0 }))
        .pb(px(8.0))
        .bg(rgb(theme::bg_rail()))
        .min_h(px(200.0));
    stage = if voice_page {
        stage.flex_1().min_w_0().min_h_0()
    } else {
        stage
            .h(px(height))
            .border_b_1()
            .border_color(rgba(theme::bg_selected()))
    };
    let area = div()
        .relative()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_wrap()
        .justify_center()
        .content_center()
        .gap(px(8.0))
        .overflow_hidden()
        .when(focused, |d| d.flex_col().flex_nowrap().justify_start())
        .child(probe)
        .children(children)
        .e2e(
            "div",
            if focused {
                "call-cards focused"
            } else {
                "call-cards"
            },
        );
    stage
        .child(area)
        .child(stage_controls(shell, cx, call))
        .e2e(
            "div",
            if voice_page {
                "call-stage voice-page"
            } else {
                "call-stage"
            },
        )
        .attr("data-cards", count.to_string())
        // Video tracks being received (a hidden camera is not): for the tests.
        .attr(
            "data-receiving",
            morpheus_call::call_diag()
                .map_or(0, |d| d.tracks.iter().filter(|t| t.direction == "receiving").count())
                .to_string(),
        )
        .into_any_element()
}

/// Above the chat of the room this account is calling in: a card each, and
/// the call's controls along the bottom; its bottom edge is dragged to
/// resize it.
pub fn call_stage(
    shell: &Shell,
    cx: &mut Context<Shell>,
    room: &OwnedRoomId,
    window: &Window,
) -> Option<AnyElement> {
    let call = shell.calls.state.as_ref().filter(|c| &c.room_id == room)?;
    let stage = stage_body(shell, cx, call, false, window);
    let view = cx.entity();
    let start = shell
        .calls
        .stage_height
        .unwrap_or_else(|| default_stage_height(f32::from(window.viewport_size().height)));
    let handle = div()
        .flex_none()
        .h(px(6.0))
        .mt(px(-3.0))
        .w_full()
        .cursor_row_resize()
        .hover(|s| s.bg(rgb(theme::green())))
        .on_mouse_down(MouseButton::Left, move |event, _, cx| {
            let y = f32::from(event.position.y);
            view.update(cx, |this, cx| {
                this.calls.stage_drag = Some((y, start));
                cx.notify();
            });
        })
        .e2e("div", "call-stage-resize")
        .attr("title", "Drag to resize");
    Some(
        div()
            .flex_none()
            .flex()
            .flex_col()
            .child(stage)
            .child(handle)
            .into_any_element(),
    )
}

/// A server's voice channel opened: all call, as Discord has it (the cards
/// fill the page, the controls below, no handle to resize), or a way to join
/// when not in it.
pub fn voice_page(
    shell: &Shell,
    cx: &mut Context<Shell>,
    room: &RoomSummary,
    window: &Window,
) -> AnyElement {
    match shell.calls.state.as_ref().filter(|c| c.room_id == room.id) {
        Some(call) => stage_body(shell, cx, call, true, window),
        None => {
            let (id, name) = (room.id.clone(), room.name.clone());
            div()
                .flex_1()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(12.0))
                .bg(rgb(theme::bg_rail()))
                .text_color(rgb(theme::text_muted()))
                .child(icon("volume-2", 40.0))
                .child(
                    div()
                        .text_size(px(1.5 * theme::REM))
                        .font_weight(FontWeight::BOLD)
                        .text_color(rgb(theme::text()))
                        .child(room.name.clone())
                        .e2e("h2", "")
                        .text(room.name.clone()),
                )
                .child(
                    div()
                        .child("No one has to be here first: join, and others can drop in.")
                        .e2e("p", "muted")
                        .text("No one has to be here first: join, and others can drop in."),
                )
                .child(
                    div()
                        .id("join-voice")
                        .flex()
                        .items_center()
                        .gap(px(8.0))
                        .px(px(18.0))
                        .py(px(10.0))
                        .rounded(px(theme::RADIUS_SM))
                        .bg(rgb(theme::green()))
                        .text_color(rgb(theme::on_green()))
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .child(icon("phone", 18.0).text_color(rgb(theme::on_green())))
                        .child("Join voice")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.join_call(id.clone(), name.clone(), cx)
                        }))
                        .e2e("button", "primary join-voice")
                        .text("Join voice"),
                )
                .e2e("div", "voice-page-join")
                .into_any_element()
        }
    }
}

/// A voice channel's page: the call, with the chat button in a strip above
/// it, and the channel's text chat beside it, as a rounded panel, when opened.
pub fn voice_channel(
    shell: &Shell,
    cx: &mut Context<Shell>,
    room: &RoomSummary,
    chat: Option<AnyElement>,
    window: &Window,
) -> AnyElement {
    let open = shell.calls.voice_chat;
    let unread = room.unread_messages > 0 || room.highlight_count > 0 || room.marked_unread;
    let toggle = div()
        .id("voice-chat-toggle")
        .relative()
        .size(px(32.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
        .cursor_pointer()
        .hover(|s| s.bg(rgba(theme::bg_hover())))
        .child(icon("messages-square", 18.0).text_color(rgb(theme::text())))
        .when(unread && !open, |d| {
            d.child(
                div()
                    .absolute()
                    .top(px(-3.0))
                    .right(px(-3.0))
                    .size(px(12.0))
                    .rounded_full()
                    .bg(rgb(theme::danger()))
                    .border_2()
                    .border_color(rgb(theme::bg_rail()))
                    .e2e("span", "unread-dot"),
            )
        })
        .on_click(cx.listener(|this, _, window, cx| {
            this.calls.voice_chat = !this.calls.voice_chat;
            if this.calls.voice_chat
                && let Some((_, composer)) = &this.composer
            {
                composer.update(cx, |composer, cx| composer.focus_input(window, cx));
            }
            cx.notify();
        }))
        .e2e(
            "button",
            if open {
                "ghost voice-chat-toggle active"
            } else {
                "ghost voice-chat-toggle"
            },
        )
        .attr(
            "title",
            if open {
                "Hide the chat"
            } else {
                "Show the chat"
            },
        );
    // The call's column: the button's strip, then the page (min width 0, so
    // a narrow window shrinks the call instead of pushing the chat out).
    let call = div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .flex_col()
        .bg(rgb(theme::bg_rail()))
        .child(
            div()
                .flex_none()
                .h(px(48.0))
                .px(px(12.0))
                .flex()
                .items_center()
                .justify_end()
                .gap(px(12.0))
                // Open, the chat's own X closes it: no button here.
                .children((!open).then_some(toggle)),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .flex()
                .flex_col()
                .child(voice_page(shell, cx, room, window)),
        );
    let mut row = div()
        .flex_1()
        .min_w_0()
        .min_h_0()
        .flex()
        .relative()
        .bg(rgb(theme::bg_rail()))
        .child(call);
    if open && let Some(chat) = chat {
        // A card floating beside the call: 40% of the page, 240-380 wide.
        row = row.child(
            div()
                .flex_none()
                .w(gpui::relative(0.4))
                .min_w(px(248.0))
                .max_w(px(388.0))
                .flex()
                .py(px(8.0))
                .pr(px(8.0))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .min_h_0()
                        .flex()
                        .flex_col()
                        .rounded(px(12.0))
                        .overflow_hidden()
                        .border_1()
                        .border_color(rgba(theme::bg_selected()))
                        .bg(rgb(theme::bg_chat()))
                        .child(
                            div().flex_none().h(px(40.0)).px(px(8.0)).flex().items_center().justify_end().child(
                                div()
                                    .id("voice-chat-close")
                                    .size(px(28.0))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(theme::RADIUS_SM))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(rgba(theme::bg_hover())))
                                    .child(icon("x", 18.0).text_color(rgb(theme::text_muted())))
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.calls.voice_chat = false;
                                        cx.notify();
                                    }))
                                    .e2e("button", "ghost voice-chat-close")
                                    .attr("title", "Close the chat"),
                            ),
                        )
                        .child(chat)
                        .e2e("div", "voice-chat"),
                ),
        );
    }
    row.e2e("div", "voice-channel").into_any_element()
}

/// Someone calling: who, with answer and decline.
pub fn incoming_calls(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let ringing = shell.ringing(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or_default(),
    );
    if ringing.is_empty() {
        return None;
    }
    let cards = ringing.into_iter().map(|call| {
        let room = shell.rooms.iter().find(|r| r.id == call.room_id);
        let locked = crate::locks::is_locked(&call.room_id);
        let (name, picture) = match room {
            // A locked chat: not who, not which, only that it rings.
            _ if locked => (crate::locks::LOCKED_CALL.to_owned(), None),
            Some(room) => (room.name.clone(), shell.picture(&room.avatar, 48)),
            None => (call.caller.localpart().to_owned(), None),
        };
        let (accept, decline) = (call.clone(), call.clone());
        div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .px(px(14.0))
            .py(px(12.0))
            .rounded(px(theme::RADIUS_LG))
            .bg(rgb(theme::bg_elevated()))
            .text_color(rgb(theme::text()))
            .shadow_lg()
            .child(if locked {
                div()
                    .size(px(48.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(rgba(theme::bg_selected()))
                    .child(icon("lock", 24.0).text_color(rgb(theme::text_muted())))
                    .into_any_element()
            } else {
                avatar(picture, &name, 48.0, None).into_any_element()
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(name.clone())
                            .e2e("span", "incoming-call-name")
                            .text(name.clone()),
                    )
                    .child(
                        div()
                            .text_size(px(0.85 * theme::REM))
                            .text_color(rgb(theme::text_muted()))
                            .child("Incoming call…")
                            .e2e("span", "incoming-call-what")
                            .text("Incoming call…"),
                    ),
            )
            .child(
                div()
                    .id(gpui::SharedString::from(format!(
                        "decline-{}",
                        call.notification
                    )))
                    .size(px(40.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(rgb(theme::danger()))
                    .cursor_pointer()
                    .child(icon("phone-off", 20.0).text_color(rgb(theme::on_danger())))
                    .on_click(cx.listener(move |this, _, _, cx| this.decline_ring(&decline, cx)))
                    .e2e("button", "decline-call")
                    .attr("title", "Decline"),
            )
            .child(
                div()
                    .id(gpui::SharedString::from(format!(
                        "accept-{}",
                        call.notification
                    )))
                    .size(px(40.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(rgb(theme::green()))
                    .cursor_pointer()
                    .child(icon("phone", 20.0).text_color(rgb(theme::on_green())))
                    .on_click(cx.listener(move |this, _, _, cx| this.answer_call(&accept, cx)))
                    .e2e("button", "accept-call")
                    .attr("title", "Accept"),
            )
            .e2e("div", "incoming-call")
            .attr("data-room", call.room_id.to_string())
    });
    Some(
        div()
            .absolute()
            .top(px(16.0))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .w(px(360.0))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .children(cards),
            )
            .e2e("div", "incoming-calls")
            .into_any_element(),
    )
}

/// The header's call button: DMs and (on a desktop) voice channels.
pub fn header_call_button(
    shell: &Shell,
    cx: &mut Context<Shell>,
    room: &RoomSummary,
) -> AnyElement {
    let in_this = shell.calls.in_call(&room.id);
    let title =
        crate::view_model::calls::call_button_title(in_this, !room.call_participants.is_empty());
    let (id, name) = (room.id.clone(), room.name.clone());
    let colour = if in_this {
        theme::green()
    } else {
        crate::chat::bar_colour(shell, "call-button", theme::text_muted())
    };
    div()
        .id("call-button")
        .px(px(8.0))
        .h(px(28.0))
        .flex()
        .items_center()
        .rounded(px(theme::RADIUS_SM))
        .cursor_pointer()
        .when(in_this, |d| d.bg(rgba(theme::bg_selected())))
        .on_hover(crate::chat::bar_hover(cx, "call-button"))
        .child(icon(if in_this { "phone-off" } else { "phone" }, 16.0).text_color(rgb(colour)))
        .on_click(cx.listener(move |this, _, _, cx| {
            if this.calls.in_call(&id) {
                this.hang_up(cx);
            } else {
                this.join_call(id.clone(), name.clone(), cx);
            }
        }))
        .tip(title.to_owned())
        .e2e(
            "button",
            if in_this {
                "ghost call-button active"
            } else {
                "ghost call-button"
            },
        )
        .attr("title", title)
        .attr("data-color", format!("{colour:06x}"))
        .attr(
            "data-bg",
            if in_this {
                format!("{:08x}", theme::bg_selected())
            } else {
                String::new()
            },
        )
        .into_any_element()
}

impl Shell {
    /// Show the call's view with `user`'s shared screen focused, else their card.
    pub fn focus_person_in_call(
        &mut self,
        room: OwnedRoomId,
        user: &OwnedUserId,
        cx: &mut Context<Self>,
    ) {
        let key = self.calls.state.as_ref().and_then(|call| {
            let feed = call
                .video
                .iter()
                .find(|f| f.screen && &f.user == user)
                .or_else(|| call.video.iter().find(|f| &f.user == user));
            Some(feed.map_or_else(|| user.to_string(), |f| f.key.clone()))
        });
        let Some(key) = key else { return };
        if self.selected.as_ref() != Some(&room) {
            self.open_room_now(room, cx);
        }
        // The first click of the double click opened their profile card.
        self.card = None;
        self.calls.focus = Some(key);
        for frame in self.calls.frames.values() {
            frame.update(cx, |f, cx| f.reset_zoom(cx));
        }
        cx.notify();
    }

    /// Focus a card, or, focused already, go back to the grid.
    pub fn toggle_focus(&mut self, card: &str, cx: &mut Context<Self>) {
        self.calls.focus = toggled_focus(&self.calls.focus, card);
        // Another tile in front: a zoomed screen goes back to fit.
        for frame in self.calls.frames.values() {
            frame.update(cx, |f, cx| f.reset_zoom(cx));
        }
        cx.notify();
    }

    /// Escape in a call with a card focused: back to the grid.
    pub fn clear_call_focus(&mut self, cx: &mut Context<Self>) -> bool {
        if self.calls.focus.take().is_some() {
            cx.notify();
            return true;
        }
        false
    }

    /// One entity per video feed shown, so a new picture redraws only that
    /// card; the entities of feeds that ended go.
    pub fn sync_video_frames(&mut self, cx: &mut Context<Self>) {
        let wanted: Vec<String> = self
            .calls
            .state
            .as_ref()
            .map(|s| s.video.iter().map(|f| f.key.clone()).collect())
            .unwrap_or_default();
        self.calls.frames.retain(|key, _| wanted.contains(key));
        for feed in self.calls.state.iter().flat_map(|s| s.video.iter()) {
            if !self.calls.frames.contains_key(&feed.key) {
                let (key, screen) = (feed.key.clone(), feed.screen);
                let shell = cx.weak_entity();
                let frame = cx.new(|cx| VideoFrame::new(key.clone(), screen, shell, cx));
                self.calls.frames.insert(key, frame);
            }
        }
    }
}

/// One video feed's picture, drawn from the raw pixels of the latest frame:
/// it checks for a new frame 60 times a second and, when there is one, wraps
/// the pixels as an image (no encoding or decoding) and redraws itself only.
pub struct VideoFrame {
    key: String,
    screen: bool,
    seen: u64,
    /// The picture shown, and those replaced that wait to leave the atlas.
    pictures: crate::pictures::Pictures,
    /// What the pictures look like, for tests: pixel colours left and right.
    sample: (String, String),
    pub dim: (u32, u32),
    /// Told when the picture's shape changes (its card follows it).
    on_resize: Box<dyn Fn(&mut gpui::App)>,
    /// The settings preview, not a call's tile.
    preview: bool,
    /// Zoom and pan of the picture in its tile (a focused screen).
    pub view: ZoomPan,
    /// Where the tile was painted (window coordinates).
    area: Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>>,
    /// A pan going: where the pointer went down and the pan then.
    panning: Option<((f32, f32), (f32, f32))>,
    /// The press became a drag: its click is not a click.
    dragged: bool,
    born: std::time::Instant,
}

impl VideoFrame {
    pub fn new(
        key: String,
        screen: bool,
        shell: gpui::WeakEntity<Shell>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self::with(
            key,
            screen,
            false,
            Box::new(move |cx| {
                let _ = shell.update(cx, |_, cx| cx.notify());
            }),
            cx,
        )
    }

    /// A feed drawn for someone who wants to hear (`on_resize`) when its shape
    /// changes; `preview`: it is Settings' camera preview.
    pub fn with(
        key: String,
        screen: bool,
        preview: bool,
        on_resize: Box<dyn Fn(&mut gpui::App)>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let Ok(()) = this.update(cx, |this, cx| this.poll(cx)) else {
                    break;
                };
            }
        })
        .detach();
        Self {
            key,
            screen,
            seen: 0,
            pictures: Default::default(),
            sample: (String::new(), String::new()),
            dim: (0, 0),
            on_resize,
            preview,
            view: ZoomPan::default(),
            area: Rc::new(std::cell::Cell::new(gpui::Bounds::default())),
            panning: None,
            dragged: false,
            born: std::time::Instant::now(),
        }
    }

    fn tile(&self) -> ((f32, f32), (f32, f32)) {
        let b = self.area.get();
        (
            (f32::from(b.origin.x), f32::from(b.origin.y)),
            (f32::from(b.size.width), f32::from(b.size.height)),
        )
    }

    pub fn reset_zoom(&mut self, cx: &mut Context<Self>) {
        if self.view != ZoomPan::default() || self.panning.is_some() {
            self.view.reset();
            self.panning = None;
            cx.notify();
        }
    }

    pub fn zoom_halfway(&mut self, cx: &mut Context<Self>) {
        let (_, size) = self.tile();
        let center = (size.0 / 2.0, size.1 / 2.0);
        self.view.zoom_at(2.0, center, size, size);
        cx.notify();
    }

    pub fn wheel(&mut self, event: &gpui::ScrollWheelEvent, cx: &mut Context<Self>) {
        let Some(factor) = ZoomPan::wheel_factor(event.delta) else {
            return;
        };
        let (origin, size) = self.tile();
        let at = (
            f32::from(event.position.x) - origin.0,
            f32::from(event.position.y) - origin.1,
        );
        self.view.zoom_at(self.view.zoom * factor, at, size, size);
        cx.notify();
    }

    /// A press on the tile: starts a pan when it is zoomed.
    pub fn press(&mut self, event: &gpui::MouseDownEvent, _cx: &mut Context<Self>) {
        self.dragged = false;
        let at = (f32::from(event.position.x), f32::from(event.position.y));
        if self.view.zoomed() {
            self.panning = Some((at, self.view.offset));
        }
    }

    pub fn drag(&mut self, event: &gpui::MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((start, offset)) = self.panning else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.panning = None;
            return;
        }
        let (dx, dy) = (
            f32::from(event.position.x) - start.0,
            f32::from(event.position.y) - start.1,
        );
        if dx.abs() + dy.abs() > 3.0 {
            self.dragged = true;
        }
        let (_, size) = self.tile();
        self.view.offset = (offset.0 + dx, offset.1 + dy);
        self.view.clamp(size, size);
        cx.notify();
    }

    pub fn release(&mut self, _cx: &mut Context<Self>) {
        self.panning = None;
    }

    /// The tile was clicked: returns `false` if that was a drag's end;
    /// `true` if it was a plain click that should toggle focus.
    pub fn clicked(&mut self) -> bool {
        !std::mem::take(&mut self.dragged)
    }

    /// Your own camera (in a call, or the preview) is a mirror.
    fn mirrored(&self) -> bool {
        self.key == morpheus_call::LOCAL_CAMERA || self.key == morpheus_call::PREVIEW_CAMERA
    }

    /// Takes the latest frame, if it is a new one.
    fn poll(&mut self, cx: &mut Context<Self>) {
        // Test builds can hold the first picture back, to look at the skeleton.
        if self.seen == 0
            && std::env::var_os("MORPHEUS_E2E_PORT").is_some()
            && let Some(ms) = std::env::var("MORPHEUS_E2E_FRAME_DELAY_MS")
                .ok()
                .and_then(|v| v.parse::<u64>().ok())
            && self.born.elapsed() < Duration::from_millis(ms)
        {
            return;
        }
        // Your own share's preview freezes on its last picture while the window is
        // in the background (the first picture still comes).
        if self.key == morpheus_call::LOCAL_SCREEN && self.seen != 0 && in_background() {
            return;
        }
        let Some(frame) = morpheus_call::latest_frame(&self.key) else {
            return;
        };
        if frame.seq == self.seen {
            return;
        }
        self.seen = frame.seq;
        let (width, height) = (frame.width, frame.height);
        if self.dim != (width, height) {
            self.dim = (width, height);
            // The card is the picture's shape: the shell lays it out again.
            (self.on_resize)(cx);
        }
        // Your own camera is drawn mirrored, like a mirror (what is sent is not).
        let mut pixels = frame.bgra.to_vec();
        if self.mirrored() && width > 0 {
            for row in pixels.chunks_exact_mut(width as usize * 4) {
                let (mut left, mut right) = (0, width as usize - 1);
                while left < right {
                    for byte in 0..4 {
                        row.swap(left * 4 + byte, right * 4 + byte);
                    }
                    left += 1;
                    right -= 1;
                }
            }
        }
        let sample = |x: u32, y: u32| {
            let at = ((y * width + x) * 4) as usize;
            pixels
                .get(at..at + 3)
                .map(|p| format!("{:02x}{:02x}{:02x}", p[2], p[1], p[0]))
                .unwrap_or_default()
        };
        self.sample = (
            sample(width / 4, height / 2),
            sample(3 * width / 4, height / 2),
        );
        // The pixels are B, G, R, A: what GPUI's images are made of.
        let Some(buffer) = image::RgbaImage::from_raw(width, height, pixels) else {
            return;
        };
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        self.pictures.show(image);
        cx.notify();
    }
}

impl gpui::Render for VideoFrame {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let me = cx.entity();
        let panning = self.panning.is_some();
        self.pictures.drop_stale(window);
        let shown = self.pictures.image().is_some();
        let view = self.view;
        let area = self.area.clone();
        // Pinned to the card by its edges: a percentage height under a
        // flex-sized card is indefinite, and the image then took its own
        // shape from the width, taller than the card and cut off by it.
        let base = div().absolute().inset_0();
        match self.pictures.to_paint() {
            Some((image, painted)) => {
                base.child(
                    gpui::canvas(
                        |_, _, _| (),
                        move |bounds, (), window, _| {
                            area.set(bounds);
                            // A pan follows the pointer wherever it goes.
                            if panning {
                                let me = me.clone();
                                window.on_mouse_event(
                                    move |event: &gpui::MouseMoveEvent, phase, _, cx| {
                                        if phase == gpui::DispatchPhase::Capture {
                                            me.update(cx, |f, cx| f.drag(event, cx));
                                        }
                                    },
                                );
                            }
                            // The whole picture, in its own shape, centred; its
                            // corners are rounded here because the card's clip is
                            // a rectangle and a percentage size under a flex-sized
                            // card is not definite (the image then took its shape
                            // from the width and ran past the card's bottom).
                            let mut fitted = ObjectFit::Contain.get_bounds(bounds, image.size(0));
                            if view.zoom != 1.0 {
                                let size = fitted.size.map(|s| s * view.zoom);
                                let middle = fitted.center()
                                    + gpui::point(px(view.offset.0), px(view.offset.1));
                                fitted = gpui::Bounds::centered_at(middle, size);
                            }
                            let radius = px(theme::RADIUS_LG)
                                .min(fitted.size.width / 2.0)
                                .min(fitted.size.height / 2.0);
                            painted.set(true);
                            let _ = window.paint_image(
                                fitted,
                                gpui::Corners::all(radius),
                                image.clone(),
                                0,
                                false,
                            );
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
            }
            None => base,
        }
        .e2e(
            "img",
            if self.preview {
                "preview-video"
            } else {
                "video"
            },
        )
        .attr(
            "data-mirrored",
            if self.mirrored() { "true" } else { "false" },
        )
        .attr("data-shown", if shown { "true" } else { "false" })
        .attr("data-seq", self.seen.to_string())
        .attr("data-size", format!("{}x{}", self.dim.0, self.dim.1))
        .attr(
            "data-publish",
            morpheus_call::published_video(&self.key)
                .map(|p| p.to_string())
                .unwrap_or_default(),
        )
        .attr("data-left", self.sample.0.clone())
        .attr("data-right", self.sample.1.clone())
    }
}

/// Used by the shell: a type to name the entity of a feed.
pub type Frames = std::collections::HashMap<String, Entity<VideoFrame>>;

/// The measured stage size, shared with the canvas that measures it.
pub type StageMeasure = Rc<std::cell::Cell<(f32, f32)>>;

/// Widget tests: the shell drawn on gpui's test platform with a call fed in
/// by hand (no session, so nothing reaches LiveKit or the server).
#[cfg(all(test, feature = "e2e"))]
mod widget_tests {
    use super::*;
    use crate::e2e::{self, query};
    use crate::model::tests::room;
    use crate::view_model::calls::CallState;
    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point};

    struct Harness(Entity<Shell>);

    impl gpui::Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            e2e::frame(div().size_full().child(self.0.clone()))
        }
    }

    fn open(
        cx: &mut TestAppContext,
        rooms: Vec<RoomSummary>,
    ) -> (Entity<Shell>, &mut VisualTestContext) {
        morpheus_call::use_test_tone();
        let (harness, cx) = cx.add_window_view(|_, cx| Harness(cx.new(|_| Shell::bare(None))));
        let shell = harness.read_with(cx, |h, _| h.0.clone());
        shell.update(cx, |s, cx| {
            s.rooms = rooms;
            cx.notify();
        });
        redraw(cx);
        (shell, cx)
    }

    fn redraw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.draw(
            point(px(0.0), px(0.0)),
            gpui::size(px(1100.0), px(720.0)),
            |_, _| div(),
        );
        cx.run_until_parked();
    }

    fn click(cx: &mut VisualTestContext, selector: &str) {
        let (_, (x, y)) = query(selector)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no {selector}"));
        cx.simulate_click(point(px(x), px(y)), Modifiers::none());
        redraw(cx);
    }

    fn texts(selector: &str) -> Vec<String> {
        query(selector).into_iter().map(|(t, _)| t).collect()
    }

    fn user(name: &str) -> OwnedUserId {
        OwnedUserId::try_from(format!("@{name}:x")).expect("user id")
    }

    /// A voice channel with bob in its call, muted.
    fn channel() -> RoomSummary {
        let mut lounge = room("!lounge:x", "lounge");
        lounge.is_voice = true;
        lounge.call_participants = vec![user("bob")];
        lounge.call_media = vec![(
            user("bob"),
            morpheus_core::CallMedia {
                muted: true,
                ..Default::default()
            },
        )];
        lounge
    }

    #[gpui::test]
    fn the_people_in_a_call_are_listed_under_their_room_with_their_marks(cx: &mut TestAppContext) {
        let (_shell, _cx) = open(cx, vec![room("!g:x", "general"), channel()]);
        assert_eq!(query(".room.voice .voice-icon").len(), 1);
        assert_eq!(query(".room.voice .hash").len(), 0);
        assert_eq!(
            query(".room-call[data-room=\"!lounge:x\"] .call-participant").len(),
            1
        );
        assert_eq!(
            query(".call-participant[data-user=\"@bob:x\"] .voice-state.muted").len(),
            1
        );
        // A room with nobody in its call lists no one.
        assert_eq!(query(".room-call[data-room=\"!g:x\"]").len(), 0);
    }

    #[gpui::test]
    fn the_voice_panel_follows_the_call(cx: &mut TestAppContext) {
        let (shell, cx) = open(cx, vec![room("!g:x", "general"), channel()]);
        assert!(query(".voice-panel").is_empty());
        shell.update(cx, |s, cx| {
            s.calls.state = Some(CallState::joining(
                OwnedRoomId::try_from("!lounge:x").expect("room id"),
                "lounge".into(),
            ));
            cx.notify();
        });
        redraw(cx);
        assert_eq!(texts(".voice-panel .voice-state"), ["Connecting…"]);
        assert_eq!(query(".room.voice.in-call").len(), 1);
        shell.update(cx, |s, cx| {
            if let Some(state) = s.calls.state.as_mut() {
                state.connected = true;
            }
            cx.notify();
        });
        redraw(cx);
        assert_eq!(
            texts(".voice-panel .voice-state.connected"),
            ["Voice connected"]
        );
        assert_eq!(texts(".voice-panel .voice-room"), ["lounge"]);
        // Hanging up ends it.
        click(cx, ".voice-panel .hang-up");
        assert!(query(".voice-panel").is_empty());
        shell.read_with(cx, |s, _| assert_eq!(s.calls.sounds, ["leave"]));
    }

    #[gpui::test]
    fn mute_marks_the_pair_and_notes_its_chime(cx: &mut TestAppContext) {
        let (shell, cx) = open(cx, vec![room("!g:x", "general")]);
        assert!(query(".user-pill .device-pair.active").is_empty());
        click(cx, ".user-pill .mute-button");
        assert_eq!(
            query(".user-pill .device-pair.active .mute-button.active").len(),
            1
        );
        shell.read_with(cx, |s, _| assert_eq!(s.calls.sounds, ["mute"]));
        // Deafening marks the other pair; unmuting undeafens too.
        click(cx, ".user-pill .deafen-button");
        assert_eq!(query(".user-pill .device-pair.active").len(), 2);
        click(cx, ".user-pill .mute-button");
        assert!(query(".user-pill .device-pair.active").is_empty());
        shell.read_with(cx, |s, _| {
            assert_eq!(s.calls.sounds, ["mute", "deafen", "undeafen"])
        });
    }

    #[gpui::test]
    fn the_stage_shows_a_card_each_and_a_click_focuses_one(cx: &mut TestAppContext) {
        let mut dm = room("!dm:x", "bob");
        dm.is_dm = true;
        dm.dm_user = Some(user("bob"));
        dm.call_participants = vec![user("bob"), user("carol")];
        let (shell, cx) = open(cx, vec![dm.clone()]);
        shell.update(cx, |s, cx| {
            s.selected = Some(dm.id.clone());
            let mut call = CallState::joining(dm.id.clone(), "bob".into());
            call.connected = true;
            s.calls.state = Some(call);
            cx.notify();
        });
        redraw(cx);
        assert_eq!(query(".call-stage[data-cards=\"2\"]").len(), 1);
        assert_eq!(query(".call-stage .call-tile").len(), 2);
        for control in [".mute-button", ".deafen-button", ".hang-up"] {
            assert_eq!(
                query(&format!(".call-stage .call-controls {control}")).len(),
                1,
                "{control}"
            );
        }
        assert!(query(".call-cards.focused").is_empty());
        click(cx, ".call-stage .call-tile[data-user=\"@carol:x\"]");
        assert_eq!(query(".call-cards.focused").len(), 1);
        assert_eq!(
            query(".call-focus-main > .call-tile[data-user=\"@carol:x\"]").len(),
            1
        );
        assert_eq!(
            query(".call-strip > .call-tile[data-user=\"@bob:x\"]").len(),
            1
        );
        // Clicking it again goes back to the grid.
        click(cx, ".call-focus-main > .call-tile");
        assert!(query(".call-cards.focused").is_empty());
    }
}

impl Shell {
    /// Right-click on someone's card in the call: their user menu with the
    /// call's items (volume, mute for you, focus) above it.
    pub fn open_call_card_menu(
        &mut self,
        room: OwnedRoomId,
        user: OwnedUserId,
        key: String,
        screen: bool,
        at: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let own = self.session.as_ref().and_then(|s| s.user_id()).as_ref() == Some(&user);
        let (volume, muted) = if screen {
            (
                self.calls.screen_volumes.get(&user).copied().unwrap_or(1.0),
                self.calls.stream_muted.contains(&user),
            )
        } else {
            (
                self.calls.user_volumes.get(&user).copied().unwrap_or(1.0),
                self.calls.muted_for_me.contains(&user),
            )
        };
        let watching = self
            .calls
            .state
            .as_ref()
            .is_some_and(|c| c.watching.contains(&user));
        let hidden = self.calls.hidden_video.contains(&user);
        let extra =
            crate::view_model::calls::call_card_items(&user, own, screen, volume, muted, hidden, watching);
        let name = shown(self, &user);
        self.open_user_menu_extra(room, user, name, extra, at, cx);
    }
}

/// The window is not the one being used (another has the focus): your own
/// preview of your share stops being redrawn, to save the decode and render
/// work; what is sent is not affected.
static BACKGROUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// A test can say the window is in the background (1) or the foreground (2)
/// whatever the system says; 0 leaves it to the system (a test window is in the
/// foreground then: headless windows never have the focus).
pub static FORCE_BACKGROUND: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Note whether the window has the focus; whether that changed.
pub fn set_background(background: bool) -> bool {
    BACKGROUND.swap(background, std::sync::atomic::Ordering::Relaxed) != background
}

pub fn in_background() -> bool {
    BACKGROUND.load(std::sync::atomic::Ordering::Relaxed)
}
