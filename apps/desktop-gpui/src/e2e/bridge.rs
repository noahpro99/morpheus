//! The structured test bridge: `MORPHEUS_E2E_PORT` is a loopback port that
//! takes newline-delimited JSON requests and answers each with
//! `{"id", "ok", "value", "error"}`, like the Dioxus bridge
//! (`crates/ui/src/e2e.rs`), but the requests are operations on the tagged
//! element tree instead of JavaScript:
//!
//! `{"id": 1, "op": "count", "selector": ".room"}`
//!
//! Operations: `count`, `texts`, `text`, `value`, `bounds`, `scroll_state` (reads);
//! `scroll` (to `top`/`bottom` or `from_bottom` pixels, on an element that
//! offers a [`super::Scrollable`]);
//! `click`, `right_click`, `type`, `press`, `submit` (input, injected through
//! the window's own dispatch, [`gpui::Window::dispatch_event`] and
//! [`gpui::Window::dispatch_keystroke`], at the element's centre or to the
//! focused element). Every operation takes `selector` and optional filters
//! on the matches: `contains` / `exact` (their text) and `index`.

use std::{
    io::{BufRead, BufReader, Write},
    net::TcpListener,
    time::Duration,
};

use futures::{
    StreamExt,
    channel::{mpsc, oneshot},
};
use gpui::{
    App, AppContext as _, AsyncApp, Keystroke, Modifiers, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PlatformInput, point, px,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    selector::Selector,
    tag::with_state,
    tree::{Rect, Tree},
};

const PORT_ENV: &str = "MORPHEUS_E2E_PORT";
/// How long an input waits for the app to draw its effect.
const FRAME_WAIT: Duration = Duration::from_millis(150);

#[derive(Debug, Default, Deserialize)]
struct Request {
    #[serde(default)]
    id: u64,
    op: Option<String>,
    /// Only to answer a JavaScript request with a clear error.
    js: Option<String>,
    selector: Option<String>,
    index: Option<usize>,
    contains: Option<String>,
    exact: Option<String>,
    /// Only matches inside an element matching this selector (and, with
    /// `within_contains`, whose text has this in it).
    within: Option<String>,
    within_contains: Option<String>,
    /// With `within_contains`: the text that counts is that of the
    /// descendants of the `within` element matching this selector (a
    /// message's `.body`, not the quote of another message in it).
    within_in: Option<String>,
    /// The text for `type`.
    text: Option<String>,
    /// The key for `press` (`Enter`, `ArrowDown`, `a`, …).
    key: Option<String>,
    #[serde(default)]
    modifiers: Vec<String>,
    /// For `scroll`: `top` or `bottom`, or `from_bottom` pixels.
    to: Option<String>,
    from_bottom: Option<f32>,
    /// For `drag`: how far to move the pressed pointer, in pixels.
    dx: Option<f32>,
    dy: Option<f32>,
    /// For `attach_file`: the file (`data` is base64), and `via` `paste` or `drop`.
    name: Option<String>,
    mime: Option<String>,
    data: Option<String>,
    via: Option<String>,
}

type Reply = Result<Value, String>;

/// Listen on `MORPHEUS_E2E_PORT` (if set) and serve requests on the main thread.
pub fn start(cx: &mut App) {
    let Some(port) = std::env::var(PORT_ENV)
        .ok()
        .and_then(|port| port.parse::<u16>().ok())
    else {
        return;
    };
    // The listener lives as long as the process: the window can be closed to
    // the tray and opened again (a new run of the window system), and the
    // test's connection must go on.
    let (requests, mut incoming) = mpsc::unbounded::<(Request, oneshot::Sender<Reply>)>();
    if let Ok(mut current) = CURRENT.lock() {
        *current = Some(requests);
    }
    static STARTED: std::sync::Once = std::sync::Once::new();
    STARTED.call_once(|| {
        let listener = match TcpListener::bind(("127.0.0.1", port)) {
            Ok(listener) => listener,
            Err(err) => {
                eprintln!("morpheus-gpui: e2e bridge could not bind {port}: {err}");
                return;
            }
        };
        std::thread::spawn(move || serve(&listener));
    });
    cx.spawn(async move |cx| {
        while let Some((request, reply)) = incoming.next().await {
            let _ = reply.send(handle(&request, cx).await);
        }
    })
    .detach();
}

