import SwiftUI

struct SignInView: View {
    @Environment(AppModel.self) private var model
    @State private var homeserver = ""
    @State private var username = ""
    @State private var password = ""
    @FocusState private var focus: Field?

    private enum Field { case homeserver, username, password }

    var body: some View {
        // Thumb-first: the form sits low, above the keyboard.
        ScrollView {
        VStack(spacing: 16) {
            Spacer(minLength: 24)
            Image(systemName: "bubble.left.and.bubble.right.fill")
                .font(.largeTitle)
                .foregroundStyle(model.theme.accent)
                .accessibilityHidden(true)
            Text("Morpheus").font(.largeTitle.bold()).foregroundStyle(model.theme.text)
            field("Homeserver", text: $homeserver, id: "sign-in-homeserver")
                .focused($focus, equals: .homeserver)
                .textContentType(.URL)
                .keyboardType(.URL)
                .submitLabel(.next)
                .onSubmit { focus = .username }
            field("Username", text: $username, id: "sign-in-username")
                .focused($focus, equals: .username)
                .textContentType(.username)
                .submitLabel(.next)
                .onSubmit { focus = .password }
            SecureField("Password", text: $password)
                .textFieldStyle(.roundedBorder)
                .textContentType(.password)
                .focused($focus, equals: .password)
                .submitLabel(.go)
                .onSubmit(submit)
                .accessibilityIdentifier("sign-in-password")
            if let error = model.error {
                Text(error).font(.footnote).foregroundStyle(model.theme.danger)
                    .accessibilityIdentifier("sign-in-error")
            }
            Button(action: submit) {
                Group {
                    if model.signingIn { ProgressView() } else { Text("Sign in") }
                }
                .frame(maxWidth: .infinity, minHeight: 44)
            }
            .buttonStyle(.borderedProminent)
            .foregroundStyle(model.theme.onAccent)
            .disabled(model.signingIn)
            .accessibilityIdentifier("sign-in-submit")
        }
        .padding(24)
        .frame(maxWidth: 480)
        }
        .defaultScrollAnchor(.bottom)
        .scrollBounceBehavior(.basedOnSize)
        .onAppear { if homeserver.isEmpty { homeserver = model.defaultHomeserver } }
    }

    private func field(_ title: String, text: Binding<String>, id: String) -> some View {
        TextField(title, text: text)
            .textFieldStyle(.roundedBorder)
            .textInputAutocapitalization(.never)
            .autocorrectionDisabled()
            .accessibilityIdentifier(id)
    }

    private func submit() {
        guard !username.isEmpty, !password.isEmpty else {
            model.error = "Enter your username and password."
            return
        }
        Haptics.tap()
        Task { await model.signIn(homeserver: homeserver, username: username, password: password) }
    }
}
