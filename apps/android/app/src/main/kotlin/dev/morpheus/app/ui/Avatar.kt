package dev.morpheus.app.ui

import android.graphics.BitmapFactory
import android.util.LruCache
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import dev.morpheus.app.AppState
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.morpheus_ffi.MediaHandle

/**
 * Decoded pictures by the core's media key and size, so scrolling does not fetch twice, and kept on disk (what was fetched
 * once is drawn at once after a relaunch, from the key alone, before there is a session to fetch with).
 */
object Images {
    private val cache = LruCache<String, ImageBitmap>(160)

    /** The folder of the pictures kept (the app's cache), set at start. */
    @Volatile
    var dir: java.io.File? = null

    private fun file(id: String): java.io.File? = dir?.let { java.io.File(it, java.security.MessageDigest.getInstance("SHA-1").digest(id.toByteArray()).joinToString("") { b -> "%02x".format(b) }) }

    private fun decode(bytes: ByteArray): ImageBitmap? = BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()

    /** A picture seen before, by its key: from memory or the disk; nothing is fetched. */
    suspend fun kept(key: String, width: Int, height: Int): ImageBitmap? {
        val id = "$key@${width}x$height"
        cache.get(id)?.let { return it }
        return withContext(Dispatchers.IO) {
            val bytes = file(id)?.takeIf { it.exists() }?.readBytes() ?: return@withContext null
            decode(bytes)?.also { cache.put(id, it) }
        }
    }

    suspend fun load(media: MediaHandle, width: Int, height: Int): ImageBitmap? {
        kept(media.key(), width, height)?.let { return it }
        val id = "${media.key()}@${width}x$height"
        return withContext(Dispatchers.Default) {
            AppState.imageFetches++
            val bytes = media.thumbnail(width.toUInt(), height.toUInt()) ?: return@withContext null
            runCatching { file(id)?.writeBytes(bytes) }
            decode(bytes)?.also { cache.put(id, it) }
        }
    }
}

/** A round picture, or the first letter of the name on a colour made from it while there is none. */
@Composable
fun Avatar(media: MediaHandle?, name: String, size: Dp, modifier: Modifier = Modifier, square: Boolean = false, key: String? = null) {
    val shape = if (square) androidx.compose.ui.graphics.RectangleShape else CircleShape
    val px = with(LocalDensity.current) { size.roundToPx() }
    // The picture of this key if it was seen before (the list of the last run has keys, no pictures), else fetched.
    val bitmap by produceState<ImageBitmap?>(null, media?.key() ?: key, px) {
        value = media?.let { Images.load(it, px * 2, px * 2) } ?: key?.let { Images.kept(it, px * 2, px * 2) }
    }
    val shown = bitmap
    if (shown != null) {
        androidx.compose.runtime.LaunchedEffect(shown) { AppState.avatarsDrawn++ }
        Image(
            shown, name,
            modifier.size(size).clip(shape),
            contentScale = ContentScale.Crop,
        )
    } else {
        Box(
            modifier.size(size).clip(shape).background(colourOf(name)),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                name.trimStart('#', '@', ' ').take(1).uppercase(),
                color = onColour(colourOf(name)),
                fontWeight = androidx.compose.ui.text.font.FontWeight.SemiBold,
                fontSize = (size.value * 0.42f).sp,
            )
        }
    }
}

private val hues = listOf(0xFF5865F2, 0xFF3BA55D, 0xFFED4245, 0xFFFAA61A, 0xFFEB459E, 0xFF9B59B6, 0xFF1ABC9C)

fun colourOf(name: String): Color = Color(hues[(name.hashCode() and 0x7fffffff) % hues.size])
