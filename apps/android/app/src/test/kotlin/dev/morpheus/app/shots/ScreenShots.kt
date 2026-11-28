package dev.morpheus.app.shots

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.background
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.ui.draw.clip
import androidx.compose.material3.MaterialTheme
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.compose.ui.unit.dp
import com.github.takahirom.roborazzi.RoborazziATFAccessibilityCheckOptions
import com.github.takahirom.roborazzi.RoborazziATFAccessibilityChecker
import com.github.takahirom.roborazzi.checkRoboAccessibility
import com.google.android.apps.common.testing.accessibility.framework.AccessibilityCheckPreset
import com.github.takahirom.roborazzi.captureRoboImage
import dev.morpheus.app.R
import dev.morpheus.app.ui.CheckRow
import dev.morpheus.app.ui.ChoiceRow
import dev.morpheus.app.ui.GlyphIcon
import dev.morpheus.app.ui.IconAction
import dev.morpheus.app.ui.ListRow
import dev.morpheus.app.ui.EmptyState
import dev.morpheus.app.ui.MentionPill
import dev.morpheus.app.ui.MorpheusTheme
import dev.morpheus.app.ui.RoomRow
import dev.morpheus.app.ui.Palette
import dev.morpheus.app.ui.ScreenScaffold
import dev.morpheus.app.ui.SearchField
import dev.morpheus.app.ui.SectionHeader
import dev.morpheus.app.ui.SkeletonRows
import dev.morpheus.app.ui.ToggleRow
import dev.morpheus.app.ui.TopBar
import dev.morpheus.app.ui.UnreadBadge
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.ParameterizedRobolectricTestRunner
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The approved look of the screens, light and dark (src/test/screenshots): `just android-shots-verify` fails when a
 * pixel moves, `just android-shots-record` approves a change. They are drawn on the JVM by Robolectric from the same
 * composables the app shows, on the real theme, with fixed data (the screens that need a signed-in account are drawn
 * from their parts: the app model cannot be made without the Rust core).
 */
