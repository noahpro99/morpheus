package dev.morpheus.app.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import uniffi.morpheus_ffi.IncomingRing

/** A call ringing while the app is on screen: who, and Answer or Decline. */
@Composable
fun RingCard(ring: IncomingRing, onAnswer: () -> Unit, onDecline: () -> Unit) {
    Column(
        Modifier.fillMaxWidth().padding(8.dp).clip(RoundedCornerShape(12.dp)).background(Palette.Raised).padding(12.dp).testTag("ring-card"),
    ) {
        Text(t("%{name} is calling", "name" to ring.caller), fontWeight = FontWeight.Bold, color = Palette.Text, modifier = Modifier.testTag("ring-who"), style = MaterialTheme.typography.bodyLarge)
        Text(t("Incoming call"), color = Palette.Muted, style = MaterialTheme.typography.bodyLarge)
        Row(Modifier.fillMaxWidth().padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
            Button(onDecline, Modifier.testTag("ring-decline"), colors = ButtonDefaults.buttonColors(containerColor = Palette.Danger)) { Text(t("Decline"), color = Palette.OnDanger) }
            Button(onAnswer, Modifier.testTag("ring-answer"), colors = ButtonDefaults.buttonColors(containerColor = Color(0xFF23A559))) { Text(t("Answer"), color = onColour(Color(0xFF23A559))) }
        }
    }
}
