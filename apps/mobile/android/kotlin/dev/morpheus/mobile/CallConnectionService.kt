package dev.morpheus.mobile

import android.telecom.Connection
import android.telecom.ConnectionRequest
import android.telecom.ConnectionService
import android.telecom.PhoneAccountHandle
import android.telecom.TelecomManager

/** Where Telecom makes Morpheus's calls (`CallRinger`). */
class CallConnectionService : ConnectionService() {
    override fun onCreateIncomingConnection(account: PhoneAccountHandle?, request: ConnectionRequest?): Connection {
        // Ours, where Telecom puts them (nested, or at the top on some versions).
        val extras = request?.extras?.let { it.getBundle(TelecomManager.EXTRA_INCOMING_CALL_EXTRAS) ?: it }
        android.util.Log.d("MorpheusCalls", "Telecom made the connection for ${extras?.getString("room_id")}")
        val roomId = extras?.getString("room_id").orEmpty()
        val caller = extras?.getString("caller").orEmpty().ifEmpty { "Someone" }
        val connection = CallConnection(applicationContext, roomId, caller)
        CallRinger.attach(roomId, connection)
        connection.setRinging()
        return connection
    }

    override fun onCreateIncomingConnectionFailed(account: PhoneAccountHandle?, request: ConnectionRequest?) {
        // Telecom said no (a phone call going, say): the notification rings instead.
        request?.extras?.getString("room_id")?.let { CallRinger.refused(applicationContext, it) }
    }
}