/// Accept clients one at a time (tests are sequential), pass each request to
/// the main thread and write its reply.
/// Where requests go: the current run of the window system, if there is one.
type Requests = mpsc::UnboundedSender<(Request, oneshot::Sender<Reply>)>;
static CURRENT: std::sync::Mutex<Option<Requests>> = std::sync::Mutex::new(None);
/// Counts the runs of the window system that have ended.
static RUNS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The window system's run is over (the window closed to the tray): requests
/// in flight are answered, and new ones wait for the next window.
pub fn run_ended() {
    if let Ok(mut current) = CURRENT.lock() {
        *current = None;
    }
    RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
}

fn serve(listener: &TcpListener) {
    for stream in listener.incoming().flatten() {
        let Ok(read) = stream.try_clone() else {
            continue;
        };
        let mut write = stream;
        for line in BufReader::new(read).lines() {
            let Ok(line) = line else {
                break;
            };
            let (id, reply) = match serde_json::from_str::<Request>(&line) {
                Ok(request) => {
                    let id = request.id;
                    let (reply, wait) = oneshot::channel();
                    // With the window closed to the tray a request waits for the next one.
                    let mut pending = Some((request, reply));
                    let waiting_since = std::time::Instant::now();
                    let (mut sent, mut run) = (false, 0);
                    while let Some(message) = pending.take() {
                        let mut message = Some(message);
                        if let Ok(current) = CURRENT.lock()
                            && let Some(requests) = current.as_ref()
                        {
                            run = RUNS.load(std::sync::atomic::Ordering::Relaxed);
                            if let Some(m) = message.take() {
                                match requests.unbounded_send(m) {
                                    Ok(()) => sent = true,
                                    Err(failed) => message = Some(failed.into_inner()),
                                }
                            }
                        }
                        if sent || waiting_since.elapsed() > std::time::Duration::from_secs(20) {
                            break;
                        }
                        pending = message;
                        std::thread::sleep(std::time::Duration::from_millis(50));
                    }
                    let reply = if sent {
                        let mut wait = wait;
                        loop {
                            match wait.try_recv() {
                                Ok(Some(reply)) => break reply,
                                Ok(None) => {}
                                Err(_) => break Err("the window closed".into()),
                            }
                            // The request closed the window to the tray: the
                            // run that held it is over and answers no more.
                            if RUNS.load(std::sync::atomic::Ordering::Relaxed) != run {
                                std::thread::sleep(std::time::Duration::from_millis(300));
                                break Ok(Value::Null);
                            }
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        }
                    } else {
                        Err("the window is closed (in the tray)".into())
                    };
                    (id, reply)
                }
                Err(err) => (0, Err(format!("bad request: {err}"))),
            };
            let mut encoded = match reply {
                Ok(value) => json!({ "id": id, "ok": true, "value": value }),
                Err(error) => json!({ "id": id, "ok": false, "error": error }),
            }
            .to_string();
            encoded.push('\n');
            if write.write_all(encoded.as_bytes()).is_err() {
                break;
            }
        }
    }
}

fn no_op(request: &Request) -> String {
    if request.js.is_some() {
        "the GPUI app runs no JavaScript: the test needs porting to selector calls".into()
    } else {
        "missing op".into()
    }
}

