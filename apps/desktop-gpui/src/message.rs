//! One timeline row drawn: a message (its header and avatar, reply quote,
//! body, reactions), a system line, a day divider, the NEW divider, the
//! start of the room and placeholders. Classes and attributes are the
//! Dioxus app's (`crates/ui/src/components/timeline.rs`) so tests find them.

use std::{ops::Range, rc::Rc, sync::Arc};

use gpui::{
    AnyElement, App, Div, ElementId, FontStyle, FontWeight, Hsla, InteractiveText, Image,
    ObjectFit, SharedString, StrikethroughStyle, StyledImage as _, StyledText, TextRun,
    UnderlineStyle, div, font, img, prelude::*, px, relative, rgb, rgba,
};
use morpheus_core::{MessageKind, Reaction, ThreadSummary};

use crate::{
    e2e::{self, E2eExt as _},
    view_model::format::{human_duration, human_size},
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
}

/// A file's download, as its card shows it.
#[derive(Clone, Debug, PartialEq)]
#[allow(dead_code)] // the card shows them; starting a download is not ported yet
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
                theme::LINK
            } else if style.faint {
                theme::TEXT_FAINT
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
                background_color: style.code.then(|| hsla(theme::BG_INPUT)),
                underline: style.underline.then(|| UnderlineStyle {
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

/// One paragraph as text; with links, clickable ones.
fn para_el(para: &Para, key: &str, base: Base, links: &LinkHandler) -> AnyElement {
    let styled = StyledText::new(SharedString::from(para.text.clone())).with_runs(runs(para, base));
    let found = para.links();
    if found.is_empty() {
        return styled.into_any_element();
    }
    let layout = styled.layout().clone();
    let ranges: Vec<Range<usize>> = found.iter().map(|(r, _)| r.clone()).collect();
    let urls: Vec<Arc<str>> = found.iter().map(|(_, u)| u.clone()).collect();
    let tags = found
        .iter()
        .map(|(r, u)| (r.clone(), u.to_string()))
        .collect();
    let on_link = links.clone();
    let interactive = InteractiveText::new(
        ElementId::NamedInteger(SharedString::from(key.to_owned()), 0),
        styled,
    )
    .on_click(ranges, move |ix, _, cx| on_link(&urls[ix], cx));
    e2e::text_links(interactive, &layout, tags).into_any_element()
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
                .bg(rgb(theme::BG_INPUT))
                .font_family(theme::MONO)
                .text_size(rem(0.85))
                .text_color(rgb(base.color))
                .child(code.clone())
                .into_any_element(),
            Block::Quote(inner) => div()
                .my(px(4.0))
                .pl(px(12.0))
                .border_l(px(3.0))
                .border_color(rgb(theme::BG_ELEVATED))
                .child(blocks_el(
                    inner,
                    &key,
                    Base {
                        color: theme::TEXT_MUTED,
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
                .bg(rgb(theme::BG_ELEVATED))
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
        .bg(rgb(theme::BG_ELEVATED))
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
                            .bg(rgb(theme::BG_ELEVATED))
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

fn reactions_el(reactions: &[Reaction]) -> impl IntoElement {
    div()
        .flex()
        .flex_wrap()
        .gap(px(4.0))
        .mt(px(4.0))
        .children(reactions.iter().map(|reaction| {
            let mut chip = div()
                .flex()
                .items_center()
                .gap(px(4.0))
                .px(px(8.0))
                .py(px(2.0))
                .rounded(px(10.0))
                .border_1()
                .text_size(rem(0.85))
                .cursor_pointer();
            chip = if reaction.mine {
                chip.border_color(rgb(theme::ACCENT))
                    .bg(rgba(theme::ACCENT_TINT))
                    .text_color(rgb(theme::TEXT))
            } else {
                chip.border_color(gpui::transparent_black())
                    .bg(rgb(theme::BG_ELEVATED))
                    .text_color(rgb(theme::TEXT_MUTED))
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

fn thread_chip(thread: &ThreadSummary) -> impl IntoElement {
    let count = if thread.reply_count == 1 {
        "1 reply".to_owned()
    } else {
        format!("{} replies", thread.reply_count)
    };
    let mut chip = div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .mt(px(6.0))
        .px(px(10.0))
        .py(px(4.0))
        .rounded(px(10.0))
        .bg(rgb(theme::BG_ELEVATED))
        .text_color(rgb(theme::TEXT_MUTED))
        .text_size(rem(0.82))
        .cursor_pointer()
        .child(
            div()
                .text_color(rgb(theme::TEXT_MUTED))
                .child(icon("messages-square", 14.0))
                .e2e("span", "thread-chip-icon"),
        )
        .child(
            div()
                .text_color(rgb(theme::ACCENT))
                .font_weight(FontWeight::SEMIBOLD)
                .child(count.clone())
                .e2e("span", "thread-chip-count")
                .text(count),
        );
    if let Some(name) = &thread.latest_sender_name {
        let text = format!("· last from {name}");
        chip = chip.child(
            div()
                .max_w(px(160.0))
                .truncate()
                .child(text.clone())
                .e2e("span", "thread-chip-latest")
                .text(text),
        );
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
        .border_color(rgb(theme::TEXT_FAINT))
        .text_size(rem(0.8))
        .text_color(rgb(theme::TEXT_MUTED))
        .overflow_hidden()
        .child(div().flex_none().child("↩").e2e("span", "").text("↩"))
        .child(
            div()
                .flex_none()
                .font_weight(FontWeight::BOLD)
                .text_color(rgb(theme::TEXT))
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
                .bg(rgb(theme::BG_ELEVATED))
                .e2e("span", "skeleton skeleton-inline"),
        ),
    };
    quote.e2e("div", "reply-quote").attr("title", reply.event_id.to_string())
}

/// A file or audio card: icon, name, size, and its Download button.
fn attachment_card(
    glyph: &str,
    audio: bool,
    name: &str,
    meta: &str,
    download: Option<&Download>,
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
                .text_color(rgb(theme::TEXT_MUTED))
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
                theme::DANGER
            } else {
                theme::TEXT_MUTED
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
            .flex_none()
            .px(px(12.0))
            .py(px(6.0))
            .rounded(px(theme::RADIUS_SM))
            .bg(rgb(theme::BG_INPUT))
            .text_size(rem(0.85))
            .text_color(rgb(theme::TEXT_MUTED))
            .cursor_pointer()
            .child("Download")
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
        .bg(rgb(theme::BG_ELEVATED))
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

fn image_el(body: &str, width: Option<u32>, height: Option<u32>, picture: Option<Arc<Image>>) -> AnyElement {
    let (w, h) = image_size(width, height);
    let base = div()
        .mt(px(4.0))
        .w(px(w))
        .h(px(h))
        .max_w_full()
        .flex_none()
        .rounded(px(theme::RADIUS))
        .overflow_hidden()
        .bg(rgb(theme::BG_INPUT));
    match picture {
        Some(image) => base
            .cursor_pointer()
            .child(img(image).size_full().object_fit(ObjectFit::Contain))
            .e2e("img", "image")
            .attr("alt", body)
            .attr("title", body)
            .into_any_element(),
        None => base
            .flex()
            .items_center()
            .justify_center()
            .text_color(rgb(theme::TEXT_FAINT))
            .text_size(rem(0.85))
            .child(body.to_owned())
            .e2e("div", "image placeholder")
            .text(body)
            .into_any_element(),
    }
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
        .w(px(w))
        .h(px(h))
        .max_w_full()
        .flex_none()
        .rounded(px(theme::RADIUS))
        .overflow_hidden()
        .bg(rgb(theme::BG_INPUT));
    card = match thumbnail {
        Some(image) => card.child(
            img(image)
                .size_full()
                .object_fit(ObjectFit::Cover)
                .e2e("img", "video-thumb")
                .attr("alt", body)
                .attr("title", body),
        ),
        None => card.child(
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(theme::TEXT_FAINT))
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
                .bg(rgba(0x141720bf))
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
    card.child(overlay.e2e("div", "video-overlay"))
        .e2e("div", "video-card")
        .into_any_element()
}

/// The body of a message, by what it is.
fn body_el(
    row: &MessageRow,
    key: &str,
    pics: &Pictures,
    download: Option<&Download>,
    links: &LinkHandler,
) -> AnyElement {
    let (class, base, rich_html) = match &row.message.kind {
        MessageKind::Text { html, .. } => ("body", Base { color: theme::TEXT, italic: false }, html.is_some()),
        MessageKind::Notice { html, .. } => (
            "body notice",
            Base { color: theme::TEXT_MUTED, italic: false },
            html.is_some(),
        ),
        MessageKind::Emote { html, .. } => (
            "body emote",
            Base { color: theme::TEXT, italic: true },
            html.is_some(),
        ),
        MessageKind::Image {
            body,
            width,
            height,
            ..
        } => return image_el(body, *width, *height, pics.media.clone()),
        MessageKind::File { body, size, .. } => {
            let meta = size.map(human_size).unwrap_or_default();
            return attachment_card("📎", false, body, &meta, download).into_any_element();
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
            return attachment_card("🎵", true, body, &meta, download).into_any_element();
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
        MessageKind::UnableToDecrypt => {
            return div()
                .w(relative(0.55))
                .max_w(px(420.0))
                .min_w(px(120.0))
                .h(px(14.0))
                .my(px(5.0))
                .rounded(px(7.0))
                .bg(rgb(theme::BG_ELEVATED))
                .e2e("div", "body skeleton skeleton-line undecrypted")
                .attr("title", "Waiting for the keys to decrypt this message")
                .attr("aria-label", "Encrypted message, waiting for keys")
                .into_any_element();
        }
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
        .text_color(rgb(theme::TEXT_MUTED))
        .child(text.to_owned())
        .e2e("p", "body muted")
        .text(text)
        .into_any_element()
}

/// A message's row. `show_header`: its sender's avatar, name and time lead it.
pub fn message_el(
    row: &MessageRow,
    show_header: bool,
    pics: &Pictures,
    download: Option<&Download>,
    links: &LinkHandler,
) -> AnyElement {
    let m = &row.message;
    let event_id = match &m.id {
        morpheus_core::MessageId::Event(id) => id.to_string(),
        morpheus_core::MessageId::Transaction(_) => String::new(),
    };
    let key = SharedString::from(format!("msg-{}", m.unique_id));

    // A membership or state change: a quiet line.
    if let MessageKind::State(text) = &m.kind {
        return div()
            .id(ElementId::from(key))
            .flex()
            .gap(px(12.0))
            .py(px(2.0))
            .pl(px(68.0))
            .pr(px(16.0))
            .text_size(rem(0.85))
            .text_color(rgb(theme::TEXT_MUTED))
            .child(
                div()
                    .w(px(40.0))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .text_size(rem(0.75))
                    .text_color(rgb(theme::TEXT_FAINT))
                    .child(row.time.clone())
                    .e2e("span", "time")
                    .text(row.time.clone()),
            )
            .child(
                div()
                    .child(text.clone())
                    .e2e("span", "system-text")
                    .text(text.clone()),
            )
            .e2e("div", "message system")
            .attr("data-event-id", event_id)
            .into_any_element();
    }

    let mut main = div().flex_1().min_w_0().flex().flex_col();
    if show_header {
        let name = m.sender_name.clone();
        main = main.child(
            div()
                .flex()
                .items_baseline()
                .gap(px(8.0))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(if m.is_own { theme::OWN } else { theme::TEXT }))
                        .cursor_pointer()
                        .child(name.clone())
                        .e2e("button", "sender")
                        .attr("title", m.sender.to_string())
                        .text(name),
                )
                .child(
                    div()
                        .text_size(rem(0.75))
                        .text_color(rgb(theme::TEXT_FAINT))
                        .child(row.time.clone())
                        .e2e("span", "time")
                        .text(row.time.clone()),
                )
                .e2e("div", "message-meta"),
        );
    }
    if let Some(shield) = &m.shield {
        // Dioxus floats it right of the message; here it leads the body.
        main = main.child(
            div()
                .text_color(rgb(if shield.red {
                    theme::DANGER
                } else {
                    theme::TEXT_FAINT
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
    main = main.child(body_el(row, &m.unique_id, pics, download, links));
    if !m.reactions.is_empty() {
        main = main.child(reactions_el(&m.reactions));
    }
    if let Some(thread) = &m.thread {
        main = main.child(thread_chip(thread));
    }

    let group = SharedString::from("message");
    let mut line = div()
        .id(ElementId::from(key))
        .group(group.clone())
        .flex()
        .gap(px(12.0))
        .px(px(16.0))
        .py(px(2.0))
        .hover(|s| s.bg(rgba(theme::BG_HOVER)));
    if m.is_local_echo {
        line = line.opacity(0.55);
    }
    line = if show_header {
        line.child(
            div()
                .mt(px(2.0))
                .cursor_pointer()
                .child(avatar(pics.avatar.clone(), &m.sender_name, 40.0, None))
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
                .text_color(rgb(theme::TEXT_FAINT))
                .opacity(0.0)
                .group_hover(group, |s| s.opacity(1.0))
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
    let line = line
        .child(main.e2e("div", "message-main"))
        .e2e("div", &classes)
        .attr("data-event-id", event_id);
    if show_header {
        div().pt(px(12.0)).child(line).into_any_element()
    } else {
        line.into_any_element()
    }
}

/// A row that is not a message.
pub fn other_row(row: &Row) -> AnyElement {
    match row {
        Row::Date(label, _) => div()
            .flex()
            .items_center()
            .gap(px(12.0))
            .mt(px(16.0))
            .mb(px(4.0))
            .mx(px(16.0))
            .text_color(rgb(theme::TEXT_FAINT))
            .text_size(rem(0.75))
            .font_weight(FontWeight::SEMIBOLD)
            .child(div().flex_1().h(px(1.0)).bg(rgb(theme::BG_ELEVATED)))
            .child(div().child(label.clone()).e2e("span", "").text(label.clone()))
            .child(div().flex_1().h(px(1.0)).bg(rgb(theme::BG_ELEVATED)))
            .e2e("div", "date-divider")
            .into_any_element(),
        Row::Start => div()
            .p(px(16.0))
            .flex()
            .justify_center()
            .text_color(rgb(theme::TEXT_FAINT))
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
            .text_color(rgb(theme::DANGER))
            .text_size(rem(0.7))
            .font_weight(FontWeight::BOLD)
            .child(div().flex_1().h(px(1.0)).bg(rgb(theme::DANGER)))
            .child(
                div()
                    .px(px(6.0))
                    .rounded(px(4.0))
                    .bg(rgb(theme::DANGER))
                    .text_color(rgb(0xffffff))
                    .child("NEW")
                    .e2e("span", "")
                    .text("NEW"),
            )
            .child(div().flex_1().h(px(1.0)).bg(rgb(theme::DANGER)))
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
            color: theme::TEXT,
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
        assert_eq!(runs[3].color, hsla(theme::LINK));
        assert_eq!(runs[0].color, hsla(theme::TEXT));
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
