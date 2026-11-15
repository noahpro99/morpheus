//! The press feedback of round and squircle icon buttons (the rail, the
//! call's controls): the button sinks `DEPTH` pixels while the pointer holds it
//! down and comes back on release or when the pointer leaves, over a short
//! ease. Its size and place in the layout are unchanged: it is drawn lower.

use std::{
    collections::HashMap,
    rc::Rc,
    sync::Mutex,
    time::{Duration, Instant},
};

use gpui::{
    App, InteractiveElement, MouseButton, ParentElement, StatefulInteractiveElement, Styled,
    Window, px,
};

/// How far a pressed button sinks, in pixels.
pub const DEPTH: f32 = 1.5;
/// How long the move takes to change.
const SPAN: Duration = Duration::from_millis(90);
/// The frames drawn while it changes.
const FRAME: Duration = Duration::from_millis(12);

#[derive(Clone, Copy)]
struct State {
    pressed: bool,
    /// The offset when the last change began.
    from: f32,
    since: Instant,
}

static STATES: Mutex<Option<HashMap<String, State>>> = Mutex::new(None);

/// The offset at `now`: from where the last change began to its target, eased out.
fn offset_at(state: State, now: Instant) -> f32 {
    let target = if state.pressed { DEPTH } else { 0.0 };
    let t =
        (now.saturating_duration_since(state.since).as_secs_f32() / SPAN.as_secs_f32()).min(1.0);
    let eased = 1.0 - (1.0 - t) * (1.0 - t);
    state.from + (target - state.from) * eased
}

fn offset_of(key: &str) -> f32 {
    let states = STATES.lock().ok();
    states
        .as_ref()
        .and_then(|s| s.as_ref()?.get(key).copied())
        .map_or(0.0, |state| offset_at(state, Instant::now()))
}

/// Presses or lets go of `key`, and draws frames until the change is done.
fn set(key: &str, pressed: bool, window: &mut Window, cx: &App) {
    {
        let Ok(mut states) = STATES.lock() else {
            return;
        };
        let states = states.get_or_insert_with(HashMap::new);
        let now = Instant::now();
        let was = states.get(key).copied();
        if was.map_or(!pressed, |s| s.pressed == pressed) {
            return;
        }
        let from = was.map_or(0.0, |s| offset_at(s, now));
        states.insert(
            key.to_owned(),
            State {
                pressed,
                from,
                since: now,
            },
        );
    }
    window.refresh();
    window
        .spawn(cx, async move |cx| {
            let frames = (SPAN.as_millis() / FRAME.as_millis()) as u32 + 1;
            for _ in 0..frames {
                cx.background_executor().timer(FRAME).await;
                if cx.update(|window, _| window.refresh()).is_err() {
                    break;
                }
            }
        })
        .detach();
}

/// Where each button was drawn last (the pointer leaving it is told from this).
static BOUNDS: Mutex<Option<HashMap<String, gpui::Bounds<gpui::Pixels>>>> = Mutex::new(None);

fn bounds_of(key: &str) -> Option<gpui::Bounds<gpui::Pixels>> {
    BOUNDS.lock().ok()?.as_ref()?.get(key).copied()
}

/// While `key` is held, watches the pointer: out of the button it lets go.
/// (A held button's own hover listener says "not hovered", so the pointer's
/// place is compared with where the button was drawn.)
fn watch(key: String, window: &mut Window, cx: &App) {
    window
        .spawn(cx, async move |cx| {
            loop {
                cx.background_executor().timer(FRAME).await;
                let outside = cx.update(|window, cx| {
                    let held = STATES
                        .lock()
                        .ok()
                        .and_then(|s| s.as_ref()?.get(&key).map(|s| s.pressed))
                        .unwrap_or(false);
                    if !held {
                        return None;
                    }
                    let inside =
                        bounds_of(&key).is_none_or(|b| b.contains(&window.mouse_position()));
                    if !inside {
                        set(&key, false, window, cx);
                    }
                    Some(inside)
                });
                match outside {
                    Ok(Some(true)) => {}
                    _ => break,
                }
            }
        })
        .detach();
}

/// Gives `button` (identified by `key`) the press
/// feedback. `on_hover` hears the pointer coming and going, as the button's
/// own hover listener would (a button has only one).
pub fn press<E>(button: E, key: &str, on_hover: Option<Rc<dyn Fn(bool, &mut App)>>) -> E
where
    E: Styled + InteractiveElement + StatefulInteractiveElement + ParentElement,
{
    let (down, up, up_out, seen) = (
        key.to_owned(),
        key.to_owned(),
        key.to_owned(),
        key.to_owned(),
    );
    let button = button
        .relative()
        .top(px(offset_of(key)))
        .child(
            gpui::canvas(
                move |bounds, _, _| {
                    if let Ok(mut all) = BOUNDS.lock() {
                        all.get_or_insert_with(HashMap::new)
                            .insert(seen.clone(), bounds);
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            set(&down, true, window, cx);
            watch(down.clone(), window, cx);
        })
        .on_mouse_up(MouseButton::Left, move |_, window, cx| {
            set(&up, false, window, cx)
        })
        .on_mouse_up_out(MouseButton::Left, move |_, window, cx| {
            set(&up_out, false, window, cx)
        });
    match on_hover {
        Some(on_hover) => button.on_hover(move |over, _, cx| on_hover(*over, cx)),
        None => button,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_offset_eases_to_the_target_and_back() {
        let start = Instant::now();
        let down = State {
            pressed: true,
            from: 0.0,
            since: start,
        };
        assert_eq!(offset_at(down, start), 0.0);
        let mid = offset_at(down, start + SPAN / 2);
        assert!(mid > 0.0 && mid < DEPTH, "{mid}");
        assert_eq!(offset_at(down, start + SPAN * 2), DEPTH);
        let up = State {
            pressed: false,
            from: DEPTH,
            since: start,
        };
        assert_eq!(offset_at(up, start + SPAN * 2), 0.0);
    }
}
