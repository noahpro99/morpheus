//! One timeline row drawn: a message (its header and avatar, reply quote,
//! body, reactions), a system line, a day divider, the NEW divider, the
//! start of the room and placeholders. Classes and attributes are the
//! Dioxus app's (`crates/ui/src/components/timeline.rs`) so tests find them.

use std::{cell::Cell, ops::Range, rc::Rc, sync::Arc};

use gpui::{
    AnyElement, App, Bounds, DispatchPhase, Div, Element, ElementId, GlobalElementId,
    InspectorElementId, LayoutId, MouseMoveEvent, FontStyle, FontWeight, Hsla, InteractiveText, Image,
    ObjectFit, SharedString, Stateful, StrikethroughStyle, StyledImage as _, StyledText, TextRun,
    UnderlineStyle, MouseButton, Pixels, Point, Window, div, font, img, prelude::*, px, relative, rgb,
    rgba,
};
use morpheus_core::{Message, MessageKind, Reaction, ThreadSummary};

use crate::hover_underline::underline_on_hover;
use crate::{
    e2e::{self, E2eExt as _},
    view_model::format::{human_duration, human_size},
    view_model::keys::{
        KeyAccess, LOCKED_HINT, LOCKED_TEXT, Undecryptable, WAITING_HINT, undecryptable,
    },
    icons::icon,
    media::avatar,
    view_model::rich::{self, Block, Para},
    view_model::rows::{MessageRow, Row},
    theme,
};

/// What a click on a link does (the timeline hands it on).
pub type LinkHandler = Rc<dyn Fn(&str, &mut App)>;

/// The text colour and slant a block's runs start from.
#[derive(Clone, Copy)]
pub struct Base {
    pub color: u32,
    pub italic: bool,
}

/// Pictures the row needs, if fetched yet.
#[derive(Default)]
pub struct Pictures {
    pub avatar: Option<Arc<Image>>,
    /// An image message's picture, or a video's thumbnail.
    pub media: Option<Arc<Image>>,
    /// Whether this device can read encrypted history: a message it cannot
    /// decrypt is a locked row or a loading shimmer by it.
    pub keys: KeyAccess,
    /// Your nickname for the sender, shown over their name.
    pub nickname: Option<String>,
    /// Jumped to from a search result: glowing for a moment.
    pub highlight: bool,
    /// Its right-click menu is open: stays highlighted (`data-menu-open`).
    pub menu_open: bool,
    /// Its sender's picture is held down: a pixel lower.
    pub avatar_pressed: bool,
}

/// A file's download, as its card shows it.
#[derive(Clone, Debug, PartialEq)]
pub enum Download {
    Downloading,
    Saved(String),
    Failed(String),
}

fn rem(n: f32) -> gpui::Pixels {
    px(n * theme::REM)
}

fn hsla(color: u32) -> Hsla {
    rgb(color).into()
}

/// The text runs of a paragraph: its spans over `base`.
pub fn runs(para: &Para, base: Base) -> Vec<TextRun> {
    runs_hovering(para, base, None)
}

/// `runs`, with the text in `hover` (a link under the pointer) underlined.
fn runs_hovering(para: &Para, base: Base, hover: Option<&Range<usize>>) -> Vec<TextRun> {
    let len = para.text.len();
    let mut edges = vec![0, len];
    for span in &para.spans {
        edges.push(span.range.start.min(len));
        edges.push(span.range.end.min(len));
    }
    edges.sort_unstable();
    edges.dedup();
    edges
        .windows(2)
        .map(|w| {
            let mut style = rich::Style::default();
            for span in para
                .spans
                .iter()
                .filter(|s| s.range.start <= w[0] && w[1] <= s.range.end)
            {
                let s = &span.style;
                style.bold |= s.bold;
                style.italic |= s.italic;
                style.underline |= s.underline;
                style.strike |= s.strike;
                style.code |= s.code;
                style.faint |= s.faint;
                if s.link.is_some() {
                    style.link = s.link.clone();
                }
            }
            let color = if style.link.is_some() {
                theme::link()
            } else if style.faint {
                theme::text_faint()
            } else {
                base.color
            };
            let mut f = font(if style.code { theme::MONO } else { "Inter" });
            if style.bold {
                f.weight = FontWeight::BOLD;
            }
            if style.italic || base.italic {
                f.style = FontStyle::Italic;
            }
            TextRun {
                len: w[1] - w[0],
                font: f,
                color: hsla(color),
                background_color: style.code.then(|| hsla(theme::bg_input())),
                underline: (style.underline || hover.is_some_and(|h| h.start <= w[0] && w[1] <= h.end)).then(|| UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(hsla(color)),
                    wavy: false,
                }),
                strikethrough: style.strike.then(|| StrikethroughStyle {
                    thickness: px(1.0),
                    color: Some(hsla(color)),
                }),
            }
        })
        .collect()
}

/// One paragraph as text; with links, clickable ones, the one under the
/// pointer underlined (gpui shapes text before it knows where the pointer
/// is, so the hover is tracked here and the runs restyled).
fn para_el(para: &Para, key: &str, base: Base, links: &LinkHandler) -> AnyElement {
    let found = para.links();
    if found.is_empty() {
        return StyledText::new(SharedString::from(para.text.clone()))
            .with_runs(runs(para, base))
            .into_any_element();
    }
    let para = para.clone();
    let links = links.clone();
    let key = key.to_owned();
    LinkText {
        id: ElementId::NamedInteger(SharedString::from(key.clone()), 1),
        build: Box::new(move |hovered, cell| {
            let found = para.links();
            let styled = StyledText::new(SharedString::from(para.text.clone())).with_runs(
                runs_hovering(&para, base, hovered.and_then(|n| found.get(n)).map(|(r, _)| r)),
            );
            let layout = styled.layout().clone();
            let ranges: Vec<Range<usize>> = found.iter().map(|(r, _)| r.clone()).collect();
            let urls: Vec<Arc<str>> = found.iter().map(|(_, u)| u.clone()).collect();
            let tags = found
                .iter()
                .map(|(r, u)| (r.clone(), u.to_string()))
                .collect();
            let on_link = links.clone();
            let under = ranges.clone();
            let interactive = InteractiveText::new(
                ElementId::NamedInteger(SharedString::from(key.clone()), 0),
                styled,
            )
            .on_click(ranges, move |ix, _, cx| on_link(&urls[ix], cx))
            .on_hover(move |at, _, _, _| {
                cell.set(at.and_then(|at| under.iter().position(|r| r.contains(&at))));
            });
            e2e::text_links(interactive, &layout, tags, hovered).into_any_element()
        }),
    }
    .into_any_element()
}