async fn handle(request: &Request, cx: &mut AsyncApp) -> Reply {
    let Some(op) = request.op.as_deref() else {
        return Err(no_op(request));
    };
    match op {
        "count" | "texts" | "attrs" | "text" | "value" | "bounds" | "all_bounds" => {
            with_state(|state| read(&state.tree, op, request))
        }
        "scroll_state" => {
            let probe = probe(request)?;
            let s = probe.scroll_state();
            Ok(json!({ "from_bottom": s.from_bottom, "range": s.range, "viewport": s.viewport }))
        }
        "scroll" => {
            let probe = probe(request)?;
            let from_bottom = match (request.to.as_deref(), request.from_bottom) {
                (Some("top"), _) => f32::MAX,
                (Some("bottom"), _) => 0.0,
                (Some(other), _) => return Err(format!("scroll to {other:?}: top or bottom")),
                (None, Some(px)) => px,
                (None, None) => return Err("scroll needs `to` or `from_bottom`".into()),
            };
            let before = with_state(|state| state.frame);
            probe.scroll_to(from_bottom);
            let window = cx
                .update(|cx| cx.windows().first().copied())
                .map_err(|err| err.to_string())?
                .ok_or("the app has no window")?;
            cx.update_window(window, |_, window, _| window.refresh())
                .map_err(|err| err.to_string())?;
            next_frame(cx, before).await;
            Ok(Value::Null)
        }
        "attach_file" => attach_file(request, cx).await,
        "click" | "right_click" | "type" | "press" | "submit" | "drag" => act(op, request, cx).await,
        _ => Err(format!("unknown op: {op}")),
    }
}

/// The scroll probe of the element the request's selector matches.
fn probe(request: &Request) -> Result<std::rc::Rc<dyn super::Scrollable>, String> {
    with_state(|state| {
        let index = target(&state.tree, request)?.ok_or_else(|| missing(request))?;
        state
            .scroll
            .get(&index)
            .cloned()
            .ok_or_else(|| format!("{} does not scroll", request.selector.as_deref().unwrap_or("")))
    })
}

fn parse_selector(request: &Request) -> Result<Selector, String> {
    Selector::parse(request.selector.as_deref().ok_or("missing selector")?)
}

/// The matches of the request's selector that pass its text filters.
fn matches(tree: &Tree, request: &Request) -> Result<Vec<usize>, String> {
    let selector = parse_selector(request)?;
    let inner: Option<Vec<usize>> = match &request.within_in {
        None => None,
        Some(selector) => Some(Selector::parse(selector)?.select(tree)),
    };
    let scope_text = |i: usize| -> String {
        let Some(inner) = &inner else {
            return tree.text(i);
        };
        inner
            .iter()
            .filter(|&&n| {
                let mut at = n;
                while let Some(parent) = tree.nodes[at].parent {
                    if parent == i {
                        return true;
                    }
                    at = parent;
                }
                false
            })
            .map(|&n| tree.text(n))
            .collect()
    };
    let scopes: Option<Vec<usize>> = match &request.within {
        None => None,
        Some(within) => Some(
            Selector::parse(within)?
                .select(tree)
                .into_iter()
                .filter(|&i| {
                    request
                        .within_contains
                        .as_ref()
                        .is_none_or(|c| scope_text(i).contains(c))
                })
                .collect(),
        ),
    };
    let inside = |mut i: usize| -> bool {
        let Some(scopes) = &scopes else { return true };
        while let Some(parent) = tree.nodes[i].parent {
            if scopes.contains(&parent) {
                return true;
            }
            i = parent;
        }
        false
    };
    Ok(selector
        .select(tree)
        .into_iter()
        .filter(|&i| {
            if !inside(i) {
                return false;
            }
            let text = tree.text(i);
            request.contains.as_ref().is_none_or(|c| text.contains(c))
                && request.exact.as_ref().is_none_or(|e| text.trim() == e)
        })
        .collect())
}

/// The match an operation applies to: the `index`th (default first).
fn target(tree: &Tree, request: &Request) -> Result<Option<usize>, String> {
    Ok(matches(tree, request)?
        .get(request.index.unwrap_or(0))
        .copied())
}

