package dev.morpheus.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.Screen

/** Placeholder until the servers and rooms arrive (#165): who is signed in, and a way out. */
@Composable
fun SignedInScreen(model: AppModel, state: Screen.SignedIn) {
    Column(
        Modifier.fillMaxSize().padding(24.dp).testTag("signed-in"),
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("Signed in as ${state.account.userId}", modifier = Modifier.testTag("account"))
        Button({ model.signOut() }, Modifier.testTag("sign-out")) { Text("Sign out") }
    }
}
