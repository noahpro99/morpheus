package dev.morpheus.app.push

import android.Manifest
import android.app.Activity
import android.app.Application
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.BitmapFactory
import android.os.Build
import android.os.Bundle
import android.util.Log
import dev.morpheus.app.MainActivity
import kotlinx.coroutines.runBlocking
import org.unifiedpush.android.connector.FailedReason
import org.unifiedpush.android.connector.PushService
import org.unifiedpush.android.connector.UnifiedPush
import org.unifiedpush.android.connector.data.PushEndpoint
import org.unifiedpush.android.connector.data.PushMessage
import uniffi.morpheus_ffi.previewPush

/** Notifications through UnifiedPush (Sunup, ntfy, ...): the endpoint, the push service and the notification. */
object Push {
    private const val CHANNEL = "messages"
    private const val CHANNEL_QUIET = "messages-quiet"
    private const val PREFS = "push"
    const val EXTRA_ROOM = "room_id"

    /** Whether one of our screens is showing: it shows the message itself then. */
    @Volatile
    var visible = false

    fun track(app: Application) {
        app.registerActivityLifecycleCallbacks(object : Application.ActivityLifecycleCallbacks {
            override fun onActivityResumed(a: Activity) { visible = true }
            override fun onActivityPaused(a: Activity) { visible = false }
            override fun onActivityCreated(a: Activity, b: Bundle?) = Unit
            override fun onActivityStarted(a: Activity) = Unit
            override fun onActivityStopped(a: Activity) = Unit
            override fun onActivitySaveInstanceState(a: Activity, b: Bundle) = Unit
            override fun onActivityDestroyed(a: Activity) = Unit
        })
    }

    fun endpoint(context: Context): String? =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString("endpoint", null)

