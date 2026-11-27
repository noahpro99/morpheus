package dev.morpheus.app.ui

import android.graphics.BitmapFactory
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.MicTest
import dev.morpheus.app.MorpheusApp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import uniffi.morpheus_ffi.cameraPreviewFrame
import uniffi.morpheus_ffi.cameraPreviewStart
import uniffi.morpheus_ffi.cameraPreviewStop

/**
 * Settings, Voice & Video (#442, #443): the camera shown before a call, the microphone's test with its level and its
 * volume, and the volume of each person you chose one for. The speaker has no volume of the app's (the volume keys are
 * the system's, #496); where the sound goes is picked in a call, from its speaker button.
 */
@Composable
internal fun VoicePage(app: AppModel, t: (String) -> String) {
    val inCall = app.calls.snapshot.collectAsState().value != null
    Column(Modifier.fillMaxWidth().testTag("voice-page"), verticalArrangement = Arrangement.spacedBy(Space.md)) {
        SmallHeading(t("Camera"), Modifier.padding(start = Space.lg, end = Space.lg, top = Space.lg))
        CameraCheck(app, t, inCall)
        SmallHeading(t("Microphone"), Modifier.padding(start = Space.lg, end = Space.lg, top = Space.lg))
        MicCheck(app, t, inCall)
        SmallHeading(t("People"), Modifier.padding(start = Space.lg, end = Space.lg, top = Space.lg))
        KeptPeople(app, t)
    }
}

