//! Windows: the launcher's icon (the file's icon in Explorer, the pin's, the
//! Start menu's). The launcher has no window of its own.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=windows");
    println!("cargo:rerun-if-changed=../desktop/icons/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile("windows/icon.rc", embed_resource::NONE)
            .manifest_optional()
            .unwrap_or_else(|err| panic!("embedding the launcher's icon failed: {err}"));
    }
}
