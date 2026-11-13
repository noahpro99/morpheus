//! Calls, what they look like (`crates/ui/src/components/call.rs`, its
//! second half): the people listed under a room's row, the voice panel in
//! the profile card, the call view above a room's chat (cards, controls, the
//! handle that resizes it), a voice channel's page, the rings. Video frames
//! are drawn natively by [`VideoFrame`] from the call crate's raw pixels.

use std::{rc::Rc, sync::Arc, time::Duration};

use gpui::{
    AnyElement, App, Context, Entity, FontWeight, IntoElement, MouseButton, ObjectFit,
    RenderImage, Styled as _, StyledImage as _, Window, canvas, div, img, prelude::*, px, rgb, rgba,
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
            CARD_RATIO, Card, CallState, VideoFeed, card_width, default_stage_height,
            focused_card, listed, marks, media_of, tiles, toggled_focus,
        },
        nicknames::shown_name,
    },
};

/// A person's shown name: your nickname for them, else their profile's.
fn shown(shell: &Shell, user: &OwnedUserId) -> String {
    shown_name(&shell.nicknames, user.as_str(), &shell.calls.name_of(user))
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
                .child(icon(glyph, size).text_color(rgb(if mark.class.starts_with("voice-state") {
                    theme::danger()
                } else {
                    theme::text_muted()
                })))
                .e2e("span", mark.class)
                .attr("title", mark.title)
                .into_any_element(),
            None => div()
                .flex_none()
                .px(px(5.0))
                .py(px(1.0))
                .rounded(px(4.0))
                .bg(rgb(theme::danger()))
                .text_color(rgb(0xffffff))
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
    let mut participants = room.call_participants.clone();
    if !in_this && shell.calls.just_left(&room.id) {
        participants.retain(|user| Some(user) != me.as_ref());
    }
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
                .text_color(rgb(if speaks { theme::text() } else { theme::text_muted() }))
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
            row.e2e("div", &classes).attr("data-user", user.to_string())
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

/// A round button of the call (mute, deafen, camera, screen, leave).
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
        (rgb(theme::danger()), rgb(0xffffff))
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
        .rounded_full()
        .bg(bg)
        .when(!enabled, |d| d.opacity(0.4))
        .when(pending, |d| d.opacity(0.7))
        .cursor_pointer()
        .hover(|s| s.opacity(0.9))
        .child(icon(glyph, size).text_color(fg))
        .on_click(on_click);
    if let Some(on_right) = on_right {
        button = button.on_mouse_down(MouseButton::Right, on_right);
    }
    let tagged = button
        .e2e("button", &classes)
        .attr("title", title)
        .attr("data-bg", if hang || (mute && active) { "#ff6b6b" } else { "" });
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
    let mut row = div()
        .flex_none()
        .flex()
        .justify_center()
        .gap(px(12.0));
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
        None,
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
        None,
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
    row.e2e("div", "call-controls stage-controls").into_any_element()
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

/// The camera button's right-click: the cameras, to pick from.
fn camera_right_click(cx: &mut Context<Shell>) -> Box<dyn Fn(&gpui::MouseDownEvent, &mut Window, &mut App)> {
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
            rgb(0xffffff)
        } else if active {
            rgb(theme::accent())
        } else {
            rgb(theme::text_muted())
        }))
        .on_click(on_click);
    if let Some(on_right) = on_right {
        button = button.on_mouse_down(MouseButton::Right, on_right);
    }
    let tagged = button
        .e2e("button", &classes)
        .attr("title", title)
        .attr("data-bg", if hang { "#ff6b6b" } else { "" });
    if enabled {
        tagged.into_any_element()
    } else {
        tagged.attr("disabled", "").into_any_element()
    }
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
    /// One of the strip under the focused card.
    Strip,
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
        Fit::Strip => (144.0, 81.0),
    };
    match (fit, ratio) {
        (Fit::Main(..), None) => base.flex_1().min_w_0().min_h_0(),
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
        .group_hover(gpui::SharedString::from(group.to_owned()), |s| s.opacity(1.0))
        .cursor_pointer()
        .child(
            icon(if focused { "minimize-2" } else { "maximize-2" }, 16.0)
                .text_color(rgb(0xffffff)),
        )
        .on_click(cx.listener(move |this, _, _, cx| {
            cx.stop_propagation();
            this.toggle_focus(&card, cx);
        }))
        .e2e("button", "card-enlarge")
        .attr("title", if focused { "Shrink" } else { "Enlarge" })
        .into_any_element()
}

