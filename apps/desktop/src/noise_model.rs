//! The low-delay noise suppression model (#37), in Settings > Voice & Video >
//! Noise suppression: choosing it downloads the file (36 MB, once) into the
//! data directory, with progress and errors shown in the setting; it is used
//! only once its SHA-256 has been checked. The default model is built in.

use crate::view_model::i18n::t;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU8, Ordering},
};

use gpui::{AnyElement, Context, IntoElement, MouseButton, div, prelude::*, px, rgb, rgba};
use morpheus_call::NoiseModel;
use morpheus_core::DownloadProgress;

use crate::{e2e::E2eExt as _, shell::Shell, theme};

/// Where the downloaded models live.
pub fn dir() -> std::path::PathBuf {
    crate::data_dir().join("models")
}

/// Tell the call crate where to look (at startup).
pub fn init() {
    morpheus_call::set_model_dir(dir());
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Downloading,
    Failed(String),
}

static STATE: Mutex<State> = Mutex::new(State::Idle);
static PROGRESS: Mutex<Option<Arc<DownloadProgress>>> = Mutex::new(None);
/// Whether the file is there and checked: 0 not looked yet, 1 yes, 2 no.
static READY: AtomicU8 = AtomicU8::new(0);

fn state() -> State {
    STATE.lock().map(|s| s.clone()).unwrap_or(State::Idle)
}

fn set_state(state: State) {
    if let Ok(mut now) = STATE.lock() {
        *now = state;
    }
}

/// Whether the low-delay model is downloaded and checked (looked at once,
/// until a download ends).
pub fn ready() -> bool {
    match READY.load(Ordering::Relaxed) {
        1 => true,
        2 => false,
        _ => {
            let ok = morpheus_call::low_delay_path()
                .is_some_and(|path| morpheus_call::read_low_delay(&path).is_ok());
            READY.store(if ok { 1 } else { 2 }, Ordering::Relaxed);
            ok
        }
    }
}

/// Megabytes, as shown.
fn mb(bytes: u64) -> String {
    format!("{:.0}", bytes as f64 / 1_000_000.0)
}

/// What the setting says under its choices.
pub fn status_text() -> Option<String> {
    match state() {
        State::Downloading => {
            let (done, total) = PROGRESS
                .lock()
                .ok()
                .and_then(|p| p.clone())
                .map(|p| (p.done.load(Ordering::Relaxed), p.total.load(Ordering::Relaxed)))
                .unwrap_or_default();
            let total = if total == 0 { morpheus_call::LOW_DELAY_BYTES } else { total };
            Some(crate::view_model::i18n::tf("Downloading the model: %{done} of %{total} MB", &[("done", &mb(done).to_string()), ("total", &mb(total).to_string())]))
        }
        State::Failed(why) => Some(crate::view_model::i18n::tf("Could not download the model: %{why}", &[("why", &why.to_string())])),
        State::Idle => None,
    }
}

impl Shell {
    /// A model was picked in Settings: the built-in one at once; the low-delay
    /// one at once if it is downloaded, else downloaded first (and used when
    /// it has been checked).
    pub fn choose_noise_model(&mut self, model: NoiseModel, cx: &mut Context<Self>) {
        let low = model == NoiseModel::DeepFilterNet3LowDelay;
        if !low || ready() {
            crate::noise::tune(|t| morpheus_call::NoiseTuning { model, ..t });
            self.refresh_settings(cx);
            cx.notify();
            return;
        }
        if state() == State::Downloading {
            return;
        }
        let (Some(session), Some(path)) = (self.session.clone(), morpheus_call::low_delay_path())
        else {
            set_state(State::Failed("there is no place to keep it".into()));
            self.refresh_settings(cx);
            return;
        };
        let progress = Arc::new(DownloadProgress::default());
        if let Ok(mut p) = PROGRESS.lock() {
            *p = Some(progress.clone());
        }
        set_state(State::Downloading);
        let fetch = session.download_verified(
            morpheus_call::low_delay_url(),
            path,
            morpheus_call::LOW_DELAY_SHA256.to_owned(),
            morpheus_call::LOW_DELAY_BYTES,
            progress,
        );
        cx.spawn(async move |this, cx| poll_download(cx, &this, fetch).await)
        .detach();
        self.refresh_settings(cx);
    }
}

