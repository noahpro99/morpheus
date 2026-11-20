package dev.morpheus.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Person
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.ServiceInfo
import android.os.Build
import android.os.IBinder

/**
 * A call is the phone's call while it lasts: a foreground service (which also
 * keeps the microphone working with the app in the background) with an
 * ongoing call notification, in Android's call style where there is one,
 * naming the room, with Hang up. [CallModel] starts and stops it.
 */
class CallService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        when (intent?.action) {
            STOP -> {
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
            HANG_UP -> {
                CallModel.current?.hangUp()
                stopForeground(STOP_FOREGROUND_REMOVE)
                stopSelf()
            }
            else -> {
                val room = intent?.getStringExtra("room").orEmpty().ifEmpty { "Call" }
                val notification = build(room)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
                    startForeground(ID, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE)
                } else {
                    startForeground(ID, notification)
                }
            }
        }
        return START_NOT_STICKY
    }

    private fun build(room: String): Notification {
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Ongoing calls", NotificationManager.IMPORTANCE_DEFAULT).apply { setSound(null, null) },
        )
        val hangUp = PendingIntent.getService(
            this, 1, Intent(this, CallService::class.java).setAction(HANG_UP),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val open = PendingIntent.getActivity(
            this, 2, Intent(this, MainActivity::class.java).apply { flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val builder = Notification.Builder(this, CHANNEL)
            .setSmallIcon(applicationInfo.icon)
            .setContentTitle(room)
            .setContentText("In a call")
            .setOngoing(true)
            .setCategory(Notification.CATEGORY_CALL)
            .setContentIntent(open)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            builder.setStyle(Notification.CallStyle.forOngoingCall(Person.Builder().setName(room).build(), hangUp))
        } else {
            builder.addAction(Notification.Action.Builder(null, "Hang up", hangUp).build())
        }
        return builder.build()
    }

    companion object {
        const val CHANNEL = "ongoing_calls"
        const val ID = 4242
        const val STOP = "dev.morpheus.app.STOP_CALL"
        const val HANG_UP = "dev.morpheus.app.HANG_UP"

        /** The call in `roomId` is going (empty: it ended). */
        fun update(context: Context, roomId: String, room: String) {
            val intent = Intent(context, CallService::class.java)
            try {
                if (roomId.isEmpty()) {
                    context.startService(intent.setAction(STOP))
                } else {
                    context.startForegroundService(intent.putExtra("room_id", roomId).putExtra("room", room))
                }
            } catch (e: Exception) {
                android.util.Log.w("MorpheusCalls", "the call service did not start: $e")
            }
        }
    }
}
