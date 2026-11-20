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
        manager.createNotificationChannel(NotificationChannel(CHANNEL, "Messages", NotificationManager.IMPORTANCE_HIGH))
        val open = Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
            .putExtra(EXTRA_ROOM, payload.roomId)
        val tap = PendingIntent.getActivity(
            context, payload.roomId.hashCode(), open,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val builder = Notification.Builder(context, CHANNEL)
            .setSmallIcon(dev.morpheus.app.R.drawable.ic_notification)
            .setContentTitle(notice?.title ?: "Morpheus")
            .setContentText(notice?.body ?: if (payload.unread > 1) "${payload.unread} unread messages" else "New message")
            .setAutoCancel(true)
            .setContentIntent(tap)
        notice?.icon?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }?.let { builder.setLargeIcon(it) }
        notice?.image?.let { BitmapFactory.decodeByteArray(it, 0, it.size) }?.let {
            builder.setStyle(Notification.BigPictureStyle().bigPicture(it))
        }
        manager.notify(payload.roomId.hashCode(), builder.build())
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