fn missing(request: &Request) -> String {
    let mut what = format!(
        "no element matching {}",
        request.selector.as_deref().unwrap_or("")
    );
    if let Some(text) = request.contains.as_ref().or(request.exact.as_ref()) {
        what.push_str(&format!(" with text {text:?}"));
    }
    what
}

fn rect_json(rect: Rect) -> Value {
    json!({ "x": rect.x, "y": rect.y, "w": rect.w, "h": rect.h })
}

/// The operations that only look at the tree.
fn read(tree: &Tree, op: &str, request: &Request) -> Reply {
    if op == "count" {
        return Ok(json!(matches(tree, request)?.len()));
    }
    if op == "texts" {
        return Ok(json!(
            matches(tree, request)?
                .into_iter()
                .map(|i| tree.text(i))
                .collect::<Vec<_>>()
        ));
    }
    if op == "all_bounds" {
        return Ok(json!(
            matches(tree, request)?
                .into_iter()
                .map(|i| rect_json(tree.nodes[i].bounds))
                .collect::<Vec<_>>()
        ));
    }
    if op == "attrs" {
        let name = request.name.as_deref().unwrap_or_default();
        return Ok(json!(
            matches(tree, request)?
                .into_iter()
                .map(|i| tree.nodes[i].attr(name))
                .collect::<Vec<_>>()
        ));
    }
    let Some(index) = target(tree, request)? else {
        return Ok(Value::Null);
    };
    Ok(match op {
        "text" => json!(tree.text(index)),
        "value" => tree.nodes[index]
            .attr("value")
            .map_or(Value::Null, Value::from),
        _ => rect_json(tree.nodes[index].bounds),
    })
}

/// The element to click for `submit`: the target itself or its first
/// `[type=submit]` descendant.
fn submit_control(tree: &Tree, index: usize) -> Option<usize> {
    let is_submit = |i: usize| tree.nodes[i].attr("type").as_deref() == Some("submit");
    if is_submit(index) {
        return Some(index);
    }
    let depth = tree.nodes[index].depth;
    (index + 1..tree.nodes.len())
        .take_while(|&i| tree.nodes[i].depth > depth)
        .find(|&i| is_submit(i))
}

/// Modifier names as the driver sends them.
fn modifiers(names: &[String]) -> Result<Modifiers, String> {
    let mut mods = Modifiers::default();
    for name in names {
        match name.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods.control = true,
            "alt" | "option" => mods.alt = true,
            "shift" => mods.shift = true,
            "meta" | "super" | "cmd" | "command" | "platform" => mods.platform = true,
            other => return Err(format!("unknown modifier: {other}")),
        }
    }
    Ok(mods)
}

/// A DOM key name (`Enter`, `ArrowDown`, `a`) as a GPUI keystroke.
fn keystroke(key: &str, names: &[String]) -> Result<Keystroke, String> {
    let name = match key {
        "Enter" => "enter",
        "Escape" | "Esc" => "escape",
        "ArrowUp" => "up",
        "ArrowDown" => "down",
        "ArrowLeft" => "left",
        "ArrowRight" => "right",
        "Backspace" => "backspace",
        "Delete" => "delete",
        "Tab" => "tab",
        " " | "Space" => "space",
        "Home" => "home",
        "End" => "end",
        "PageUp" => "pageup",
        "PageDown" => "pagedown",
        "Insert" => "insert",
        f if f.len() >= 2 && f.starts_with('F') && f[1..].chars().all(|c| c.is_ascii_digit()) => {
            return Ok(Keystroke {
                modifiers: modifiers(names)?,
                key: f.to_ascii_lowercase(),
                key_char: None,
            });
        }
        single if single.chars().count() == 1 => {
            return Ok(Keystroke {
                modifiers: modifiers(names)?,
                key: single.to_lowercase(),
                key_char: None,
            });
        }
        other => return Err(format!("unknown key: {other}")),
    };
    Ok(Keystroke {
        modifiers: modifiers(names)?,
        key: name.to_owned(),
        key_char: None,
    })
}

