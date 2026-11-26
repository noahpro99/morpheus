import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
    id("io.gitlab.arturbosch.detekt")
    id("io.github.takahirom.roborazzi")
}

// The Rust half: crates/ffi built for these ABIs (`scripts/android-ffi.sh`),
// as `-PrustAbis=x86_64` (the emulator) or `arm64-v8a` (a phone), comma
// separated; both by default.
val rustAbis = (findProperty("rustAbis") as String?) ?: "arm64-v8a,x86_64"
val repoRoot = rootDir.resolve("../..").canonicalFile
// `-PrustPrebuilt=<dir>`: the Kotlin bindings and libwebrtc.jar are already in <dir> (kotlin/, libs/),
// from scripts/android-check-prep.sh, so nothing is cross-built: lint, detekt and the JVM tests
// (screenshots, rules) need only the Kotlin side.
val rustPrebuilt = findProperty("rustPrebuilt") as String?
val rustOut = if (rustPrebuilt != null) layout.dir(provider { File(rustPrebuilt) }) else layout.buildDirectory.dir("rust")

val buildRust = tasks.register<Exec>("buildRust") {
    description = "Builds crates/ffi for Android and generates its Kotlin bindings"
    workingDir = repoRoot
    commandLine(
        "bash", "scripts/android-ffi.sh",
        rustOut.get().asFile.absolutePath, rustAbis,
        (findProperty("rustProfile") as String?) ?: "android-dev",
    )
    // Up to date when no Rust source, manifest or this script changed, so a
    // Kotlin-only change does not even start cargo.
    inputs.files(
        fileTree(repoRoot) {
            include("crates/ffi/**", "crates/core/**", "crates/view-model/**", "Cargo.toml", "Cargo.lock", "scripts/android-ffi.sh")
            exclude("**/target/**")
        },
    )
    inputs.property("abis", rustAbis)
    inputs.property("profile", (findProperty("rustProfile") as String?) ?: "android-dev")
    outputs.dir(rustOut)
}

android {
    namespace = "dev.morpheus.app"
    compileSdk = 34

    defaultConfig {
        // The Dioxus phone app's ID: this app updates over its installs.
        applicationId = "dev.morpheus.mobile"
        minSdk = 26
        targetSdk = 34
        // Same scheme as scripts/release-android.sh: 0.2.6 -> 206.
        val version = Properties().also {
            rootDir.resolve("../../Cargo.toml").reader().use { r ->
                // `version = "x.y.z"` of [workspace.package]
                val line = r.readLines().first { l -> l.startsWith("version = ") }
                it["version"] = line.substringAfter('"').substringBefore('"')
            }
        }.getProperty("version")
        val (major, minor, patch) = version.split('.').map { it.toInt() }
        versionCode = major * 10000 + minor * 100 + patch
        versionName = version
    }

    val keystore = File(System.getProperty("user.home"), ".config/morpheus/release.keystore")
    val keystorePassword = File(System.getProperty("user.home"), ".config/morpheus/release.keystore.password")
    // `-PreleaseSigning=debug` signs the release build with the debug key (the e2e smoke test
    // on the emulator), never the release key.
    val debugSigned = findProperty("releaseSigning") == "debug"
    signingConfigs {
        if (debugSigned) {
            create("release") {
                storeFile = File(System.getProperty("user.home"), ".android/debug.keystore")
                storePassword = "android"
                keyAlias = "androiddebugkey"
                keyPassword = "android"
            }
        } else if (keystore.exists() && keystorePassword.exists()) {
            create("release") {
                storeFile = keystore
                storePassword = keystorePassword.readText().trim()
                keyAlias = "morpheus"
                keyPassword = keystorePassword.readText().trim()
            }
        }
    }

    buildTypes {
        debug {
            // The dev homeserver on the host, from the emulator.
            buildConfigField("String", "DEFAULT_HOMESERVER", "\"http://10.0.2.2:8008\"")
        }
        release {
            isMinifyEnabled = true
            buildConfigField("String", "DEFAULT_HOMESERVER", "\"matrix.org\"")
            proguardFiles(getDefaultProguardFile("proguard-android-optimize.txt"), "proguard-rules.pro")
            signingConfigs.findByName("release")?.let { signingConfig = it }
        }
    }

    sourceSets["main"].apply {
        jniLibs.srcDir(rustOut.map { it.dir("jniLibs") })
        kotlin.srcDir(rustOut.map { it.dir("kotlin") })
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    kotlinOptions { jvmTarget = "17" }
    buildFeatures {
        compose = true
        buildConfig = true
    }
    packaging { jniLibs.useLegacyPackaging = false }
    testOptions {
        // Robolectric (the screenshot and accessibility tests, on the JVM) reads the app's resources and manifest.
        unitTests.isIncludeAndroidResources = true
        unitTests.isReturnDefaultValues = true
    }

    // `just android-lint`: Android Lint, with the Compose and Material 3 checks that ship inside
    // those libraries, fails the build on any warning (what is accepted is in lint-baseline.xml).
    lint {
        warningsAsErrors = true
        abortOnError = true
        checkDependencies = false
        baseline = file("lint-baseline.xml")
        lintConfig = file("lint.xml")
        htmlReport = false
        xmlReport = false
        textReport = true
        textOutput = file("build/reports/lint-results.txt")
    }
}

if (rustPrebuilt == null) {
    tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }.configureEach { dependsOn(buildRust) }
    tasks.matching { it.name.startsWith("compile") && it.name.endsWith("Kotlin") }.configureEach { dependsOn(buildRust) }
}

