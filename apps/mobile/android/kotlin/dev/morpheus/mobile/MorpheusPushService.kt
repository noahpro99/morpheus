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
            Log.d(TAG, "caught up in $roomId: clearing its notification")
            manager().cancel(roomId.hashCode())
            return
        }
        Log.d(TAG, "showing a notification for $roomId (unread $unread)")
        show(roomId, roomName(roomId), unread)
    }

    private fun roomName(roomId: String): String? = try {
        JSONObject(File(dataDir(this), "room_names.json").readText()).optString(roomId).ifEmpty { null }
    } catch (_: Exception) {
        null
    }

    private fun manager() = getSystemService(NotificationManager::class.java)

    private fun show(roomId: String, room: String?, unread: Int) {
        val manager = manager()
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Messages", NotificationManager.IMPORTANCE_HIGH)
        )
        val open = packageManager.getLaunchIntentForPackage(packageName)?.apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
            putExtra("room_id", roomId)
        }
        val tap = open?.let {
            PendingIntent.getActivity(
                this, roomId.hashCode(), it,
                PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
            )
        }
        val icon = resources.getIdentifier("ic_notification", "drawable", packageName)
            .takeIf { it != 0 } ?: applicationInfo.icon
        val text = if (unread > 1) "$unread unread messages" else "New message"
        val built = Notification.Builder(this, CHANNEL)
            .setSmallIcon(icon)
            .setContentTitle(room ?: "Morpheus")
            .setContentText(text)
            .setAutoCancel(true)
            .setContentIntent(tap)
            .build()
        manager.notify(roomId.hashCode(), built)
    }

    companion object {
        const val CHANNEL = "messages"
        const val TAG = "MorpheusPush"
    }
}