/// What typing `c` is: its key, shifted for capitals, with the character.
fn char_keystroke(c: char) -> Keystroke {
    if c == '\n' {
        // A line break typed into a box: a new line, not a send.
        return Keystroke {
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::default()
            },
            key: "enter".into(),
            key_char: None,
        };
    }
    Keystroke {
        modifiers: Modifiers {
            shift: c.is_uppercase(),
            ..Modifiers::default()
        },
        key: c.to_lowercase().to_string(),
        key_char: Some(c.to_string()),
    }
}

/// A pointer click at `(x, y)`: move, press, release.
fn click(
    window: &mut gpui::Window,
    cx: &mut App,
    (x, y): (f32, f32),
    button: MouseButton,
    modifiers: Modifiers,
) {
    let position = point(px(x), px(y));
    let _ = window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers,
        }),
        cx,
    );
    let _ = window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
    let _ = window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button,
            position,
            modifiers,
            click_count: 1,
        }),
        cx,
    );
}

/// The first half of a click (move, press) and its second (release), apart:
/// text with links only listens for the release once a frame has been drawn
/// after the press, as a real click's time between them lets it.
fn press_at(
    window: &mut gpui::Window,
    cx: &mut App,
    (x, y): (f32, f32),
    button: MouseButton,
    modifiers: Modifiers,
) {
    let position = point(px(x), px(y));
    let _ = window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: None,
            modifiers,
        }),
        cx,
    );
    let _ = window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            button,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }),
        cx,
    );
}

fn release_at(
    window: &mut gpui::Window,
    cx: &mut App,
    (x, y): (f32, f32),
    button: MouseButton,
    modifiers: Modifiers,
) {
    let _ = window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            button,
            position: point(px(x), px(y)),
            modifiers,
            click_count: 1,
        }),
        cx,
    );
}

/// Wait (briefly) for a frame after the input just dispatched: its effect
/// shows in the tree once the app has redrawn. The counter is read now, not
/// before the input: GPUI draws pending changes before handling an event, so
/// a frame counted since the input began may predate its last effect. A
/// no-op input draws nothing.
async fn next_frame(cx: &mut AsyncApp, _before: u64) {
    let before = with_state(|state| state.frame);
    let step = Duration::from_millis(5);
    let mut waited = Duration::ZERO;
    while with_state(|state| state.frame) == before && waited < FRAME_WAIT {
        cx.background_executor().timer(step).await;
        waited += step;
    }
}

thread_local! {
    /// The open room's composer, for `attach_file`.
    static ATTACH: std::cell::RefCell<Option<gpui::WeakEntity<crate::composer::Composer>>> =
        const { std::cell::RefCell::new(None) };
}

pub fn set_attach_target(composer: gpui::WeakEntity<crate::composer::Composer>) {
    ATTACH.with(|target| *target.borrow_mut() = Some(composer));
}

/// A file pasted into the message box or dropped on the chat: what the
/// platform's clipboard and drag-and-drop would deliver, made by a test.
/// `via` `paste` goes through the box's paste handling (as a picture on the
/// clipboard would); `drop` writes the file to disk and hands the composer
/// its path, as a dropped file is.
async fn attach_file(request: &Request, cx: &mut AsyncApp) -> Reply {
    use base64::Engine as _;
    let name = request.name.clone().ok_or("attach_file needs `name`")?;
    let mime = request.mime.clone().ok_or("attach_file needs `mime`")?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(request.data.as_deref().ok_or("attach_file needs `data`")?)
        .map_err(|err| format!("bad base64: {err}"))?;
    let dropped = match request.via.as_deref() {
        Some("drop") => true,
        Some("paste") | None => false,
        Some(other) => return Err(format!("attach_file via {other:?}: paste or drop")),
    };
    let target = ATTACH
        .with(|target| target.borrow().clone())
        .ok_or("no composer is open")?;
    let before = with_state(|state| state.frame);
    cx.update(|cx| {
        target.update(cx, |composer, cx| {
            composer.attach_from_bridge(
                crate::input::PastedFile { name, mime, bytes },
                dropped,
                cx,
            )
        })
    })
    .map_err(|err| err.to_string())?
    .map_err(|err| err.to_string())?;
    next_frame(cx, before).await;
    Ok(Value::Null)
}

