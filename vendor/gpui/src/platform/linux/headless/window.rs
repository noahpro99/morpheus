//! Morpheus patch: an offscreen window for the headless client, so an app can
//! run with no display server (`ZED_HEADLESS`) and still lay out and paint
//! every frame: element bounds, hit-testing and input dispatch are real;
//! only the drawing to pixels is skipped (`draw` drops the scene). Frames are
//! requested by the client's 60 Hz timer (`HeadlessClient::tick`).

use std::{
    cell::RefCell,
    rc::Rc,
    sync::Arc,
};

use collections::HashMap;
use parking_lot::Mutex;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use crate::{
    AtlasKey, AtlasTextureId, AtlasTile, Bounds, Capslock, Decorations, DispatchEventResult,
    DisplayId, GpuSpecs, Modifiers, Pixels, PlatformAtlas, PlatformDisplay, PlatformInput,
    PlatformInputHandler, PlatformWindow, Point, PromptButton, RequestFrameOptions, Scene, Size,
    TileId, Tiling, WindowAppearance, WindowBackgroundAppearance, WindowBounds,
    WindowControlArea, WindowDecorations, WindowParams, px,
};

/// The one display of a headless run: as big as the e2e harness's hidden
/// test monitor (3840x2160 at scale 1).
#[derive(Debug)]
pub(crate) struct HeadlessDisplay {
    uuid: uuid::Uuid,
}

impl HeadlessDisplay {
    pub(crate) fn new() -> Self {
        Self {
            uuid: uuid::Uuid::from_u128(0x6d6f_7270_6865_7573_6865_6164_6c65_7373),
        }
    }
}

impl PlatformDisplay for HeadlessDisplay {
    fn id(&self) -> DisplayId {
        DisplayId(1)
    }

    fn uuid(&self) -> anyhow::Result<uuid::Uuid> {
        Ok(self.uuid)
    }

    fn bounds(&self) -> Bounds<Pixels> {
        Bounds::from_corners(Point::default(), Point::new(px(3840.), px(2160.)))
    }
}

#[derive(Default)]
pub(crate) struct HeadlessCallbacks {
    pub(crate) request_frame: Option<Box<dyn FnMut(RequestFrameOptions)>>,
    pub(crate) input: Option<Box<dyn FnMut(PlatformInput) -> DispatchEventResult>>,
    pub(crate) resize: Option<Box<dyn FnMut(Size<Pixels>, f32)>>,
    pub(crate) close: Option<Box<dyn FnOnce()>>,
    pub(crate) should_close: Option<Box<dyn FnMut() -> bool>>,
}

pub(crate) struct HeadlessWindowState {
    pub(crate) bounds: Bounds<Pixels>,
    pub(crate) appearance: WindowAppearance,
    pub(crate) title: String,
    pub(crate) decorations: WindowDecorations,
    pub(crate) fullscreen: bool,
    pub(crate) maximized: bool,
    /// A size asked for by the app, applied (resize callback) on the next tick.
    pub(crate) pending_resize: Option<Size<Pixels>>,
    /// The platform window was dropped: its close callback runs on the next tick.
    pub(crate) closed: bool,
    pub(crate) input_handler: Option<PlatformInputHandler>,
    pub(crate) sprite_atlas: Arc<HeadlessAtlas>,
}

/// Shared between the window gpui holds and the client's frame timer.
#[derive(Clone)]
pub(crate) struct HeadlessWindowInner {
    pub(crate) state: Rc<RefCell<HeadlessWindowState>>,
    pub(crate) callbacks: Rc<RefCell<HeadlessCallbacks>>,
}

impl HeadlessWindowInner {
    /// One frame: a pending resize, then a frame request (gpui draws when
    /// something changed). Returns false once the window has closed.
    pub(crate) fn tick(&self) -> bool {
        if self.state.borrow().closed {
            let close = self.callbacks.borrow_mut().close.take();
            if let Some(close) = close {
                close();
            }
            return false;
        }
        let resize = self.state.borrow_mut().pending_resize.take();
        if let Some(size) = resize {
            self.state.borrow_mut().bounds.size = size;
            let callback = self.callbacks.borrow_mut().resize.take();
            if let Some(mut callback) = callback {
                callback(size, 1.0);
                let mut callbacks = self.callbacks.borrow_mut();
                if callbacks.resize.is_none() {
                    callbacks.resize = Some(callback);
                }
            }
        }
        let callback = self.callbacks.borrow_mut().request_frame.take();
        if let Some(mut callback) = callback {
            callback(RequestFrameOptions::default());
            let mut callbacks = self.callbacks.borrow_mut();
            if callbacks.request_frame.is_none() {
                callbacks.request_frame = Some(callback);
            }
        }
        true
    }
}

