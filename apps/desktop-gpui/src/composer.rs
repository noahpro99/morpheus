//! The message box (`crates/ui/src/components/composer.rs`): the growing
//! text box with its attach and emoji buttons, the reply and edit banner,
//! files waiting on the message as tiles, the `:emoji:` and `@member`
//! suggestions and the emoji panel. What it decides (suggestions, modes,
//! what Enter sends) is in `view_model::composer`; this draws it and wires
//! the text box's events to the room's timeline handle.

use std::{collections::HashMap, path::PathBuf, sync::Arc};

use gpui::{
    AnyElement, Context, ElementId, Entity, ExternalPaths, FontWeight, Image, ImageFormat,
    IntoElement, ObjectFit, PathPromptOptions, Render, StyledImage as _, Window, div,
    img, prelude::*, px, relative, rgb, rgba,
};
use morpheus_core::{Member, Message, OwnedRoomId, Session, TimelineHandle};

use crate::{
    e2e::E2eExt as _,
    icons::icon,
    input::{Changed, Nav, NavKey, Pasted, Submit, TextInput},
    theme,
    view_model::{
        composer::{
            self, ComposerMode, ComposerState, SendKind, SendPlan, Suggestion, mime_of,
        },
        emoji,
    },
};

/// The emoji panel's width (`.emoji-panel`).
const PANEL_W: f32 = 340.0;

fn rem(n: f32) -> gpui::Pixels {
    px(n * theme::REM)
}

/// A file waiting, as its tile draws it.
struct Tile {
    id: u64,
    name: String,
    /// The picture, for an image.
    image: Option<Arc<Image>>,
    /// What the test bridge reads as the tile's picture source.
    src: String,
}

pub struct Composer {
    room_id: OwnedRoomId,
    room_name: String,
    is_dm: bool,
    session: Option<Session>,
    handle: Option<TimelineHandle>,
    input: Entity<TextInput>,
    state: ComposerState,
    tiles: Vec<Tile>,
    last_own: Option<Message>,
    suggestions: Vec<Suggestion>,
    active: usize,
    members: Option<Vec<Member>>,
    fetching_members: bool,
    emoji_open: bool,
    /// Put the caret in the box on the next frame (a window is needed).
    pending_focus: bool,
}

