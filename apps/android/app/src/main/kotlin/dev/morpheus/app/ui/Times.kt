package dev.morpheus.app.ui

import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.produceState
import kotlinx.coroutines.delay

/** The time now, in Unix ms, refreshed every 10 s so "Today" and "5 min ago" roll over as the desktop's do. */
@Composable
fun rememberNow(): Long {
    val now by produceState(System.currentTimeMillis() + dev.morpheus.app.AppState.clockOffsetMs) {
        while (true) {
            delay(10_000)
            value = System.currentTimeMillis() + dev.morpheus.app.AppState.clockOffsetMs
        }
    }
    return now
}

/** When a message was sent, worded and clocked by the shared view-model (the 12/24-hour setting included). */
@Composable
fun whenSent(timestampMs: Long, now: Long): String {
    Words.version
    return Words.morpheus?.messageTime(timestampMs.toULong(), now.toULong()) ?: ""
}

/** A date divider's label: Today, Yesterday, or the date. */
@Composable
fun dayLabel(timestampMs: Long, now: Long): String {
    Words.version
    return Words.morpheus?.dayLabel(timestampMs.toULong(), now.toULong()) ?: ""
}
