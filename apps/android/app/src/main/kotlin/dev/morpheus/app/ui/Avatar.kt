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
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.sp
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.morpheus_ffi.MediaHandle

/** Decoded pictures by the core's media key and size, so scrolling does not fetch twice. */
object Images {
    private val cache = LruCache<String, ImageBitmap>(160)

    suspend fun load(media: MediaHandle, width: Int, height: Int): ImageBitmap? {
        val key = "${media.key()}@${width}x$height"
        cache.get(key)?.let { return it }
        return withContext(Dispatchers.Default) {
            val bytes = media.thumbnail(width.toUInt(), height.toUInt()) ?: return@withContext null
            BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()?.also { cache.put(key, it) }
        }
    }
}

/** A round picture, or the first letter of the name on a colour made from it while there is none. */
@Composable
fun Avatar(media: MediaHandle?, name: String, size: Dp, modifier: Modifier = Modifier) {
    val px = with(LocalDensity.current) { size.roundToPx() }
    val bitmap by produceState<ImageBitmap?>(null, media?.key(), px) {
        value = media?.let { Images.load(it, px * 2, px * 2) }
    }
    val shown = bitmap
    if (shown != null) {
        Image(
            shown, name,
            modifier.size(size).clip(CircleShape),
            contentScale = ContentScale.Crop,
        )
    } else {
        Box(
            modifier.size(size).clip(CircleShape).background(colourOf(name)),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                name.trimStart('#', '@', ' ').take(1).uppercase(),
                color = Color.White,
                fontSize = (size.value * 0.42f).sp,
            )
        }
    }
}

private val hues = listOf(0xFF5865F2, 0xFF3BA55D, 0xFFED4245, 0xFFFAA61A, 0xFFEB459E, 0xFF9B59B6, 0xFF1ABC9C)

fun colourOf(name: String): Color = Color(hues[(name.hashCode() and 0x7fffffff) % hues.size])
