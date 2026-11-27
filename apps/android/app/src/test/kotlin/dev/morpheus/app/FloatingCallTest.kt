package dev.morpheus.app

import dev.morpheus.app.ui.FloatSpot
import dev.morpheus.app.ui.spotOf
import dev.morpheus.app.ui.spotPosition
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class FloatingCallTest {
    @Test
    fun a_drop_snaps_to_the_nearer_side_and_keeps_its_height() {
        assertEquals(FloatSpot(right = false, yFraction = 0.1f), spotOf(100f, 200f, 1000f, 2000f, 240f))
        assertEquals(FloatSpot(right = true, yFraction = 0.5f), spotOf(700f, 1000f, 1000f, 2000f, 240f))
    }

    @Test
    fun the_square_sits_against_its_side_at_its_height_inside_the_usable_band() {
        // 1000 wide, 2000 high; usable from y = 150 down to 1700 (the search bar's top); a 240 square with a 24 margin.
        val (lx, ly) = spotPosition(FloatSpot(false, 0.4f), 1000f, 2000f, 150f, 1700f, 240f, 24f)
        assertEquals(24f, lx, 0f)
        assertEquals(800f, ly, 0f)
        val (rx, _) = spotPosition(FloatSpot(true, 0.4f), 1000f, 2000f, 150f, 1700f, 240f, 24f)
        assertEquals(736f, rx, 0f)
    }

    @Test
    fun a_height_is_kept_clear_of_the_top_and_of_what_is_at_the_foot() {
        val (_, high) = spotPosition(FloatSpot(true, 0f), 1000f, 2000f, 150f, 1700f, 240f, 24f)
        assertEquals(150f, high, 0f)
        val (_, low) = spotPosition(FloatSpot(true, 0.95f), 1000f, 2000f, 150f, 1700f, 240f, 24f)
        assertEquals(1436f, low, 0f)
        assertTrue("above the bar", low + 240f <= 1700f)
    }
}