detekt {
    // Only the compose-rules ruleset (io.nlopez.compose.rules), nothing of detekt's own style rules.
    buildUponDefaultConfig = false
    config.setFrom(file("detekt.yml"))
    baseline = file("detekt-baseline.xml")
    source.setFrom("src/main/kotlin", "src/debug/kotlin", "src/release/kotlin")
}

dependencies {
    detektPlugins("io.nlopez.compose.rules:detekt:0.4.22")
    implementation(platform("androidx.compose:compose-bom:2024.10.01"))
    implementation("androidx.compose.ui:ui")
    implementation("androidx.compose.foundation:foundation")
    implementation("androidx.compose.material3:material3")
    implementation("androidx.activity:activity-compose:1.9.3")
    implementation("androidx.core:core-ktx:1.13.1")
    implementation("androidx.lifecycle:lifecycle-viewmodel-compose:2.8.7")
    implementation("androidx.lifecycle:lifecycle-runtime-compose:2.8.7")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-android:1.9.0")
    // UniFFI's Kotlin calls the library through JNA.
    implementation("net.java.dev.jna:jna:5.14.0@aar")
    implementation("org.unifiedpush.android:connector:3.0.10")
    // WebRTC's Java half (calls): built with crates/call, copied out by scripts/android-ffi.sh.
    implementation(files(rustOut.map { it.file("libs/libwebrtc.jar") }).let { if (rustPrebuilt == null) it.builtBy(buildRust) else it })
    testImplementation("junit:junit:4.13.2")
    testImplementation("net.java.dev.jna:jna:5.14.0")
    testImplementation("org.json:json:20231013")
    // Screenshot and accessibility tests on the JVM (#468): Robolectric renders Compose, Roborazzi records and
    // compares the pictures and runs the Accessibility Test Framework over the same screen.
    testImplementation("org.robolectric:robolectric:4.14.1")
    testImplementation("androidx.compose.ui:ui-test-junit4")
    testImplementation("androidx.test.ext:junit:1.2.1")
    testImplementation("io.github.takahirom.roborazzi:roborazzi:1.33.0")
    testImplementation("io.github.takahirom.roborazzi:roborazzi-compose:1.33.0")
    testImplementation("io.github.takahirom.roborazzi:roborazzi-junit-rule:1.33.0")
    testImplementation("io.github.takahirom.roborazzi:roborazzi-accessibility-check:1.33.0")
    debugImplementation("androidx.compose.ui:ui-test-manifest")
}

roborazzi {
    // The approved pictures, committed: `just android-shots-verify` fails when a screen drifts from them,
    // `just android-shots-record` approves a change.
    outputDir.set(file("src/test/screenshots"))
}

// The tests call the Rust core (colour helpers, words) through JNA: the host build of crates/ffi, which
// scripts/android-check-prep.sh makes, is what they load.
tasks.withType<Test>().configureEach {
    systemProperty("jna.library.path", (if (rustPrebuilt != null) File(rustPrebuilt, "lib") else repoRoot.resolve("target/debug")).absolutePath)
}
