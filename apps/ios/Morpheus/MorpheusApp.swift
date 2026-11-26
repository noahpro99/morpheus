import SwiftUI

@main
struct MorpheusApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
        }
    }
}

struct RootView: View {
    @Environment(AppModel.self) private var model
    @Environment(\.colorScheme) private var scheme

    var body: some View {
        Group {
            switch model.phase {
            case .launching:
                ProgressView().accessibilityIdentifier("launching")
            case .signedOut:
                SignInView()
            case .signedIn:
                ListsView()
            }
        }
        .background(model.theme.background)
        .tint(model.theme.accent)
        .task { await model.start() }
        // The look is the shared theme (crates/view-model theme): the system's light or dark picks the palette.
        .onChange(of: scheme, initial: true) { _, now in model.follow(dark: now == .dark) }
    }
}
