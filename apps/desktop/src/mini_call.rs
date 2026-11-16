//! The floating mini call window (issue #86): over the app, inside its
//! window, while the call view is out of sight and someone in your call has
//! a camera or a shared screen on. It shows one feed (which one is
//! `view_model::mini_call::MiniPick`'s choice), is dragged by its body (and
//! snaps to the left or right edge when let go), resized from its inner
//! bottom corner, and goes back to the call view from its back button. On
//! hover: mute, deafen, hang up. Its place is kept on this device.
//!
//! The picture is the call view's own [`crate::call_ui::VideoFrame`] entity
//! for the feed (the call view is not drawn meanwhile), so a frame is turned
//! into a GPU image once, whichever of the two shows it.

use std::{sync::OnceLock, time::Instant};

use gpui::{
    AnyElement, Context, FontWeight, IntoElement, MouseButton, Window, canvas, div, prelude::*,
    px, rgb,
};

use crate::{
    call_ui::shown,
    e2e::E2eExt as _,
    icons::icon,
    shell::Shell,
    theme,
    view_model::mini_call::{MiniPlace, Side},
};

/// The app's title bar, over which the mini window never goes.
const TITLEBAR_H: f32 = 32.0;

/// A press on the mini window going on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MiniDrag {
    /// Moving it: the pointer's offset from its top left corner, where its
    /// top left is now (in the area under the title bar), and whether the
    /// pointer has gone far enough to count as a move.
    Move {
        grab: (f32, f32),
        at: (f32, f32),
        moved: bool,
        start: (f32, f32),
    },
    /// Resizing it from its corner: where the pointer went down, its width then.
    Resize { start: (f32, f32), width: f32 },
}