/// The paragraph's text and links, rebuilt every frame from the link under
/// the pointer (kept here, in the element's state).
struct LinkText {
    id: ElementId,
    build: Box<dyn Fn(Option<usize>, Rc<Cell<Option<usize>>>) -> AnyElement>,
}

impl IntoElement for LinkText {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

type Hovered = Rc<Cell<Option<usize>>>;

fn hovered_state(id: Option<&GlobalElementId>, window: &mut Window) -> Hovered {
    window.with_optional_element_state::<Hovered, _>(id, |state, _| {
        let cell = state.flatten().unwrap_or_default();
        (cell.clone(), Some(cell))
    })
}

impl Element for LinkText {
    type RequestLayoutState = AnyElement;
    type PrepaintState = Hovered;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, AnyElement) {
        let cell = hovered_state(id, window);
        let mut inner = (self.build)(cell.get(), cell);
        let layout = inner.request_layout(window, cx);
        (layout, inner)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        inner: &mut AnyElement,
        window: &mut Window,
        cx: &mut App,
    ) -> Hovered {
        inner.prepaint(window, cx);
        hovered_state(id, window)
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        inner: &mut AnyElement,
        cell: &mut Hovered,
        window: &mut Window,
        cx: &mut App,
    ) {
        inner.paint(window, cx);
        // The inner text sees only moves over itself: leaving it clears the link.
        let cell = cell.clone();
        let view = window.current_view();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble
                && cell.get().is_some()
                && !bounds.contains(&event.position)
            {
                cell.set(None);
                cx.notify(view);
            }
        });
    }
}

/// Blocks of formatted text: paragraphs, code blocks, quotes, lists, rules.
pub fn blocks_el(blocks: &[Block], key: &str, base: Base, links: &LinkHandler) -> Div {
    let mut column = div().flex().flex_col().gap(px(4.0)).min_w_0();
    for (i, block) in blocks.iter().enumerate() {
        let key = format!("{key}.{i}");
        column = column.child(match block {
            Block::Para(para) => para_el(para, &key, base, links),
            Block::Pre(code) => div()
                .px(px(12.0))
                .py(px(8.0))
                .my(px(4.0))
                .rounded(px(theme::RADIUS_SM))
                .bg(rgb(theme::bg_input()))
                .font_family(theme::MONO)
                .text_size(rem(0.85))
                .text_color(rgb(base.color))
                .child(code.clone())
                .into_any_element(),
            Block::Quote(inner) => div()
                .my(px(4.0))
                .pl(px(12.0))
                .border_l(px(3.0))
                .border_color(rgb(theme::bg_elevated()))
                .child(blocks_el(
                    inner,
                    &key,
                    Base {
                        color: theme::text_muted(),
                        ..base
                    },
                    links,
                ))
                .into_any_element(),
            Block::List {
                ordered,
                start,
                items,
            } => div()
                .flex()
                .flex_col()
                .gap(px(2.0))
                .my(px(4.0))
                .pl(px(8.0))
                .children(items.iter().enumerate().map(|(n, item)| {
                    let marker = if *ordered {
                        format!("{}.", *start as usize + n)
                    } else {
                        "•".to_owned()
                    };
                    div()
                        .flex()
                        .gap(px(6.0))
                        .child(div().flex_none().min_w(px(16.0)).child(marker))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .child(blocks_el(item, &format!("{key}.{n}"), base, links)),
                        )
                }))
                .into_any_element(),
            Block::Rule => div()
                .h(px(1.0))
                .my(px(6.0))
                .bg(rgb(theme::bg_elevated()))
                .into_any_element(),
        });
    }
    column
}

/// The size an image of `width` x `height` is drawn at: its own, fitted
/// into 420 x 360 (`.image`), else a default box.
pub fn image_size(width: Option<u32>, height: Option<u32>) -> (f32, f32) {
    match (width, height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => {
            let (w, h) = (w as f32, h as f32);
            let scale = (420.0 / w).min(360.0 / h).min(1.0);
            ((w * scale).round(), (h * scale).round())
        }
        _ => (320.0, 240.0),
    }
}

fn shimmer_bar(width: f32, key: &str) -> impl IntoElement {
    div()
        .h(px(14.0))
        .my(px(5.0))
        .w(relative(width / 100.0))
        .rounded(px(7.0))
        .bg(rgb(theme::bg_elevated()))
        .id(ElementId::from(SharedString::from(key.to_owned())))
}

/// Placeholder rows: message-shaped, grey, for a room still loading and for
/// the history above the oldest message.
fn skeleton(older: bool) -> AnyElement {
    const LINES: [(f32, f32); 6] = [
        (45.0, 70.0),
        (30.0, 0.0),
        (60.0, 40.0),
        (25.0, 55.0),
        (50.0, 0.0),
        (35.0, 65.0),
    ];
    let rows = LINES.iter().enumerate().map(|(index, (first, second))| {
        let mut lines = div()
            .flex_1()
            .min_w_0()
            .child(shimmer_bar(*first, &format!("sk{older}{index}a")));
        if *second > 0.0 {
            lines = lines.child(shimmer_bar(*second, &format!("sk{older}{index}b")));
        }
        div()
            .pt(px(12.0))
            .child(
                div()
                    .flex()
                    .gap(px(12.0))
                    .px(px(16.0))
                    .py(px(2.0))
                    .child(
                        div()
                            .size(px(40.0))
                            .mt(px(2.0))
                            .flex_none()
                            .rounded_full()
                            .bg(rgb(theme::bg_elevated()))
                            .e2e("div", "avatar skeleton"),
                    )
                    .child(lines.e2e("div", "message-main"))
                    .e2e("div", "message with-header skeleton-row"),
            )
            .into_any_element()
    });
    let column = div().flex().flex_col().children(rows);
    if older {
        column.e2e("div", "older-skeleton").into_any_element()
    } else {
        column.into_any_element()
    }
}

