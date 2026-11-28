# The benchmark build is the release code plus the test bridge: keep the bridge and what it reaches by name.
-keep class dev.morpheus.app.TestBridge** { *; }
-keep class dev.morpheus.app.DebugHooks { *; }
-keep class dev.morpheus.app.DebugShareProvider { *; }
