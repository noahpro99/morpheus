package dev.morpheus.app

import dev.morpheus.app.ui.FloatCorner
import dev.morpheus.app.ui.cornerPosition
import dev.morpheus.app.ui.nearestCorner
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class FloatingCallTest {
    @Test
    fun a_drop_snaps_to_the_corner_of_the_quarter_it_is_in() {
        assertEquals(FloatCorner(right = false, bottom = false), nearestCorner(100f, 200f, 1000f, 2000f))
        assertEquals(FloatCorner(right = true, bottom = false), nearestCorner(900f, 200f, 1000f, 2000f))
        assertEquals(FloatCorner(right = false, bottom = true), nearestCorner(100f, 1800f, 1000f, 2000f))
        assertEquals(FloatCorner(right = true, bottom = true), nearestCorner(900f, 1800f, 1000f, 2000f))
    }

    @Test
    fun the_corners_sit_inside_the_area_above_what_must_stay_clear() {
        // 1000 wide; usable from y = 150 down to 1700 (the search bar's top); a 200 square with a 24 margin.
        val (lx, ty) = cornerPosition(FloatCorner(right = false, bottom = false), 1000f, 150f, 1700f, 200f, 24f)
        assertEquals(24f, lx, 0f)
        assertEquals(174f, ty, 0f)
        val (rx, by) = cornerPosition(FloatCorner(right = true, bottom = true), 1000f, 150f, 1700f, 200f, 24f)
        assertEquals(776f, rx, 0f)
        assertEquals(1476f, by, 0f)
        assertTrue("the bottom corner ends above the bar", by + 200f <= 1700f)
    }

    @Test
    fun a_small_area_never_puts_the_square_above_its_top() {
        val (_, y) = cornerPosition(FloatCorner(right = true, bottom = true), 1000f, 150f, 300f, 200f, 24f)
        assertEquals(150f, y, 0f)
    }
}
