package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.unit.dp
import uniffi.morpheus_ffi.Morpheus

/**
 * The custom colour of Settings > Appearance (#446): the hex box, hue, saturation and value sliders, a small preview of the theme the colour
 * would make, the colours used lately and Use. The sums are `view_model::theme` (through ffi), the desktop's.
 */
@Composable
fun ColourPicker(morpheus: Morpheus, text: String, onText: (String) -> Unit, label: String, refreshKey: Int = 0, showPreview: Boolean = true, showUse: Boolean = true, onUse: () -> Unit) {
    val hsv = remember(text) { uniffi.morpheus_ffi.colourHsv(text) }
    val preview = remember(text) { uniffi.morpheus_ffi.themePreview(text) }
    var seen by remember { mutableIntStateOf(0) }
    val recent = remember(seen, refreshKey) { morpheus.recentColours() }
    Column(Modifier.padding(horizontal = Space.lg), verticalArrangement = Arrangement.spacedBy(Space.sm)) {
        // Where the colour sits on each slider (a colour not typed yet starts them in the middle); moving one writes the hex.
        val (h, s, v) = hsv?.let { Triple(it.h, it.s, it.v) } ?: Triple(220f, 0.5f, 0.5f)
        PickerSlider(t("Hue"), h, 0f..360f, "colour-hue") { onText(uniffi.morpheus_ffi.colourFromHsv(it, s, v)) }
        PickerSlider(t("Saturation"), s, 0f..1f, "colour-saturation") { onText(uniffi.morpheus_ffi.colourFromHsv(h, it, v)) }
        PickerSlider(t("Value"), v, 0f..1f, "colour-value") { onText(uniffi.morpheus_ffi.colourFromHsv(h, s, it)) }
        if (showPreview) preview?.let { ThemePreviewCard(it, text) }
        if (recent.isNotEmpty()) {
            Text(t("Recent colours"), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
            LazyRow(horizontalArrangement = Arrangement.spacedBy(Space.sm), modifier = Modifier.testTag("recent-colours")) {
                items(recent, key = { it }) { hex ->
                    // A round swatch with a ring (concentric: the ring is the swatch's radius plus its 4 dp gap).
                    androidx.compose.material3.IconButton({ onText(hex) }, Modifier.size(Size.target).semantics { contentDescription = "#$hex" }.testTag("recent-colour-$hex")) {
                        Box(Modifier.size(Size.target - Space.sm).clip(CircleShape).background(swatch(java.lang.Long.parseLong(hex, 16).toUInt())))
                    }
                }
            }
        }
        // The colour in use, then the box that takes a new one and Use: they stay where the thumb is, under the sliders (#482).
        Text(label, Modifier.testTag("custom-colour-label"), color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium)
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(Space.sm)) {
            OutlinedTextField(text, onText, Modifier.weight(1f).testTag("colour-input"), placeholder = { Text("2a4d8f") }, singleLine = true)
            if (showUse) Button({ onUse(); seen++ }, Modifier.testTag("colour-apply"), enabled = text.isNotBlank()) { Text(t("Use")) }
        }
    }
}

private fun swatch(rgb: UInt) = Color(0xFF000000.toInt() or rgb.toInt())

@Composable
private fun PickerSlider(label: String, value: Float, range: ClosedFloatingPointRange<Float>, tag: String, onChange: (Float) -> Unit) {
    Row(Modifier.testTag(tag), verticalAlignment = Alignment.CenterVertically) {
        Text(label, Modifier.width(96.dp), style = MaterialTheme.typography.bodyMedium)
        Slider(value.coerceIn(range), onChange, Modifier.weight(1f), valueRange = range)
    }
}

/** A small chat in the theme the colour makes: the rail, the sidebar, two messages and a button, every text in the palette's own readable colour. */
@Composable
private fun ThemePreviewCard(p: uniffi.morpheus_ffi.ThemePreview, text: String) {
    val colour = { c: UInt -> swatch(c) }
    Row(
        Modifier.fillMaxWidth().height(96.dp).clip(RoundedCornerShape(16.dp)).semantics { stateDescription = "#" + String.format("%06x", p.chat.toInt() and 0xFFFFFF) }.testTag("colour-preview"),
    ) {
        Box(Modifier.width(16.dp).height(96.dp).background(colour(p.rail)))
        Column(Modifier.width(72.dp).height(96.dp).background(colour(p.sidebar)).padding(8.dp), verticalArrangement = Arrangement.spacedBy(Space.sm)) {
            Text("# general", color = colour(p.text), style = MaterialTheme.typography.labelSmall, maxLines = 1)
            Text("# random", color = colour(p.muted), style = MaterialTheme.typography.labelSmall, maxLines = 1)
        }
        Column(Modifier.weight(1f).height(96.dp).background(colour(p.chat)).padding(8.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text(t("Hello there"), color = colour(p.text), style = MaterialTheme.typography.bodySmall, maxLines = 1)
            Text(t("A quieter line"), color = colour(p.muted), style = MaterialTheme.typography.bodySmall, maxLines = 1)
            Box(Modifier.clip(RoundedCornerShape(12.dp)).background(colour(p.accentSolid)).padding(horizontal = Space.md, vertical = Space.xs)) {
                Text(t("Send"), color = colour(uniffi.morpheus_ffi.onColour(p.accentSolid)), style = MaterialTheme.typography.labelSmall)
            }
        }
    }
}