/// The name label at a card's bottom left.
fn name_label(text: String) -> AnyElement {
    div()
        .absolute()
        .left(px(8.0))
        .bottom(px(8.0))
        .max_w(px(1000.0))
        .px(px(8.0))
        .py(px(2.0))
        .rounded_full()
        .bg(black(0.55))
        .text_color(rgb(0xffffff))
        .text_size(px(0.8 * theme::REM))
        .truncate()
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
    let strip = matches!(fit, Fit::Strip);
    let size = if strip { 40.0 } else { 72.0 };
    let mut classes = String::from("call-tile");
    if speaking {
        classes.push_str(" speaking");
    }
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
    .bg(rgb(theme::bg_elevated()))
    .border_2()
    .border_color(if speaking {
        gpui::Hsla::from(rgb(theme::green()))
    } else {
        gpui::Hsla::from(gpui::transparent_black())
    })
    .cursor_pointer()
    .on_click(cx.listener(move |this, _, _, cx| this.toggle_focus(&click_key, cx)))
    .child(
        div()
            .rounded_full()
            .border_3()
            .border_color(if speaking {
                gpui::Hsla::from(rgb(theme::green()))
            } else {
                gpui::Hsla::from(gpui::transparent_black())
            })
            .child(avatar(
                picture_of(shell, user, 72),
                &name,
                size,
                None,
            )),
    )
    .child(focus_button(shell, cx, &key, &group))
    .child(name_label(name));
    if let Some(marks) = card_marks(flags) {
        tile = tile.child(marks);
    }
    tile.e2e("div", &classes)
        .attr("data-user", user.to_string())
        .into_any_element()
}