@Composable
private fun CameraCheck(app: AppModel, t: (String) -> String, inCall: Boolean) {
    val context = LocalContext.current
    var on by remember { mutableStateOf(false) }
    var failed by remember { mutableStateOf(false) }
    var flips by remember { androidx.compose.runtime.mutableIntStateOf(0) }
    fun begin() {
        (context.applicationContext as MorpheusApp).camera
        failed = !cameraPreviewStart()
        on = !failed
    }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted -> if (granted) begin() else failed = true }
    DisposableEffect(Unit) { onDispose { cameraPreviewStop() } }
    // The camera is let go when the app leaves the screen too.
    val owner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val watch = androidx.lifecycle.LifecycleEventObserver { _, event -> if (event == androidx.lifecycle.Lifecycle.Event.ON_STOP && on) { cameraPreviewStop(); on = false } }
        owner.lifecycle.addObserver(watch)
        onDispose { owner.lifecycle.removeObserver(watch) }
    }
    if (inCall) {
        Text(t("The camera is the call's while you are in one."), Modifier.padding(horizontal = Space.lg).testTag("camera-preview-busy"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        return
    }
    Row(Modifier.padding(horizontal = Space.lg), horizontalArrangement = Arrangement.spacedBy(Space.sm), verticalAlignment = Alignment.CenterVertically) {
        Button({
            if (on) { cameraPreviewStop(); on = false }
            else if (androidx.core.content.ContextCompat.checkSelfPermission(context, android.Manifest.permission.CAMERA) == android.content.pm.PackageManager.PERMISSION_GRANTED) begin()
            else ask.launch(android.Manifest.permission.CAMERA)
        }, Modifier.testTag("camera-preview")) { Text(if (on) t("Stop checking") else t("Check camera")) }
        if (on && app.calls.canFlipCamera) {
            TextButton({ app.calls.flipCamera(); flips++ }, Modifier.testTag("camera-preview-flip")) { Text(t("Flip camera")) }
        }
    }
    if (failed) Text(t("The camera could not be opened."), Modifier.padding(horizontal = Space.lg).testTag("camera-preview-failed"), color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium)
    if (on) {
        var picture by remember { mutableStateOf<ImageBitmap?>(null) }
        LaunchedEffect(Unit) {
            var seen = -1L
            while (true) {
                val frame = withContext(Dispatchers.Default) { cameraPreviewFrame() }
                if (frame != null && frame.seq.toLong() != seen) {
                    seen = frame.seq.toLong()
                    picture = withContext(Dispatchers.Default) { BitmapFactory.decodeByteArray(frame.jpeg, 0, frame.jpeg.size)?.asImageBitmap() }
                }
                delay(100)
            }
        }
        val shown = picture
        Box(Modifier.padding(horizontal = Space.lg).fillMaxWidth().height(220.dp).clip(RoundedCornerShape(12.dp)).background(Palette.Divider), contentAlignment = Alignment.Center) {
            if (shown != null) Image(shown, t("Camera preview"), Modifier.fillMaxWidth().testTag("camera-preview-picture"), contentScale = ContentScale.Fit)
            else Text("…", color = Palette.Muted)
        }
    }
}

@Composable
private fun MicCheck(app: AppModel, t: (String) -> String, inCall: Boolean) {
    val context = LocalContext.current
    var volume by remember { mutableFloatStateOf(app.calls.volumes().input) }
    val test = remember { MicTest { volume } }
    var on by remember { mutableStateOf(false) }
    var level by remember { mutableFloatStateOf(0f) }
    fun begin() { test.start(); on = true }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { granted -> if (granted) begin() }
    DisposableEffect(Unit) { MicTestState.current = test; onDispose { test.stop(); MicTestState.current = null } }
    val owner = androidx.lifecycle.compose.LocalLifecycleOwner.current
    DisposableEffect(owner) {
        val watch = androidx.lifecycle.LifecycleEventObserver { _, event -> if (event == androidx.lifecycle.Lifecycle.Event.ON_STOP && on) { test.stop(); on = false } }
        owner.lifecycle.addObserver(watch)
        onDispose { owner.lifecycle.removeObserver(watch) }
    }
    LaunchedEffect(on) {
        while (on) {
            level = test.level
            if (!test.running) on = false
            delay(50)
        }
        level = 0f
    }
    VolumeRow(t("Microphone volume"), "input-volume", volume) { volume = it; app.calls.setInputVolume(it) }
    if (inCall) {
        Text(t("The microphone is the call's while you are in one."), Modifier.padding(horizontal = Space.lg).testTag("mic-test-busy"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        return
    }
    Row(Modifier.padding(horizontal = Space.lg), horizontalArrangement = Arrangement.spacedBy(Space.sm), verticalAlignment = Alignment.CenterVertically) {
        Button({
            if (on) { test.stop(); on = false }
            else if (androidx.core.content.ContextCompat.checkSelfPermission(context, android.Manifest.permission.RECORD_AUDIO) == android.content.pm.PackageManager.PERMISSION_GRANTED) begin()
            else ask.launch(android.Manifest.permission.RECORD_AUDIO)
        }, Modifier.testTag("mic-test")) { Text(if (on) t("Stop testing") else t("Test microphone")) }
        Text("${Math.round(level * 100)}%", Modifier.testTag("mic-test-level"))
    }
    Box(Modifier.padding(horizontal = Space.lg).fillMaxWidth().height(8.dp).clip(RoundedCornerShape(4.dp)).background(Palette.Divider)) {
        Box(Modifier.fillMaxWidth(level.coerceIn(0f, 1f)).height(8.dp).background(MaterialTheme.colorScheme.primary))
    }
    Text(t("Shows how loud you are. Nothing is played back."), Modifier.padding(horizontal = Space.lg), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
    test.problem?.let { Text(it, Modifier.padding(horizontal = Space.lg).testTag("mic-test-failed"), color = MaterialTheme.colorScheme.error) }
}

/** What the debug bridge reads of the running test. */
object MicTestState {
    @Volatile var current: MicTest? = null
}

/** The people this account gave a volume: each a row to change it again (the volume and silenced for me), kept as in a call. */
@Composable
private fun KeptPeople(app: AppModel, t: (String) -> String) {
    val people = remember { app.morpheus.keptPeople() }
    val names by dev.morpheus.app.People.names.collectAsState()
    if (people.isEmpty()) {
        Text(t("Open a person's volume in a call and it is kept here."), Modifier.padding(horizontal = Space.lg).testTag("kept-people-none"), color = Palette.Muted, style = MaterialTheme.typography.bodyMedium)
        return
    }
    for (person in people) {
        LaunchedEffect(person.userId) { dev.morpheus.app.People.want(person.userId) }
        val local = person.userId.trimStart('@').substringBefore(':')
        var volume by remember(person.userId) { mutableFloatStateOf(person.volume) }
        var muted by remember(person.userId) { mutableStateOf(person.muted) }
        Column(Modifier.padding(horizontal = Space.lg).testTag("kept-person-$local")) {
            Text(dev.morpheus.app.People.nameOf(person.userId, names), style = MaterialTheme.typography.bodyLarge)
            VolumeRow(t("Volume"), "kept-voice-$local", person.volume) { volume = it; app.calls.setPersonVolume(person.userId, it, muted) }
            ToggleRow(t("Muted for me"), muted, "kept-mute-$local") { _ -> muted = !muted; app.calls.setPersonVolume(person.userId, volume, muted) }
        }
    }
}
