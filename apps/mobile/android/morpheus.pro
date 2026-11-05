# Release builds: no shrinking or renaming. Much of the app's Java and
# Kotlin is only ever called from native code, by name (CameraFeed,
# WebRTC's Java half, the webview glue), which shrinking cannot see and
# would strip or rename; and it is small beside the native library.
-dontshrink
-dontoptimize
-dontobfuscate

# Optional pieces of libraries that the app never loads (Tink's key
# downloader, compile-time annotations, an alternative JNI loader).
-dontwarn com.google.api.client.**
-dontwarn com.google.errorprone.**
-dontwarn javax.lang.model.**
-dontwarn livekit.org.jni_zero.**