/// Someone's video: the latest picture, as it comes.
fn video_card(
    shell: &Shell,
    cx: &mut Context<Shell>,
    room: &OwnedRoomId,
    feed: &VideoFeed,
    fit: Fit,
) -> AnyElement {
    let key = feed.key.clone();
    let name = shown(shell, &feed.user);
    let label = if feed.screen {
        format!("{name}'s screen")
    } else {
        name
    };
    let group = format!("card-{key}");
    let flags = media(shell, room, &feed.user);
    let click_key = key.clone();
    let own = key == morpheus_call::LOCAL_CAMERA || key == morpheus_call::LOCAL_SCREEN;
    let mut classes = String::from("video-tile");
    if own {
        classes.push_str(" self");
    }
    if feed.screen {
        classes.push_str(" screen");
    }
    // The picture's own shape, once there is a picture.
    let (pw, ph) = shell
        .calls
        .frames
        .get(&key)
        .map_or((0, 0), |frame| frame.read(cx).dim);
    let ratio = if pw > 0 && ph > 0 {
        pw as f32 / ph as f32
    } else {
        CARD_RATIO
    };
    let mut tile = sized(
        div().id(gpui::SharedString::from(format!("card-{key}"))),
        fit,
        Some(ratio),
    )
    .group(gpui::SharedString::from(group.clone()))
    .relative()
    .overflow_hidden()
    .rounded(px(theme::RADIUS_LG))
    .bg(if feed.screen {
        rgb(0x000000)
    } else {
        rgb(theme::bg_elevated())
    })
    .cursor_pointer()
    .on_click(cx.listener(move |this, _, _, cx| this.toggle_focus(&click_key, cx)));
    if let Some(frame) = shell.calls.frames.get(&key) {
        tile = tile.child(frame.clone());
    }
    tile = tile
        .child(focus_button(shell, cx, &key, &group))
        .child(name_label(label));
    if !feed.screen
        && let Some(marks) = card_marks(flags)
    {
        tile = tile.child(marks);
    }
    tile.e2e("div", &classes)
        .attr("data-video-size", format!("{pw}x{ph}"))
        .attr("data-feed", key)
        .attr("data-user", feed.user.to_string())
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
) -> (Vec<AnyElement>, bool) {
    let focused = focused_card(&shell.calls.focus, &all);
    let mut draw = |card: &Card, fit: Fit, cx: &mut Context<Shell>| match card {
        Card::Video(feed) => video_card(shell, cx, room, feed, fit),
        Card::Person(user) => person_card(shell, cx, room, user, speaking.contains(user), fit),
    };
    let Some(focused) = focused else {
        return (
            all.iter().map(|c| draw(c, Fit::Grid(card_w), cx)).collect(),
            false,
        );
    };
    let (main, strip): (Vec<&Card>, Vec<&Card>) = all.iter().partition(|c| c.key() == focused);
    // The focused card's room: the stage less the strip under it (81 high,
    // 8 apart) and a little for rounding.
    let main_room = (
        (stage.0 - 1.0).max(40.0),
        (stage.1 - if strip.is_empty() { 0.0 } else { 89.0 } - 1.0).max(40.0),
    );
    let mut out = vec![
        div()
            .flex_1()
            .min_h_0()
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
    if !strip.is_empty() {
        out.push(
            div()
                .flex_none()
                .flex()
                .justify_center()
                .gap(px(8.0))
                .children(strip.into_iter().map(|c| draw(c, Fit::Strip, cx)))
                .e2e("div", "call-strip")
                .into_any_element(),
        );
    }
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
    let all = tiles(call, &participants, me);
    let count = all.len();
    // The stage's inner size as last painted: cards are as big as lets all fit.
    let measured = shell.calls.stage_measure.get();
    let card_w = card_width(count, measured.0, measured.1);
    let (children, focused) = cards(shell, cx, &call.room_id, all, &call.speaking, card_w, measured);
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
        stage.flex_1().min_h_0()
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
        .e2e("div", if focused { "call-cards focused" } else { "call-cards" });
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
        .hover(|s| s.bg(rgb(theme::accent())))
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
                        .text_color(rgb(0xffffff))
                        .font_weight(FontWeight::SEMIBOLD)
                        .cursor_pointer()
                        .child(icon("phone", 18.0).text_color(rgb(0xffffff)))
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

/// A voice channel's page: the call, the chat button in its corner, and the
/// channel's text chat beside it when opened.
pub fn voice_channel(
    shell: &Shell,
    cx: &mut Context<Shell>,
    room: &RoomSummary,
    chat: Option<AnyElement>,
    window: &Window,
) -> AnyElement {
    let open = shell.calls.voice_chat;
    let toggle = div()
        .id("voice-chat-toggle")
        .absolute()
        .top(px(12.0))
        .right(px(12.0))
        .size(px(36.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
        .bg(if open {
            gpui::Hsla::from(rgba(theme::bg_selected()))
        } else {
            black(0.35)
        })
        .cursor_pointer()
        .child(icon("messages-square", 18.0).text_color(rgb(theme::text())))
        .on_click(cx.listener(|this, _, _, cx| {
            this.calls.voice_chat = !this.calls.voice_chat;
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
        .attr("title", if open { "Hide the chat" } else { "Show the chat" });
    let mut row = div()
        .flex_1()
        .min_h_0()
        .flex()
        .relative()
        .child(voice_page(shell, cx, room, window))
        .child(toggle);
    if open && let Some(chat) = chat {
        row = row.child(
            div()
                .w(px(380.0))
                .flex_none()
                .min_h_0()
                .flex()
                .flex_col()
                .relative()
                .border_l_1()
                .border_color(rgba(theme::bg_selected()))
                .bg(rgb(theme::bg_chat()))
                .child(chat)
                .e2e("div", "voice-chat"),
        );
    }
    row.e2e("div", "voice-channel").into_any_element()
}

/// Someone calling: who, with answer and decline.
pub fn incoming_calls(shell: &Shell, cx: &mut Context<Shell>) -> Option<AnyElement> {
    let ringing = shell.ringing(std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or_default());
    if ringing.is_empty() {
        return None;
    }
    let cards = ringing.into_iter().map(|call| {
        let room = shell.rooms.iter().find(|r| r.id == call.room_id);
        let (name, picture) = match room {
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
            .child(avatar(picture, &name, 48.0, None))
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
                    .id(gpui::SharedString::from(format!("decline-{}", call.notification)))
                    .size(px(40.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(rgb(theme::danger()))
                    .cursor_pointer()
                    .child(icon("phone-off", 20.0).text_color(rgb(0xffffff)))
                    .on_click(cx.listener(move |this, _, _, cx| this.decline_ring(&decline, cx)))
                    .e2e("button", "decline-call")
                    .attr("title", "Decline"),
            )
            .child(
                div()
                    .id(gpui::SharedString::from(format!("accept-{}", call.notification)))
                    .size(px(40.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .bg(rgb(theme::green()))
                    .cursor_pointer()
                    .child(icon("phone", 20.0).text_color(rgb(0xffffff)))
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
pub fn header_call_button(shell: &Shell, cx: &mut Context<Shell>, room: &RoomSummary) -> AnyElement {
    let in_this = shell.calls.in_call(&room.id);
    let title = crate::view_model::calls::call_button_title(
        in_this,
        !room.call_participants.is_empty(),
    );
    let (id, name) = (room.id.clone(), room.name.clone());
    div()
        .id("call-button")
        .px(px(8.0))
        .h_full().flex().items_center()
        .rounded(px(theme::RADIUS_SM))
        .cursor_pointer()
        .when(in_this, |d| d.bg(rgba(theme::bg_selected())))
        .hover(|s| s.bg(rgba(theme::bg_hover())))
        .child(icon(if in_this { "phone-off" } else { "phone" }, 16.0).text_color(rgb(
            if in_this { theme::green() } else { theme::text_muted() },
        )))
        .on_click(cx.listener(move |this, _, _, cx| {
            if this.calls.in_call(&id) {
                this.hang_up(cx);
            } else {
                this.join_call(id.clone(), name.clone(), cx);
            }
        }))
        .e2e(
            "button",
            if in_this {
                "ghost call-button active"
            } else {
                "ghost call-button"
            },
        )
        .attr("title", title)
        .into_any_element()
}

impl Shell {
    /// Focus a card, or, focused already, go back to the grid.
    pub fn toggle_focus(&mut self, card: &str, cx: &mut Context<Self>) {
        self.calls.focus = toggled_focus(&self.calls.focus, card);
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
    image: Option<Arc<RenderImage>>,
    /// Pictures replaced, to take out of the atlas on the next frame.
    stale: Vec<Arc<RenderImage>>,
    /// What the pictures look like, for tests: pixel colours left and right.
    sample: (String, String),
    pub dim: (u32, u32),
    /// The shell, told when the picture's shape changes (its card follows it).
    shell: gpui::WeakEntity<Shell>,
}

impl VideoFrame {
    pub fn new(
        key: String,
        screen: bool,
        shell: gpui::WeakEntity<Shell>,
        cx: &mut Context<Self>,
    ) -> Self {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(Duration::from_millis(16)).await;
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
            image: None,
            stale: Vec::new(),
            sample: (String::new(), String::new()),
            dim: (0, 0),
            shell,
        }
    }

    /// Takes the latest frame, if it is a new one.
    fn poll(&mut self, cx: &mut Context<Self>) {
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
            let _ = self.shell.update(cx, |_, cx| cx.notify());
        }
        let sample = |x: u32, y: u32| {
            let at = ((y * width + x) * 4) as usize;
            frame
                .bgra
                .get(at..at + 3)
                .map(|p| format!("{:02x}{:02x}{:02x}", p[2], p[1], p[0]))
                .unwrap_or_default()
        };
        self.sample = (sample(width / 4, height / 2), sample(3 * width / 4, height / 2));
        // The pixels are B, G, R, A: what GPUI's images are made of.
        let Some(buffer) = image::RgbaImage::from_raw(width, height, frame.bgra.to_vec()) else {
            return;
        };
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        if let Some(old) = self.image.replace(image) {
            self.stale.push(old);
        }
        cx.notify();
    }
}

impl gpui::Render for VideoFrame {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        for old in self.stale.drain(..) {
            let _ = window.drop_image(old);
        }
        let shown = self.image.is_some();
        // Pinned to the card by its edges: a percentage height under a
        // flex-sized card is indefinite, and the image then took its own
        // shape from the width, taller than the card and cut off by it.
        let base = div().absolute().inset_0();
        match &self.image {
            Some(image) => {
                let image = image.clone();
                base.child(
                    gpui::canvas(
                        |_, _, _| (),
                        move |bounds, (), window, _| {
                            // The whole picture, in its own shape, centred; its
                            // corners are rounded here because the card's clip is
                            // a rectangle and a percentage size under a flex-sized
                            // card is not definite (the image then took its shape
                            // from the width and ran past the card's bottom).
                            let fitted = ObjectFit::Contain.get_bounds(bounds, image.size(0));
                            let radius = px(theme::RADIUS_LG)
                                .min(fitted.size.width / 2.0)
                                .min(fitted.size.height / 2.0);
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
        .e2e("img", "video")
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
        assert_eq!(query(".room-call[data-room=\"!lounge:x\"] .call-participant").len(), 1);
        assert_eq!(query(".call-participant[data-user=\"@bob:x\"] .voice-state.muted").len(), 1);
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
        assert_eq!(texts(".voice-panel .voice-state.connected"), ["Voice connected"]);
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
        assert_eq!(query(".user-pill .device-pair.active .mute-button.active").len(), 1);
        shell.read_with(cx, |s, _| assert_eq!(s.calls.sounds, ["mute"]));
        // Deafening marks the other pair; unmuting undeafens too.
        click(cx, ".user-pill .deafen-button");
        assert_eq!(query(".user-pill .device-pair.active").len(), 2);
        click(cx, ".user-pill .mute-button");
        assert!(query(".user-pill .device-pair.active").is_empty());
        shell.read_with(cx, |s, _| assert_eq!(s.calls.sounds, ["mute", "deafen", "undeafen"]));
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
            assert_eq!(query(&format!(".call-stage .call-controls {control}")).len(), 1, "{control}");
        }
        assert!(query(".call-cards.focused").is_empty());
        click(cx, ".call-stage .call-tile[data-user=\"@carol:x\"]");
        assert_eq!(query(".call-cards.focused").len(), 1);
        assert_eq!(query(".call-focus-main > .call-tile[data-user=\"@carol:x\"]").len(), 1);
        assert_eq!(query(".call-strip > .call-tile[data-user=\"@bob:x\"]").len(), 1);
        // Clicking it again goes back to the grid.
        click(cx, ".call-focus-main > .call-tile");
        assert!(query(".call-cards.focused").is_empty());
    }
}
