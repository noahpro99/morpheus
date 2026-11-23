package dev.morpheus.app

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import androidx.core.content.IntentCompat
import androidx.lifecycle.viewModelScope
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

/** What another app shared with Morpheus (ACTION_SEND, ACTION_SEND_MULTIPLE): text, a link, and files kept in the cache until they are sent. */
class Share(val text: String?, val files: List<SharedFile>)

class SharedFile(val name: String, val mime: String, val file: File)

/**
 * A share that came in and has not been sent or cancelled. It lives outside the screens: a share that comes in
 * before the session is back (or before signing in) waits here, and the share screen shows when the app is signed in (#340).
 */
object ShareInbox {
    private val _pending = MutableStateFlow<Share?>(null)
    val pending: StateFlow<Share?> = _pending.asStateFlow()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var counter = 0

    /** Take what an intent shares, if it is a share. The files are copied at once: the permission to read them goes with the intent. */
    fun receive(context: Context, intent: Intent?) {
        if (intent == null) return
        if (intent.action != Intent.ACTION_SEND && intent.action != Intent.ACTION_SEND_MULTIPLE) return
        val text = intent.getCharSequenceExtra(Intent.EXTRA_TEXT)?.toString()?.takeIf { it.isNotBlank() }
        val uris = ArrayList<Uri>()
        if (intent.action == Intent.ACTION_SEND) IntentCompat.getParcelableExtra(intent, Intent.EXTRA_STREAM, Uri::class.java)?.let { uris.add(it) }
        else IntentCompat.getParcelableArrayListExtra(intent, Intent.EXTRA_STREAM, Uri::class.java)?.let { uris.addAll(it) }
        val type = intent.type
        // A one-time delivery: a recreated activity gets the same intent again.
        intent.action = null
        if (text == null && uris.isEmpty()) return
        if (uris.isEmpty()) {
            _pending.value = Share(text, emptyList())
            return
        }
        val app = context.applicationContext
        scope.launch {
            val dir = File(app.cacheDir, "share").apply { mkdirs() }
            val files = uris.mapNotNull { copy(app, it, dir, type) }
            if (files.isNotEmpty() || text != null) _pending.value = Share(text, files)
        }
    }

    private fun copy(context: Context, uri: Uri, dir: File, intentType: String?): SharedFile? = try {
        var name = uri.lastPathSegment?.substringAfterLast('/') ?: "file"
        context.contentResolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
            if (it.moveToFirst()) it.getString(0)?.let { n -> name = n }
        }
        val mime = context.contentResolver.getType(uri) ?: intentType?.takeIf { !it.endsWith("/*") } ?: "application/octet-stream"
        val file = File(dir, "${++counter}-${System.nanoTime()}-" + name.replace('/', '_'))
        context.contentResolver.openInputStream(uri)?.use { input -> file.outputStream().use { input.copyTo(it) } } ?: throw java.io.IOException("no stream")
        SharedFile(name, mime, file)
    } catch (_: Exception) {
        null
    }

    /** Give the share up. */
    fun cancel() {
        val gone = _pending.value
        _pending.value = null
        gone?.files?.forEach { it.file.delete() }
    }

    /**
     * Send the share to a room (files first, then the message: for text alone, the text, then the message when there is one),
     * and open the room. The sends are the room's own (its queue waits for the session and outlives the screen).
     */
    fun send(model: AppModel, roomId: String, message: String) {
        val share = _pending.value ?: return
        val room = model.sidebar.value?.scopes?.flatMap { it.sections }?.flatMap { it.rooms }?.firstOrNull { it.id == roomId } ?: return
        _pending.value = null
        model.viewModelScope.launch {
            val queue = RoomModel(model.morpheus, room, null) { model.awaitSession() }
            for (shared in share.files) {
                val bytes = withContext(Dispatchers.IO) { shared.file.readBytes().also { shared.file.delete() } }
                queue.sendFile(shared.name, shared.mime, bytes)
            }
            val body = if (share.files.isEmpty()) share.text else null
            if (!body.isNullOrBlank()) queue.send(body)
            if (message.isNotBlank()) queue.send(message.trim())
            queue.close()
        }
        model.openRoomWhenKnown(roomId)
    }
}
