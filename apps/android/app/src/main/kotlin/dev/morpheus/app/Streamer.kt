package dev.morpheus.app

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import uniffi.morpheus_ffi.Morpheus

/**
 * Streamer mode (#540): while this phone shares its screen in a call (when "automatic" is on), or when turned on by hand, people's names show
 * as their first letter and dots and IDs as dots. The rules are Rust's (`view_model::streamer`); this holds what is on now, as Compose
 * state, so what shows names repaints when it changes.
 */
object Streamer {
    private var shown by mutableStateOf(false)
    private var maskNames by mutableStateOf(true)
    private var maskIds by mutableStateOf(true)

    /** This phone is sharing its screen in a call. */
    @Volatile
    var sharing: Boolean = false

    /** Whether the mode is on now. */
    val active: Boolean get() = shown

    /** Read the choices and the share again (after either changed). */
    fun update(morpheus: Morpheus) {
        val choices = morpheus.streamerChoices()
        maskNames = choices.names
        maskIds = choices.ids
        shown = morpheus.streamerActive(sharing)
    }

    /** A person's name as it is shown. */
    fun name(name: String): String = if (shown && maskNames) uniffi.morpheus_ffi.streamerMaskName(name) else name

    /** An ID, address or link as it is shown. */
    fun id(id: String): String = if (shown && maskIds) uniffi.morpheus_ffi.streamerMaskId(id) else id
}