fn reactions_el(
    reactions: &[Reaction],
    unique_id: &str,
    actions: Option<&ActionHandler>,
) -> impl IntoElement {
    let unique_id = unique_id.to_owned();
    let actions = actions.cloned();
    div()
        .flex()
        .flex_wrap()
        .gap(px(4.0))
        .mt(px(4.0))
        .children(reactions.iter().map(|reaction| {
            let key = reaction.key.clone();
            let (actions, unique_id) = (actions.clone(), unique_id.clone());
            let mut chip = div()
                .id(ElementId::from(SharedString::from(format!("reaction-{unique_id}-{key}"))))
                .flex()
                .items_center()
                .gap(px(4.0))
                .px(px(8.0))
                .py(px(2.0))
                .rounded(px(10.0))
                .border_1()
                .text_size(rem(0.85))
                .cursor_pointer()
                .when_some(actions, |chip, actions| {
                    chip.on_click(move |_, window, cx| {
                        actions(RowAction::Toggle(key.clone()), &unique_id, window, cx)
                    })
                });
            chip = if reaction.mine {
                chip.border_color(rgb(theme::accent()))
                    .bg(rgba(theme::accent_tint()))
                    .text_color(rgb(theme::text()))
            } else {
                chip.border_color(gpui::transparent_black())
                    .bg(rgb(theme::bg_elevated()))
                    .text_color(rgb(theme::text_muted()))
            };
            let count = reaction.count.to_string();
            chip.child(
                div()
                    .child(reaction.key.clone())
                    .e2e("span", "reaction-key")
                    .text(reaction.key.clone()),
            )
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(count.clone())
                    .e2e("span", "reaction-count")
                    .text(count),
            )
            .e2e(
                "button",
                if reaction.mine {
                    "reaction mine"
                } else {
                    "reaction"
                },
            )
        }))
        .e2e("div", "reactions")
}

fn thread_chip(
    thread: &ThreadSummary,
    unique_id: &str,
    actions: Option<&ActionHandler>,
) -> impl IntoElement {
    let count = crate::view_model::threads::chip_count(thread);
    let mut chip = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .mt(px(6.0))
        .px(px(10.0))
        .py(px(4.0))
        .rounded(px(10.0))
        .bg(rgb(theme::bg_elevated()))
        .text_color(rgb(theme::text_muted()))
        .text_size(rem(0.82))
        .cursor_pointer()
        .child(
            div()
                .text_color(rgb(theme::text_muted()))
                .child(icon("messages-square", 14.0))
                .e2e("span", "thread-chip-icon"),
        )
        .child(
            div()
                .text_color(rgb(theme::accent()))
                .font_weight(FontWeight::SEMIBOLD)
                .child(count.clone())
                .e2e("span", "thread-chip-count")
                .text(count),
        );
    if let Some(text) = crate::view_model::threads::chip_latest(thread) {
        chip = chip.child(
            div()
                .max_w(px(160.0))
                .truncate()
                .child(text.clone())
                .e2e("span", "thread-chip-latest")
                .text(text),
        );
    }
    let mut chip = chip
        .id(ElementId::from(SharedString::from(format!("thread-chip-{unique_id}"))))
        .hover(|s| s.bg(rgba(theme::bg_hover())).text_color(rgb(theme::text())));
    if let Some(actions) = actions {
        let (actions, unique_id) = (actions.clone(), unique_id.to_owned());
        chip = chip.on_click(move |_, window, cx| {
            actions(RowAction::Thread, &unique_id, window, cx)
        });
    }
    chip.w_auto().e2e("button", "thread-chip")
}

fn reply_quote(reply: &morpheus_core::ReplyPreview) -> impl IntoElement {
    let name = reply.sender_name.clone().unwrap_or_else(|| "…".into());
    let mut quote = div()
        .flex()
        .gap(px(6.0))
        .mb(px(2.0))
        .pl(px(8.0))
        .border_l(px(2.0))
        .border_color(rgb(theme::text_faint()))
        .text_size(rem(0.8))
        .text_color(rgb(theme::text_muted()))
        .overflow_hidden()
        .child(div().flex_none().child("↩").e2e("span", "").text("↩"))
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(theme::text()))
                .child(name.clone())
                .e2e("b", "")
                .text(name),
        );
    quote = match &reply.summary {
        Some(summary) => quote.child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(summary.clone())
                .e2e("span", "")
                .text(summary.clone()),
        ),
        None => quote.child(
            div()
                .w(px(120.0))
                .h(px(10.0))
                .mt(px(3.0))
                .rounded(px(5.0))
                .bg(rgb(theme::bg_elevated()))
                .e2e("span", "skeleton skeleton-inline"),
        ),
    };
    quote.e2e("div", "reply-quote").attr("title", reply.event_id.to_string())
}

/// What a card's Download button does: ask the timeline for this message's file.
fn start_download(actions: Option<&ActionHandler>, key: &str) -> Option<(ActionHandler, String)> {
    actions.cloned().map(|actions| (actions, key.to_owned()))
}

