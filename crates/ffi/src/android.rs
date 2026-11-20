//! Android: runs as the system loads the library (`System.loadLibrary`, from
//! `MorpheusApp`), on a Java thread: the one place WebRTC can find its Java
//! classes (`libwebrtc.jar`), which it looks up once here. Native threads
//! only see the system's classes. (UniFFI's JNA load does not run this.)

/// Gives WebRTC the Java VM.
#[unsafe(no_mangle)]
pub extern "system" fn JNI_OnLoad(vm: jni::JavaVM, _reserved: *mut std::ffi::c_void) -> jni::sys::jint {
    morpheus_call::initialize_android(&vm);
    jni::sys::JNI_VERSION_1_6
}