@RunWith(ParameterizedRobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(sdk = [34], qualifiers = "w411dp-h891dp-xhdpi")
class ScreenShots(private val light: Boolean) {
    companion object {
        @JvmStatic
        @ParameterizedRobolectricTestRunner.Parameters(name = "{0}")
        fun modes() = listOf(arrayOf<Any>(true), arrayOf<Any>(false))
    }

    @get:Rule
    val compose = createComposeRule()

    private val mode get() = if (light) "light" else "dark"

    /** Every tappable thing is at least 48 dp each way. The accessibility checker did not flag a full-width row at 44 dp (the guard test below shows it), so the size is checked here directly (#478). */
    private fun assertTouchTargets() {
        val density = compose.density.density
        val small = compose.onAllNodes(androidx.compose.ui.test.hasClickAction()).fetchSemanticsNodes()
            .filter { it.size.height < 48 * density - 1 || it.size.width < 48 * density - 1 }
            .map { "${it.size.width / density} x ${it.size.height / density} dp" }
        org.junit.Assert.assertTrue("tappable things under 48 dp: $small", small.isEmpty())
    }

    private fun shoot(name: String, content: @Composable () -> Unit) {
        Palette.useScheme(if (light) lightColorScheme() else darkColorScheme(), light)
        compose.setContent { MorpheusTheme { content() } }
        compose.onRoot().captureRoboImage("src/test/screenshots/$name-$mode.png")
        assertTouchTargets()
        // The Accessibility Test Framework over the same screen: touch targets of 48 dp, text and image contrast,
        // content descriptions and the rest of its latest preset. Any error fails the test.
        compose.onRoot().checkRoboAccessibility(
            RoborazziATFAccessibilityCheckOptions(
                RoborazziATFAccessibilityChecker(AccessibilityCheckPreset.LATEST),
                RoborazziATFAccessibilityChecker.CheckLevel.Error,
            ),
        )
    }

    @Test
    fun empty_list() = shoot("empty-list") {
        ScreenScaffold("empty", topBar = { TopBar("DMs") }) { pad ->
            EmptyState(R.drawable.ic_ms_forum, "No conversations yet", "Start one with the + button.", Modifier.padding(pad))
        }
    }

    @Test
    fun loading_list() = shoot("loading-list") {
        ScreenScaffold("loading", topBar = { TopBar("Rooms") }) { pad ->
            SkeletonRows(modifier = Modifier.padding(pad))
        }
    }

    @Test
    fun settings_rows() = shoot("settings-rows") {
        ScreenScaffold("settings", topBar = { TopBar("Settings") }) { pad ->
            Column(Modifier.padding(pad).fillMaxWidth()) {
                SectionHeader("Look")
                ChoiceRow("System", chosen = true, tag = "a", onPick = {})
                ChoiceRow("Light", chosen = false, tag = "b", onPick = {})
                ChoiceRow("Dark", chosen = false, tag = "c", supporting = "Easier on the eyes at night", onPick = {})
                SectionHeader("Notifications")
                ToggleRow("Messages", checked = true, tag = "d", onToggle = {})
                ToggleRow("Mentions only", checked = false, tag = "e", supporting = "Quiet unless someone names you", onToggle = {})
                CheckRow("Sound", checked = true, tag = "f", onToggle = {})
                CheckRow("Vibrate", checked = false, tag = "g", onToggle = {})
            }
        }
    }

    @Test
    fun room_list() = shoot("room-list") {
        ScreenScaffold("rooms", topBar = {
            TopBar("Rooms") {
                IconAction(R.drawable.ic_ms_add, "New", "new", {})
            }
        }) { pad ->
            Column(Modifier.padding(pad).fillMaxWidth()) {
                SectionHeader("Engineering")
                ListRow("general", "r1", supporting = "alice: see you at ten", leading = { GlyphIcon(R.drawable.ic_ms_tag) }, trailing = { UnreadBadge(3UL, "b1") }, strong = true, onClick = {})
                ListRow("random", "r2", supporting = "bob: lol", leading = { GlyphIcon(R.drawable.ic_ms_tag) }, quiet = true, onClick = {})
                ListRow("leave", "r3", leading = { GlyphIcon(R.drawable.ic_ms_delete) }, danger = true, onClick = {})
            }
        }
    }

    private fun room(name: String, dm: Boolean = false, unread: ULong = 0UL, highlights: ULong = 0UL, muted: Boolean = false) = uniffi.morpheus_ffi.RoomItem(
        id = name, name = name, topic = null, avatar = null, avatarKey = null, isDm = dm, isVoice = false, isEncrypted = false,
        unread = unread, highlights = highlights, muted = muted, pinned = false, call = emptyList(), dmUser = null,
    )

    /** The channel list as the phone draws it (#478): the real rows, full width, with counts and a selection; the touch-target check runs over them. */
    @Test
    fun channel_list() = shoot("channel-list") {
        ScreenScaffold("channels", topBar = { TopBar("Engineering") }) { pad ->
            Column(Modifier.padding(pad).fillMaxWidth()) {
                SectionHeader("Text channels")
                RoomRow(room("general", unread = 3UL), selected = false, onOpen = {}, onMenu = {}, callName = { it })
                RoomRow(room("random"), selected = true, onOpen = {}, onMenu = {}, callName = { it })
                RoomRow(room("a-channel-with-a-very-long-name-that-must-not-run-under-the-counts", unread = 120UL, highlights = 2UL), selected = false, onOpen = {}, onMenu = {}, callName = { it })
                RoomRow(room("announcements", muted = true), selected = false, onOpen = {}, onMenu = {}, callName = { it })
                SectionHeader("Direct messages")
                RoomRow(room("bob", dm = true, unread = 1UL), selected = false, onOpen = {}, onMenu = {}, callName = { it })
            }
        }
    }

    /** The accessibility check itself (#478): a 44 dp tappable row is flagged by the touch-target check the pictures run, so a row that small can never pass the screenshot tests. */
    @Test
    fun the_accessibility_check_flags_a_44_dp_row() {
        Palette.useScheme(if (light) lightColorScheme() else darkColorScheme(), light)
        compose.setContent {
            MorpheusTheme {
                ScreenScaffold("small-row", topBar = {}) { pad ->
                    Column(Modifier.padding(pad).fillMaxWidth()) { ListRow("general", "small", Modifier.heightIn(max = 44.dp), onClick = {}) }
                }
            }
        }
        val flagged = try {
            assertTouchTargets()
            false
        } catch (e: AssertionError) {
            true
        }
        org.junit.Assert.assertTrue("a 44 dp row must fail the touch-target check", flagged)
    }

    /** The composer area with the button row shown (#534): the pill and the row stand on the one surface, no band of another fill behind the row. */
    @Test
    fun composer_with_actions() = shoot("composer-with-actions") {
        ScreenScaffold("room", topBar = {}) { pad ->
            Column(Modifier.padding(pad).fillMaxSize()) {
                Box(Modifier.weight(1f).fillMaxWidth())
                dev.morpheus.app.ui.ComposerDock {
                    Box(Modifier.fillMaxWidth().padding(8.dp).heightIn(min = 56.dp).clip(RoundedCornerShape(28.dp)).background(dev.morpheus.app.ui.ComposerColours.fill(MaterialTheme.colorScheme)))
                    dev.morpheus.app.ui.ChatActions(true, {}, {}, {}, {})
                }
            }
        }
    }

    /** The channel list panel (#536): rounded at the bottom left too, and the bottom row's highlight reaches the panel's bottom edge, its corners the panel's. */
    @Test
    fun channel_panel() = shoot("channel-panel") {
        Box(Modifier.fillMaxSize().background(Palette.Background).padding(end = 72.dp)) {
            Column(Modifier.fillMaxSize().clip(dev.morpheus.app.ui.channelPanelShape).background(Palette.Surface), verticalArrangement = androidx.compose.foundation.layout.Arrangement.Bottom) {
                RoomRow(room("general", unread = 3UL), selected = false, onOpen = {}, onMenu = {}, callName = { it })
                RoomRow(room("random"), selected = true, onOpen = {}, onMenu = {}, callName = { it })
            }
        }
    }

    /** The reply icon (#535): the Material Symbols glyph in the banner above the composer, on its fill, with the contrast helpers. */
    @Test
    fun reply_banner() = shoot("reply-banner") {
        ScreenScaffold("reply", topBar = {}) { pad ->
            Column(Modifier.padding(pad).fillMaxWidth()) {
                dev.morpheus.app.ui.ComposerBanner(R.drawable.ic_ms_reply, "Replying to bob: see you at five", "reply-bar", "reply-banner", "Cancel reply", "reply-cancel") {}
            }
        }
    }

    @Test
    fun search_and_badges() = shoot("search-and-badges") {
        ScreenScaffold("search", topBar = { TopBar("Search") }) { pad ->
            Column(Modifier.padding(pad).padding(16.dp)) {
                SearchField("", {}, "Search messages", "q1")
                SearchField("hello", {}, "Search messages", "q2", Modifier.padding(top = 16.dp))
                androidx.compose.foundation.layout.Row(Modifier.padding(top = 16.dp)) {
                    UnreadBadge(3UL, "u1")
                    UnreadBadge(120UL, "u2", muted = true, modifier = Modifier.padding(start = 16.dp))
                    MentionPill(2UL, "m1", Modifier.padding(start = 16.dp))
                }
            }
        }
    }
}