/// A file or audio card: icon, name, size, and its Download button.
fn attachment_card(
    glyph: &str,
    audio: bool,
    name: &str,
    meta: &str,
    download: Option<&Download>,
    start: Option<(ActionHandler, String)>,
) -> impl IntoElement {
    let mut info = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(
            div()
                .truncate()
                .child(name.to_owned())
                .e2e("span", "attachment-name")
                .attr("title", name)
                .text(name),
        );
    if !meta.is_empty() {
        info = info.child(
            div()
                .text_size(rem(0.8))
                .text_color(rgb(theme::text_muted()))
                .child(meta.to_owned())
                .e2e("span", "attachment-size muted")
                .text(meta),
        );
    }
    let status = |text: String, error: bool, title: Option<String>| {
        let mut tagged = div()
            .flex_none()
            .max_w(px(160.0))
            .truncate()
            .text_size(rem(0.8))
            .text_color(rgb(if error {
                theme::danger()
            } else {
                theme::text_muted()
            }))
            .child(text.clone())
            .e2e(
                "span",
                if error {
                    "attachment-status error"
                } else {
                    "attachment-status muted"
                },
            )
            .text(text);
        if let Some(title) = title {
            tagged = tagged.attr("title", title);
        }
        tagged.into_any_element()
    };
    let action = match download {
        None => div()
            .id(SharedString::from(format!("download-{name}")))
            .flex_none()
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(theme::RADIUS_SM))
            .bg(rgb(theme::bg_input()))
            .text_size(rem(0.85))
            .text_color(rgb(theme::text_muted()))
            .cursor_pointer()
            .child("Download")
            .when_some(start, |d, (actions, key)| {
                d.on_click(move |_, window, cx| actions(RowAction::Download, &key, window, cx))
            })
            .e2e("button", "ghost attachment-download")
            .text("Download")
            .into_any_element(),
        Some(Download::Downloading) => status("Downloading…".into(), false, None),
        Some(Download::Saved(path)) => {
            status(format!("Saved to {path}"), false, Some(path.clone()))
        }
        Some(Download::Failed(message)) => status(message.clone(), true, None),
    };
    div()
        .flex()
        .items_center()
        .gap(px(10.0))
        .mt(px(4.0))
        .px(px(12.0))
        .py(px(10.0))
        .max_w(px(360.0))
        .rounded(px(theme::RADIUS))
        .bg(rgb(theme::bg_elevated()))
        .child(
            div()
                .flex_none()
                .text_size(rem(1.4))
                .child(glyph.to_owned())
                .e2e("span", "attachment-icon")
                .text(glyph),
        )
        .child(info.e2e("div", "attachment-info"))
        .child(action)
        .e2e(
            "div",
            if audio {
                "attachment-card audio"
            } else {
                "attachment-card"
            },
        )
}

/// A blurred stand-in for a picture, drawn from its BlurHash (decoded once
/// per hash): shown behind the picture, and alone until it has loaded.
fn blur_image(hash: &str) -> Option<Arc<Image>> {
    use std::{collections::HashMap, sync::Mutex};
    static CACHE: Mutex<Option<HashMap<String, Option<Arc<Image>>>>> = Mutex::new(None);
    let mut cache = CACHE.lock().ok()?;
    cache
        .get_or_insert_with(HashMap::new)
        .entry(hash.to_owned())
        .or_insert_with(|| {
            use base64::Engine as _;
            let url = morpheus_core::blur_placeholder(hash)?;
            let data = url.strip_prefix("data:image/png;base64,")?;
            let bytes = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
            Some(Arc::new(Image::from_bytes(gpui::ImageFormat::Png, bytes)))
        })
        .clone()
}

/// An empty box `w` x `h` wide and tall when there is room, and that shape,
/// scaled down, when there is less: a picture's box, whose height follows its
/// width. (Percentage widths with an aspect ratio are measured differently
/// for a row's height than for drawing it, which left a gap under a shrunk
/// picture; a measured box answers both the same.)
struct Shape {
    w: f32,
    h: f32,
}

impl IntoElement for Shape {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for Shape {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        _cx: &mut App,
    ) -> (gpui::LayoutId, ()) {
        let (w, h) = (self.w, self.h);
        let id = window.request_measured_layout(gpui::Style::default(), move |known, available, _, _| {
            let width = match (known.width, available.width) {
                (Some(width), _) => width,
                (None, gpui::AvailableSpace::Definite(room)) => px(w).min(room),
                (None, gpui::AvailableSpace::MaxContent) => px(w),
                (None, gpui::AvailableSpace::MinContent) => px(0.0),
            };
            let height = known.height.unwrap_or(width * (h / w));
            gpui::size(width, height)
        });
        (id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: gpui::Bounds<Pixels>,
        _state: &mut (),
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _id: Option<&gpui::GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        _bounds: gpui::Bounds<Pixels>,
        _state: &mut (),
        _prepaint: &mut (),
        _window: &mut Window,
        _cx: &mut App,
    ) {
    }
}

fn image_el(
    body: &str,
    width: Option<u32>,
    height: Option<u32>,
    blurhash: Option<&str>,
    picture: Option<Arc<Image>>,
    open: Option<(ActionHandler, String)>,
) -> AnyElement {
    let (w, h) = image_size(width, height);
    let blur = blurhash.and_then(blur_image);
    let noted = blur.is_some();
    let base = div()
        .id(SharedString::from(format!("image-{body}")))
        .relative()
        .mt(px(4.0))
        .min_w_0()
        .rounded(px(theme::RADIUS))
        .overflow_hidden()
        .bg(rgb(theme::bg_input()))
        .child(Shape { w, h })
        .children(blur.map(|blur| {
            img(blur)
                .absolute()
                .size_full()
                .rounded(px(theme::RADIUS))
                .object_fit(ObjectFit::Fill)
        }));
    let mut element = match picture {
        Some(image) => base
            .cursor_pointer()
            .when_some(open, |el, (actions, key)| {
                el.on_click(move |_, window, cx| actions(RowAction::OpenImage, &key, window, cx))
            })
            .child(
                img(image)
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .rounded(px(theme::RADIUS))
                    .object_fit(ObjectFit::Contain),
            )
            .e2e("img", "image")
            .attr("data-fit", "contain")
            .attr("alt", body)
            .attr("title", body),
        None => base
            .text_color(rgb(theme::text_faint()))
            .text_size(rem(0.85))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(body.to_owned()),
            )
            .e2e("div", "image placeholder")
            .attr("alt", body)
            .text(body),
    };
    if noted {
        element = element.attr("data-blur", "1");
    }
    div().flex().child(element).into_any_element()
}

