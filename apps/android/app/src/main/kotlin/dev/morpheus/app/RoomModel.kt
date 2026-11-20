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
class RoomModel(private val morpheus: Morpheus, val room: RoomItem, private val threadRoot: String? = null) {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var timeline: RoomTimeline? = null
    private val _loadingOlder = MutableStateFlow(false)

    /** A request for older messages is out (the screen asks again when it ends, if it still needs more). */
    val loadingOlder: StateFlow<Boolean> = _loadingOlder.asStateFlow()

    private val _rows = MutableStateFlow<List<TimelineRow>>(emptyList())

    /** Oldest first. */
    val rows: StateFlow<List<TimelineRow>> = _rows.asStateFlow()

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
                val listener = object : TimelineListener {
                    override fun onRows(rows: List<TimelineRow>) {
                        _rows.value = rows
                    }
                }
                timeline = if (threadRoot == null) morpheus.openRoom(room.id, listener) else morpheus.openThread(room.id, threadRoot, listener)
                typingHandle = timeline?.watchTyping(object : uniffi.morpheus_ffi.TypingListener {
                    override fun onTyping(line: String?) {
                        _typing.value = line
                    }
                })
            } catch (e: Exception) {
                _error.value = e.sentence()
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

    fun delete(eventId: String) = launchSafely { timeline?.deleteMessage(eventId) }

    fun send(text: String) {
        val editing = _editing.value
        if (editing?.eventId != null) {
            _editing.value = null
            launchSafely { timeline?.editMessage(editing.eventId!!, text) }
            return
        }
        val reply = _replyingTo.value
        _replyingTo.value = null
        launchSafely {
            val open = timeline ?: return@launchSafely
            if (reply?.eventId != null) open.sendReply(text, reply.eventId!!) else open.sendText(text)
        }
    }

    fun sendFile(name: String, mime: String, bytes: ByteArray) = launchSafely { timeline?.sendFile(name, mime, bytes) }

    fun react(eventId: String, key: String) = launchSafely { timeline?.toggleReaction(eventId, key) }

    /** Older messages, one request at a time. */
    fun loadOlder() {
        if (_loadingOlder.value) return
        _loadingOlder.value = true
        scope.launch {
            try {
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
        scope.cancel()
        typingHandle?.cancel()
        timeline?.close()
        timeline = null
    }
}
