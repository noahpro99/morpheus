# JNA finds its native side by name, and UniFFI's generated Kotlin is reached
# through it: keep both whole.
-keep class com.sun.jna.** { *; }
-keep class uniffi.** { *; }
-dontwarn java.awt.*