fn video_el(
    body: &str,
    width: Option<u32>,
    height: Option<u32>,
    duration: Option<std::time::Duration>,
    thumbnail: Option<Arc<Image>>,
) -> AnyElement {
    let (w, h) = image_size(width, height);
    let mut card = div()
        .relative()
        .mt(px(4.0))
        .min_w_0()
        .rounded(px(theme::RADIUS))
        .overflow_hidden()
        .bg(rgb(theme::bg_input()))
        .child(Shape { w, h });
    card = match thumbnail {
        Some(image) => card.child(
            img(crate::media::covering(&image, w, h))
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .rounded(px(theme::RADIUS))
                .object_fit(ObjectFit::Fill)
                .e2e("img", "video-thumb")
                .attr("alt", body)
                .attr("title", body),
        ),
        None => card.child(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(theme::text_faint()))
                .text_size(rem(0.85))
                .child(body.to_owned())
                .e2e("div", "image placeholder")
                .text(body),
        ),
    };
    let mut overlay = div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(rgba(0x00000040))
        .child(
            div()
                .size(px(48.0))
                .rounded_full()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(theme::alpha(theme::bg_sidebar(), 0xbf)))
                .text_size(rem(1.2))
                .cursor_pointer()
                .child("▶")
                .e2e("button", "play-button")
                .attr("title", "Play")
                .text("▶"),
        );
    if let Some(duration) = duration {
        let label = human_duration(duration);
        overlay = overlay.child(
            div()
                .absolute()
                .bottom(px(8.0))
                .right(px(10.0))
                .px(px(6.0))
                .py(px(2.0))
                .rounded(px(4.0))
                .bg(rgba(0x00000099))
                .text_size(rem(0.75))
                .child(label.clone())
                .e2e("span", "video-duration")
                .text(label),
        );
    }
    div()
        .flex()
        .child(card.child(overlay.e2e("div", "video-overlay")).e2e("div", "video-card"))
        .into_any_element()
}

/// The body of a message, by what it is.
fn body_el(
    row: &MessageRow,
    key: &str,
    pics: &Pictures,
    download: Option<&Download>,
    links: &LinkHandler,
    actions: Option<&ActionHandler>,
) -> AnyElement {
    let (class, base, rich_html) = match &row.message.kind {
        MessageKind::Text { html, .. } => ("body", Base { color: theme::text(), italic: false }, html.is_some()),
        MessageKind::Notice { html, .. } => (
            "body notice",
            Base { color: theme::text_muted(), italic: false },
            html.is_some(),
        ),
        MessageKind::Emote { html, .. } => (
            "body emote",
            Base { color: theme::text(), italic: true },
            html.is_some(),
        ),
        MessageKind::Image {
            body,
            width,
            height,
            blurhash,
            ..
        } => {
            let open = actions.cloned().map(|actions| (actions, key.to_owned()));
            return image_el(body, *width, *height, blurhash.as_deref(), pics.media.clone(), open);
        }
        MessageKind::File { body, size, .. } => {
            let meta = size.map(human_size).unwrap_or_default();
            return attachment_card("📎", false, body, &meta, download, start_download(actions, key)).into_any_element();
        }
        MessageKind::Audio {
            body,
            size,
            duration,
            ..
        } => {
            let meta = [duration.map(human_duration), size.map(human_size)]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" · ");
            return attachment_card("🎵", true, body, &meta, download, start_download(actions, key)).into_any_element();
        }
        MessageKind::Video {
            body,
            width,
            height,
            duration,
            ..
        } => return video_el(body, *width, *height, *duration, pics.media.clone()),
        MessageKind::Redacted => {
            return muted_body("Message deleted");
        }
        // On a device that can read encrypted history, keys usually arrive
        // shortly: a loading shimmer. On one that cannot, they never will
        // until the user acts, so say so instead of looking like loading.
        MessageKind::UnableToDecrypt => match undecryptable(pics.keys) {
            Undecryptable::Locked => {
                return div()
                    .flex()
                    .items_center()
                    .gap(px(6.0))
                    .italic()
                    .text_color(rgb(theme::text_muted()))
                    .child(icon("lock", 14.0).text_color(rgb(theme::text_muted())))
                    .child(LOCKED_TEXT)
                    .e2e("p", "body muted undecrypted locked")
                    .attr("title", LOCKED_HINT)
                    .text(LOCKED_TEXT)
                    .into_any_element();
            }
            Undecryptable::Waiting => {
                return div()
                    .w(relative(0.55))
                    .max_w(px(420.0))
                    .min_w(px(120.0))
                    .h(px(14.0))
                    .my(px(5.0))
                    .rounded(px(7.0))
                    .bg(rgb(theme::bg_elevated()))
                    .e2e("div", "body skeleton skeleton-line undecrypted")
                    .attr("title", WAITING_HINT)
                    .attr("aria-label", "Encrypted message, waiting for keys")
                    .into_any_element();
            }
        },
        MessageKind::State(text) => return muted_body(text),
        MessageKind::Unsupported(what) => return muted_body(&format!("Unsupported: {what}")),
        MessageKind::ProfileChange | MessageKind::CallPlumbing => return div().into_any_element(),
    };
    let text = rich::text_of(&row.blocks);
    blocks_el(&row.blocks, key, base, links)
        .e2e(
            if rich_html { "div" } else { "p" },
            &format!("{class}{}", if rich_html { " rich" } else { "" }),
        )
        .text(text)
        .into_any_element()
}

fn muted_body(text: &str) -> AnyElement {
    div()
        .text_color(rgb(theme::text_muted()))
        .child(text.to_owned())
        .e2e("p", "body muted")
        .text(text)
        .into_any_element()
}

