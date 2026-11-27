package dev.morpheus.app

import dev.morpheus.app.ui.RailEntry
import dev.morpheus.app.ui.railEntries
import org.junit.Assert.assertEquals
import org.junit.Test
import uniffi.morpheus_ffi.RailRowItem
import uniffi.morpheus_ffi.ScopeItem
import uniffi.morpheus_ffi.ScopeKind

class RailFoldersTest {
    private fun scope(id: String, unread: Boolean = false) = ScopeItem("space:$id", ScopeKind.SERVER, id, null, null, unread, 0uL, emptyList())
    private val scopes = listOf(ScopeItem("rooms", ScopeKind.ROOMS, "Rooms", null, null, false, 0uL, emptyList()), scope("a"), scope("b", unread = true), scope("c"))
    private val rows = listOf(RailRowItem("a", null, "", listOf("a")), RailRowItem("folder:f", "f", "Work", listOf("b", "c")))

    @Test
    fun a_closed_folder_stands_where_its_first_server_was_and_hides_its_servers() {
        val out = railEntries(scopes, rows, emptySet(), "space:a")
        assertEquals(listOf("rooms", "space:a", "folder:f"), out.map { it.key })
        val folder = out.last() as RailEntry.Folder
        assertEquals(true, folder.unread)
        assertEquals(false, folder.selected)
    }

    @Test
    fun an_open_folder_lists_its_servers_after_it_and_a_selected_server_inside_a_closed_one_marks_the_folder() {
        val open = railEntries(scopes, rows, setOf("f"), "space:c")
        assertEquals(listOf("rooms", "space:a", "folder:f", "space:b", "space:c"), open.map { it.key })
        assertEquals(listOf(false, false, true, true), open.filterIsInstance<RailEntry.Scope>().map { it.inFolder }.let { listOf(it[0], it[1]) + listOf(it[2], it[3]) })
        val closed = railEntries(scopes, rows, emptySet(), "space:c")
        assertEquals(true, (closed.last() as RailEntry.Folder).selected)
    }
}
