package dev.morpheus.mobile

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Person
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.RingtoneManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.telecom.Connection
import android.telecom.DisconnectCause
import android.telecom.PhoneAccount
import android.telecom.PhoneAccountHandle
import android.telecom.TelecomManager
import android.util.Log
import org.json.JSONObject
import java.io.File

/**
 * Incoming calls as the phone's, as Discord does: Morpheus is a self-managed
 * calling app to Android (Telecom: headsets and cars can answer, a phone
 * call meanwhile is handled), and a ring shows the call notification
 * (Answer, Decline) and, over the lock screen, the full-screen incoming
 * call, ringing with the phone's ringtone. Rung by the app when it's not on
 * screen (`apps/mobile/src/push.rs`) or by a push when it isn't running;
 * Answer and Decline go back to the app through `call_action`.
 */
object CallRinger {
    private const val TAG = "MorpheusCalls"
    const val CHANNEL = MorpheusPushService.CALL_CHANNEL
    private const val NOTIFICATION = 4343

    /** A call ringing here, and its Telecom connection once Android made one. */
    class Ring(val roomId: String, val notification: String, val caller: String) {
        var connection: CallConnection? = null
    }

    private val rings = mutableMapOf<String, Ring>()
    /** Rings the app's own card shows (it was on screen): the phone's once it isn't. */
    private val waiting = mutableMapOf<String, Ring>()
    /** Rings over (answered, declined, stopped): never rung again. */
    private val done = mutableSetOf<String>()
    /** A call going in the room already (answered here). */
    private fun overIn(roomId: String) = active.any { it.roomId == roomId }

    // A ring event can come again (synced once more after answering).
    private fun finish(@Suppress("UNUSED_PARAMETER") roomId: String, notification: String) {
        done += notification
    }
    /** The calls answered, while they go. */
    private val active = mutableListOf<CallConnection>()

    /** From the app or a push: `caller` is ringing in `roomId` (`notification`, the ring's event). */
    @JvmStatic
    @Synchronized
    fun ring(context: Context, roomId: String, notification: String, caller: String) {
        Log.d(TAG, "ring event $notification in $roomId (done ${notification in done}, ringing ${rings.containsKey(roomId)})")
        // A ring over already (its event again), or a call going in the room.
        if (notification in done || rings.containsKey(roomId) || overIn(roomId)) return
        // On screen, the app's own card rings.
        if (PushStartup.visible) {
            Log.d(TAG, "ring in $roomId: the app is on screen, its card rings")
            waiting[roomId] = Ring(roomId, notification, caller)
            return
        }
        waiting.remove(roomId)
        Log.d(TAG, "ring in $roomId from $caller: as the phone's")
        val ring = Ring(roomId, notification, caller)
        rings[roomId] = ring
        val app = context.applicationContext
        try {
            val telecom = app.getSystemService(TelecomManager::class.java)
            val handle = account(app)
            if (telecom.isIncomingCallPermitted(handle)) {
                val extras = Bundle().apply {
                    putString("room_id", roomId)
                    putString("notification", notification)
                    putString("caller", caller)
                }
                telecom.addNewIncomingCall(handle, Bundle().apply {
                    putBundle(TelecomManager.EXTRA_INCOMING_CALL_EXTRAS, extras)
                })
                Log.d(TAG, "handed to Telecom")
                return
            }
        } catch (e: Exception) {
            Log.w(TAG, "Telecom refused the call: $e")
        }
        // No Telecom (busy in a phone call, or refused): the notification rings.
        showIncoming(app, ring)
    }

    /** From the app: the ring in `roomId` is over (answered elsewhere, given up, out of time). */
    @JvmStatic
    @Synchronized
    fun stop(context: Context, roomId: String) {
        waiting.remove(roomId)?.let { finish(roomId, it.notification) }
        val ring = rings.remove(roomId) ?: return
        finish(roomId, ring.notification)
        ring.connection?.let {
            it.setDisconnected(DisconnectCause(DisconnectCause.MISSED))
            it.destroy()
        }
        clearIncoming(context.applicationContext, roomId)
    }