/// What a message's hover bar does.
#[derive(Clone, Debug, PartialEq)]
pub enum RowAction {
    Reply,
    /// "Reply in thread", or a thread chip: open the thread panel.
    Thread,
    Edit,
    React(&'static str),
    /// The "more reactions" button, where it is (the picker opens there).
    MoreReactions(Point<Pixels>),
    /// The bin; true when Shift was held: no question asked.
    Delete(bool),
    /// A right click on the message, where.
    Context(Point<Pixels>),
    /// Their picture or name clicked, where: their profile.
    Sender(Point<Pixels>),
    /// Their picture or name right-clicked, where: their member menu.
    SenderMenu(Point<Pixels>),
    /// Their picture held down (true) or let go (false).
    AvatarPress(bool),
    /// A picture clicked: opens it full size.
    OpenImage,
    /// A reaction chip clicked: adds our reaction, or takes it back.
    Toggle(String),
    /// A file card's Download button.
    Download,
}

/// What a click on a hover action does: the action, the message's unique ID.
pub type ActionHandler = Rc<dyn Fn(RowAction, &str, &mut Window, &mut App)>;

use crate::view_model::menu::QUICK_REACTIONS;

/// One button of the hover bar.
fn action_button(
    id: SharedString,
    classes: &str,
    title: &str,
    glyph: AnyElement,
    action: RowAction,
    unique_id: &str,
    actions: &ActionHandler,
) -> AnyElement {
    let (actions, unique_id) = (actions.clone(), unique_id.to_owned());
    div()
        .id(ElementId::from(id))
        .flex()
        .items_center()
        .justify_center()
        .p(px(6.0))
        .rounded(px(4.0))
        .text_size(rem(0.95))
        .text_color(rgb(theme::text_muted()))
        .cursor_pointer()
        .hover(|s| s.bg(rgba(theme::bg_hover())).text_color(rgb(theme::text())))
        .child(glyph)
        .on_click(move |event, window, cx| {
            // Where it was clicked (the picker opens there) and whether
            // Shift was held (delete without asking).
            let action = match &action {
                RowAction::MoreReactions(_) => RowAction::MoreReactions(event.position()),
                RowAction::Delete(_) => RowAction::Delete(event.modifiers().shift),
                other => other.clone(),
            };
            actions(action, &unique_id, window, cx)
        })
        .e2e("button", classes)
        .attr("title", title)
        .into_any_element()
}

/// The bar that shows on hover, top right of a message: quick reactions,
/// reply, and edit on your own text.
fn actions_bar(m: &Message, group: SharedString, actions: &ActionHandler) -> AnyElement {
    let mut bar = div()
        .absolute()
        .top(px(-14.0))
        .right(px(16.0))
        .flex()
        .gap(px(2.0))
        .p(px(2.0))
        .rounded(px(theme::RADIUS_SM))
        .bg(rgb(theme::bg_elevated()))
        .shadow_md()
        .opacity(0.0)
        .group_hover(group, |s| s.opacity(1.0));
    let uid = m.unique_id.as_str();
    for key in QUICK_REACTIONS {
        bar = bar.child(action_button(
            format!("react-{uid}-{key}").into(),
            "action quick-reaction",
            &format!("React with {key}"),
            div().child(key).into_any_element(),
            RowAction::React(key),
            uid,
            actions,
        ));
    }
    // Every other emoji: the full picker, beside this button.
    bar = bar.child(action_button(
        format!("more-{uid}").into(),
        "action more-reactions",
        "Add reaction",
        icon("smile-plus", 15.0).into_any_element(),
        RowAction::MoreReactions(Point::default()),
        uid,
        actions,
    ));
    let can_reply = matches!(m.id, morpheus_core::MessageId::Event(_));
    if can_reply {
        bar = bar.child(action_button(
            format!("reply-{uid}").into(),
            "action reply",
            "Reply",
            icon("reply", 15.0).into_any_element(),
            RowAction::Reply,
            uid,
            actions,
        ));
        bar = bar.child(action_button(
            format!("thread-{uid}").into(),
            "action thread",
            "Reply in thread",
            icon("messages-square", 15.0).into_any_element(),
            RowAction::Thread,
            uid,
            actions,
        ));
    }
    if m.can_edit {
        bar = bar.child(action_button(
            format!("edit-{uid}").into(),
            "action edit",
            "Edit",
            icon("pencil", 15.0).into_any_element(),
            RowAction::Edit,
            uid,
            actions,
        ));
    }
    if m.is_own && can_reply {
        bar = bar.child(action_button(
            format!("delete-{uid}").into(),
            "action delete",
            "Delete",
            icon("trash-2", 15.0).into_any_element(),
            RowAction::Delete(false),
            uid,
            actions,
        ));
    }
    bar.e2e("div", "actions").into_any_element()
}

/// Who sent a message (their picture or name): a click opens their
/// profile, a right click their member menu (not the message's).
fn sender_target(
    el: Div,
    id: String,
    unique_id: &str,
    actions: Option<&ActionHandler>,
) -> Stateful<Div> {
    let mut el = el.id(ElementId::from(SharedString::from(id))).cursor_pointer();
    if let Some(actions) = actions {
        let (open, uid) = (actions.clone(), unique_id.to_owned());
        el = el.on_click(move |event, window, cx| {
            open(RowAction::Sender(event.position()), &uid, window, cx)
        });
        let (menu, uid) = (actions.clone(), unique_id.to_owned());
        el = el.on_mouse_down(MouseButton::Right, move |event, window, cx| {
            cx.stop_propagation();
            menu(RowAction::SenderMenu(event.position), &uid, window, cx)
        });
    }
    el
}

/// A sender's picture as a button (as tall as the picture, not the row: a
/// stretched button made the shadow a tall pill hanging below the circle): a
/// small even shadow, no y offset, that grows under the
/// pointer, and a pixel lower while held down (the timeline keeps the held
/// state, `pressed`, as layout is not restyled by the pointer in gpui).
fn avatar_button(
    el: Stateful<Div>,
    pressed: bool,
    unique_id: &str,
    actions: Option<&ActionHandler>,
) -> Stateful<Div> {
    let shadow = |alpha: f32, y: f32, blur: f32| {
        vec![gpui::BoxShadow {
            color: gpui::hsla(0.0, 0.0, 0.0, alpha),
            offset: gpui::point(px(0.0), px(y)),
            blur_radius: px(blur),
            spread_radius: px(0.0),
        }]
    };
    let mut el = el
        .relative()
        .top(if pressed { px(1.0) } else { px(0.0) })
        .rounded_full()
        .flex_none()
        .shadow(shadow(0.35, 0.0, 4.0))
        .hover(move |s| s.shadow(shadow(0.6, 0.0, 10.0)));
    if let Some(actions) = actions {
        for press in [true, false] {
            let (handler, uid) = (actions.clone(), unique_id.to_owned());
            el = if press {
                el.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    handler(RowAction::AvatarPress(true), &uid, window, cx)
                })
            } else {
                el.on_mouse_up(MouseButton::Left, move |_, window, cx| {
                    handler(RowAction::AvatarPress(false), &uid, window, cx)
                })
            };
        }
        let (handler, uid) = (actions.clone(), unique_id.to_owned());
        el = el.on_mouse_up_out(MouseButton::Left, move |_, window, cx| {
            handler(RowAction::AvatarPress(false), &uid, window, cx)
        });
    }
    el
}

