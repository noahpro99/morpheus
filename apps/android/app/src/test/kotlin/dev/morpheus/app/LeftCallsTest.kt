package dev.morpheus.app

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class LeftCallsTest {
    @Test
    fun a_room_just_left_hides_my_name_until_i_join_again() {
        assertFalse(LeftCalls.hidesMe("!a:x"))
        LeftCalls.mark("!a:x")
        assertTrue(LeftCalls.hidesMe("!a:x"))
        assertFalse("only that room", LeftCalls.hidesMe("!b:x"))
        LeftCalls.clear("!a:x")
        assertFalse(LeftCalls.hidesMe("!a:x"))
    }
}