impl Composer {
    pub fn new(
        session: Option<Session>,
        room_id: OwnedRoomId,
        room_name: String,
        is_dm: bool,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            TextInput::new("composer", "", false, "", cx)
                .multiline(8)
                .bare("textarea", "composer-input")
        });
        input.update(cx, |input, cx| input.set_disabled(true, cx));
        cx.subscribe(&input, |this, _, _: &Changed, cx| this.edited(cx))
            .detach();
        cx.subscribe(&input, |this, _, _: &Submit, cx| this.send(cx))
            .detach();
        cx.subscribe(&input, |this, _, nav: &Nav, cx| this.nav(nav.0, cx))
            .detach();
        cx.subscribe(&input, |this, _, pasted: &Pasted, cx| {
            for file in &pasted.0 {
                this.attach(file.name.clone(), file.mime.clone(), file.bytes.clone(), cx);
            }
        })
        .detach();
        let mut this = Self {
            room_id,
            room_name,
            is_dm,
            session,
            handle: None,
            input,
            state: ComposerState::default(),
            tiles: Vec::new(),
            last_own: None,
            suggestions: Vec::new(),
            active: 0,
            members: None,
            fetching_members: false,
            emoji_open: false,
            pending_focus: false,
        };
        this.refresh_placeholder(cx);
        #[cfg(feature = "e2e")]
        crate::e2e::on_attach(cx.weak_entity());
        this
    }

    fn refresh_placeholder(&mut self, cx: &mut Context<Self>) {
        let text = if self.is_dm {
            format!("Message @{}", self.room_name)
        } else {
            format!("Message #{}", self.room_name)
        };
        self.input
            .update(cx, |input, cx| input.set_placeholder(&text, cx));
    }

    /// The room's timeline is open: messages can go out.
    pub fn opened(&mut self, handle: TimelineHandle, cx: &mut Context<Self>) {
        self.handle = Some(handle);
        self.input
            .update(cx, |input, cx| input.set_disabled(false, cx));
        self.pending_focus = true;
        cx.notify();
    }

    /// Your newest editable message changed (for Up to edit it).
    pub fn set_last_own(&mut self, message: Option<Message>) {
        self.last_own = message;
    }

    /// "Reply" on a message.
    pub fn reply_to(&mut self, message: &Message, cx: &mut Context<Self>) {
        if self.state.start_reply(message) {
            self.pending_focus = true;
            cx.notify();
        }
    }

    /// "Edit" on a message: its text goes in the box.
    pub fn edit(&mut self, message: &Message, cx: &mut Context<Self>) {
        let original = self.state.start_edit(message);
        self.input
            .update(cx, |input, cx| input.set_value(&original, cx));
        self.suggestions.clear();
        self.pending_focus = true;
        cx.notify();
    }

    /// "Mention" on a member: `@name ` goes at the end of the draft.
    pub fn mention(&mut self, name: &str, cx: &mut Context<Self>) {
        let text = composer::with_mention(&self.draft(cx), name);
        self.set_draft(&text, cx);
        self.pending_focus = true;
        cx.notify();
    }

    fn draft(&self, cx: &gpui::App) -> String {
        self.input.read(cx).value().to_owned()
    }

    fn set_draft(&mut self, text: &str, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.set_value(text, cx));
    }

    /// The text changed by the user: suggestions, and the typing notice.
    fn edited(&mut self, cx: &mut Context<Self>) {
        let draft = self.draft(cx);
        self.recompute(cx);
        if !draft.is_empty()
            && let Some(handle) = &self.handle
        {
            let notice = handle.renew_typing();
            cx.spawn(async move |_, _| {
                if let Err(err) = notice.await {
                    eprintln!("morpheus-gpui: typing notice: {err}");
                }
            })
            .detach();
        }
    }

    /// The suggestions for the word being typed (members are fetched the
    /// first time an `@` is typed).
    fn recompute(&mut self, cx: &mut Context<Self>) {
        let draft = self.draft(cx);
        if composer::wants_members(&draft)
            && self.members.is_none()
            && !self.fetching_members
            && let Some(session) = self.session.clone()
        {
            self.fetching_members = true;
            let listing = session.members(self.room_id.clone());
            cx.spawn(async move |this, cx| {
                let list = listing.await;
                this.update(cx, |this, cx| {
                    this.fetching_members = false;
                    match list {
                        Ok(list) => {
                            this.members = Some(list);
                            this.recompute(cx);
                        }
                        Err(err) => eprintln!("morpheus-gpui: members for mentions: {err}"),
                    }
                })
                .ok();
            })
            .detach();
        }
        self.suggestions = composer::suggest(&draft, self.members.as_deref(), &HashMap::new());
        self.active = 0;
        self.sync_capture(cx);
        cx.notify();
    }

    fn sync_capture(&self, cx: &mut Context<Self>) {
        let open = !self.suggestions.is_empty();
        self.input
            .update(cx, |input, _| input.set_capture_nav(open));
    }

    /// Replace the word being typed with the chosen suggestion.
    fn accept(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(choice) = self.suggestions.get(index).cloned() else {
            return;
        };
        let text = composer::accept(&self.draft(cx), &choice);
        self.set_draft(&text, cx);
        self.suggestions.clear();
        self.sync_capture(cx);
        cx.notify();
    }

    fn nav(&mut self, key: NavKey, cx: &mut Context<Self>) {
        let count = self.suggestions.len();
        let open = count > 0;
        match key {
            NavKey::Down if open => self.active = (self.active + 1) % count,
            NavKey::Up if open => self.active = (self.active + count - 1) % count,
            NavKey::Enter | NavKey::Tab if open => self.accept(self.active, cx),
            NavKey::Escape if open => {
                self.suggestions.clear();
                self.sync_capture(cx);
            }
            NavKey::Escape if self.emoji_open => self.emoji_open = false,
            NavKey::Escape => self.cancel(cx),
            NavKey::Up => {
                // Up in an empty box edits your newest message.
                if let Some(message) = self.last_own.clone() {
                    self.edit(&message, cx);
                }
            }
            NavKey::Down | NavKey::Enter | NavKey::Tab => {}
        }
        cx.notify();
    }

    /// Esc or the banner's ✕: leaves reply or edit mode, clearing the box.
    fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.state.cancel() {
            self.set_draft("", cx);
        }
        cx.notify();
    }

    fn can_send(&self, cx: &gpui::App) -> bool {
        self.handle.is_some() && self.state.can_send(&self.draft(cx))
    }

    /// Enter: the text goes out as the mode says, then the waiting files.
    fn send(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        if !self.can_send(cx) {
            return;
        }
        let draft = self.draft(cx);
        let Some(SendPlan { text, files }) = self.state.take_send(&draft) else {
            return;
        };
        self.tiles
            .retain(|tile| self.state.attachments.iter().any(|a| a.id == tile.id));
        self.set_draft("", cx);
        self.suggestions.clear();
        self.sync_capture(cx);
        // Sending first: the message shows (greyed) the moment it is queued.
        let sending = text.map(|(kind, body)| match kind {
            SendKind::Text => handle.send_text(body),
            SendKind::Reply(event_id) => handle.send_reply(body, event_id),
            SendKind::Edit(id) => handle.edit(id, body),
        });
        let typing = handle.stop_typing();
        cx.spawn(async move |_, _| {
            let _ = typing.await;
            if let Some(sending) = sending
                && let Err(err) = sending.await
            {
                eprintln!("morpheus-gpui: sending: {err}");
            }
            // The files after the text, so they read below it.
            for file in files {
                if let Err(err) = handle
                    .send_attachment_bytes(file.name, file.mime, file.bytes)
                    .await
                {
                    eprintln!("morpheus-gpui: upload: {err}");
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// A file waits on the message being written.
    pub fn attach(&mut self, name: String, mime: String, bytes: Vec<u8>, cx: &mut Context<Self>) {
        let image = ImageFormat::from_mime_type(&mime)
            .filter(|_| mime.starts_with("image/"))
            .map(|format| Arc::new(Image::from_bytes(format, bytes.clone())));
        let src = if image.is_some() {
            preview_src(&mime, &bytes)
        } else {
            String::new()
        };
        let id = self.state.attach(name.clone(), mime, bytes);
        self.tiles.push(Tile {
            id,
            name,
            image,
            src,
        });
        cx.notify();
    }

    /// Files dropped on the chat: read, then waiting like pasted ones.
    pub fn attach_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        for path in paths.to_vec() {
            cx.spawn(async move |this, cx| {
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "file".into());
                let reading = {
                    let path = path.clone();
                    cx.background_spawn(async move { std::fs::read(path) })
                };
                match reading.await {
                    Ok(bytes) => {
                        this.update(cx, |this, cx| {
                            let mime = mime_of(&name).to_owned();
                            this.attach(name, mime, bytes, cx);
                        })
                        .ok();
                    }
                    Err(err) => eprintln!("morpheus-gpui: reading {}: {err}", path.display()),
                }
            })
            .detach();
        }
    }

    /// A file the test bridge hands over, as pasted or dropped.
    #[cfg(feature = "e2e")]
    pub fn attach_from_bridge(&mut self, file: crate::input::PastedFile, dropped: bool, cx: &mut Context<Self>) {
        if dropped {
            let dir = std::env::temp_dir().join(format!("morpheus-gpui-drop-{}", std::process::id()));
            let path = dir.join(&file.name);
            if std::fs::create_dir_all(&dir)
                .and_then(|()| std::fs::write(&path, &file.bytes))
                .is_ok()
            {
                self.attach_paths(&[path], cx);
            }
        } else {
            self.input
                .update(cx, |_, cx| cx.emit(Pasted(vec![file])));
        }
    }

    /// The attach button: pick a file, which goes out at once.
    fn pick_file(&mut self, cx: &mut Context<Self>) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let picking = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: false,
            prompt: Some("Attach a file".into()),
        });
        cx.spawn(async move |_, _| {
            let Ok(Ok(Some(paths))) = picking.await else {
                return;
            };
            for path in paths {
                if let Err(err) = handle.send_attachment(path).await {
                    eprintln!("morpheus-gpui: attaching: {err}");
                }
            }
        })
        .detach();
    }

    fn banner(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let mode = self.state.mode.as_ref()?;
        let (label, preview): (AnyElement, Option<String>) = match mode {
            ComposerMode::Reply {
                sender_name,
                summary,
                ..
            } => (
                div()
                    .flex()
                    .gap(px(4.0))
                    .child("Replying to")
                    .child(div().font_weight(FontWeight::BOLD).child(sender_name.clone()))
                    .e2e("span", "banner-label")
                    .text(format!("Replying to {sender_name}"))
                    .into_any_element(),
                Some(summary.clone()),
            ),
            ComposerMode::Edit { .. } => (
                div()
                    .child("Editing message")
                    .e2e("span", "banner-label")
                    .text("Editing message")
                    .into_any_element(),
                None,
            ),
        };
        let mut banner = div()
            .flex()
            .items_center()
            .gap(px(8.0))
            .px(px(12.0))
            .py(px(6.0))
            .rounded_t(px(theme::RADIUS_LG))
            .bg(rgb(theme::BG_ELEVATED))
            .text_size(rem(0.85))
            .text_color(rgb(theme::TEXT_MUTED))
            .child(label);
        banner = banner.child(match preview {
            Some(preview) => div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(rgb(theme::TEXT_FAINT))
                .child(preview.clone())
                .e2e("span", "banner-preview")
                .text(preview)
                .into_any_element(),
            None => div().flex_1().into_any_element(),
        });
        banner = banner.child(
            div()
                .id("composer-cancel")
                .px(px(6.0))
                .rounded(px(theme::RADIUS_SM))
                .cursor_pointer()
                .hover(|s| s.bg(rgba(theme::BG_HOVER)).text_color(rgb(theme::TEXT)))
                .child("✕")
                .on_click(cx.listener(|this, _, _, cx| this.cancel(cx)))
                .e2e("button", "ghost")
                .attr("title", "Cancel (Esc)")
                .text("✕"),
        );
        Some(banner.e2e("div", "composer-banner").into_any_element())
    }

    fn tiles_el(&self, with_banner: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.tiles.is_empty() {
            return None;
        }
        let mut row = div()
            .flex()
            .gap(px(10.0))
            .px(px(12.0))
            .pt(px(12.0))
            .pb(px(8.0))
            .bg(rgb(theme::BG_ELEVATED))
            .border_b_1()
            .border_color(rgba(theme::BG_HOVER));
        if !with_banner {
            row = row.rounded_t(px(theme::RADIUS_LG));
        }
        for tile in &self.tiles {
            let id = tile.id;
            let picture = match &tile.image {
                Some(image) => img(image.clone())
                    .w_full()
                    .h(px(110.0))
                    .rounded(px(theme::RADIUS - 2.0))
                    .object_fit(ObjectFit::Cover)
                    .e2e("img", "")
                    .attr("src", tile.src.clone())
                    .attr("alt", tile.name.clone())
                    .into_any_element(),
                None => div()
                    .w_full()
                    .h(px(110.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(theme::RADIUS - 2.0))
                    .bg(rgb(theme::BG_CHAT))
                    .text_color(rgb(theme::TEXT_MUTED))
                    .child(icon("paperclip", 28.0))
                    .e2e("div", "attachment-file")
                    .into_any_element(),
            };
            row = row.child(
                div()
                    .relative()
                    .flex_none()
                    .flex()
                    .flex_col()
                    .gap(px(6.0))
                    .w(px(140.0))
                    .p(px(8.0))
                    .rounded(px(theme::RADIUS))
                    .bg(rgb(theme::BG_INPUT))
                    .child(picture)
                    .child(
                        div()
                            .truncate()
                            .text_size(rem(0.75))
                            .text_color(rgb(theme::TEXT_MUTED))
                            .child(tile.name.clone())
                            .e2e("span", "attachment-label")
                            .text(tile.name.clone()),
                    )
                    .child(
                        div()
                            .id(ElementId::Name(format!("remove-{id}").into()))
                            .absolute()
                            .top(px(-6.0))
                            .right(px(-6.0))
                            .size(px(24.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(rgb(theme::BG_ELEVATED))
                            .text_color(rgb(theme::TEXT_MUTED))
                            .shadow_sm()
                            .cursor_pointer()
                            .hover(|s| s.bg(rgb(theme::DANGER)).text_color(rgb(0xffffff)))
                            .child(icon("x", 14.0))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.state.remove(id);
                                this.tiles.retain(|t| t.id != id);
                                cx.notify();
                            }))
                            .e2e("button", "remove-attachment")
                            .attr("title", "Remove")
                            .attr("aria-label", format!("Remove {}", tile.name)),
                    )
                    .e2e("div", "composer-attachment")
                    .attr("title", tile.name.clone()),
            );
        }
        Some(row.e2e("div", "composer-attachments").into_any_element())
    }

    fn autocomplete_el(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.suggestions.is_empty() {
            return None;
        }
        let mut list = div()
            .flex()
            .flex_col()
            .gap(px(2.0))
            .p(px(6.0))
            .rounded(px(theme::RADIUS_LG))
            .bg(rgb(theme::BG_ELEVATED))
            .border_1()
            .border_color(rgba(0xffffff14))
            .shadow_lg()
            .occlude();
        for (index, row) in self.suggestions.iter().enumerate() {
            let (glyph, label, muted): (String, String, Option<String>) = match row {
                Suggestion::Emoji { emoji, code } => ((*emoji).to_owned(), format!(":{code}:"), None),
                Suggestion::Member {
                    name,
                    nickname: Some(nickname),
                    user_id,
                } => ("@".into(), nickname.clone(), Some(format!("{name} · {user_id}"))),
                Suggestion::Member {
                    name,
                    nickname: None,
                    user_id,
                } => ("@".into(), name.clone(), Some(user_id.clone())),
            };
            let mut item = div()
                .id(ElementId::Name(format!("suggestion-{index}").into()))
                .flex()
                .items_center()
                .gap(px(10.0))
                .px(px(10.0))
                .py(px(6.0))
                .rounded(px(theme::RADIUS_SM))
                .cursor_pointer()
                .hover(|s| s.bg(rgba(theme::BG_HOVER)))
                .child(
                    div()
                        .w(px(24.0))
                        .flex()
                        .justify_center()
                        .text_size(rem(1.1))
                        .child(glyph.clone())
                        .e2e("span", "emoji")
                        .text(glyph),
                )
                .child(div().child(label.clone()).e2e("span", "").text(label.clone()));
            if let Some(muted) = muted {
                item = item.child(
                    div()
                        .text_size(rem(0.8))
                        .text_color(rgb(theme::TEXT_MUTED))
                        .child(muted.clone())
                        .e2e("span", "muted")
                        .text(muted.clone()),
                );
            }
            let active = index == self.active;
            if active {
                item = item.bg(rgba(theme::BG_HOVER));
            }
            list = list.child(
                item.on_click(cx.listener(move |this, _, _, cx| this.accept(index, cx)))
                    .e2e(
                        "button",
                        if active {
                            "autocomplete-item active"
                        } else {
                            "autocomplete-item"
                        },
                    ),
            );
        }
        Some(
            div()
                .absolute()
                .bottom(relative(1.0))
                .left(px(16.0))
                .right(px(16.0))
                .mb(px(-8.0))
                .child(list.e2e("div", "autocomplete").attr("role", "listbox"))
                .into_any_element(),
        )
    }

    fn emoji_panel_el(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.emoji_open {
            return None;
        }
        let mut grid = div()
            .id("emoji-panel")
            .flex()
            .flex_wrap()
            .gap(px(2.0))
            .w(px(PANEL_W))
            .max_h(px(300.0))
            .overflow_y_scroll()
            .p(px(6.0))
            .rounded(px(theme::RADIUS_LG))
            .bg(rgb(theme::BG_ELEVATED))
            .border_1()
            .border_color(rgba(0xffffff14))
            .shadow_lg()
            .occlude();
        for (glyph, code) in emoji::EMOJI.iter().copied() {
            grid = grid.child(
                div()
                    .id(ElementId::Name(format!("emoji-{code}").into()))
                    .w(px((PANEL_W - 12.0 - 18.0) / 10.0))
                    .py(px(4.0))
                    .flex()
                    .justify_center()
                    .rounded(px(theme::RADIUS_SM))
                    .text_size(rem(1.25))
                    .cursor_pointer()
                    .hover(|s| s.bg(rgba(theme::BG_HOVER)))
                    .child(glyph)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let text = composer::with_emoji(&this.draft(cx), glyph);
                        this.set_draft(&text, cx);
                        this.emoji_open = false;
                        this.pending_focus = true;
                        cx.notify();
                    }))
                    .e2e("button", "")
                    .attr("title", code)
                    .text(glyph),
            );
        }
        Some(
            div()
                .absolute()
                .bottom(relative(1.0))
                .right(px(16.0))
                .mb(px(-8.0))
                .child(grid.e2e("div", "emoji-panel").attr("role", "dialog"))
                .into_any_element(),
        )
    }
}

