package dev.morpheus.app

import android.content.Context
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import uniffi.morpheus_ffi.IncomingRing
import uniffi.morpheus_ffi.RingListener

/**
 * Calls ringing this account while the app runs (Rust: `watch_rings`). On
 * screen the app shows a card; away from it the phone rings with
 * [Ringer]'s notification, and both stop when the ring is over.
 */
class RingModel(private val context: Context) : RingListener {
    private val _rings = MutableStateFlow<List<IncomingRing>>(emptyList())

    /** What is ringing now, newest last. */
    val rings: StateFlow<List<IncomingRing>> = _rings.asStateFlow()

    override fun onRing(ring: IncomingRing) {
        _rings.value = _rings.value.filter { it.roomId != ring.roomId } + ring
        if (!dev.morpheus.app.push.Push.visible) Ringer.show(context, ring.roomId, ring.notification, ring.caller)
    }

    override fun onRingOver(roomId: String, notification: String) {
        _rings.value = _rings.value.filter { it.notification != notification }
        Ringer.cancel(context, roomId)
    }

    /** This one is answered or declined here. */
    fun done(ring: IncomingRing) {
        _rings.value = _rings.value.filter { it.notification != ring.notification }
        Ringer.cancel(context, ring.roomId)
    }

    /** The app came on screen: its card rings, not the notification. */
    fun onScreen() {
        _rings.value.forEach { Ringer.cancel(context, it.roomId) }
    }
}
