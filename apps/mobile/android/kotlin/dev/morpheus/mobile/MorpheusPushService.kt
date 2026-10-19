package dev.morpheus.mobile

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.util.Log
import org.json.JSONObject
import org.unifiedpush.android.connector.FailedReason
import org.unifiedpush.android.connector.PushService
import org.unifiedpush.android.connector.data.PushEndpoint
import org.unifiedpush.android.connector.data.PushMessage
import java.io.File

/**
 * Receives UnifiedPush events. The endpoint goes to a file the Rust side
 * reads and registers as a Matrix pusher (`apps/mobile/src/push.rs`); a push
 * becomes an Android notification unless the app is on screen (it shows the
 * message itself then). Pushes carry ids only (`event_id_only`), so the
 * notification names the room from the list the Rust side keeps.
 */
class MorpheusPushService : PushService() {
    private fun dataDir(context: Context) = File(context.filesDir, "morpheus").apply { mkdirs() }

    override fun onNewEndpoint(endpoint: PushEndpoint, instance: String) {
        File(dataDir(this), "push_endpoint").writeText(endpoint.url)
    }

    override fun onRegistrationFailed(reason: FailedReason, instance: String) {
        File(dataDir(this), "push_status").writeText("failed: $reason")
    }

    override fun onUnregistered(instance: String) {
        File(dataDir(this), "push_endpoint").delete()
    }

    override fun onMessage(message: PushMessage, instance: String) {
        Log.d(TAG, "push: visible=${PushStartup.visible} decrypted=${message.decrypted} bytes=${message.content.size}")
        // On screen, the app shows the message itself.
        if (PushStartup.visible) return
        val notification = try {
            JSONObject(String(message.content)).optJSONObject("notification")
        } catch (_: Exception) {
            null
        }
        val roomId = notification?.optString("room_id").orEmpty()
        // Nothing unread (the user caught up elsewhere): nothing to show.
        val unread = notification?.optJSONObject("counts")?.optInt("unread", 1) ?: 1
        if (notification != null && unread == 0) {
            Log.d(TAG, "caught up in $roomId: clearing its notification")
            manager().cancel(roomId.hashCode())
            return
        }
        val eventId = notification?.optString("event_id").orEmpty()
        // Off the main thread: wait briefly for the running app's notice.
        Thread {
            val notice = awaitNotice(eventId)
            Log.d(TAG, "showing a notification for $roomId (unread $unread, notice ${notice != null})")
            val room = notice?.optString("room")?.ifEmpty { null } ?: roomName(roomId)
            val sender = notice?.optString("sender")?.ifEmpty { null }
            val title = if (sender != null && room != null) "$sender in $room" else room
            val text = notice?.optString("body")?.ifEmpty { null }
                ?: if (unread > 1) "$unread unread messages" else "New message"
            show(roomId, title, text)
        }.start()
    }

    /**
     * Who said what, from the app if it is running (`apps/mobile/src/push.rs`
     * writes a notice per message it has decrypted). A push carries ids only;
     * without the app there is nothing more to show.
     */
    private fun awaitNotice(eventId: String): JSONObject? {
        if (eventId.isEmpty()) return null
        val name = eventId.map { if (it.isLetterOrDigit() && it.code < 128) it else '_' }.joinToString("")
        val file = File(File(dataDir(this), "notices"), name)
        val until = System.currentTimeMillis() + NOTICE_WAIT_MS
        while (System.currentTimeMillis() < until) {
            if (file.exists()) {
                return try {
                    JSONObject(file.readText())
                } catch (_: Exception) {
                    null
                }
            }
            Thread.sleep(150)
        }
        return null
    }

    private fun roomName(roomId: String): String? = try {
        JSONObject(File(dataDir(this), "room_names.json").readText()).optString(roomId).ifEmpty { null }
    } catch (_: Exception) {
        null
    }

    private fun manager() = getSystemService(NotificationManager::class.java)

    private fun show(roomId: String, title: String?, text: String) {
        val manager = manager()
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Messages", NotificationManager.IMPORTANCE_HIGH)
        )
        // Through OpenRoomActivity, which tells the app which room to show.
        val open = Intent(this, OpenRoomActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK
            putExtra("room_id", roomId)
        }
        val tap = PendingIntent.getActivity(
            this, roomId.hashCode(), open,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val icon = resources.getIdentifier("ic_notification", "drawable", packageName)
            .takeIf { it != 0 } ?: applicationInfo.icon
        val built = Notification.Builder(this, CHANNEL)
            .setSmallIcon(icon)
            .setContentTitle(title ?: "Morpheus")
            .setContentText(text)
            .setAutoCancel(true)
            .setContentIntent(tap)
            .build()
        manager.notify(roomId.hashCode(), built)
    }

    companion object {
        const val CHANNEL = "messages"
        const val TAG = "MorpheusPush"
        /** How long a push waits for the running app to say who wrote what. */
        const val NOTICE_WAIT_MS = 4000L
    }
}