pub(crate) struct HeadlessWindow(pub(crate) HeadlessWindowInner);

impl HeadlessWindow {
    pub(crate) fn new(params: WindowParams, appearance: WindowAppearance) -> Self {
        let mut bounds = params.bounds;
        // The harness's test monitor: windows start at its top-left corner.
        bounds.origin = Point::default();
        Self(HeadlessWindowInner {
            state: Rc::new(RefCell::new(HeadlessWindowState {
                bounds,
                appearance,
                title: String::new(),
                decorations: WindowDecorations::Server,
                fullscreen: false,
                maximized: false,
                pending_resize: None,
                closed: false,
                input_handler: None,
                sprite_atlas: Arc::new(HeadlessAtlas::new()),
            })),
            callbacks: Rc::new(RefCell::new(HeadlessCallbacks::default())),
        })
    }
}

impl Drop for HeadlessWindow {
    fn drop(&mut self) {
        self.0.state.borrow_mut().closed = true;
    }
}

impl HasWindowHandle for HeadlessWindow {
    fn window_handle(
        &self,
    ) -> Result<raw_window_handle::WindowHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::NotSupported)
    }
}

impl HasDisplayHandle for HeadlessWindow {
    fn display_handle(
        &self,
    ) -> Result<raw_window_handle::DisplayHandle<'_>, raw_window_handle::HandleError> {
        Err(raw_window_handle::HandleError::NotSupported)
    }
}

impl PlatformWindow for HeadlessWindow {
    fn bounds(&self) -> Bounds<Pixels> {
        self.0.state.borrow().bounds
    }

    fn is_maximized(&self) -> bool {
        self.0.state.borrow().maximized
    }

    fn window_bounds(&self) -> WindowBounds {
        let state = self.0.state.borrow();
        if state.fullscreen {
            WindowBounds::Fullscreen(state.bounds)
        } else if state.maximized {
            WindowBounds::Maximized(state.bounds)
        } else {
            WindowBounds::Windowed(state.bounds)
        }
    }

    fn content_size(&self) -> Size<Pixels> {
        self.0.state.borrow().bounds.size
    }

    fn resize(&mut self, size: Size<Pixels>) {
        self.0.state.borrow_mut().pending_resize = Some(size);
    }

    fn scale_factor(&self) -> f32 {
        1.0
    }

    fn appearance(&self) -> WindowAppearance {
        self.0.state.borrow().appearance
    }

