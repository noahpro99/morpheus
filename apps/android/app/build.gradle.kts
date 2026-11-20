import java.util.Properties

plugins {
    id("com.android.application")
    id("org.jetbrains.kotlin.android")
    id("org.jetbrains.kotlin.plugin.compose")
}

// The Rust half: crates/ffi built for these ABIs (`scripts/android-ffi.sh`),
// as `-PrustAbis=x86_64` (the emulator) or `arm64-v8a` (a phone), comma
// separated; both by default.
val rustAbis = (findProperty("rustAbis") as String?) ?: "arm64-v8a,x86_64"
val repoRoot = rootDir.resolve("../..").canonicalFile
val rustOut = layout.buildDirectory.dir("rust")

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
    signingConfigs {
        if (keystore.exists() && keystorePassword.exists()) {
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
}

tasks.matching { it.name.startsWith("merge") && it.name.endsWith("JniLibFolders") }.configureEach { dependsOn(buildRust) }
tasks.matching { it.name.startsWith("compile") && it.name.endsWith("Kotlin") }.configureEach { dependsOn(buildRust) }

dependencies {
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
    testImplementation("junit:junit:4.13.2")
    testImplementation("org.json:json:20231013")
}
