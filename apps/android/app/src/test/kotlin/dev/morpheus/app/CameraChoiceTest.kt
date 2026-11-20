package dev.morpheus.app

import android.hardware.camera2.CameraCharacteristics.LENS_FACING_BACK as BACK
import android.hardware.camera2.CameraCharacteristics.LENS_FACING_FRONT as FRONT
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class CameraChoiceTest {
    @Test
    fun the_wanted_side_is_used_when_the_phone_has_it() {
        assertEquals(1, CameraChoice.pick(listOf(BACK, FRONT), front = true))
        assertEquals(0, CameraChoice.pick(listOf(BACK, FRONT), front = false))
    }

    @Test
    fun a_phone_with_one_camera_uses_it_whichever_side_is_wanted() {
        assertEquals(0, CameraChoice.pick(listOf(BACK), front = true))
        assertNull(CameraChoice.pick(emptyList(), front = true))
    }

    @Test
    fun only_a_phone_with_both_sides_can_switch() {
        assertTrue(CameraChoice.canSwitch(listOf(BACK, FRONT)))
        assertFalse(CameraChoice.canSwitch(listOf(BACK)))
    }
}