/// `data:` URL of a picture, as a browser would show it: what the test
/// bridge reports as a tile's `src`.
#[cfg(feature = "e2e")]
fn preview_src(mime: &str, bytes: &[u8]) -> String {
    use base64::Engine as _;
    format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    )
}

#[cfg(not(feature = "e2e"))]
fn preview_src(_mime: &str, _bytes: &[u8]) -> String {
    String::new()
}

/// A 36 px button beside the box (attach, emoji); `enabled` false marks it
/// `[disabled]`.
fn tool_button(
    id: &'static str,
    classes: &'static str,
    title: &'static str,
    glyph: AnyElement,
    enabled: bool,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let tagged = div()
        .id(id)
        .flex_none()
        .size(px(36.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(theme::RADIUS_SM))
        .text_color(rgb(theme::TEXT_MUTED))
        .cursor_pointer()
        .hover(|s| s.bg(rgba(theme::BG_HOVER)).text_color(rgb(theme::TEXT)))
        .child(glyph)
        .on_click(on_click)
        .e2e("button", classes)
        .attr("title", title);
    if enabled {
        tagged
    } else {
        tagged.attr("disabled", "")
    }
}

impl Render for Composer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if std::mem::take(&mut self.pending_focus) {
            self.input.read(cx).focus(window);
        }
        let ready = self.handle.is_some();
        let banner = self.banner(cx);
        let tiles = self.tiles_el(banner.is_some(), cx);
        let above = banner.is_some() || tiles.is_some();
        let lines = self.draft(cx).lines().count().max(1);

        let attach = tool_button(
            "attach",
            "attach-button",
            "Attach a file (Ctrl+Shift+U)",
            icon("plus", 18.0).into_any_element(),
            ready,
            cx.listener(|this, _, _, cx| this.pick_file(cx)),
        );
        let emoji = tool_button(
            "emoji",
            if self.emoji_open {
                "emoji-button active"
            } else {
                "emoji-button"
            },
            "Emoji",
            icon("smile", 20.0).into_any_element(),
            ready,
            cx.listener(|this, _, _, cx| {
                this.emoji_open = !this.emoji_open;
                cx.notify();
            }),
        );

        let mut row = div()
            .flex()
            .gap(px(6.0))
            .pl(px(4.0))
            .pr(px(6.0))
            .min_h(px(theme::BOTTOM_H))
            .bg(rgb(theme::BG_ELEVATED))
            .text_size(rem(1.0))
            .line_height(relative(theme::LINE_HEIGHT))
            .child(attach)
            .child(self.input.clone())
            .child(emoji);
        row = if lines > 1 {
            row.items_end().pb(px(7.0))
        } else {
            row.items_center()
        };
        row = if above {
            row.rounded_b(px(theme::RADIUS_LG))
        } else {
            row.rounded(px(theme::RADIUS_LG))
        };

        let mut footer = div()
            .relative()
            .flex_none()
            .px(px(8.0))
            .pb(px(theme::BOTTOM_GAP));
        if let Some(banner) = banner {
            footer = footer.child(banner);
        }
        if let Some(tiles) = tiles {
            footer = footer.child(tiles);
        }
        footer = footer.child(row.e2e("div", "composer-row"));
        if let Some(popup) = self.autocomplete_el(cx) {
            footer = footer.child(popup);
        }
        if let Some(panel) = self.emoji_panel_el(cx) {
            footer = footer.child(panel);
        }
        footer.e2e("footer", "composer")
    }
}

