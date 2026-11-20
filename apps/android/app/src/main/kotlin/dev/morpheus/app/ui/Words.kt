package dev.morpheus.app.ui

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.setValue
import uniffi.morpheus_ffi.Morpheus

/**
 * The words of the screens. The English text is the key (as on the desktop,
 * `view_model::i18n`): `t("Send")` is "Send" in English and what
 * `locales/<code>.json` says in the language picked in Settings. A text with
 * no translation stays English. Reading [version] makes whoever draws a word
 * redraw when the language changes.
 */
object Words {
    var morpheus: Morpheus? = null

    var version by mutableIntStateOf(0)
        private set

    fun changed() {
        version++
    }
}

/** The text for `key` in the language in use; `%{name}` placeholders are filled from `args`. */
fun t(key: String, vararg args: Pair<String, String>): String {
    Words.version
    var text = Words.morpheus?.translate(key) ?: key
    for ((name, value) in args) text = text.replace("%{$name}", value)
    return text
}
