# JNA finds its native side by name, and UniFFI's generated Kotlin is reached
# through it: keep both whole.
-keep class com.sun.jna.** { *; }
-keep class uniffi.** { *; }
-dontwarn java.awt.*
# WebRTC (calls): its native half finds these classes and methods by name.
-keep class livekit.org.webrtc.** { *; }
-keep class org.webrtc.** { *; }
-dontwarn livekit.org.webrtc.**
-dontwarn livekit.org.jni_zero.**