    @Synchronized
    fun answer(context: Context, roomId: String) {
        Log.d(TAG, "answer in $roomId: ringing ${rings.containsKey(roomId)}")
        waiting.remove(roomId)
        val ring = rings.remove(roomId) ?: return
        finish(roomId, ring.notification)
        val app = context.applicationContext
        clearIncoming(app, roomId)
        ring.connection?.let {
            it.setActive()
            active += it
        }
        tell(app, JSONObject().put("action", "answer").put("room_id", roomId))
        // The app, in the call.
        app.startActivity(Intent(app, OpenRoomActivity::class.java).apply {
            flags = Intent.FLAG_ACTIVITY_NEW_TASK
            putExtra("room_id", roomId)
        })
    }

    @Synchronized
    fun decline(context: Context, roomId: String) {
        waiting.remove(roomId)
        val ring = rings.remove(roomId) ?: return
        finish(roomId, ring.notification)
        val app = context.applicationContext
        clearIncoming(app, roomId)
        ring.connection?.let {
            it.setDisconnected(DisconnectCause(DisconnectCause.REJECTED))
            it.destroy()
        }
        tell(app, JSONObject().put("action", "decline").put("room_id", roomId).put("notification", ring.notification))
    }

    /**
     * The app came on screen while ringing: its own card rings and answers
     * from here, so the phone's ringing (notification, Telecom) stops.
     */
    @JvmStatic
    @Synchronized
    fun appOnScreen(context: Context) {
        val app = context.applicationContext
        for (ring in rings.values.toList()) {
            // Rings on in the app; the phone's again if the app goes.
            waiting[ring.roomId] = Ring(ring.roomId, ring.notification, ring.caller)
            ring.connection?.let {
                it.setDisconnected(DisconnectCause(DisconnectCause.OTHER))
                it.destroy()
            }
            clearIncoming(app, ring.roomId)
        }
        rings.clear()
        // Rung by a push with the app closed: that notification too.
        val manager = app.getSystemService(NotificationManager::class.java)
        manager.activeNotifications
            .filter { it.notification.channelId == CHANNEL }
            .forEach { manager.cancel(it.tag, it.id) }
    }

    /** The app left the screen: the rings its card showed ring as the phone's. */
    @JvmStatic
    @Synchronized
    fun appLeftScreen(context: Context) {
        for (ring in waiting.values.toList()) {
            ring(context, ring.roomId, ring.notification, ring.caller)
        }
    }

    private val main = android.os.Handler(android.os.Looper.getMainLooper())
    private val endLater = Runnable { endNow() }

    /**
     * The app says no call is going (hung up in it or from its notification).
     * Ended a moment later, unless it says the call goes on: right after
     * joining it can say so briefly while the call settles.
     */
    @JvmStatic
    fun callEnded() {
        main.removeCallbacks(endLater)
        main.postDelayed(endLater, 1500)
    }

    /** The app says the call goes (again). */
    @JvmStatic
    fun callGoing() {
        main.removeCallbacks(endLater)
    }

    @Synchronized
    private fun endNow() {
        Log.d(TAG, "call ended: ${active.size} to disconnect")
        for (connection in active) {
            connection.setDisconnected(DisconnectCause(DisconnectCause.LOCAL))
            connection.destroy()
        }
        active.clear()
    }

    /** Telecom hung up the call answered (a headset's button, say). */
    @Synchronized
    fun hangUpFromTelecom(context: Context) {
        if (active.isEmpty()) return
        main.removeCallbacks(endLater)
        endNow()
        File(dataDir(context), "hang_up_call").writeText("")
    }

    @Synchronized
    fun attach(roomId: String, connection: CallConnection): Ring? =
        rings[roomId]?.also { it.connection = connection }

    @Synchronized
    fun refused(context: Context, roomId: String) {
        rings[roomId]?.let { showIncoming(context.applicationContext, it) }
    }

    @Synchronized
    fun ringFor(roomId: String): Ring? = rings[roomId]

    /** The call notification: Answer and Decline, the full-screen incoming call, the ringtone. */
    fun showIncoming(context: Context, ring: Ring) {
        try {
            postIncoming(context, ring)
            Log.d(TAG, "call notification posted for ${ring.roomId}")
        } catch (e: Exception) {
            Log.e(TAG, "call notification failed", e)
        }
    }

