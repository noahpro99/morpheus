package dev.morpheus.app

import dev.morpheus.app.push.PushPayload
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PushPayloadTest {
    @Test
    fun anIdsOnlyPushNamesTheRoomAndEvent() {
        val body = """{"notification":{"room_id":"!r:hs","event_id":"${'$'}e","counts":{"unread":3}}}"""
        assertEquals(PushPayload("!r:hs", "${'$'}e", 3), PushPayload.parse(body.toByteArray()))
    }

    @Test
    fun noCountsMeansOneUnread() {
        val body = """{"notification":{"room_id":"!r:hs","event_id":"${'$'}e"}}"""
        assertEquals(1, PushPayload.parse(body.toByteArray())?.unread)
    }

    @Test
    fun aZeroCountIsPassedOnSoTheNotificationGoes() {
        val body = """{"notification":{"room_id":"!r:hs","counts":{"unread":0}}}"""
        assertEquals(0, PushPayload.parse(body.toByteArray())?.unread)
    }

    @Test
    fun whatIsNotAPushIsIgnored() {
        assertNull(PushPayload.parse("nonsense".toByteArray()))
        assertNull(PushPayload.parse("""{"notification":{}}""".toByteArray()))
        assertNull(PushPayload.parse("{}".toByteArray()))
    }
}
