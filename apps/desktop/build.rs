//! macOS: link libwebrtc with `-ObjC`. Its Objective-C categories (on
//! NSString and others) are only kept when the linker is told to load every
//! object holding Objective-C; without them the app aborts as it loads
//! ("+[NSString stringForStdString:]: unrecognized selector"). webrtc-sys
//! asks for this, but its linker arguments only reach its own targets, so
//! the app has to ask for it itself.
//!
//! Windows: the program's icon (the file's icon in Explorer, the taskbar's).
//! GPUI's own build script embeds its manifest (per-monitor DPI awareness,
//! common controls) only when it runs on Windows, so a build cross-compiled
//! from Linux gets the same manifest here.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // What the diagnostics (Settings > Help & Feedback) say about this build.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short=10", "HEAD"])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=MORPHEUS_COMMIT={commit}");
    let lock = std::fs::read_to_string("../../Cargo.lock").unwrap_or_default();
    let sdk = lock
        .split("[[package]]")
        .find(|package| package.contains("name = \"matrix-sdk\"\n"))
        .and_then(|package| package.lines().find_map(|l| l.strip_prefix("version = ")))
        .map_or("unknown", |v| v.trim_matches('"'));
    println!("cargo:rustc-env=MORPHEUS_MATRIX_SDK={sdk}");
    println!("cargo:rerun-if-changed=windows");
    println!("cargo:rerun-if-changed=icons/icon.ico");
    match std::env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("macos" | "ios") => println!("cargo:rustc-link-arg=-ObjC"),
        Ok("windows") => {
            let rc = if cfg!(windows) {
                "windows/icon.rc"
            } else {
                "windows/icon-manifest.rc"
            };
            embed_resource::compile(rc, embed_resource::NONE)
                .manifest_required()
                .unwrap_or_else(|err| panic!("embedding the Windows resources failed: {err}"));
        }
        _ => {}
    }
}
