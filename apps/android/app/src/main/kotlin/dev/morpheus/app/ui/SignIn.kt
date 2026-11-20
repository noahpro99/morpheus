package dev.morpheus.app.ui

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Button
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.testTag
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import dev.morpheus.app.AppModel
import dev.morpheus.app.BuildConfig

/** Homeserver, username, password. The page scrolls when the keyboard leaves too little room. */
@Composable
fun SignInScreen(model: AppModel) {
    val signingIn by model.signingIn.collectAsState()
    val error by model.error.collectAsState()
    var homeserver by rememberSaveable { mutableStateOf(BuildConfig.DEFAULT_HOMESERVER) }
    var username by rememberSaveable { mutableStateOf("") }
    var password by rememberSaveable { mutableStateOf("") }
    val submit = { model.signIn(homeserver, username, password) }

    Column(
        Modifier
            .fillMaxWidth()
            .verticalScroll(rememberScrollState())
            .padding(24.dp)
            .testTag("sign-in"),
        verticalArrangement = Arrangement.spacedBy(12.dp, Alignment.CenterVertically),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text(t("Morpheus"), style = MaterialTheme.typography.headlineLarge)
        Text(t("Sign in with your Matrix account"), color = MaterialTheme.colorScheme.onSurfaceVariant)
        val field = Modifier.fillMaxWidth().widthIn(max = 480.dp)
        OutlinedTextField(
            homeserver, { homeserver = it },
            field.testTag("homeserver"),
            label = { Text(t("Homeserver")) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, imeAction = ImeAction.Next),
        )
        OutlinedTextField(
            username, { username = it },
            field.testTag("username"),
            label = { Text(t("Username")) },
            singleLine = true,
            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Next),
        )
        OutlinedTextField(
            password, { password = it },
            field.testTag("password"),
            label = { Text(t("Password")) },
            singleLine = true,
            visualTransformation = PasswordVisualTransformation(),
            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, imeAction = ImeAction.Done),
            keyboardActions = KeyboardActions(onDone = { submit() }),
        )
        error?.let {
            Text(it, color = MaterialTheme.colorScheme.error, modifier = Modifier.testTag("sign-in-error"))
        }
        Button(
            submit,
            field.testTag("sign-in-button"),
            enabled = !signingIn && username.isNotBlank() && password.isNotEmpty(),
        ) { Text(if (signingIn) "Signing in…" else "Sign in") }
    }
}