/// What the room's chat area needs to hand a dropped file to the composer.
pub fn dropped(composer: &Entity<Composer>, paths: &ExternalPaths, cx: &mut gpui::App) {
    composer.update(cx, |composer, cx| composer.attach_paths(paths.paths(), cx));
}

#[cfg(all(test, feature = "e2e"))]
mod tests {
    use super::*;
    use crate::e2e::{self, query};
    use gpui::{TestAppContext, VisualTestContext, point, size};

    struct Harness(Entity<Composer>);

    impl Render for Harness {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            e2e::frame(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .justify_end()
                    .font_family("Inter")
                    .text_size(px(theme::REM))
                    .child(self.0.clone()),
            )
        }
    }

    /// A composer with no timeline behind it: its box is disabled.
    fn open_without_timeline(
        cx: &mut TestAppContext,
    ) -> (Entity<Composer>, &mut VisualTestContext) {
        let (harness, cx) = cx.add_window_view(|_, cx| {
            Harness(cx.new(|cx| {
                Composer::new(None, "!r:x".try_into().expect("room"), "general".into(), false, cx)
            }))
        });
        let composer = harness.read_with(cx, |h, _| h.0.clone());
        redraw(cx);
        (composer, cx)
    }

    /// The same with the box enabled, as once the timeline is open (the
    /// handle itself needs an account).
    fn open(cx: &mut TestAppContext) -> (Entity<Composer>, &mut VisualTestContext) {
        let (composer, cx) = open_without_timeline(cx);
        let input = composer.read_with(cx, |c, _| c.input.clone());
        input.update(cx, |input, cx| input.set_disabled(false, cx));
        redraw(cx);
        (composer, cx)
    }

    fn redraw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.draw(
            point(px(0.0), px(0.0)),
            size(px(600.0), px(500.0)),
            |_, _| div(),
        );
        cx.run_until_parked();
    }

    fn texts(selector: &str) -> Vec<String> {
        query(selector).into_iter().map(|(t, _)| t).collect()
    }

    fn click(cx: &mut VisualTestContext, selector: &str) {
        let (_, (x, y)) = query(selector)
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no {selector}"));
        cx.simulate_click(point(px(x), px(y)), gpui::Modifiers::none());
        redraw(cx);
    }

    fn focus(composer: &Entity<Composer>, cx: &mut VisualTestContext) {
        let input = composer.read_with(cx, |c, _| c.input.clone());
        input.update_in(cx, |input, window, _| input.focus(window));
        redraw(cx);
    }

    fn draft(composer: &Entity<Composer>, cx: &VisualTestContext) -> String {
        composer.read_with(cx, |c, cx| c.draft(cx))
    }

    #[gpui::test]
    fn without_a_timeline_the_box_is_disabled_and_names_the_room(cx: &mut TestAppContext) {
        let (_composer, cx) = open_without_timeline(cx);
        assert_eq!(query(".composer .composer-input[disabled]").len(), 1);
        assert_eq!(query(".composer .composer-input:not([disabled])").len(), 0);
        assert_eq!(query(".composer .attach-button[disabled]").len(), 1);
        let _ = cx;
        let placeholder = e2e::query_attr(".composer .composer-input", "placeholder");
        assert_eq!(placeholder.as_deref(), Some("Message #general"));
    }

    #[gpui::test]
    fn typing_a_colon_code_offers_emoji_and_enter_inserts_it(cx: &mut TestAppContext) {
        let (composer, cx) = open(cx);
        focus(&composer, cx);
        cx.simulate_input("hot :fir");
        redraw(cx);
        let items = texts(".autocomplete .autocomplete-item");
        assert!(items.first().is_some_and(|t| t.contains("fire")), "{items:?}");
        assert_eq!(query(".autocomplete-item.active").len(), 1);
        cx.simulate_keystrokes("enter");
        redraw(cx);
        assert_eq!(draft(&composer, cx), "hot 🔥 ");
        assert_eq!(query(".autocomplete").len(), 0, "suggestions close");
    }

    #[gpui::test]
    fn escape_closes_suggestions_but_keeps_the_draft(cx: &mut TestAppContext) {
        let (composer, cx) = open(cx);
        focus(&composer, cx);
        cx.simulate_input(":hea");
        redraw(cx);
        assert_eq!(query(".autocomplete").len(), 1);
        cx.simulate_keystrokes("escape");
        redraw(cx);
        assert_eq!(query(".autocomplete").len(), 0);
        assert_eq!(draft(&composer, cx), ":hea");
    }

    #[gpui::test]
    fn down_moves_the_highlight_and_tab_takes_it(cx: &mut TestAppContext) {
        let (composer, cx) = open(cx);
        focus(&composer, cx);
        cx.simulate_input(":he");
        redraw(cx);
        let items = texts(".autocomplete-item");
        assert!(items.len() >= 2, "{items:?}");
        cx.simulate_keystrokes("down");
        redraw(cx);
        let active = texts(".autocomplete-item.active");
        assert_eq!(active, [items[1].clone()]);
        cx.simulate_keystrokes("tab");
        redraw(cx);
        let draft = draft(&composer, cx);
        assert!(draft.ends_with(' ') && !draft.contains(":he"), "{draft:?}");
    }

    #[gpui::test]
    fn the_emoji_panel_adds_to_the_draft_and_closes(cx: &mut TestAppContext) {
        let (composer, cx) = open(cx);
        // Only a ready composer opens it (there is no timeline here), so
        // open it directly.
        composer.update(cx, |c, cx| {
            c.emoji_open = true;
            cx.notify();
        });
        redraw(cx);
        assert_eq!(query(".emoji-panel").len(), 1);
        assert!(query(".emoji-panel button").len() > 60);
        click(cx, ".emoji-panel button[title=rocket]");
        assert_eq!(draft(&composer, cx), "🚀 ");
        assert_eq!(query(".emoji-panel").len(), 0);
    }

    #[gpui::test]
    fn pasted_pictures_wait_as_tiles_and_the_x_takes_them_off(cx: &mut TestAppContext) {
        let (composer, cx) = open(cx);
        composer.update(cx, |c, cx| {
            c.attach("a.png".into(), "image/png".into(), vec![1, 2, 3], cx);
            c.attach("b.txt".into(), "text/plain".into(), b"hi".to_vec(), cx);
        });
        redraw(cx);
        assert_eq!(query(".composer .composer-attachment").len(), 2);
        assert_eq!(
            query(".composer .composer-attachment img[src^='data:image/png']").len(),
            1
        );
        assert_eq!(texts(".composer-attachment .attachment-label"), ["a.png", "b.txt"]);
        click(cx, ".composer .composer-attachment .remove-attachment");
        assert_eq!(texts(".composer-attachment .attachment-label"), ["b.txt"]);
        composer.read_with(cx, |c, _| assert_eq!(c.state.attachments.len(), 1));
    }

    #[gpui::test]
    fn a_reply_and_an_edit_show_a_banner_the_x_and_escape_cancel(cx: &mut TestAppContext) {
        use morpheus_core::{MessageId, MessageKind, OwnedEventId, OwnedUserId};
        let message = Message {
            unique_id: "t".into(),
            id: MessageId::Event(OwnedEventId::try_from("$t").expect("event id")),
            sender: OwnedUserId::try_from("@carol:localhost").expect("user id"),
            sender_name: "carol".into(),
            sender_avatar: None,
            timestamp_ms: 0,
            is_own: true,
            is_local_echo: false,
            is_edited: false,
            can_edit: true,
            reactions: Vec::new(),
            in_reply_to: None,
            thread_root: None,
            thread: None,
            kind: MessageKind::Text {
                body: "sliding sync is working".into(),
                html: None,
            },
            shield: None,
        };
        let (composer, cx) = open(cx);
        focus(&composer, cx);
        composer.update(cx, |c, cx| c.reply_to(&message, cx));
        redraw(cx);
        let banner = texts(".composer-banner");
        assert!(
            banner[0].contains("Replying to carol") && banner[0].contains("sliding sync"),
            "{banner:?}"
        );
        click(cx, ".composer-banner .ghost");
        assert_eq!(query(".composer-banner").len(), 0);

        composer.update(cx, |c, cx| c.edit(&message, cx));
        redraw(cx);
        assert!(texts(".composer-banner")[0].contains("Editing message"));
        assert_eq!(draft(&composer, cx), "sliding sync is working");
        focus(&composer, cx);
        cx.simulate_keystrokes("escape");
        redraw(cx);
        assert_eq!(query(".composer-banner").len(), 0);
        assert_eq!(draft(&composer, cx), "", "cancelling an edit clears the box");
    }

    #[gpui::test]
    fn up_in_an_empty_box_edits_the_newest_own_message(cx: &mut TestAppContext) {
        use morpheus_core::{MessageId, MessageKind, OwnedUserId};
        let message = Message {
            unique_id: "m".into(),
            id: MessageId::Transaction("txn".into()),
            sender: OwnedUserId::try_from("@alice:localhost").expect("user id"),
            sender_name: "alice".into(),
            sender_avatar: None,
            timestamp_ms: 0,
            is_own: true,
            is_local_echo: false,
            is_edited: false,
            can_edit: true,
            reactions: Vec::new(),
            in_reply_to: None,
            thread_root: None,
            thread: None,
            kind: MessageKind::Text {
                body: "edit me".into(),
                html: None,
            },
            shield: None,
        };
        let (composer, cx) = open(cx);
        focus(&composer, cx);
        composer.update(cx, |c, _| c.set_last_own(Some(message)));
        cx.simulate_keystrokes("up");
        redraw(cx);
        assert!(texts(".composer-banner")[0].contains("Editing message"));
        assert_eq!(draft(&composer, cx), "edit me");
        // Up with text in the box moves the caret instead.
        cx.simulate_keystrokes("escape");
        cx.simulate_input("x");
        cx.simulate_keystrokes("up");
        redraw(cx);
        assert_eq!(query(".composer-banner").len(), 0);
    }
}
