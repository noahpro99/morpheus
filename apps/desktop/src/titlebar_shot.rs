//! `MORPHEUS_TITLEBAR_SHOT=<seconds>`: opens the app's window on a shell with
//! no account (the real title bar, "Direct messages" and the logo in it) for
//! that long and quits, so CI can take a screenshot of it (apple.yml). No
//! account, no keychain, no network.

use std::time::Duration;

use gpui::{App, AppContext as _, Application};

use crate::shell::Shell;

/// How long to show the window, when asked for.
pub fn requested() -> Option<u64> {
    std::env::var("MORPHEUS_TITLEBAR_SHOT").ok()?.parse().ok()
}

pub fn run(seconds: u64) -> ! {
    Application::new().with_assets(crate::Assets).run(move |cx: &mut App| {
        crate::load_fonts(cx);
        let options = crate::window_options(cx);
        if let Err(e) = cx.open_window(options, |_, cx| cx.new(|_| Shell::bare(None))) {
            eprintln!("morpheus-desktop: titlebar shot: opening the window failed: {e}");
            std::process::exit(1);
        }
        #[cfg(target_os = "macos")]
        cx.activate(true);
        eprintln!("morpheus-desktop: titlebar shot: window open");
        // The GPU diagnostics name the GPU on this system (#277): CI greps this.
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_secs(3)).await;
            let line = match gpui::gpu_info().and_then(|i| i.drawing) {
                Some((name, driver, _)) if !name.is_empty() => format!("gpu self-check: ok: {name} ({driver})\n"),
                _ => "gpu self-check: failed: the diagnostics say the GPU is unknown\n".to_owned(),
            };
            eprint!("{line}");
            // A release build on Windows has no console: a file, if asked.
            if let Some(out) = std::env::var_os("MORPHEUS_TITLEBAR_SHOT_OUT") {
                let _ = std::fs::write(out, &line);
            }
            let _ = cx;
        })
        .detach();
        cx.spawn(async move |cx| {
            cx.background_executor().timer(Duration::from_secs(seconds)).await;
            cx.update(|cx| cx.quit()).ok();
        })
        .detach();
    });
    std::process::exit(0)
}
