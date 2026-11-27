package dev.morpheus.app

import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import uniffi.morpheus_ffi.MessageItem
import uniffi.morpheus_ffi.Morpheus
import uniffi.morpheus_ffi.RoomItem
import uniffi.morpheus_ffi.RoomTimeline
import uniffi.morpheus_ffi.CustomEmojiSection
import uniffi.morpheus_ffi.EmojiSection
import uniffi.morpheus_ffi.SuggestionItem
import uniffi.morpheus_ffi.emojiSections as emojiSectionsOf
import uniffi.morpheus_ffi.TimelineListener
import uniffi.morpheus_ffi.TimelineRow

/**
 * One open room: its rows (pushed by Rust whenever they change), and what the
 * screen does with them. Closed with the screen; that stops the Rust side.
 */
class RoomModel(
    private val morpheus: Morpheus,
    val room: RoomItem,
    private val threadRoot: String? = null,
    /** Returns once the session is up: until then the room shows what the last run kept (#313). */
    private val ready: suspend () -> Unit = {},
) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var timeline: RoomTimeline? = null

    /** What is waiting to be sent, in order: a send made before the room is open (just after a launch or opening it) waits, it is not lost. */
    private class Outgoing(val id: Long, val text: String?, val run: suspend (RoomTimeline) -> Unit)

    private val outbox = kotlinx.coroutines.channels.Channel<Outgoing>(kotlinx.coroutines.channels.Channel.UNLIMITED)
    private val opened = kotlinx.coroutines.CompletableDeferred<RoomTimeline?>()
    private var outgoingIds = 0L
    private val failedWhat = HashMap<Long, Outgoing>()
    private val _failed = MutableStateFlow<List<FailedSend>>(emptyList())

    /** Messages that did not go, with why; each can be sent again or dropped. */
    val failed: StateFlow<List<FailedSend>> = _failed.asStateFlow()

    /** Sends outlive the screen: leaving a room straight after sending still sends. */
    private val detached = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private val consumer = detached.launch {
        for (item in outbox) {
            val open = opened.await()
            try {
                if (open == null) throw IllegalStateException("The room could not be opened.")
                item.run(open)
            } catch (e: kotlinx.coroutines.CancellationException) {
                throw e
            } catch (e: Exception) {
                failedWhat[item.id] = item
                _failed.value = _failed.value + FailedSend(item.id, item.text, e.sentence())
            }
        }
    }

    private fun enqueue(text: String? = null, run: suspend (RoomTimeline) -> Unit) {
        outbox.trySend(Outgoing(++outgoingIds, text, run))
    }

    /** Send a message that did not go again. */
    fun retry(id: Long) {
        val item = failedWhat.remove(id) ?: return
        _failed.value = _failed.value.filter { it.id != id }
        outbox.trySend(item)
    }

    /** Give up on a message that did not go. */
    fun discard(id: Long) {
        failedWhat.remove(id)
        _failed.value = _failed.value.filter { it.id != id }
    }
    private val _loadingOlder = MutableStateFlow(false)

    /** A request for older messages is out (the screen asks again when it ends, if it still needs more). */
    val loadingOlder: StateFlow<Boolean> = _loadingOlder.asStateFlow()

    private val _rows = MutableStateFlow<List<TimelineRow>>(if (threadRoot == null) morpheus.cachedTimeline(room.id).orEmpty() else emptyList())

    /** Oldest first. */
    val rows: StateFlow<List<TimelineRow>> = _rows.asStateFlow()

    private val _live = MutableStateFlow(false)

    /** The rows are the room's own now, not what the last run kept (#313). */
    val live: StateFlow<Boolean> = _live.asStateFlow()

    private val _replyingTo = MutableStateFlow<MessageItem?>(null)
    val replyingTo: StateFlow<MessageItem?> = _replyingTo.asStateFlow()

    private val _error = MutableStateFlow<String?>(null)
    val error: StateFlow<String?> = _error.asStateFlow()

    private val previewSettings = morpheus.previewSettings()

    /** Whether this room gets link cards (a setting of this device, and encrypted rooms ask for their own). */
    val previewsAllowed: Boolean = previewSettings.enabled && (previewSettings.inEncrypted || !room.isEncrypted)
    private val _dismissed = MutableStateFlow(previewSettings.dismissed.toSet())
    val dismissed: StateFlow<Set<String>> = _dismissed.asStateFlow()
    private val cards = HashMap<String, uniffi.morpheus_ffi.PreviewCard?>()

    /** The card for a link, asked of the homeserver once. */
    suspend fun preview(url: String): uniffi.morpheus_ffi.PreviewCard? {
        if (cards.containsKey(url)) return cards[url]
        val card = try { timeline?.linkPreview(url) } catch (_: Exception) { null }
        cards[url] = card
        return card
    }

    fun dismissPreview(eventId: String) {
        try { morpheus.dismissPreview(eventId) } catch (_: Exception) {}
        _dismissed.value = _dismissed.value + eventId
    }

    private val _typing = MutableStateFlow<String?>(null)

    /** "bob is typing…", or nothing. */
    val typing: StateFlow<String?> = _typing.asStateFlow()
    private var typingHandle: uniffi.morpheus_ffi.TaskHandle? = null
    private var lastTyping = 0L

    init {
        scope.launch {
            try {
                ready()
                val listener = object : TimelineListener {
                    override fun onRows(rows: List<TimelineRow>) {
                        _rows.value = rows
                        _live.value = true
                    }
                }
                timeline = if (threadRoot == null) morpheus.openRoom(room.id, listener) else morpheus.openThread(room.id, threadRoot, listener)
                opened.complete(timeline)
                typingHandle = timeline?.watchTyping(object : uniffi.morpheus_ffi.TypingListener {
                    override fun onTyping(line: String?) {
                        _typing.value = line
                    }
                })
            } catch (e: Exception) {
                _error.value = e.sentence()
                opened.complete(null)
            }
        }
    }

    fun replyTo(message: MessageItem?) {
        _replyingTo.value = message
    }

    private val _editing = MutableStateFlow<MessageItem?>(null)

    /** The message whose text the composer is replacing, if any. */
    val editing: StateFlow<MessageItem?> = _editing.asStateFlow()

    fun edit(message: MessageItem?) {
        _editing.value = message
        if (message != null) _replyingTo.value = null
    }

    fun delete(eventId: String) = enqueue { it.deleteMessage(eventId) }

    private val _hidden = MutableStateFlow<Set<String>>(emptySet())
    /** Messages deleted a moment ago, out of the chat while "Undo" is on offer (#415). */
    val hidden: StateFlow<Set<String>> = _hidden.asStateFlow()

    /** Delete with a few seconds to take it back: the message goes from the chat now and is deleted when the toast has gone. */
    fun deleteUndoable(eventId: String) {
        _hidden.value = _hidden.value + eventId
        undoing++
        dev.morpheus.app.ui.Toasts.undoable(
            dev.morpheus.app.ui.t("Message deleted"),
            onUndo = { _hidden.value = _hidden.value - eventId; undone() },
            // The chat may have been left meanwhile: the delete still goes, and the room closes behind it.
            onCommit = { delete(eventId); undone() },
        )
    }

    /** Deletes still waiting out their Undo moment, and whether the chat was closed meanwhile (it is then closed when the last is settled). */
    private var undoing = 0
    private var closing = false

    private fun undone() {
        undoing--
        if (closing && undoing <= 0) finishClose()
    }

    fun send(text: String) {
        val editing = _editing.value
        if (editing?.eventId != null) {
            _editing.value = null
            enqueue(text) { it.editMessage(editing.eventId!!, text) }
            return
        }
        val reply = _replyingTo.value
        _replyingTo.value = null
        enqueue(text) { open -> if (reply?.eventId != null) open.sendReply(text, reply.eventId!!) else open.sendText(text) }
    }

    fun sendFile(name: String, mime: String, bytes: ByteArray) = enqueue { it.sendFile(name, mime, bytes) }

    fun react(eventId: String, key: String) = enqueue { it.toggleReaction(eventId, key) }

    /** Older messages, one request at a time. */
    fun loadOlder() {
        if (_loadingOlder.value) return
        _loadingOlder.value = true
        scope.launch {
            try {
                if (AppState.olderDelayMs > 0) kotlinx.coroutines.delay(AppState.olderDelayMs)
                timeline?.loadOlder()
            } catch (e: Exception) {
                _error.value = e.sentence()
            } finally {
                _loadingOlder.value = false
            }
        }
    }

    fun fail(message: String) {
        _error.value = message
    }

    suspend fun suggest(draft: String): List<SuggestionItem> = try {
        timeline?.suggest(draft).orEmpty()
    } catch (_: Exception) {
        emptyList()
    }

    suspend fun emojiSections(query: String): List<EmojiSection> = emojiSectionsOf(query)

    /** The panel as this device keeps it: the tone, the tones and the sections with the emoji used lately first (#420). */
    fun emojiPanel(query: String): uniffi.morpheus_ffi.EmojiPanelInfo = morpheus.emojiPanel(query)
    fun noteEmojiUsed(glyph: String) = morpheus.noteEmojiUsed(glyph)
    fun setEmojiTone(number: UByte) = morpheus.setEmojiTone(number)

    suspend fun customEmoji(query: String): List<CustomEmojiSection> = try {
        timeline?.customEmoji(query).orEmpty()
    } catch (_: Exception) {
        emptyList()
    }

    fun markRead() = launchSafely { timeline?.markRead() }

    suspend fun search(query: String): List<uniffi.morpheus_ffi.HitItem> = try { timeline?.search(query).orEmpty() } catch (_: Exception) { emptyList() }

    private val _jump = MutableStateFlow<String?>(null)

    /** A message to scroll to (a search result), by event ID, once it is in the list. */
    val jump: StateFlow<String?> = _jump.asStateFlow()
    private val _highlight = MutableStateFlow<String?>(null)
    val highlight: StateFlow<String?> = _highlight.asStateFlow()

    fun clearJump() {
        _jump.value = null
    }

    fun highlight(eventId: String?) {
        _highlight.value = eventId
    }

    /** Page back until the message is in the list (as far as the start of the room), then ask the screen to scroll to it. */
    fun jumpTo(eventId: String) {
        scope.launch {
            var tries = 0
            fun has() = _rows.value.any { it is TimelineRow.Message && it.message.eventId == eventId }
            while (!has() && tries < 50 && _rows.value.none { it is TimelineRow.Start }) {
                tries++
                try { timeline?.loadOlder() } catch (_: Exception) { break }
                kotlinx.coroutines.delay(150)
            }
            _jump.value = eventId
        }
    }

    /** The person is typing: tell the room, at most every few seconds (the notice outlives that). */
    fun typing() {
        val now = System.currentTimeMillis()
        if (now - lastTyping < 3_000) return
        lastTyping = now
        launchSafely { timeline?.renewTyping() }
    }

    fun stopTyping() {
        lastTyping = 0
        launchSafely { timeline?.stopTyping() }
    }

    suspend fun members(): List<uniffi.morpheus_ffi.MemberRow> = timeline?.members().orEmpty()

    suspend fun notifyMode(): String = try { timeline?.notifyMode() ?: "all" } catch (_: Exception) { "all" }

    fun setNotifyMode(mode: String) = launchSafely { timeline?.setNotifyMode(mode) }

    suspend fun profile(userId: String): uniffi.morpheus_ffi.ProfileCard? = timeline?.profile(userId)

    private fun launchSafely(block: suspend () -> Unit) {
        scope.launch {
            try {
                block()
            } catch (e: Exception) {
                _error.value = e.sentence()
            }
        }
    }

    fun close() {
        // A delete in its Undo moment is still to be sent: the room closes after it.
        closing = true
        if (undoing <= 0) finishClose()
    }

    private fun finishClose() {
        // What was sent but is waiting still goes; the room closes behind it.
        outbox.close()
        detached.launch {
            consumer.join()
            scope.cancel()
            typingHandle?.cancel()
            timeline?.close()
            timeline = null
            detached.cancel()
        }
    }
}

/** A message that did not go: its text (none for a file, reaction or edit), and why. */
class FailedSend(val id: Long, val text: String?, val reason: String)
