//! Mobile entry point. Same UI as desktop; only the renderer and the
//! storage location differ. The shared shell switches to one pane at a
//! time on phone-width windows by itself.

use std::path::PathBuf;

use dioxus::prelude::*;
use morpheus_ui::AppConfig;
use tracing_subscriber::EnvFilter;

#[cfg(target_os = "android")]
mod push;

const DEFAULT_HOMESERVER: &str = "http://10.0.2.2:8008"; // host machine from the Android emulator

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "morpheus=debug,warn".into()),
        )
        .init();

    #[allow(unused_mut)]
    let mut config = AppConfig::new(
        data_dir(),
        std::env::var("MORPHEUS_HOMESERVER").unwrap_or_else(|_| DEFAULT_HOMESERVER.into()),
    );
    #[cfg(target_os = "android")]
    {
        config.open_url = Some(std::sync::Arc::new(android::open_url));
        // Calls: WebRTC set up once; the microphone asked for at the first call.
        android::init_webrtc();
        config.microphone = Some(std::sync::Arc::new(android::microphone));
        config.speakerphone = Some(std::sync::Arc::new(android::speakerphone));
        // Test builds send a tone instead (no environment to say so here).
        #[cfg(feature = "e2e")]
        morpheus_call::use_test_tone();
        // Push notifications through the phone's UnifiedPush app (see push.rs).
        let dir = config.data_dir.clone();
        config.on_session = Some(std::sync::Arc::new(move |session| {
            push::on_session(session, dir.clone())
        }));
    }
    #[cfg(target_os = "android")]
    push::watch_open_room(config.data_dir.clone(), config.commands.clone());
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

    use jni::objects::{JObject, JString, JValue};

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

    /// Show `url` in the browser: an `ACTION_VIEW` intent started from the
    /// app's context (single sign-on). The desktop opener cannot work here.
    #[allow(unsafe_code)]
    pub fn open_url(url: &str) -> std::io::Result<()> {
        let failed = |what: &str| std::io::Error::other(format!("opening the browser: {what}"));
        let ctx = ndk_context::android_context();
        // SAFETY: as in `files_dir`.
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.map_err(|_| failed("no VM"))?;
        let mut env = vm.attach_current_thread().map_err(|_| failed("attach"))?;
        let context = unsafe { JObject::from_raw(ctx.context().cast()) };
        let result = (|| -> jni::errors::Result<()> {
            let url = env.new_string(url)?;
            let uri = env
                .call_static_method(
                    "android/net/Uri",
                    "parse",
                    "(Ljava/lang/String;)Landroid/net/Uri;",
                    &[JValue::Object(&url)],
                )?
                .l()?;
            let action = env.new_string("android.intent.action.VIEW")?;
            let intent = env.new_object(
                "android/content/Intent",
                "(Ljava/lang/String;Landroid/net/Uri;)V",
                &[JValue::Object(&action), JValue::Object(&uri)],
            )?;
            // Started from outside an activity's own call stack.
            const FLAG_ACTIVITY_NEW_TASK: i32 = 0x1000_0000;
            env.call_method(
                &intent,
                "addFlags",
                "(I)Landroid/content/Intent;",
                &[JValue::Int(FLAG_ACTIVITY_NEW_TASK)],
            )?;
            env.call_method(
                &context,
                "startActivity",
                "(Landroid/content/Intent;)V",
                &[JValue::Object(&intent)],
            )?;
            Ok(())
        })();
        if result.is_err() {
            // No browser installed (ActivityNotFoundException) and the like.
            let _ = env.exception_clear();
        }
        result.map_err(|err| failed(&err.to_string()))
    }

    /// Calls: WebRTC needs the Java VM and the application context (for the
    /// microphone and speakers) before any call starts.
    #[allow(unsafe_code)]
    pub fn init_webrtc() {
        let ctx = ndk_context::android_context();
        // SAFETY: as in `files_dir`.
        let Ok(vm) = (unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }) else {
            return;
        };
        let Ok(mut env) = vm.attach_current_thread() else {
            return;
        };
        let context = unsafe { JObject::from_raw(ctx.context().cast()) };
        let app = env
            .call_method(
                &context,
                "getApplicationContext",
                "()Landroid/content/Context;",
                &[],
            )
            .and_then(|value| value.l());
        let app = match app {
            Ok(app) => app,
            Err(_) => {
                let _ = env.exception_clear();
                context
            }
        };
        if !morpheus_call::initialize_android_context(&vm, &app) {
            tracing::warn!("WebRTC could not take the app context; calls may have no audio");
        }
    }

    const RECORD_AUDIO: &str = "android.permission.RECORD_AUDIO";

    /// Call audio out of the loudspeaker (`true`) or the earpiece, through
    /// `AudioManager` (WebRTC leaves the route to the system: the earpiece).
    #[allow(unsafe_code)]
    pub fn speakerphone(on: bool) {
        tracing::debug!(on, "call audio route");
        let ctx = ndk_context::android_context();
        // SAFETY: as in `files_dir`.
        let Ok(vm) = (unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }) else {
            return;
        };
        let Ok(mut env) = vm.attach_current_thread() else {
            return;
        };
        let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
        let routed = (|| -> jni::errors::Result<()> {
            let name = env.new_string("audio")?;
            let manager = env
                .call_method(
                    &activity,
                    "getSystemService",
                    "(Ljava/lang/String;)Ljava/lang/Object;",
                    &[JValue::Object(&name)],
                )?
                .l()?;
            env.call_method(
                &manager,
                "setSpeakerphoneOn",
                "(Z)V",
                &[JValue::Bool(u8::from(on))],
            )?;
            Ok(())
        })();
        if let Err(err) = routed {
            let _ = env.exception_clear();
            tracing::warn!(%err, "could not route call audio");
        }
    }

    /// Ask for the microphone (a runtime permission) and wait for the
    /// answer: the system dialog shows over the app. `true` once granted.
    #[allow(unsafe_code)]
    pub fn microphone() -> bool {
        let ctx = ndk_context::android_context();
        // SAFETY: as in `files_dir`.
        let Ok(vm) = (unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }) else {
            return false;
        };
        let Ok(mut env) = vm.attach_current_thread() else {
            return false;
        };
        let activity = unsafe { JObject::from_raw(ctx.context().cast()) };
        let granted = |env: &mut jni::JNIEnv| -> bool {
            let Ok(name) = env.new_string(RECORD_AUDIO) else {
                return false;
            };
            let answer = env
                .call_method(
                    &activity,
                    "checkSelfPermission",
                    "(Ljava/lang/String;)I",
                    &[JValue::Object(&name)],
                )
                .and_then(|value| value.i());
            match answer {
                Ok(state) => state == 0, // PackageManager.PERMISSION_GRANTED
                Err(_) => {
                    let _ = env.exception_clear();
                    false
                }
            }
        };
        if granted(&mut env) {
            return true;
        }
        let asked = (|| -> jni::errors::Result<()> {
            let name = env.new_string(RECORD_AUDIO)?;
            let names = env.new_object_array(1, "java/lang/String", &name)?;
            env.call_method(
                &activity,
                "requestPermissions",
                "([Ljava/lang/String;I)V",
                &[JValue::Object(&names), JValue::Int(7302)],
            )?;
            Ok(())
        })();
        if asked.is_err() {
            let _ = env.exception_clear();
            return false;
        }
        // The dialog answers asynchronously; a minute is plenty to decide.
        for _ in 0..240 {
            std::thread::sleep(std::time::Duration::from_millis(250));
            if granted(&mut env) {
                return true;
            }
        }
        false
    }
}

/// Runs as Android loads the app's library, on a Java thread: the one place
/// WebRTC can find its Java classes (`libwebrtc.jar`), which it looks up
/// once here. Native threads only see the system's classes.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub extern "system" fn JNI_OnLoad(
    vm: jni::JavaVM,
    _reserved: *mut std::ffi::c_void,
) -> jni::sys::jint {
    morpheus_call::initialize_android(&vm);
    jni::sys::JNI_VERSION_1_6
}
