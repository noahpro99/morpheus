//! Mobile entry point. Same UI as desktop; only the renderer and the
//! storage location differ. The shared shell switches to one pane at a
//! time on phone-width windows by itself.

use std::path::PathBuf;

use dioxus::prelude::*;
use morpheus_ui::AppConfig;
use tracing_subscriber::EnvFilter;

#[cfg(target_os = "android")]
mod push;

/// The homeserver the sign-in form suggests: in test builds the dev server
/// on the machine running the emulator, matrix.org otherwise.
fn default_homeserver() -> &'static str {
    if cfg!(feature = "e2e") {
        "http://10.0.2.2:8008"
    } else {
        "matrix.org"
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| "morpheus=debug,warn".into()),
        )
        .init();

    #[allow(unused_mut)]
    let mut config = AppConfig::new(
        data_dir(),
        std::env::var("MORPHEUS_HOMESERVER").unwrap_or_else(|_| default_homeserver().into()),
    );
    #[cfg(target_os = "android")]
    {
        config.open_url = Some(std::sync::Arc::new(android::open_url));
        // Calls: WebRTC set up once; the microphone asked for at the first call.
        android::init_webrtc();
        config.microphone = Some(std::sync::Arc::new(android::microphone));
        config.speakerphone = Some(std::sync::Arc::new(android::speakerphone));
        morpheus_ui::attach::set_file_picker(std::sync::Arc::new(android::pick_file));
        config.call_presence = Some(std::sync::Arc::new(android::call_presence));
        android::register_camera();
        // Test builds send a tone instead (no environment to say so here),
        // unless a test marked the app for the real microphone.
        #[cfg(feature = "e2e")]
        if !config.data_dir.join("real_audio").exists() {
            morpheus_call::use_test_tone();
        }
        // Push notifications through the phone's UnifiedPush app (see push.rs).
        let dir = config.data_dir.clone();
        config.on_session = Some(std::sync::Arc::new(move |session| {
            push::on_session(session, dir.clone())
        }));
    }
    #[cfg(target_os = "android")]
    push::watch_open_room(config.data_dir.clone(), config.commands.clone());
    tracing::info!(?config.data_dir, "starting morpheus mobile");

    // Dioxus runs on the runtime it finds; unasked it builds one with a
    // thread per CPU core, which the UI does not need.
    #[allow(clippy::expect_used)]
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .expect("the UI's runtime");
    let _runtime = runtime.enter();
    LaunchBuilder::new()
        .with_context(config)
        // The web view starts out the app's darkest surface, not white.
        .with_cfg(dioxus::mobile::Config::new().with_background_color((13, 15, 20, 255)))
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
        // WebRTC finds the context through its `ContextUtils` class, but its
        // own lookup (`FindClass` from this natively attached thread) only
        // sees the system's classes, not the app's: it failed, the context
        // stayed null, and the first call with the real microphone crashed
        // in WebRtcAudioManager. So: the VM for WebRTC, then the class
        // through the app's own class loader, and its initialize, from here.
        morpheus_call::initialize_android(&vm);
        let initialized = (|| -> jni::errors::Result<()> {
            let loader = env
                .call_method(&app, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
                .l()?;
            let name = env.new_string("livekit.org.webrtc.ContextUtils")?;
            let class = env
                .call_method(
                    &loader,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[JValue::Object(&name)],
                )?
                .l()?;
            env.call_static_method(
                jni::objects::JClass::from(class),
                "initialize",
                "(Landroid/content/Context;)V",
                &[JValue::Object(&app)],
            )?;
            Ok(())
        })();
        if let Err(err) = initialized {
            let _ = env.exception_clear();
            tracing::warn!(%err, "WebRTC could not take the app context; calls cannot use the microphone");
        }
    }

    const RECORD_AUDIO: &str = "android.permission.RECORD_AUDIO";
    const CAMERA: &str = "android.permission.CAMERA";

    /// The phone's camera for calls (`CameraFeed.kt`): its frames come back
    /// through [`Java_dev_morpheus_mobile_CameraFeed_frame`].
    pub fn register_camera() {
        morpheus_call::set_platform_camera(morpheus_call::PlatformCamera {
            start: Box::new(|| permission(CAMERA, 7303) && camera_feed("start")),
            stop: Box::new(|| {
                camera_feed("stop");
            }),
        });
    }

    /// `CameraFeed.start(context)` or `CameraFeed.stop()`, through the app's
    /// class loader (a natively attached thread cannot find app classes).
    #[allow(unsafe_code)]
    fn camera_feed(method: &str) -> bool {
        let ctx = ndk_context::android_context();
        // SAFETY: as in `files_dir`.
        let Ok(vm) = (unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }) else {
            return false;
        };
        let Ok(mut env) = vm.attach_current_thread() else {
            return false;
        };
        let context = unsafe { JObject::from_raw(ctx.context().cast()) };
        let result = (|| -> jni::errors::Result<bool> {
            let loader = env
                .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
                .l()?;
            let name = env.new_string("dev.morpheus.mobile.CameraFeed")?;
            let class = env
                .call_method(
                    &loader,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[JValue::Object(&name)],
                )?
                .l()?;
            let class = jni::objects::JClass::from(class);
            if method == "start" {
                env.call_static_method(
                    class,
                    "start",
                    "(Landroid/content/Context;)Z",
                    &[JValue::Object(&context)],
                )?
                .z()
            } else {
                env.call_static_method(class, "stop", "()V", &[])?;
                Ok(true)
            }
        })();
        match result {
            Ok(done) => done,
            Err(err) => {
                let _ = env.exception_clear();
                tracing::warn!(%err, method, "camera");
                false
            }
        }
    }

    /// The system's file picker (`PickFileActivity`), for any file or only
    /// pictures: blocks until something is picked (copied into the app's
    /// files, its path returned) or the picker is closed.
    #[allow(unsafe_code)]
    pub fn pick_file(images: bool) -> Option<std::path::PathBuf> {
        let ctx = ndk_context::android_context();
        // SAFETY: as in `files_dir`.
        let vm = unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }.ok()?;
        let mut env = vm.attach_current_thread().ok()?;
        let context = unsafe { JObject::from_raw(ctx.context().cast()) };
        let started = (|| -> jni::errors::Result<()> {
            let loader = env
                .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
                .l()?;
            let name = env.new_string("dev.morpheus.mobile.PickFileActivity")?;
            let class = env
                .call_method(
                    &loader,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[JValue::Object(&name)],
                )?
                .l()?;
            env.call_static_method(
                jni::objects::JClass::from(class),
                "start",
                "(Landroid/content/Context;Z)V",
                &[JValue::Object(&context), JValue::Bool(u8::from(images))],
            )?;
            Ok(())
        })();
        if let Err(err) = started {
            let _ = env.exception_clear();
            tracing::warn!(%err, "file picker");
            return None;
        }
        // The activity leaves its answer in `picked` (empty: cancelled).
        let answer = super::data_dir().join("picked");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(PICK_WAIT_SECS);
        while std::time::Instant::now() < deadline {
            if let Ok(path) = std::fs::read_to_string(&answer) {
                let _ = std::fs::remove_file(&answer);
                let path = path.trim();
                return (!path.is_empty()).then(|| std::path::PathBuf::from(path));
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        None
    }

    /// The phone's record of the call going (`CallService`): its ongoing call
    /// notification, for `Some((room id, room name))`; gone for `None`.
    #[allow(unsafe_code)]
    pub fn call_presence(call: Option<(String, String)>) {
        let ctx = ndk_context::android_context();
        // SAFETY: as in `files_dir`.
        let Ok(vm) = (unsafe { jni::JavaVM::from_raw(ctx.vm().cast()) }) else {
            return;
        };
        let Ok(mut env) = vm.attach_current_thread() else {
            return;
        };
        let context = unsafe { JObject::from_raw(ctx.context().cast()) };
        let (room_id, room) = call.unwrap_or_default();
        let told = (|| -> jni::errors::Result<()> {
            let loader = env
                .call_method(&context, "getClassLoader", "()Ljava/lang/ClassLoader;", &[])?
                .l()?;
            let name = env.new_string("dev.morpheus.mobile.CallService")?;
            let class = env
                .call_method(
                    &loader,
                    "loadClass",
                    "(Ljava/lang/String;)Ljava/lang/Class;",
                    &[JValue::Object(&name)],
                )?
                .l()?;
            let room_id = env.new_string(room_id)?;
            let room = env.new_string(room)?;
            env.call_static_method(
                jni::objects::JClass::from(class),
                "update",
                "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)V",
                &[
                    JValue::Object(&context),
                    JValue::Object(&room_id),
                    JValue::Object(&room),
                ],
            )?;
            Ok(())
        })();
        if let Err(err) = told {
            let _ = env.exception_clear();
            tracing::warn!(%err, "call notification");
        }
    }

    /// How long a picker may stay open.
    const PICK_WAIT_SECS: u64 = 600;

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

    /// Ask for the microphone and wait for the answer.
    pub fn microphone() -> bool {
        permission(RECORD_AUDIO, 7302)
    }

    /// Ask for a runtime permission (`request` tells the answers apart) and
    /// wait for it: the system dialog shows over the app. `true` once granted.
    #[allow(unsafe_code)]
    fn permission(wanted: &str, request: i32) -> bool {
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
            let Ok(name) = env.new_string(wanted) else {
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
            let name = env.new_string(wanted)?;
            let names = env.new_object_array(1, "java/lang/String", &name)?;
            env.call_method(
                &activity,
                "requestPermissions",
                "([Ljava/lang/String;I)V",
                &[JValue::Object(&names), JValue::Int(request)],
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

/// A camera picture from `CameraFeed.kt`, on to the call. The planes are
/// direct buffers, valid until this returns.
#[cfg(target_os = "android")]
#[unsafe(no_mangle)]
#[allow(unsafe_code, clippy::too_many_arguments)]
pub extern "system" fn Java_dev_morpheus_mobile_CameraFeed_frame(
    env: jni::JNIEnv,
    _class: jni::objects::JClass,
    y: jni::objects::JByteBuffer,
    u: jni::objects::JByteBuffer,
    v: jni::objects::JByteBuffer,
    y_stride: jni::sys::jint,
    uv_stride: jni::sys::jint,
    uv_pixel_stride: jni::sys::jint,
    width: jni::sys::jint,
    height: jni::sys::jint,
    rotation: jni::sys::jint,
) {
    let plane = |buffer: &jni::objects::JByteBuffer| -> Option<&[u8]> {
        let at = env.get_direct_buffer_address(buffer).ok()?;
        let len = env.get_direct_buffer_capacity(buffer).ok()?;
        // SAFETY: a direct buffer of `len` bytes, which Android keeps alive
        // (the image is open) until this call returns.
        Some(unsafe { std::slice::from_raw_parts(at, len) })
    };
    let (Some(y), Some(u), Some(v)) = (plane(&y), plane(&u), plane(&v)) else {
        return;
    };
    let size = |n: jni::sys::jint| usize::try_from(n).unwrap_or(0);
    morpheus_call::camera_frame(
        y,
        u,
        v,
        size(y_stride),
        size(uv_stride),
        size(uv_pixel_stride),
        u32::try_from(width).unwrap_or(0),
        u32::try_from(height).unwrap_or(0),
        u32::try_from(rotation).unwrap_or(0),
    );
}

#[cfg(test)]
mod tests {
    /// A release suggests a real homeserver; the emulator's host is for test builds.
    #[test]
    fn suggests_matrix_org_outside_test_builds() {
        assert_eq!(super::default_homeserver(), "matrix.org");
    }
}