/// A message's row. `show_header`: its sender's avatar, name and time lead it.
pub fn message_el(
    row: &MessageRow,
    show_header: bool,
    pics: &Pictures,
    download: Option<&Download>,
    links: &LinkHandler,
    actions: Option<&ActionHandler>,
) -> AnyElement {
    let m = &row.message;
    let event_id = match &m.id {
        morpheus_core::MessageId::Event(id) => id.to_string(),
        morpheus_core::MessageId::Transaction(_) => String::new(),
    };
    let key = SharedString::from(format!("msg-{}", m.unique_id));

    // A membership or state change: a quiet line.
    if let MessageKind::State(text) = &m.kind {
        use crate::view_model::state_lines::{self, Tone};
        let look = state_lines::icon(m.state_kind);
        let tone = match look.tone {
            Tone::Good => theme::ok(),
            Tone::Bad => theme::danger(),
            Tone::Quiet => theme::text_faint(),
        };
        let class = state_lines::StateIcon::class(m.state_kind);
        // As a message: the icon in the avatar's column, the text where a
        // message's text starts, the time after it.
        return div()
            .id(ElementId::from(key))
            .w_full()
            .flex()
            .items_center()
            .gap(px(12.0))
            .hover(|s| s.bg(rgba(theme::bg_hover())))
            .py(px(2.0))
            .px(px(16.0))
            .text_size(rem(0.85))
            .text_color(rgb(theme::text_muted()))
            .child(
                div()
                    .w(px(40.0))
                    .flex_none()
                    .flex()
                    .justify_center()
                    .child(icon(look.glyph, 18.0).text_color(rgb(tone)))
                    .e2e("span", &format!("state-icon {class}"))
                    .attr("data-icon", look.glyph)
                    .attr(
                        "data-tone",
                        match look.tone {
                            Tone::Good => "good",
                            Tone::Bad => "bad",
                            Tone::Quiet => "quiet",
                        },
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .gap(px(8.0))
                    .min_w_0()
                    .child(
                        div()
                            .child(text.clone())
                            .e2e("span", "system-text")
                            .text(text.clone()),
                    )
                    .child(
                        div()
                            .text_size(rem(0.75))
                            .text_color(rgb(theme::text_faint()))
                            .child(row.time.clone())
                            .e2e("span", "time")
                            .text(row.time.clone()),
                    ),
            )
            .e2e("div", "message system")
            .attr("data-event-id", event_id)
            .into_any_element();
    }

    let mut main = div().flex_1().min_w_0().flex().flex_col();
    if show_header {
        let name = pics.nickname.clone().unwrap_or_else(|| m.sender_name.clone());
        main = main.child(
            div()
                .flex()
                .items_baseline()
                .gap(px(8.0))
                .child(
                    sender_target(
                        // gpui shapes text before the pointer is known, so a
                        // hover underline is a 1 px line drawn under the name
                        // (Dioxus: `button.sender:hover { underline }`).
                        underline_on_hover(
                            div()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(if m.is_own { theme::own() } else { theme::text() }))
                                .child(name.clone()),
                            SharedString::from(format!("sender-{}", m.unique_id)),
                            if m.is_own { theme::own() } else { theme::text() },
                            3.0,
                        ),
                        format!("sender-{}", m.unique_id),
                        &m.unique_id,
                        actions,
                    )
                        .e2e("button", "sender")
                        .attr("title", m.sender.to_string())
                        .text(name),
                )
                .child(
                    div()
                        .text_size(rem(0.75))
                        .text_color(rgb(theme::text_faint()))
                        .child(row.time.clone())
                        .e2e("span", "time")
                        .text(row.time.clone()),
                )
                .e2e("div", "message-meta"),
        );
    }
    // The shield sits at the right of the row (Dioxus floats it right), out of
    // the flow so a message with one is no taller; the text keeps clear of it.
    let mut shield_el = None;
    if let Some(shield) = &m.shield {
        main = main.pr(px(26.0));
        shield_el = Some(
            div()
                .absolute()
                .top(px(6.0))
                .right(px(16.0))
                .text_color(rgb(if shield.red {
                    theme::danger()
                } else {
                    theme::text_faint()
                }))
                .child(icon("shield", 14.0))
                .e2e(
                    "span",
                    if shield.red {
                        "msg-shield red"
                    } else {
                        "msg-shield grey"
                    },
                )
                .attr("title", shield.reason.clone())
                .attr("aria-label", shield.reason.clone()),
        );
    }
    if let Some(reply) = &m.in_reply_to {
        main = main.child(reply_quote(reply));
    }
    main = main.child(body_el(row, &m.unique_id, pics, download, links, actions));
    if !m.reactions.is_empty() {
        main = main.child(reactions_el(&m.reactions, &m.unique_id, actions));
    }
    if let Some(thread) = &m.thread {
        main = main.child(thread_chip(thread, &m.unique_id, actions));
    }

    let group = SharedString::from("message");
    let mut line = div()
        .id(ElementId::from(key))
        .group(group.clone())
        .relative()
        .w_full()
        .flex()
        .items_start()
        .gap(px(12.0))
        .px(px(16.0))
        .py(px(2.0))
        .hover(|s| s.bg(rgba(theme::bg_hover())));
    if m.is_local_echo {
        line = line.opacity(0.55);
    }
    line = if show_header {
        line.child(
            avatar_button(
                sender_target(
                    div()
                        .mt(px(2.0))
                        .child(avatar(pics.avatar.clone(), &m.sender_name, 40.0, None)),
                    format!("avatar-{}", m.unique_id),
                    &m.unique_id,
                    actions,
                ),
                pics.avatar_pressed,
                &m.unique_id,
                actions,
            )
                .e2e("button", "avatar-button")
                .attr("title", m.sender.to_string()),
        )
    } else {
        line.child(
            div()
                .w(px(40.0))
                .flex_none()
                .flex()
                .justify_end()
                .text_size(rem(0.75))
                .text_color(rgb(theme::text_faint()))
                .opacity(0.0)
                .group_hover(group.clone(), |s| s.opacity(1.0))
                .child(row.time.clone())
                .e2e("span", "time gutter")
                .text(row.time.clone()),
        )
    };
    let mut classes = String::from("message");
    if show_header {
        classes.push_str(" with-header");
    }
    if m.is_local_echo {
        classes.push_str(" pending");
    }
    if m.is_own {
        classes.push_str(" own");
    }
    if pics.highlight {
        classes.push_str(" jump-target");
        line = line.bg(rgba(theme::alpha(theme::accent(), 0x2e)));
    }
    let mut line = line.child(main.e2e("div", "message-main"));
    if let Some(shield) = shield_el {
        line = line.child(shield);
    }
    if let Some(actions) = actions {
        // Right click: the message menu.
        let (menu, unique_id) = (actions.clone(), m.unique_id.clone());
        line = line.on_mouse_down(MouseButton::Right, move |event, window, cx| {
            menu(RowAction::Context(event.position), &unique_id, window, cx)
        });
        if !m.is_local_echo {
            line = line.child(actions_bar(m, group, actions));
        }
    }
    let mut line = line;
    if pics.menu_open {
        line = line.bg(rgba(theme::bg_hover()));
    }
    let mut line = line
        .e2e("div", &classes)
        .attr("data-event-id", event_id);
    if pics.menu_open {
        line = line.attr("data-menu-open", "");
    }
    if show_header {
        div().w_full().pt(px(12.0)).child(line).into_any_element()
    } else {
        line.into_any_element()
    }
}

