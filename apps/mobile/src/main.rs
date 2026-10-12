//! Mobile entry point. Same UI as desktop; only the renderer and the
//! storage location differ. The shared shell switches to one pane at a
//! time on phone-width windows by itself.

use std::path::PathBuf;

use dioxus::prelude::*;
use morpheus_ui::AppConfig;
use tracing_subscriber::EnvFilter;

const DEFAULT_HOMESERVER: &str = "http://10.0.2.2:8008"; // host machine from the Android emulator

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "morpheus=debug,warn".into()),
        )
        .init();

    let config = AppConfig {
        data_dir: data_dir(),
        default_homeserver: std::env::var("MORPHEUS_HOMESERVER")
            .unwrap_or_else(|_| DEFAULT_HOMESERVER.into()),
    };
    tracing::info!(?config.data_dir, "starting morpheus mobile");

    LaunchBuilder::new()
        .with_context(config)
        .launch(morpheus_ui::App);
}

/// The app-private files directory: Android's `Context.getFilesDir()`,
/// the sandbox's Documents directory on iOS, else an override or a temp
/// directory so the app boots anywhere.
fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("MORPHEUS_DATA_DIR") {
        return PathBuf::from(dir);
    }
    platform_data_dir().unwrap_or_else(|| std::env::temp_dir().join("morpheus"))
}

#[cfg(target_os = "android")]
fn platform_data_dir() -> Option<PathBuf> {
    android::files_dir()
}

#[cfg(target_os = "ios")]
fn platform_data_dir() -> Option<PathBuf> {
    // The sandbox's home is the app container; Documents is backed up and
    // persists across updates.
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Documents").join("morpheus"))
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn platform_data_dir() -> Option<PathBuf> {
    None
}

/// `Context.getFilesDir()` through JNI. The only unsafe code in the
/// workspace: wrapping the raw JavaVM and Context pointers that
/// `ndk-context` hands over from the activity.
#[cfg(target_os = "android")]
mod android {
    use std::path::PathBuf;

    use jni::objects::{JObject, JString};

    #[allow(unsafe_code)]
    pub fn files_dir() -> Option<PathBuf> {
        let ctx = ndk_context::android_context();
        // SAFETY: `ndk-context` guarantees these pointers come from the
        // running activity and stay valid for the process's lifetime.
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
        let mut env = vm.attach_current_thread().ok()?;
        let context = unsafe { JObject::from_raw(ctx.context().cast()) };
        let dir = env
            .call_method(&context, "getFilesDir", "()Ljava/io/File;", &[])
            .ok()?
            .l()
            .ok()?;
        let path = env
            .call_method(&dir, "getAbsolutePath", "()Ljava/lang/String;", &[])
            .ok()?
            .l()
            .ok()?;
        let path: String = env.get_string(&JString::from(path)).ok()?.into();
        Some(PathBuf::from(path).join("morpheus"))
    }
}
