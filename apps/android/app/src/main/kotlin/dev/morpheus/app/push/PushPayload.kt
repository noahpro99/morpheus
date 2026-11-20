package dev.morpheus.app.push

import org.json.JSONObject

/**
 * What a push says. The homeserver sends ids only (`event_id_only`): which
 * room and event, and how many messages are unread there (0: the person caught
 * up elsewhere, so the notification goes).
 */
data class PushPayload(val roomId: String, val eventId: String, val unread: Int) {
    companion object {
        fun parse(bytes: ByteArray): PushPayload? = try {
            val notification = JSONObject(String(bytes)).optJSONObject("notification")
            if (notification == null) {
                null
            } else {
                val room = notification.optString("room_id")
                if (room.isEmpty()) {
                    null
                } else {
                    PushPayload(
                        room,
                        notification.optString("event_id"),
                        notification.optJSONObject("counts")?.optInt("unread", 1) ?: 1,
                    )
                }
            }
        } catch (_: Exception) {
            null
        }
    }
}
