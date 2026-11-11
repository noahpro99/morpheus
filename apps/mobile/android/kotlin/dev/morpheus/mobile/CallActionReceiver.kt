package dev.morpheus.mobile

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** The call notification's Answer and Decline. */
class CallActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val roomId = intent.getStringExtra("room_id") ?: return
        when (intent.action) {
            ANSWER -> CallRinger.answer(context, roomId)
            DECLINE -> CallRinger.decline(context, roomId)
        }
    }

    companion object {
        const val ANSWER = "dev.morpheus.mobile.ANSWER_CALL"
        const val DECLINE = "dev.morpheus.mobile.DECLINE_CALL"
    }
}