    fun saveEndpoint(context: Context, url: String?) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().putString("endpoint", url).apply()
    }

    /** Ask for the notification permission and register with the phone's push app, once per start. */
    fun start(activity: Activity) {
        if (Build.VERSION.SDK_INT >= 33 &&
            activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
        ) {
            activity.requestPermissions(arrayOf(Manifest.permission.POST_NOTIFICATIONS), 7301)
        }
        try {
            UnifiedPush.tryUseCurrentOrDefaultDistributor(activity) { found ->
                if (found) UnifiedPush.register(activity, messageForDistributor = "Morpheus")
            }
        } catch (err: Throwable) {
            Log.w("MorpheusPush", "no push distributor: $err")
        }
    }

    /** A push arrived: word it (decrypting without the app running) and show it. Blocks; call off the main thread. */
    fun onPush(context: Context, bytes: ByteArray) {
        val payload = PushPayload.parse(bytes) ?: return
        val manager = context.getSystemService(NotificationManager::class.java)
        if (payload.unread == 0) {
            manager.cancel(payload.roomId.hashCode())
            forget(payload.roomId)
            return
        }
        if (visible) return
        val notice = runBlocking {
            try {
                val dir = context.filesDir.resolve("morpheus").absolutePath
                if (payload.eventId.isEmpty()) null else previewPush(dir, payload.roomId, payload.eventId)
            } catch (err: Throwable) {
                Log.w("MorpheusPush", "preview failed: $err")
                null
            }
        }
        // A call rings as the phone's: the notification that fills the screen.
        if (notice?.call == true) {
            dev.morpheus.app.Ringer.show(context, payload.roomId, payload.eventId, notice.sender.ifEmpty { notice.title })
            return
        }
        // Notifications are off on this device (Settings > Notifications): a message shows nothing.
        val dir = context.filesDir.resolve("morpheus").absolutePath
        if (!uniffi.morpheus_ffi.pushEnabled(dir)) return
        val sounds = uniffi.morpheus_ffi.pushSounds(dir)
        val channel = if (sounds) CHANNEL else CHANNEL_QUIET
        manager.createNotificationChannel(NotificationChannel(CHANNEL, "Chat messages", NotificationManager.IMPORTANCE_HIGH))
        manager.createNotificationChannel(NotificationChannel(CHANNEL_QUIET, "Chat messages (silent)", NotificationManager.IMPORTANCE_DEFAULT).apply { setSound(null, null) })
        val open = Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            .putExtra(EXTRA_ROOM, payload.roomId)
        val tap = PendingIntent.getActivity(
            context, payload.roomId.hashCode(), open,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val sender = if (notice == null || notice.title == "Morpheus") "" else notice.sender
        val text = notice?.body ?: if (payload.unread > 1) dev.morpheus.app.ui.t("%{count} unread messages", "count" to payload.unread.toString()) else dev.morpheus.app.ui.t("New message")
        val picture = notice?.icon?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }
        val history = remember(payload.roomId, Line(sender, text, System.currentTimeMillis(), picture, payload.eventId))
        val builder = Notification.Builder(context, channel)
            .setSmallIcon(dev.morpheus.app.R.drawable.ic_notification)
            .setContentTitle(notice?.title ?: "Morpheus")
            .setContentText(text)
            .setAutoCancel(true)
            .setContentIntent(tap)
        if (Build.VERSION.SDK_INT >= 28) {
            // The last messages of the room stacked, each with its sender (and picture): the style the system groups chats by.
            val me = android.app.Person.Builder().setName(dev.morpheus.app.ui.t("You")).build()
            val style = Notification.MessagingStyle(me)
            style.setGroupConversation(notice != null && notice.title.contains(" in "))
            history.forEach { line ->
                val who = android.app.Person.Builder().setName(line.sender.ifEmpty { dev.morpheus.app.ui.t("Someone") })
                line.picture?.let { who.setIcon(android.graphics.drawable.Icon.createWithBitmap(it)) }
                style.addMessage(line.text, line.at, who.build())
            }
            builder.setStyle(style).setCategory(Notification.CATEGORY_MESSAGE)
            if (notice != null && notice.title != "Morpheus") {
                // Reply from the shade: the typed words go through the saved session, without the app. Mark as read sends the receipt and clears it.
                val input = android.app.RemoteInput.Builder(KEY_REPLY).setLabel(dev.morpheus.app.ui.t("Reply")).build()
                builder.addAction(Notification.Action.Builder(null, dev.morpheus.app.ui.t("Reply"), actionIntent(context, payload.roomId, payload.eventId, ACTION_REPLY, mutable = true)).addRemoteInput(input).setSemanticAction(Notification.Action.SEMANTIC_ACTION_REPLY).build())
                builder.addAction(Notification.Action.Builder(null, dev.morpheus.app.ui.t("Mark as read"), actionIntent(context, payload.roomId, payload.eventId, ACTION_READ, mutable = false)).setSemanticAction(Notification.Action.SEMANTIC_ACTION_MARK_AS_READ).build())
            }
        } else {
            notice?.image?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }?.let { builder.setStyle(Notification.BigPictureStyle().bigPicture(it)) }
        }
        picture?.let { builder.setLargeIcon(it) }
        manager.notify(payload.roomId.hashCode(), builder.build())
    }

    private const val KEY_REPLY = "reply"
    const val ACTION_REPLY = "dev.morpheus.app.push.REPLY"
    const val ACTION_READ = "dev.morpheus.app.push.READ"
    private const val MAX_LINES = 8

    /** One message in a room's notification. */
    class Line(val sender: String, val text: String, val at: Long, val picture: android.graphics.Bitmap?, val eventId: String)

    private val recent = HashMap<String, ArrayDeque<Line>>()

    /** The room's last messages with this one added (what the notification shows stacked). */
    private fun remember(roomId: String, line: Line): List<Line> = synchronized(recent) {
        val lines = recent.getOrPut(roomId) { ArrayDeque() }
        if (lines.none { it.eventId.isNotEmpty() && it.eventId == line.eventId }) lines.addLast(line)
        while (lines.size > MAX_LINES) lines.removeFirst()
        lines.toList()
    }

    /** The room is read or answered: its stack starts over. */
    fun forget(roomId: String) = synchronized(recent) { recent.remove(roomId); Unit }

    /** The newest message of the room that came by push, for the read receipt. */
    fun newestEvent(roomId: String): String = synchronized(recent) { recent[roomId]?.lastOrNull { it.eventId.isNotEmpty() }?.eventId.orEmpty() }

    private fun actionIntent(context: Context, roomId: String, eventId: String, action: String, mutable: Boolean): PendingIntent =
        PendingIntent.getBroadcast(
            context, roomId.hashCode() + if (mutable) 1 else 2,
            Intent(context, NotificationActionReceiver::class.java).setAction(action).putExtra(EXTRA_ROOM, roomId).putExtra("event_id", eventId),
            PendingIntent.FLAG_UPDATE_CURRENT or if (mutable) PendingIntent.FLAG_MUTABLE else PendingIntent.FLAG_IMMUTABLE,
        )

    /** Reply or Mark as read from the notification: done with the saved session, without the app; the room's notification goes. */
    fun act(context: Context, intent: Intent) {
        val room = intent.getStringExtra(EXTRA_ROOM) ?: return
        val dir = context.filesDir.resolve("morpheus").absolutePath
        val manager = context.getSystemService(NotificationManager::class.java)
        try {
            when (intent.action) {
                ACTION_REPLY -> {
                    val words = android.app.RemoteInput.getResultsFromIntent(intent)?.getCharSequence(KEY_REPLY)?.toString().orEmpty().trim()
                    if (words.isNotEmpty()) runBlocking { uniffi.morpheus_ffi.replyFromPush(dir, room, words) }
                }
                ACTION_READ -> {
                    val event = newestEvent(room).ifEmpty { intent.getStringExtra("event_id").orEmpty() }
                    if (event.isNotEmpty()) runBlocking { uniffi.morpheus_ffi.readFromPush(dir, room, event) }
                }
            }
            forget(room)
            manager.cancel(room.hashCode())
        } catch (err: Throwable) {
            Log.w("MorpheusPush", "notification action failed: $err")
        }
    }
}

/** The notification's Reply and Mark as read buttons. */
class NotificationActionReceiver : android.content.BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val pending = goAsync()
        Thread { try { Push.act(context, intent) } finally { pending.finish() } }.start()
    }
}

/** UnifiedPush events: the endpoint becomes a pusher at the next sign-in or start; a message becomes a notification. */
class MorpheusPushService : PushService() {
    override fun onNewEndpoint(endpoint: PushEndpoint, instance: String) {
        Push.saveEndpoint(this, endpoint.url)
        (application as dev.morpheus.app.MorpheusApp).registerPushNow()
    }

    override fun onRegistrationFailed(reason: FailedReason, instance: String) {
        Log.w("MorpheusPush", "registration failed: $reason")
    }

    override fun onUnregistered(instance: String) {
        Push.saveEndpoint(this, null)
    }

    override fun onMessage(message: PushMessage, instance: String) {
        Thread { Push.onPush(this, message.content) }.start()
    }
}
