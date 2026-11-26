import Foundation
import Observation
import UIKit

/// What the app knows: the session, the channel list. Everything else is Rust (crates/ffi).
@Observable
@MainActor
final class AppModel {
    enum Phase { case launching, signedOut, signedIn }

    var phase = Phase.launching
    var signingIn = false
    var error: String?
    var sidebar: Sidebar?
    var theme = Theme()

    let morpheus: Morpheus
    let defaultHomeserver: String
    private var sidebarTask: TaskHandle?
    private var syncTask: TaskHandle?
    private var started = false

    init() {
        let env = ProcessInfo.processInfo.environment
        defaultHomeserver = env["MORPHEUS_HOMESERVER"] ?? "matrix.org"
        let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("morpheus", isDirectory: true)
        // Tests start from nothing (`-reset`).
        if CommandLine.arguments.contains("-reset") {
            try? FileManager.default.removeItem(at: base)
        }
        try? FileManager.default.createDirectory(at: base, withIntermediateDirectories: true)
        guard let core = try? Morpheus(dataDir: base.path) else {
            fatalError("the core did not start")
        }
        morpheus = core
    }

    func start() async {
        guard !started else { return }
        started = true
        if CommandLine.arguments.contains("-reset") {
            phase = .signedOut
            return
        }
        if let account = try? await morpheus.restoreSession() {
            signedIn(account)
        } else {
            phase = .signedOut
        }
    }

    func signIn(homeserver: String, username: String, password: String) async {
        signingIn = true
        error = nil
        do {
            signedIn(try await morpheus.login(homeserver: homeserver, username: username, password: password))
        } catch {
            self.error = "\(error)"
        }
        signingIn = false
    }

    func signOut() async {
        sidebarTask?.cancel()
        try? await morpheus.logout()
        sidebar = nil
        phase = .signedOut
    }

    /// Light or dark follows the system; the palette is the core's.
    func follow(dark: Bool) {
        if let look = try? morpheus.setTheme(key: dark ? "dark" : "light") {
            theme = Theme(look.palette)
        }
    }

    private func signedIn(_: Account) {
        phase = .signedIn
        Task {
            let bridge = SidebarBridge { [weak self] sidebar in
                Task { @MainActor in self?.sidebar = sidebar }
            }
            sidebarTask = try? await morpheus.watchSidebar(listener: bridge)
        }
    }

    // The scope lists the screens show.
    var directMessages: [RoomItem] {
        sidebar?.scopes.filter { $0.kind == .dms }.flatMap { $0.sections.flatMap(\.rooms) } ?? []
    }

    var servers: [ScopeItem] {
        sidebar?.scopes.filter { $0.kind == .server } ?? []
    }

    func scope(_ key: String) -> ScopeItem? {
        sidebar?.scopes.first { $0.key == key }
    }
}

final class SidebarBridge: SidebarListener, @unchecked Sendable {
    let handler: @Sendable (Sidebar) -> Void
    init(_ handler: @escaping @Sendable (Sidebar) -> Void) { self.handler = handler }
    func onSidebar(sidebar: Sidebar) { handler(sidebar) }
}

enum Haptics {
    static func tap() { UIImpactFeedbackGenerator(style: .light).impactOccurred() }
    static func sent() { UINotificationFeedbackGenerator().notificationOccurred(.success) }
}
