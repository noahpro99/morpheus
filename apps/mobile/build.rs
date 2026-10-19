//! Android: keep and export libwebrtc's JNI functions in the app library.
//! WebRTC's Java classes (`libwebrtc.jar`) call into them by name; webrtc-sys
//! works out the list, but its linker arguments only reach its own targets,
//! so the app has to ask for them itself.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android")
        && let Err(err) = webrtc_sys_build::configure_jni_symbols()
    {
        println!("cargo:warning=calls will not work: {err}");
    }
}
