package dev.morpheus.app.ui

import android.graphics.ImageDecoder
import android.graphics.drawable.AnimatedImageDrawable
import android.graphics.drawable.Drawable
import android.os.Build
import android.widget.ImageView
import android.widget.MediaController
import android.widget.VideoView
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.onClick
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext
import uniffi.morpheus_ffi.MediaHandle
import uniffi.morpheus_ffi.MessageBody

/** A GIF that moves (#412): the file decoded as an animation and started; `state` says "playing" while it runs. Older systems draw the still picture. */
@Composable
fun AnimatedPicture(media: MediaHandle, caption: String, modifier: Modifier) {
    val drawable by produceState<Drawable?>(null, media.key()) {
        value = withContext(Dispatchers.Default) {
            if (Build.VERSION.SDK_INT < 28) return@withContext null
            val bytes = media.file() ?: return@withContext null
            try { ImageDecoder.decodeDrawable(ImageDecoder.createSource(java.nio.ByteBuffer.wrap(bytes))) } catch (_: Exception) { null }
        }
    }
    var running by remember { mutableStateOf(false) }
    val shown = drawable
    LaunchedEffect(shown) {
        (shown as? AnimatedImageDrawable)?.let { it.repeatCount = AnimatedImageDrawable.REPEAT_INFINITE; it.start() }
        while (shown is AnimatedImageDrawable) { running = shown.isRunning; delay(200) }
    }
    AndroidView(
        factory = { ImageView(it).apply { scaleType = ImageView.ScaleType.FIT_CENTER; contentDescription = caption } },
        modifier = modifier.semantics { stateDescription = if (running) "playing" else "still" },
        update = { it.setImageDrawable(shown) },
    )
}

/**
 * A video in the chat (#412): its poster (the message's thumbnail) with a play button and its length; a tap plays it full
 * screen with the system's controls. `state` of the card is its length, of the player "playing" or "paused".
 */
@Composable
fun VideoCard(body: MessageBody.File, onLongPress: () -> Unit) {
    var open by remember { mutableStateOf(false) }
    val poster by produceState<ImageBitmap?>(null, body.thumbnail?.key()) { value = body.thumbnail?.let { Images.load(it, 720, 720) } }
    val ratio = if ((body.width ?: 0u) > 0u && (body.height ?: 0u) > 0u) body.width!!.toFloat() / body.height!!.toFloat() else 16f / 9f
    val length = body.durationMs?.let { uniffi.morpheus_ffi.durationLabel(it) }
    Box(
        Modifier
            .padding(top = 4.dp)
            .widthIn(max = 280.dp)
            .aspectRatio(ratio.coerceIn(0.4f, 3f))
            .background(Palette.Raised, RoundedCornerShape(8.dp))
            .pointerInput(body.media.key()) { detectTapGestures(onLongPress = { onLongPress() }) { open = true } }
            .semantics { onClick("Play video") { open = true; true }; stateDescription = length ?: "video" }
            .testTag("msg-video"),
        contentAlignment = Alignment.Center,
    ) {
        poster?.let { Image(it, body.name, Modifier.fillMaxSize(), contentScale = ContentScale.Crop) }
        Box(Modifier.size(48.dp).background(Palette.Accent, CircleShape), contentAlignment = Alignment.Center) {
            GlyphIcon(dev.morpheus.app.R.drawable.ic_ms_play_arrow, size = 24.dp, tint = Palette.OnAccent)
        }
        length?.let {
            Text(it, Modifier.align(Alignment.BottomEnd).padding(Space.sm).background(Palette.Raised, CircleShape).padding(horizontal = Space.sm, vertical = 2.dp).testTag("video-length"), color = Palette.Text, style = MaterialTheme.typography.labelSmall)
        }
    }
    if (open) VideoPlayer(body.media, body.name) { open = false }
}

/** A video full screen: fetched to the app's cache, played by a [VideoView] with the system's controls; a tap outside the controls or Back closes it. */
@Composable
private fun VideoPlayer(media: MediaHandle, name: String, onClose: () -> Unit) {
    val context = LocalContext.current
    val file by produceState<File?>(null, media.key()) {
        value = withContext(Dispatchers.IO) {
            val bytes = media.file() ?: return@withContext null
            File(context.cacheDir, "video").apply { mkdirs() }.let { dir -> File(dir, "clip-${media.key().hashCode()}.${name.substringAfterLast('.', "mp4")}").also { it.writeBytes(bytes) } }
        }
    }
    var state by remember { mutableStateOf("loading") }
    Dialog(onClose, DialogProperties(usePlatformDefaultWidth = false, decorFitsSystemWindows = false)) {
        Box(Modifier.fillMaxSize().background(Color.Black).semantics { stateDescription = state }.testTag("video-player"), contentAlignment = Alignment.Center) {
            val ready = file
            if (ready != null) {
                var view by remember { mutableStateOf<VideoView?>(null) }
                LaunchedEffect(view) { while (view != null) { state = if (view!!.isPlaying) "playing" else "paused"; delay(200) } }
                AndroidView(
                    factory = { ctx ->
                        VideoView(ctx).apply {
                            setMediaController(MediaController(ctx).also { it.setAnchorView(this) })
                            setVideoPath(ready.absolutePath)
                            setOnCompletionListener { }
                            setOnPreparedListener { it.isLooping = false; start() }
                            view = this
                        }
                    },
                    modifier = Modifier.fillMaxSize(),
                )
            }
        }
    }
}
