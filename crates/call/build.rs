//! libwebrtc's screen capture on Linux talks to the desktop portal through
//! GIO, which it leaves to the program to link (the app gets it with GTK;
//! this crate's tests need it too).

fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("linux")
        && let Err(err) = pkg_config::probe_library("gio-2.0")
    {
        println!("cargo:warning=GIO not found, screen sharing will not link: {err}");
    }
}
