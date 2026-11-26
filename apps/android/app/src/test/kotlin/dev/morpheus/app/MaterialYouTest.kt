package dev.morpheus.app

import dev.morpheus.app.ui.MaterialYou
import org.junit.Assert.assertEquals
import org.junit.Test

/** #515: Material You is the default where the system offers it (API 31+); a choice the person made is kept. */
class MaterialYouTest {
    @Test fun on_by_default_from_api_31() {
        assertEquals(true, MaterialYou.resolve(null, 31))
        assertEquals(true, MaterialYou.resolve(null, 34))
    }

    @Test fun off_by_default_before_api_31() {
        assertEquals(false, MaterialYou.resolve(null, 30))
        assertEquals(false, MaterialYou.resolve(null, 26))
    }

    @Test fun an_explicit_choice_is_kept() {
        assertEquals(false, MaterialYou.resolve(false, 34))
        assertEquals(true, MaterialYou.resolve(true, 34))
    }
}