/// A click on something a scrolling ancestor has clipped (a row far down a
/// long list): scrolls it to the middle of that ancestor first, as a person
/// would scroll to it, and waits for the redraw.
async fn scroll_into_view(request: &Request, cx: &mut AsyncApp) -> Result<(), String> {
    let wanted = with_state(|state| -> Result<_, String> {
        let tree = &state.tree;
        let Some(index) = target(tree, request)? else {
            return Ok(None);
        };
        let at = tree.nodes[index].bounds;
        let middle = at.y + at.h / 2.0;
        let mut ancestor = tree.nodes[index].parent;
        while let Some(a) = ancestor {
            if let Some(probe) = state.scroll.get(&a) {
                let view = tree.nodes[a].bounds;
                if middle >= view.y && middle <= view.y + view.h {
                    return Ok(None);
                }
                let delta = middle - (view.y + view.h / 2.0);
                return Ok(Some((probe.clone(), delta)));
            }
            ancestor = tree.nodes[a].parent;
        }
        Ok(None)
    })?;
    let Some((probe, to)) = wanted else {
        return Ok(());
    };
    let before = with_state(|state| state.frame);
    probe.scroll_by(to);
    let window = cx
        .update(|cx| cx.windows().first().copied())
        .map_err(|err| err.to_string())?
        .ok_or("the app has no window")?;
    cx.update_window(window, |_, window, _| window.refresh())
        .map_err(|err| err.to_string())?;
    next_frame(cx, before).await;
    Ok(())
}