/// Milliseconds on a clock that only goes forward (for the speaker's hold).
fn now_ms() -> u64 {
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// The area the mini window lives in: the window less the title bar.
fn area(window: &Window) -> (f32, f32) {
    let size = window.viewport_size();
    (
        f32::from(size.width),
        (f32::from(size.height) - TITLEBAR_H).max(0.0),
    )
}

impl Shell {
    /// Whether the full call view is on screen: the call's room is the one
    /// open (not covered by the Friends page, a server's welcome or a lock).
    fn call_in_view(&self) -> bool {
        let Some(call) = self.calls.state.as_ref() else {
            return false;
        };
        self.selected.as_ref() == Some(&call.room_id)
            && self.welcome.is_none()
            && !self.friends_open
            && !crate::locks::is_locked(&call.room_id)
    }

    /// Which feed the mini window shows now, if it shows (and a redraw set
    /// for when a speaker's hold runs out).
    pub fn mini_call_feed(&mut self, cx: &mut Context<Self>) -> Option<String> {
        let shows = self
            .calls
            .state
            .as_ref()
            .is_some_and(|c| c.connected && !c.video.is_empty())
            && !self.call_in_view();
        if !shows {
            self.calls.mini_pick = Default::default();
            self.calls.mini_drag = None;
            self.calls.mini_hover = false;
            return None;
        }
        let call = self.calls.state.as_ref()?;
        let me = self.session.as_ref().and_then(|s| s.user_id());
        let now = now_ms();
        let key = self.calls.mini_pick.pick(
            self.calls.focus.as_deref(),
            &call.video,
            &call.speaking,
            me.as_ref(),
            now,
        );
        if let Some(wait) = self.calls.mini_pick.next_change(now) {
            let due = now + wait;
            if self.calls.mini_wake.is_none_or(|at| at > due || at <= now) {
                self.calls.mini_wake = Some(due);
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(wait))
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        this.calls.mini_wake = None;
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        key
    }

    /// The pointer moved with the mini window pressed.
    fn mini_drag_to(&mut self, at: (f32, f32), window: (f32, f32), cx: &mut Context<Self>) {
        match self.calls.mini_drag {
            Some(MiniDrag::Move {
                grab,
                moved,
                start,
                ..
            }) => {
                let far = moved || (at.0 - start.0).abs() + (at.1 - start.1).abs() > 4.0;
                self.calls.mini_drag = Some(MiniDrag::Move {
                    grab,
                    at: (at.0 - grab.0, at.1 - grab.1),
                    moved: far,
                    start,
                });
                cx.notify();
            }
            Some(MiniDrag::Resize { start, width }) => {
                self.calls.mini_place =
                    self.calls
                        .mini_place
                        .resized(width, at.0 - start.0, at.1 - start.1, window);
                cx.notify();
            }
            None => {}
        }
    }

    /// Let go of the mini window: a move snaps to the nearer edge; the
    /// place is kept.
    fn mini_drop(&mut self, window: (f32, f32), cx: &mut Context<Self>) {
        match self.calls.mini_drag.take() {
            Some(MiniDrag::Move { at, moved: true, .. }) => {
                self.calls.mini_place = self.calls.mini_place.dropped(at.0, at.1, window);
            }
            Some(MiniDrag::Resize { .. }) => {}
            _ => return,
        }
        // Widget tests have no data directory of their own.
        if !cfg!(test)
            && let Err(err) = self.calls.mini_place.save(&crate::data_dir())
        {
            tracing::warn!("could not keep the mini call window's place: {err}");
        }
        cx.notify();
    }
}

/// Reads the mini window's place kept on this device.
pub fn load_place() -> MiniPlace {
    MiniPlace::load(&crate::data_dir())
}

/// The mini call window showing feed `key`, over everything but dialogs and
/// menus.
pub fn render(
    shell: &Shell,
    key: Option<String>,
    window: &Window,
    cx: &mut Context<Shell>,
) -> Option<AnyElement> {
    let key = key?;
    let call = shell.calls.state.as_ref()?;
    let feed = call.video.iter().find(|f| f.key == key)?.clone();
    let frame = shell.calls.frames.get(&key).cloned();
    let room = (call.room_id.clone(), call.room_name.clone());
    let space = area(window);
    let place = shell.calls.mini_place;
    let (mut left, mut top, w, h) = place.bounds(space);
    if let Some(MiniDrag::Move { at, moved: true, .. }) = shell.calls.mini_drag {
        left = at.0.clamp(0.0, (space.0 - w).max(0.0));
        top = at.1.clamp(0.0, (space.1 - h).max(0.0));
    }
    let name = shown(shell, &feed.user);
    let label = if feed.screen {
        format!("{name}'s screen")
    } else {
        name
    };
    let hovered = shell.calls.mini_hover || shell.calls.mini_drag.is_some();
    let view = cx.entity();
    // While it is pressed, the pointer is followed wherever it goes (over
    // the window behind it too), and its release ends the press.
    let follow = canvas(
        |_, _, _| (),
        move |_, (), window, _| {
            let (moving, up) = (view.clone(), view.clone());
            window.on_mouse_event(move |event: &gpui::MouseMoveEvent, phase, window, cx| {
                if phase != gpui::DispatchPhase::Capture
                    || moving.read(cx).calls.mini_drag.is_none()
                {
                    return;
                }
                let at = (
                    f32::from(event.position.x),
                    f32::from(event.position.y) - TITLEBAR_H,
                );
                let space = area(window);
                moving.update(cx, |this, cx| {
                    if this.calls.mini_drag.is_some() {
                        if event.pressed_button == Some(MouseButton::Left) {
                            this.mini_drag_to(at, space, cx);
                        } else {
                            this.mini_drop(space, cx);
                        }
                    }
                });
            });
            window.on_mouse_event(move |event: &gpui::MouseUpEvent, phase, window, cx| {
                if phase != gpui::DispatchPhase::Capture
                    || event.button != MouseButton::Left
                    || up.read(cx).calls.mini_drag.is_none()
                {
                    return;
                }
                let space = area(window);
                up.update(cx, |this, cx| {
                    if this.calls.mini_drag.is_some() {
                        this.mini_drop(space, cx);
                    }
                });
            });
        },
    )
    .absolute()
    .size_full();

    let back = div()
        .id("mini-call-back")
        .absolute()
        .top(px(8.0))
        .left(px(8.0))
        .size(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(gpui::hsla(0.0, 0.0, 0.0, 0.6))
        .hover(|s| s.bg(gpui::hsla(0.0, 0.0, 0.0, 0.85)))
        .cursor_pointer()
        .child(icon("chevron-left", 18.0).text_color(rgb(0xffffff)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(cx.listener({
            let room = room.0.clone();
            move |this, _, _, cx| this.open_room(room.clone(), cx)
        }))
        .e2e("button", "ghost mini-call-back")
        .attr("title", "Back to the call");

    let name_label = div()
        .absolute()
        .bottom(px(8.0))
        .left(px(8.0))
        .max_w(px((w - 16.0).max(0.0)))
        .px(px(8.0))
        .py(px(2.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(gpui::hsla(0.0, 0.0, 0.0, 0.6))
        .text_color(rgb(0xffffff))
        .text_size(px(0.8 * theme::REM))
        .font_weight(FontWeight::SEMIBOLD)
        .truncate()
        .child(label.clone())
        .e2e("span", "mini-call-name")
        .text(label);

    // The corner away from its edge resizes it.
    let inner_left = place.side == Side::Right;
    let mut grip = div()
        .id("mini-call-resize")
        .absolute()
        .bottom_0()
        .size(px(18.0))
        .cursor(if inner_left {
            gpui::CursorStyle::ResizeUpRightDownLeft
        } else {
            gpui::CursorStyle::ResizeUpLeftDownRight
        })
        .child(
            div()
                .absolute()
                .bottom(px(4.0))
                .size(px(9.0))
                .border_b_2()
                .border_color(gpui::hsla(0.0, 0.0, 1.0, 0.7))
                .when(inner_left, |d| d.left(px(4.0)).border_l_2())
                .when(!inner_left, |d| d.right(px(4.0)).border_r_2()),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.calls.mini_drag = Some(MiniDrag::Resize {
                    start: (
                        f32::from(event.position.x),
                        f32::from(event.position.y) - TITLEBAR_H,
                    ),
                    width: w,
                });
                cx.notify();
            }),
        );
    grip = if inner_left {
        grip.left_0()
    } else {
        grip.right_0()
    };

    let mut tile = div()
        .id("mini-call")
        .absolute()
        .left(px(left))
        .top(px(top + TITLEBAR_H))
        .w(px(w))
        .h(px(h))
        .rounded(px(theme::RADIUS_LG))
        .overflow_hidden()
        .bg(rgb(0x000000))
        .border_1()
        .border_color(rgb(theme::bg_selected()))
        .shadow_lg()
        .occlude()
        .cursor(if shell.calls.mini_drag.is_some() {
            gpui::CursorStyle::ClosedHand
        } else {
            gpui::CursorStyle::OpenHand
        })
        .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
            if this.calls.mini_hover != *hovered {
                this.calls.mini_hover = *hovered;
                cx.notify();
            }
        }))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                let at = (
                    f32::from(event.position.x),
                    f32::from(event.position.y) - TITLEBAR_H,
                );
                this.calls.mini_drag = Some(MiniDrag::Move {
                    grab: (at.0 - left, at.1 - top),
                    at: (left, top),
                    moved: false,
                    start: at,
                });
                cx.notify();
            }),
        )
        .child(follow);
    if let Some(frame) = frame {
        tile = tile.child(frame);
    }
    tile = tile.child(name_label).child(back).child(grip);
    if hovered {
        tile = tile.child(controls(shell, cx));
    }
    Some(
        tile.e2e("div", "mini-call")
            .attr("data-feed", key)
            .attr("data-user", feed.user.to_string())
            .attr("data-side", place.side.key())
            .attr("data-width", format!("{w:.0}"))
            .into_any_element(),
    )
}

