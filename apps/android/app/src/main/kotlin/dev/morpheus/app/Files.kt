package dev.morpheus.app

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import androidx.core.content.FileProvider
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.morpheus_ffi.MediaHandle

/** What the open room does with a file that was picked (the e2e bridge calls it too, as a stand-in for the system picker). */
object RoomHooks {
    @Volatile
    var attach: ((name: String, mime: String, bytes: ByteArray) -> Unit)? = null
}

/** A picked file: its name, type and bytes. */
class Picked(val name: String, val mime: String, val bytes: ByteArray)

suspend fun Context.read(uri: Uri): Picked? = withContext(Dispatchers.IO) {
    try {
        var name = "file"
        contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
            if (it.moveToFirst()) name = it.getString(0) ?: name
        }
        val bytes = contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return@withContext null
        Picked(name, contentResolver.getType(uri) ?: "application/octet-stream", bytes)
    } catch (_: Exception) {
        null
    }
}

/** Fetch a file and hand it to whatever app opens it; the message when that fails. */
suspend fun Context.open(media: MediaHandle, name: String): String? = withContext(Dispatchers.IO) {
    val bytes = media.file() ?: return@withContext "Could not download $name"
    val dir = File(cacheDir, "opened").apply { mkdirs() }
    val file = File(dir, name.substringAfterLast('/').ifBlank { "file" })
    file.writeBytes(bytes)
    val uri = FileProvider.getUriForFile(this@open, "$packageName.files", file)
    val mime = contentResolver.getType(uri) ?: android.webkit.MimeTypeMap.getSingleton()
        .getMimeTypeFromExtension(file.extension.lowercase()) ?: "*/*"
    val intent = Intent(Intent.ACTION_VIEW).setDataAndType(uri, mime)
        .addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_ACTIVITY_NEW_TASK)
    try {
        startActivity(Intent.createChooser(intent, name).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        null
    } catch (_: Exception) {
        "No app can open $name"
    }
}

fun size(bytes: ULong?): String {
    val n = bytes?.toDouble() ?: return ""
    return when {
        n < 1024 -> "${n.toLong()} B"
        n < 1024 * 1024 -> "%.1f KB".format(n / 1024)
        else -> "%.1f MB".format(n / 1024 / 1024)
    }
}

/** Open a web link in the phone's browser; the message when there is none. */
fun Context.openLink(url: String): String? {
    val address = if (url.startsWith("www.", ignoreCase = true)) "https://$url" else url
    AppState.lastLink = address
    return try {
        startActivity(Intent(Intent.ACTION_VIEW, android.net.Uri.parse(address)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        null
    } catch (_: Exception) {
        "No app can open $address"
    }
}