async fn act(op: &str, request: &Request, cx: &mut AsyncApp) -> Reply {
    if matches!(op, "click" | "right_click" | "submit") {
        scroll_into_view(request, cx).await?;
    }
    // Where to act: the element's centre, and its focus handle if it has one.
    let (centre, focus) = if request.selector.is_none() && op == "press" {
        ((0.0, 0.0), None)
    } else {
        with_state(|state| -> Result<_, String> {
            let tree = &state.tree;
            let mut index = target(tree, request)?.ok_or_else(|| missing(request))?;
            if op == "submit" {
                index = submit_control(tree, index).ok_or_else(|| {
                    format!(
                        "no [type=submit] control inside {}",
                        request.selector.as_deref().unwrap_or("")
                    )
                })?;
            }
            Ok((
                tree.nodes[index].bounds.centre(),
                state.focus.get(&index).cloned(),
            ))
        })?
    };
    let window = cx
        .update(|cx| cx.windows().first().copied())
        .map_err(|err| err.to_string())?
        .ok_or("the app has no window")?;
    let held = modifiers(&request.modifiers)?;
    let before = with_state(|state| state.frame);

    match op {
        "click" | "submit" | "right_click" => {
            let button = if op == "right_click" {
                MouseButton::Right
            } else {
                MouseButton::Left
            };
            cx.update_window(window, |_, window, cx| press_at(window, cx, centre, button, held))
                .map_err(|err| err.to_string())?;
            // Let a frame draw between press and release (up to 40 ms).
            let drawn = with_state(|state| state.frame);
            for _ in 0..8 {
                if with_state(|state| state.frame) != drawn {
                    break;
                }
                cx.background_executor().timer(Duration::from_millis(5)).await;
            }
            cx.update_window(window, |_, window, cx| release_at(window, cx, centre, button, held))
                .map_err(|err| err.to_string())?;
        }
        "drag" => {
            // Press on the element, move the pointer in steps (the button held), let go.
            let (dx, dy) = (request.dx.unwrap_or(0.0), request.dy.unwrap_or(0.0));
            cx.update_window(window, |_, window, cx| {
                press_at(window, cx, centre, MouseButton::Left, held)
            })
            .map_err(|err| err.to_string())?;
            next_frame(cx, before).await;
            let steps = ((dx.abs().max(dy.abs()) / 20.0).ceil() as u32).max(1);
            for step in 1..=steps {
                let f = step as f32 / steps as f32;
                let at = point(px(centre.0 + dx * f), px(centre.1 + dy * f));
                cx.update_window(window, |_, window, cx| {
                    let _ = window.dispatch_event(
                        PlatformInput::MouseMove(MouseMoveEvent {
                            position: at,
                            pressed_button: Some(MouseButton::Left),
                            modifiers: held,
                        }),
                        cx,
                    );
                })
                .map_err(|err| err.to_string())?;
                next_frame(cx, before).await;
            }
            cx.update_window(window, |_, window, cx| {
                release_at(window, cx, (centre.0 + dx, centre.1 + dy), MouseButton::Left, held)
            })
            .map_err(|err| err.to_string())?;
        }
        "type" => {
            let text = request.text.clone().ok_or("missing text")?;
            // Focus the input (directly, or by clicking it), let it draw so
            // its input handler is the window's, then replace its content.
            cx.update_window(window, |_, window, cx| match &focus {
                Some(handle) => window.focus(handle),
                None => click(window, cx, centre, MouseButton::Left, Modifiers::default()),
            })
            .map_err(|err| err.to_string())?;
            next_frame(cx, before).await;
            let select_all = Keystroke::parse("ctrl-a").map_err(|err| err.to_string())?;
            let before = with_state(|state| state.frame);
            cx.update_window(window, |_, window, cx| {
                window.dispatch_keystroke(select_all, cx);
                if text.is_empty() {
                    // Replacing with nothing: clear what is selected.
                    if let Ok(clear) = Keystroke::parse("backspace") {
                        window.dispatch_keystroke(clear, cx);
                    }
                }
                for c in text.chars() {
                    window.dispatch_keystroke(char_keystroke(c), cx);
                }
            })
            .map_err(|err| err.to_string())?;
            next_frame(cx, before).await;
            return Ok(Value::Null);
        }
        _ => {
            let key = keystroke(
                request.key.as_deref().ok_or("missing key")?,
                &request.modifiers,
            )?;
            cx.update_window(window, |_, window, cx| {
                if let Some(handle) = &focus {
                    window.focus(handle);
                }
                window.dispatch_keystroke(key, cx);
            })
            .map_err(|err| err.to_string())?;
        }
    }
    next_frame(cx, before).await;
    Ok(Value::Null)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::e2e::tree::fixture::node;

    fn request(op: &str, selector: &str) -> Request {
        Request {
            op: Some(op.into()),
            selector: Some(selector.into()),
            ..Request::default()
        }
    }

    fn tree() -> Tree {
        let mut t = Tree::default();
        let body = t.push(node("body", "", &[], ""), None);
        let form = t.push(node("form", "login", &[], ""), Some(body));
        let mut input = node("input", "", &[("name", "user"), ("value", "bob")], "");
        input.bounds = Rect {
            x: 10.0,
            y: 20.0,
            w: 100.0,
            h: 30.0,
        };
        t.push(input, Some(form));
        t.push(
            node("button", "go", &[("type", "submit")], "Go"),
            Some(form),
        );
        t.push(node("p", "", &[], "Hello "), Some(body));
        t.push(node("p", "", &[], "world"), Some(body));
        t
    }

    #[test]
    fn reads() {
        let t = tree();
        assert_eq!(read(&t, "count", &request("count", "p")).unwrap(), json!(2));
        assert_eq!(
            read(&t, "text", &request("text", "form")).unwrap(),
            json!("Go")
        );
        assert_eq!(
            read(&t, "text", &request("text", "body")).unwrap(),
            json!("GoHello world")
        );
        assert_eq!(
            read(&t, "text", &request("text", "nope")).unwrap(),
            Value::Null
        );
        assert_eq!(
            read(&t, "value", &request("value", "input[name=user]")).unwrap(),
            json!("bob")
        );
        assert_eq!(
            read(&t, "value", &request("value", "button")).unwrap(),
            Value::Null
        );
        assert_eq!(
            read(&t, "bounds", &request("bounds", "input")).unwrap(),
            json!({ "x": 10.0, "y": 20.0, "w": 100.0, "h": 30.0 })
        );
        assert_eq!(
            read(&t, "texts", &request("texts", "p")).unwrap(),
            json!(["Hello ", "world"])
        );
    }

    #[test]
    fn filters_and_index() {
        let t = tree();
        let mut r = request("count", "p");
        r.contains = Some("wor".into());
        assert_eq!(read(&t, "count", &r).unwrap(), json!(1));
        r.contains = None;
        r.exact = Some("Hello".into());
        assert_eq!(read(&t, "count", &r).unwrap(), json!(1), "exact trims");
        r.exact = Some("Hell".into());
        assert_eq!(read(&t, "count", &r).unwrap(), json!(0));
        let mut r = request("text", "p");
        r.index = Some(1);
        assert_eq!(read(&t, "text", &r).unwrap(), json!("world"));
        r.index = Some(5);
        assert_eq!(read(&t, "text", &r).unwrap(), Value::Null);
    }

    #[test]
    fn unsupported_selector_is_an_error() {
        let err = read(&tree(), "count", &request("count", "p:first-child")).unwrap_err();
        assert!(
            err.starts_with("unsupported selector: p:first-child"),
            "{err}"
        );
        assert!(read(&tree(), "count", &Request::default()).is_err());
    }

    #[test]
    fn submit_finds_the_submit_control() {
        let t = tree();
        assert_eq!(submit_control(&t, 1), Some(3));
        assert_eq!(submit_control(&t, 3), Some(3));
        assert_eq!(submit_control(&t, 4), None);
        assert_eq!(submit_control(&t, 0), Some(3));
    }

    #[test]
    fn key_names_map_to_gpui() {
        let none: [String; 0] = [];
        for (dom, gpui) in [
            ("Enter", "enter"),
            ("Escape", "escape"),
            ("ArrowDown", "down"),
            ("ArrowUp", "up"),
            ("Backspace", "backspace"),
            (" ", "space"),
            ("a", "a"),
            ("A", "a"),
            ("F5", "f5"),
            ("PageDown", "pagedown"),
        ] {
            assert_eq!(keystroke(dom, &none).unwrap().key, gpui, "{dom}");
        }
        assert!(keystroke("Flibbertigibbet", &none).is_err());
    }

    #[test]
    fn modifiers_are_parsed() {
        let k = keystroke("k", &["Ctrl".into(), "shift".into()]).unwrap();
        assert!(k.modifiers.control && k.modifiers.shift && !k.modifiers.alt);
        assert!(keystroke("k", &["hyper".into()]).is_err());
        assert!(keystroke("k", &["meta".into()]).unwrap().modifiers.platform);
    }

    #[test]
    fn typed_characters() {
        let a = char_keystroke('A');
        assert_eq!(
            (a.key.as_str(), a.key_char.as_deref(), a.modifiers.shift),
            ("a", Some("A"), true)
        );
        let bang = char_keystroke('!');
        assert_eq!(
            (bang.key.as_str(), bang.key_char.as_deref()),
            ("!", Some("!"))
        );
        assert_eq!(char_keystroke('\n').key, "enter");
    }

    #[test]
    fn a_js_request_says_to_port_the_test() {
        let request = Request {
            js: Some("return 1".into()),
            ..Request::default()
        };
        assert!(no_op(&request).contains("porting"));
        assert_eq!(no_op(&Request::default()), "missing op");
    }
}
