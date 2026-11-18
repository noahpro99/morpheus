package dev.morpheus.app

import kotlinx.coroutines.channels.awaitClose
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.callbackFlow
import uniffi.morpheus_ffi.Morpheus
import uniffi.morpheus_ffi.SyncListener

// The Rust side pushes updates to a listener and hands back a TaskHandle; a
// callbackFlow turns that into a Flow that starts when collected and stops
// the Rust task when it is not collected any more (docs/ANDROID.md).

/** The sync state ("running", "offline", ...), now and whenever it changes. */
fun Morpheus.syncStates(): Flow<String> = callbackFlow {
    val handle = watchSync(object : SyncListener {
        override fun onSyncState(state: String) {
            trySend(state)
        }
    })
    awaitClose { handle.cancel() }
}
