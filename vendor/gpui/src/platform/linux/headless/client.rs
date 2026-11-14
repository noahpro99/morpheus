use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use calloop::timer::{TimeoutAction, Timer};
use calloop::{EventLoop, LoopHandle};
use util::ResultExt;

use super::window::{HeadlessDisplay, HeadlessWindow, HeadlessWindowInner};
use crate::platform::linux::LinuxClient;
use crate::platform::{LinuxCommon, PlatformWindow};
use crate::{
    AnyWindowHandle, CursorStyle, DisplayId, LinuxKeyboardLayout, PlatformDisplay,
    PlatformKeyboardLayout, WindowParams,
};

pub struct HeadlessClientState {
    pub(crate) _loop_handle: LoopHandle<'static, HeadlessClient>,
    pub(crate) event_loop: Option<calloop::EventLoop<'static, HeadlessClient>>,
    pub(crate) common: LinuxCommon,
    /// Morpheus patch: offscreen windows (see `window.rs`), framed by a
    /// 60 Hz timer, and a clipboard of the process's own.
    pub(crate) windows: Vec<(AnyWindowHandle, HeadlessWindowInner)>,
    pub(crate) clipboard: Option<crate::ClipboardItem>,
    pub(crate) primary: Option<crate::ClipboardItem>,
}

#[derive(Clone)]
pub(crate) struct HeadlessClient(Rc<RefCell<HeadlessClientState>>);

/// How often an offscreen window is offered a frame (a 60 Hz display).
const FRAME: Duration = Duration::from_micros(16_667);

impl HeadlessClient {
    pub(crate) fn new() -> Self {
        let event_loop = EventLoop::try_new().unwrap();

        let (common, main_receiver) = LinuxCommon::new(event_loop.get_signal());

        let handle = event_loop.handle();

        handle
            .insert_source(main_receiver, |event, _, _: &mut HeadlessClient| {
                if let calloop::channel::Event::Msg(runnable) = event {
                    runnable.run();
                }
            })
            .ok();

        handle
            .insert_source(
                Timer::from_duration(FRAME),
                |_, _, client: &mut HeadlessClient| {
                    client.tick();
                    TimeoutAction::ToDuration(FRAME)
                },
            )
            .ok();

        HeadlessClient(Rc::new(RefCell::new(HeadlessClientState {
            event_loop: Some(event_loop),
            _loop_handle: handle,
            common,
            windows: Vec::new(),
            clipboard: None,
            primary: None,
        })))
    }

    /// One frame for every open window, with nothing borrowed while gpui runs.
    fn tick(&self) {
        let windows: Vec<_> = self
            .0
            .borrow()
            .windows
            .iter()
            .map(|(_, window)| window.clone())
            .collect();
        let mut closed = Vec::new();
        for window in windows {
            if !window.tick() {
                closed.push(window.state.clone());
            }
        }
        if !closed.is_empty() {
            self.0.borrow_mut().windows.retain(|(_, window)| {
                !closed.iter().any(|c| Rc::ptr_eq(c, &window.state))
            });
        }
    }
}

impl LinuxClient for HeadlessClient {
    fn with_common<R>(&self, f: impl FnOnce(&mut LinuxCommon) -> R) -> R {
        f(&mut self.0.borrow_mut().common)
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        Box::new(LinuxKeyboardLayout::new("unknown".into()))
    }

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        vec![Rc::new(HeadlessDisplay::new())]
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(HeadlessDisplay::new()))
    }

    fn display(&self, _id: DisplayId) -> Option<Rc<dyn PlatformDisplay>> {
        Some(Rc::new(HeadlessDisplay::new()))
    }

    #[cfg(feature = "screen-capture")]
    fn is_screen_capture_supported(&self) -> bool {
        false
    }

    #[cfg(feature = "screen-capture")]
    fn screen_capture_sources(
        &self,
    ) -> futures::channel::oneshot::Receiver<anyhow::Result<Vec<Rc<dyn crate::ScreenCaptureSource>>>>
    {
        let (mut tx, rx) = futures::channel::oneshot::channel();
        tx.send(Err(anyhow::anyhow!(
            "Headless mode does not support screen capture."
        )))
        .ok();
        rx
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        None
    }

    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>> {
        Some(
            self.0
                .borrow()
                .windows
                .iter()
                .rev()
                .map(|(handle, _)| *handle)
                .collect(),
        )
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        params: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>> {
        let mut state = self.0.borrow_mut();
        let window = HeadlessWindow::new(params, state.common.appearance);
        state.windows.push((handle, window.0.clone()));
        Ok(Box::new(window))
    }

    fn compositor_name(&self) -> &'static str {
        "headless"
    }

    fn set_cursor_style(&self, _style: CursorStyle) {}

    fn open_uri(&self, _uri: &str) {}

    fn reveal_path(&self, _path: std::path::PathBuf) {}

    fn write_to_primary(&self, item: crate::ClipboardItem) {
        self.0.borrow_mut().primary = Some(item);
    }

    fn write_to_clipboard(&self, item: crate::ClipboardItem) {
        self.0.borrow_mut().clipboard = Some(item);
    }

    fn read_from_primary(&self) -> Option<crate::ClipboardItem> {
        self.0.borrow().primary.clone()
    }

    fn read_from_clipboard(&self) -> Option<crate::ClipboardItem> {
        self.0.borrow().clipboard.clone()
    }

    fn run(&self) {
        let mut event_loop = self
            .0
            .borrow_mut()
            .event_loop
            .take()
            .expect("App is already running");

        event_loop.run(None, &mut self.clone(), |_| {}).log_err();
    }
}