    fn display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(HeadlessDisplay::new()))
    }

    fn mouse_position(&self) -> Point<Pixels> {
        Point::default()
    }

    fn modifiers(&self) -> Modifiers {
        Modifiers::default()
    }

    fn capslock(&self) -> Capslock {
        Capslock::default()
    }

    fn set_input_handler(&mut self, input_handler: PlatformInputHandler) {
        self.0.state.borrow_mut().input_handler = Some(input_handler);
    }

    fn take_input_handler(&mut self) -> Option<PlatformInputHandler> {
        self.0.state.borrow_mut().input_handler.take()
    }

    fn prompt(
        &self,
        _level: crate::PromptLevel,
        _msg: &str,
        _detail: Option<&str>,
        _answers: &[PromptButton],
    ) -> Option<futures::channel::oneshot::Receiver<usize>> {
        // gpui draws its own prompt in the window, as on Wayland.
        None
    }

    fn activate(&self) {}

    fn is_active(&self) -> bool {
        // As the harness's Hyprland rule (`nofocus`): test windows never
        // take the keyboard focus of the session.
        false
    }

    fn is_hovered(&self) -> bool {
        false
    }

    fn set_title(&mut self, title: &str) {
        self.0.state.borrow_mut().title = title.to_owned();
    }

    fn get_title(&self) -> String {
        self.0.state.borrow().title.clone()
    }

    fn set_background_appearance(&self, _background: WindowBackgroundAppearance) {}

    fn minimize(&self) {}

    fn zoom(&self) {
        let mut state = self.0.state.borrow_mut();
        state.maximized = !state.maximized;
    }

    fn toggle_fullscreen(&self) {
        let mut state = self.0.state.borrow_mut();
        state.fullscreen = !state.fullscreen;
    }

    fn is_fullscreen(&self) -> bool {
        self.0.state.borrow().fullscreen
    }

    fn on_request_frame(&self, callback: Box<dyn FnMut(RequestFrameOptions)>) {
        self.0.callbacks.borrow_mut().request_frame = Some(callback);
    }

    fn on_input(&self, callback: Box<dyn FnMut(PlatformInput) -> DispatchEventResult>) {
        self.0.callbacks.borrow_mut().input = Some(callback);
    }

    fn on_active_status_change(&self, _callback: Box<dyn FnMut(bool)>) {}

    fn on_hover_status_change(&self, _callback: Box<dyn FnMut(bool)>) {}

    fn on_resize(&self, callback: Box<dyn FnMut(Size<Pixels>, f32)>) {
        self.0.callbacks.borrow_mut().resize = Some(callback);
    }

    fn on_moved(&self, _callback: Box<dyn FnMut()>) {}

    fn on_should_close(&self, callback: Box<dyn FnMut() -> bool>) {
        self.0.callbacks.borrow_mut().should_close = Some(callback);
    }

    fn on_hit_test_window_control(&self, _callback: Box<dyn FnMut() -> Option<WindowControlArea>>) {
    }

    fn on_close(&self, callback: Box<dyn FnOnce()>) {
        self.0.callbacks.borrow_mut().close = Some(callback);
    }

    fn on_appearance_changed(&self, _callback: Box<dyn FnMut()>) {}

    fn draw(&self, _scene: &Scene) {}

    fn sprite_atlas(&self) -> Arc<dyn PlatformAtlas> {
        self.0.state.borrow().sprite_atlas.clone()
    }

    fn request_decorations(&self, decorations: WindowDecorations) {
        self.0.state.borrow_mut().decorations = decorations;
    }

    fn window_decorations(&self) -> Decorations {
        match self.0.state.borrow().decorations {
            WindowDecorations::Server => Decorations::Server,
            WindowDecorations::Client => Decorations::Client {
                tiling: Tiling::default(),
            },
        }
    }

    fn update_ime_position(&self, _bounds: Bounds<Pixels>) {}

    fn gpu_specs(&self) -> Option<GpuSpecs> {
        None
    }
}

/// An atlas that hands out tiles without storing pixels (nothing is drawn).
pub(crate) struct HeadlessAtlas(Mutex<HeadlessAtlasState>);

struct HeadlessAtlasState {
    next_id: u32,
    tiles: HashMap<AtlasKey, AtlasTile>,
}

impl HeadlessAtlas {
    fn new() -> Self {
        Self(Mutex::new(HeadlessAtlasState {
            next_id: 0,
            tiles: HashMap::default(),
        }))
    }
}

impl PlatformAtlas for HeadlessAtlas {
    fn get_or_insert_with<'a>(
        &self,
        key: &AtlasKey,
        build: &mut dyn FnMut() -> anyhow::Result<
            Option<(Size<crate::DevicePixels>, std::borrow::Cow<'a, [u8]>)>,
        >,
    ) -> anyhow::Result<Option<AtlasTile>> {
        if let Some(tile) = self.0.lock().tiles.get(key) {
            return Ok(Some(tile.clone()));
        }
        let Some((size, _)) = build()? else {
            return Ok(None);
        };
        let mut state = self.0.lock();
        state.next_id += 1;
        let texture = state.next_id;
        state.next_id += 1;
        let tile = AtlasTile {
            texture_id: AtlasTextureId {
                index: texture,
                kind: crate::AtlasTextureKind::Monochrome,
            },
            tile_id: TileId(state.next_id),
            padding: 0,
            bounds: Bounds {
                origin: Point::default(),
                size,
            },
        };
        state.tiles.insert(key.clone(), tile.clone());
        Ok(Some(tile))
    }

    fn remove(&self, key: &AtlasKey) {
        self.0.lock().tiles.remove(key);
    }

    fn stats(&self) -> crate::AtlasStats {
        let state = self.0.lock();
        let mut stats = crate::AtlasStats {
            tiles: state.tiles.len(),
            textures: state.tiles.len(),
            ..Default::default()
        };
        for (key, tile) in &state.tiles {
            let colour = key.texture_kind() == crate::AtlasTextureKind::Polychrome;
            stats.colour_tiles += usize::from(colour);
            stats.bytes += tile.bounds.size.width.0.max(0) as usize
                * tile.bounds.size.height.0.max(0) as usize
                * if colour { 4 } else { 1 };
        }
        stats
    }
}