/// Redraw the setting now and then while `fetch` runs, then act on its end.
async fn poll_download(
    cx: &mut gpui::AsyncApp,
    this: &gpui::WeakEntity<Shell>,
    fetch: impl std::future::Future<Output = Result<(), morpheus_core::Error>>,
) {
    let mut fetch = std::pin::pin!(fetch);
    let result = loop {
        let tick = cx.background_executor().timer(std::time::Duration::from_millis(250));
        match futures::future::select(fetch.as_mut(), tick).await {
            futures::future::Either::Left((result, _)) => break result,
            futures::future::Either::Right(_) => {
                this.update(cx, |this, cx| this.refresh_settings(cx)).ok();
            }
        }
    };
    READY.store(0, Ordering::Relaxed);
    this.update(cx, |this, cx| {
        match result {
            Ok(()) if ready() => {
                set_state(State::Idle);
                crate::noise::tune(|t| morpheus_call::NoiseTuning {
                    model: NoiseModel::DeepFilterNet3LowDelay,
                    ..t
                });
            }
            Ok(()) => set_state(State::Failed("the file did not check out".into())),
            Err(err) => set_state(State::Failed(err.to_string())),
        }
        this.refresh_settings(cx);
        cx.notify();
    })
    .ok();
}

/// The model choice for Settings: the built-in model (30 ms of delay) or the
/// low-delay one (10 ms, a 36 MB download the first time), with progress or
/// the error beneath.
pub fn picker<V: 'static>(
    cx: &mut Context<V>,
    pick: impl Fn(&mut V, NoiseModel, &mut Context<V>) + Clone + 'static,
) -> AnyElement {
    let current = morpheus_call::noise_tuning().model;
    let downloaded = ready();
    let busy = state() == State::Downloading;
    let choices = NoiseModel::ALL.into_iter().map(|model| {
        let pick = pick.clone();
        let active = model == current && (downloaded || model == NoiseModel::DeepFilterNet3);
        let low = model == NoiseModel::DeepFilterNet3LowDelay;
        let label = if low {
            if downloaded {
                "Low delay (10 ms)".to_owned()
            } else {
                crate::view_model::i18n::tf("Low delay (10 ms, %{size} MB download)", &[("size", &mb(morpheus_call::LOW_DELAY_BYTES).to_string())])
            }
        } else {
            "Standard delay (30 ms, built in)".to_owned()
        };
        div()
            .id(gpui::SharedString::from(format!("noise-model-{}", model.key())))
            .flex_1()
            .px(px(10.0))
            .py(px(8.0))
            .rounded(px(theme::RADIUS_SM))
            .text_center()
            .cursor_pointer()
            .when(active, |d| d.bg(rgb(theme::accent_solid())).text_color(rgb(theme::on_accent_solid())))
            .when(!active, |d| {
                d.text_color(rgb(theme::text_muted()))
                    .hover(|s| s.bg(rgba(theme::bg_selected())).text_color(rgb(theme::text())))
            })
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(cx.listener(move |this, _, _, cx| pick(this, model, cx)))
            .child(label.clone())
            .e2e(
                "button",
                if active { "noise-model-choice active" } else { "noise-model-choice" },
            )
            .attr("data-value", model.key())
            .text(label)
            .into_any_element()
    });
    let status = status_text().map(|text| {
        let failed = matches!(state(), State::Failed(_));
        div()
            .text_size(px(0.8 * theme::REM))
            .text_color(if failed { rgb(theme::danger()) } else { rgb(theme::text_muted()) })
            .child(text.clone())
            .e2e("div", if failed { "noise-model-status failed" } else { "noise-model-status" })
            .text(text)
            .into_any_element()
    });
    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(div().font_weight(gpui::FontWeight::SEMIBOLD).child(t("Strong's model")))
        .child(div().flex().gap(px(8.0)).children(choices))
        .child(
            div()
                .text_size(px(0.8 * theme::REM))
                .text_color(rgb(theme::text_muted()))
                .child(t("The low-delay model sends your voice 20 ms sooner; it is downloaded once, when you choose it.")),
        )
        .children(status)
        .e2e("div", "noise-model")
        .attr("data-busy", busy.to_string())
        .attr("data-downloaded", downloaded.to_string())
        .attr("data-chosen", current.key())
        .into_any_element()
}
