package dev.morpheus.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Person
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.RingtoneManager
import android.os.Build
import android.util.Log
import dev.morpheus.app.ui.t
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch

/**
 * A call ringing while the app is away: the call notification (Answer,
 * Decline), which fills the screen over the lock screen, with the phone's
 * ringtone. On screen the app's own card rings instead (`RingCard`).
 */
object Ringer {
    private const val CHANNEL = "calls"
    private const val TAG = "MorpheusCalls"
    const val EXTRA_ANSWER = "answer_room"
    const val EXTRA_ANSWER_NAME = "answer_name"
    const val EXTRA_RING = "ring_room"
    private const val ACTION_DECLINE = "dev.morpheus.app.DECLINE_CALL"
    private const val RING_MS = 30_000L
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private fun id(roomId: String) = ("ring" + roomId).hashCode()

    fun show(context: Context, roomId: String, notification: String, caller: String) {
        val app = context.applicationContext
        val manager = app.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Calls", NotificationManager.IMPORTANCE_HIGH).apply {
                setSound(
                    RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE),
                    AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE).build(),
                )
            },
        )
        val open = Intent(app, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_NO_USER_ACTION)
            .putExtra(EXTRA_RING, roomId)
        val fullScreen = PendingIntent.getActivity(app, id(roomId), open, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val answer = PendingIntent.getActivity(
            app, id(roomId) + 1,
            Intent(app, MainActivity::class.java)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP)
                .putExtra(EXTRA_ANSWER, roomId).putExtra(EXTRA_ANSWER_NAME, caller),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val decline = PendingIntent.getBroadcast(
            app, id(roomId) + 2,
            Intent(app, RingActionReceiver::class.java).setAction(ACTION_DECLINE).putExtra("room_id", roomId).putExtra("notification", notification),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val builder = Notification.Builder(app, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(caller)
            .setContentText(t("Incoming call"))
            .setCategory(Notification.CATEGORY_CALL)
            .setOngoing(true)
            .setTimeoutAfter(RING_MS)
            .setContentIntent(fullScreen)
            .setFullScreenIntent(fullScreen, true)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            builder.setStyle(Notification.CallStyle.forIncomingCall(Person.Builder().setName(caller).setImportant(true).build(), decline, answer))
        } else {
            builder.addAction(Notification.Action.Builder(null, t("Decline"), decline).build())
            builder.addAction(Notification.Action.Builder(null, t("Answer"), answer).build())
        }
        val built = builder.build()
        built.flags = built.flags or Notification.FLAG_INSISTENT
        try {
            manager.notify(id(roomId), built)
        } catch (e: Exception) {
            Log.w(TAG, "the call notification failed: $e")
        }
    }

    fun cancel(context: Context, roomId: String) {
        context.getSystemService(NotificationManager::class.java).cancel(id(roomId))
    }

    /** Decline from the notification: the account may not be signed in yet when only a push started the process. */
    fun decline(context: Context, roomId: String, notification: String, done: () -> Unit = {}) {
        val app = context.applicationContext as MorpheusApp
        cancel(app, roomId)
        scope.launch {
            try {
                if (app.morpheus.account() == null) app.morpheus.restoreSession()
                app.morpheus.declineCall(roomId, notification)
            } catch (e: Exception) {
                Log.w(TAG, "declining failed: $e")
            } finally {
                done()
            }
        }
    }

    class RingActionReceiver : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == ACTION_DECLINE) {
                val pending = goAsync()
                decline(context, intent.getStringExtra("room_id").orEmpty(), intent.getStringExtra("notification").orEmpty()) { pending.finish() }
            }
        }
    }
}