/// One of the hover controls: a small round button over the picture. (No
/// press-sink as the call view's have: drawn lower while held, a button over
/// a moving picture would be pressed again by a test's click.)
fn mini_button(
    id: &'static str,
    classes: &str,
    title: &str,
    glyph: &'static str,
    red: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> AnyElement {
    div()
        .id(id)
        .flex_none()
        .size(px(30.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(if red {
            gpui::Hsla::from(rgb(theme::danger()))
        } else {
            gpui::hsla(0.0, 0.0, 0.0, 0.6)
        })
        .hover(|s| s.opacity(0.85))
        .cursor_pointer()
        .child(icon(glyph, 15.0).text_color(rgb(0xffffff)))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(on_click)
        .e2e("button", classes)
        .attr("title", title.to_owned())
        .into_any_element()
}

/// Mute, deafen and hang up, along the top right while hovered.
fn controls(shell: &Shell, cx: &mut Context<Shell>) -> AnyElement {
    let flags = shell.calls.voice;
    let muted = flags.mic_off();
    div()
        .absolute()
        .top(px(8.0))
        .right(px(8.0))
        .flex()
        .gap(px(6.0))
        .child(mini_button(
            "mini-mute",
            if muted {
                "call-control mute-button active"
            } else {
                "call-control mute-button"
            },
            flags.mute_title(),
            if muted { "mic-off" } else { "mic" },
            muted,
            cx.listener(|this, _, _, cx| this.toggle_mute(cx)),
        ))
        .child(mini_button(
            "mini-deafen",
            if flags.deafened {
                "call-control deafen-button active"
            } else {
                "call-control deafen-button"
            },
            flags.deafen_title(),
            if flags.deafened {
                "headphone-off"
            } else {
                "headphones"
            },
            flags.deafened,
            cx.listener(|this, _, _, cx| this.toggle_deafen(cx)),
        ))
        .child(mini_button(
            "mini-hang-up",
            "call-control hang-up",
            "Disconnect",
            "phone-off",
            true,
            cx.listener(|this, _, _, cx| this.hang_up(cx)),
        ))
        .e2e("div", "mini-call-controls")
        .into_any_element()
}

/// Widget tests: the shell on gpui's test platform with a call fed in by
/// hand (no session: nothing reaches LiveKit or the server).
#[cfg(all(test, feature = "e2e"))]
mod widget_tests {
    use super::*;
    use crate::e2e::{self, query, query_bounds};
    use crate::model::tests::room;
    use crate::view_model::calls::{CallState, VideoFeed};
    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point};
    use morpheus_core::{OwnedRoomId, OwnedUserId};

    struct Harness(Entity<Shell>);

    impl gpui::Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            e2e::frame(div().size_full().child(self.0.clone()))
        }
    }

    fn redraw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.draw(point(px(0.0), px(0.0)), gpui::size(px(1100.0), px(720.0)), |_, _| div());
        cx.run_until_parked();
    }

    fn centre(selector: &str) -> gpui::Point<gpui::Pixels> {
        let (_, (x, y)) = query(selector)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no {selector}"));
        point(px(x), px(y))
    }

    fn user(name: &str) -> OwnedUserId {
        OwnedUserId::try_from(format!("@{name}:x")).expect("user id")
    }

    /// In the lounge's call with bob sharing, general open.
    fn open(cx: &mut TestAppContext) -> (Entity<Shell>, &mut VisualTestContext) {
        morpheus_call::use_test_tone();
        let (harness, cx) = cx.add_window_view(|_, cx| Harness(cx.new(|_| Shell::bare(None))));
        let shell = harness.read_with(cx, |h, _| h.0.clone());
        let mut lounge = room("!lounge:x", "lounge");
        lounge.is_voice = true;
        lounge.call_participants = vec![user("bob")];
        shell.update(cx, |s, cx| {
            s.rooms = vec![room("!g:x", "general"), lounge];
            s.selected = Some(OwnedRoomId::try_from("!g:x").expect("room"));
            let mut call = CallState::joining(
                OwnedRoomId::try_from("!lounge:x").expect("room"),
                "lounge".into(),
            );
            call.connected = true;
            call.video.push(VideoFeed {
                user: user("bob"),
                key: "bob-screen".into(),
                screen: true,
            });
            s.calls.state = Some(call);
            cx.notify();
        });
        redraw(cx);
        (shell, cx)
    }

    #[gpui::test]
    fn the_mini_window_shows_the_share_and_its_controls_work(cx: &mut TestAppContext) {
        let (shell, cx) = open(cx);
        assert_eq!(query(".mini-call[data-feed=\"bob-screen\"]").len(), 1);
        assert!(query(".mini-call .mini-call-controls").is_empty(), "controls on hover only");
        cx.simulate_mouse_move(centre(".mini-call"), None, Modifiers::none());
        redraw(cx);
        assert_eq!(query(".mini-call .mini-call-controls .hang-up").len(), 1);
        // Moving onto a control keeps them (they are in front of it).
        cx.simulate_mouse_move(centre(".mini-call .mini-call-controls .mute-button"), None, Modifiers::none());
        redraw(cx);
        assert_eq!(query(".mini-call .mini-call-controls").len(), 1, "still hovered over a control");
        cx.simulate_click(centre(".mini-call .mini-call-controls .mute-button"), Modifiers::none());
        redraw(cx);
        assert_eq!(query(".user-pill .mute-button.active").len(), 1, "muted from it");
        assert!(shell.read_with(cx, |s, _| s.calls.mini_drag.is_none()));
        // The call view open: no mini window.
        cx.simulate_click(centre(".mini-call .mini-call-back"), Modifiers::none());
        redraw(cx);
        assert_eq!(
            shell.read_with(cx, |s, _| s.selected.clone()),
            Some(OwnedRoomId::try_from("!lounge:x").expect("room"))
        );
        assert!(query(".mini-call").is_empty());
    }

    #[gpui::test]
    fn dragged_left_it_snaps_to_the_left_edge(cx: &mut TestAppContext) {
        let (shell, cx) = open(cx);
        let from = centre(".mini-call .mini-call-name");
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        redraw(cx);
        for step in 1..=20 {
            let at = point(from.x - px(40.0 * step as f32), from.y);
            cx.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
            redraw(cx);
        }
        let end = point(from.x - px(800.0), from.y);
        cx.simulate_mouse_up(end, MouseButton::Left, Modifiers::none());
        redraw(cx);
        assert_eq!(query(".mini-call[data-side=\"left\"]").len(), 1);
        let bounds = query_bounds(".mini-call").expect("bounds");
        assert!(bounds.x < 40.0, "against the left edge: {bounds:?}");
        assert!(shell.read_with(cx, |s, _| s.calls.mini_drag.is_none()));
    }
}