    private fun postIncoming(context: Context, ring: Ring) {
        val manager = context.getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(
            NotificationChannel(CHANNEL, "Calls", NotificationManager.IMPORTANCE_HIGH).apply {
                setSound(
                    RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE),
                    AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE).build()
                )
            }
        )
        fun action(what: String) = PendingIntent.getBroadcast(
            context, (what + ring.roomId).hashCode(),
            Intent(context, CallActionReceiver::class.java).setAction(what).putExtra("room_id", ring.roomId),
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val fullScreen = PendingIntent.getActivity(
            context, ("incoming" + ring.roomId).hashCode(),
            Intent(context, IncomingCallActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_NO_USER_ACTION
                putExtra("room_id", ring.roomId)
                putExtra("caller", ring.caller)
            },
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
        )
        val icon = context.resources.getIdentifier("ic_notification", "drawable", context.packageName)
            .takeIf { it != 0 } ?: context.applicationInfo.icon
        val builder = Notification.Builder(context, CHANNEL)
            .setSmallIcon(icon)
            .setContentTitle(ring.caller)
            .setContentText("Incoming call")
            .setCategory(Notification.CATEGORY_CALL)
            .setOngoing(true)
            .setTimeoutAfter(MorpheusPushService.RING_MS)
            .setContentIntent(fullScreen)
            .setFullScreenIntent(fullScreen, true)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            builder.setStyle(
                Notification.CallStyle.forIncomingCall(
                    Person.Builder().setName(ring.caller).setImportant(true).build(),
                    action(CallActionReceiver.DECLINE),
                    action(CallActionReceiver.ANSWER),
                )
            )
        } else {
            builder.addAction(Notification.Action.Builder(null, "Decline", action(CallActionReceiver.DECLINE)).build())
            builder.addAction(Notification.Action.Builder(null, "Answer", action(CallActionReceiver.ANSWER)).build())
        }
        val built = builder.build()
        built.flags = built.flags or Notification.FLAG_INSISTENT
        manager.notify(NOTIFICATION, built)
    }

    private fun clearIncoming(context: Context, roomId: String) {
        context.getSystemService(NotificationManager::class.java).cancel(NOTIFICATION)
        IncomingCallActivity.close(roomId)
    }

    /** Morpheus as a self-managed calling app (no user setting needed for one). */
    private fun account(context: Context): PhoneAccountHandle {
        val handle = PhoneAccountHandle(ComponentName(context, CallConnectionService::class.java), "morpheus")
        val telecom = context.getSystemService(TelecomManager::class.java)
        // Registering again is a no-op (reading the account back would need
        // READ_PHONE_NUMBERS).
        telecom.registerPhoneAccount(
            PhoneAccount.builder(handle, "Morpheus")
                .setCapabilities(PhoneAccount.CAPABILITY_SELF_MANAGED)
                .build()
        )
        return handle
    }


    private fun dataDir(context: Context) = File(context.filesDir, "morpheus").apply { mkdirs() }

    private fun tell(context: Context, action: JSONObject) {
        File(dataDir(context), "call_action").writeText(action.toString())
    }

    fun address(roomId: String): Uri = Uri.fromParts("morpheus", roomId, null)
}

/** A Morpheus call to Android's Telecom. */
class CallConnection(private val context: Context, val roomId: String, caller: String) : Connection() {
    init {
        connectionProperties = PROPERTY_SELF_MANAGED
        audioModeIsVoip = true
        connectionCapabilities = CAPABILITY_MUTE or CAPABILITY_HOLD
        setAddress(CallRinger.address(roomId), TelecomManager.PRESENTATION_ALLOWED)
        setCallerDisplayName(caller, TelecomManager.PRESENTATION_ALLOWED)
    }

    override fun onShowIncomingCallUi() {
        android.util.Log.d("MorpheusCalls", "Telecom asks for the incoming call UI")
        CallRinger.ringFor(roomId)?.let { CallRinger.showIncoming(context, it) }
    }

    override fun onAnswer() = CallRinger.answer(context, roomId)
    override fun onAnswer(videoState: Int) = CallRinger.answer(context, roomId)
    override fun onReject() = CallRinger.decline(context, roomId)

    override fun onDisconnect() {
        if (CallRinger.ringFor(roomId) != null) {
            CallRinger.decline(context, roomId)
        } else {
            CallRinger.hangUpFromTelecom(context)
        }
    }
}
