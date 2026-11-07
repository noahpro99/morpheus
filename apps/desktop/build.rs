//! macOS: link libwebrtc with `-ObjC`. Its Objective-C categories (on
//! NSString and others) are only kept when the linker is told to load every
//! object holding Objective-C; without them the app aborts as it loads
//! ("+[NSString stringForStdString:]: unrecognized selector"). webrtc-sys
//! asks for this, but its linker arguments only reach its own targets, so
//! the app has to ask for it itself.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if matches!(
        std::env::var("CARGO_CFG_TARGET_OS").as_deref(),
        Ok("macos" | "ios")
    ) {
        println!("cargo:rustc-link-arg=-ObjC");
    }
}
