package dev.morpheus.app.ui

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class MaterialYouTest {
    @Test
    fun dynamic_colour_is_offered_from_android_12_and_not_before() {
        assertFalse(MaterialYou.offered(26))
        assertFalse(MaterialYou.offered(30))
        assertTrue(MaterialYou.offered(31))
        assertTrue(MaterialYou.offered(34))
    }
}