/// A row that is not a message.
pub fn other_row(row: &Row) -> AnyElement {
    match row {
        Row::Date(label, _) => div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(12.0))
            .mt(px(16.0))
            .mb(px(4.0))
            .px(px(16.0))
            .text_color(rgb(theme::text_faint()))
            .text_size(rem(0.75))
            .font_weight(FontWeight::SEMIBOLD)
            .child(div().flex_1().h(px(1.0)).bg(rgb(theme::bg_elevated())))
            .child(div().child(label.clone()).e2e("span", "").text(label.clone()))
            .child(div().flex_1().h(px(1.0)).bg(rgb(theme::bg_elevated())))
            .e2e("div", "date-divider")
            .into_any_element(),
        Row::Start => div()
            .p(px(16.0))
            .flex()
            .justify_center()
            .text_color(rgb(theme::text_faint()))
            .text_size(rem(0.85))
            .child("This is the beginning of the room.")
            .e2e("div", "timeline-start")
            .text("This is the beginning of the room.")
            .into_any_element(),
        Row::New => div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .mt(px(10.0))
            .mb(px(2.0))
            .mx(px(16.0))
            .text_color(rgb(theme::danger()))
            .text_size(rem(0.7))
            .font_weight(FontWeight::BOLD)
            .child(div().flex_1().h(px(1.0)).bg(rgb(theme::danger())))
            .child(
                div()
                    .px(px(6.0))
                    .rounded(px(4.0))
                    .bg(rgb(theme::danger()))
                    .text_color(rgb(0xffffff))
                    .child("NEW")
                    .e2e("span", "")
                    .text("NEW"),
            )
            .child(div().flex_1().h(px(1.0)).bg(rgb(theme::danger())))
            .e2e("div", "new-divider")
            .into_any_element(),
        Row::Skeleton { older } => skeleton(*older),
        Row::Message { .. } => div().into_any_element(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::view_model::rich::{Span, Style};

    fn base() -> Base {
        Base {
            color: theme::text(),
            italic: false,
        }
    }

    #[test]
    fn runs_cover_the_text_exactly_and_style_the_spans() {
        let mut para = Para::default();
        para.push("a ", Style::default());
        para.push(
            "bold",
            Style {
                bold: true,
                ..Style::default()
            },
        );
        para.push(" link", Style::default());
        para.spans.push(Span {
            range: 7..11,
            style: Style {
                link: Some("https://x.y".into()),
                ..Style::default()
            },
        });
        let runs = runs(&para, base());
        assert_eq!(runs.iter().map(|r| r.len).sum::<usize>(), para.text.len());
        assert_eq!(runs.iter().map(|r| r.len).collect::<Vec<_>>(), [2, 4, 1, 4]);
        assert_eq!(runs[0].font.weight, FontWeight::NORMAL);
        assert_eq!(runs[1].font.weight, FontWeight::BOLD);
        assert_eq!(runs[3].color, hsla(theme::link()));
        assert_eq!(runs[0].color, hsla(theme::text()));
    }

    #[test]
    fn code_changes_the_font_and_strike_draws_a_line() {
        let mut para = Para::default();
        para.push(
            "let",
            Style {
                code: true,
                strike: true,
                ..Style::default()
            },
        );
        let runs = runs(&para, base());
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].font.family.as_ref(), theme::MONO);
        assert!(runs[0].strikethrough.is_some());
        assert!(runs[0].background_color.is_some());
    }

    #[test]
    fn an_empty_paragraph_has_no_runs_to_trip_on() {
        assert!(runs(&Para::default(), base()).is_empty());
    }

    #[test]
    fn pictures_fit_in_420_by_360() {
        assert_eq!(image_size(Some(64), Some(48)), (64.0, 48.0));
        assert_eq!(image_size(Some(1680), Some(720)), (420.0, 180.0));
        assert_eq!(image_size(Some(400), Some(1000)), (144.0, 360.0));
        assert_eq!(image_size(None, Some(10)), (320.0, 240.0));
    }
}
