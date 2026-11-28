import SwiftUI

/// Where the stack can go.
enum Route: Hashable {
    case room(id: String, name: String)
    case server(key: String)
}

private enum ListPage: String, CaseIterable, Identifiable {
    case dms = "Direct messages"
    case servers = "Servers"
    var id: String { rawValue }
}

/// The two lists, side by side: a swipe moves between them, there is no tab bar. Lists rest at the bottom, where the
/// thumb is, and the search field and the page switch sit under them.
struct ListsView: View {
    @Environment(AppModel.self) private var model
    @State private var page = ListPage.dms
    @State private var query = ""
    @State private var path: [Route] = []

    var body: some View {
        NavigationStack(path: $path) {
            VStack(spacing: 0) {
                // Two pages side by side, a horizontal swipe moves between them (a paging TabView hung the app
                // on the CI simulator while the sidebar updated under it).
                Group {
                    if page == .dms { dmList } else { serverList }
                }
                .frame(maxWidth: .infinity, maxHeight: .infinity)
                .contentShape(Rectangle())
                .simultaneousGesture(DragGesture(minimumDistance: 40).onEnded { drag in
                    let across = drag.translation.width
                    guard abs(across) > abs(drag.translation.height) * 2 else { return }
                    withAnimation { page = across < 0 ? .servers : .dms }
                })
                .accessibilityIdentifier("lists")
                VStack(spacing: 8) {
                    Picker("List", selection: $page) {
                        ForEach(ListPage.allCases) { Text($0.rawValue).tag($0) }
                    }
                    .pickerStyle(.segmented)
                    .accessibilityIdentifier("list-picker")
                    HStack {
                        Image(systemName: "magnifyingglass").foregroundStyle(model.theme.muted)
                            .accessibilityHidden(true)
                        TextField("Search", text: $query)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .accessibilityIdentifier("search")
                    }
                    .padding(10)
                    .background(model.theme.raised, in: RoundedRectangle(cornerRadius: 12))
                }
                .padding(.horizontal)
                .padding(.vertical, 8)
                .background(model.theme.background)
            }
            .navigationTitle(page.rawValue)
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Menu {
                        Button(role: .destructive) { Task { await model.signOut() } } label: {
                            Label("Sign out", systemImage: "rectangle.portrait.and.arrow.right")
                        }
                    } label: {
                        Image(systemName: "ellipsis.circle").accessibilityHidden(true)
                    }
                    .accessibilityLabel("Account")
                    .accessibilityIdentifier("account-menu")
                }
            }
            .navigationDestination(for: Route.self) { route in
                switch route {
                case let .room(id, name): RoomView(roomId: id, title: name)
                case let .server(key): ChannelsView(key: key)
                }
            }
        }
        .onChange(of: page) { _, _ in Haptics.tap() }
    }

    private func matches(_ name: String) -> Bool {
        query.isEmpty || name.localizedCaseInsensitiveContains(query)
    }

    private var dmList: some View {
        BottomList {
            ForEach(model.directMessages.filter { matches($0.name) }, id: \.id) { room in
                RoomRow(room: room) { path.append(.room(id: room.id, name: room.name)) }
            }
        } empty: {
            "No direct messages yet"
        }
    }

    private var serverList: some View {
        BottomList {
            ForEach(model.servers.filter { matches($0.title) }, id: \.key) { server in
                Button { path.append(.server(key: server.key)) } label: {
                    HStack {
                        Image(systemName: "square.grid.2x2.fill").foregroundStyle(model.theme.accent)
                            .accessibilityHidden(true)
                        Text(server.title).foregroundStyle(model.theme.text)
                        Spacer()
                        if server.hasUnread { Circle().fill(model.theme.accent).frame(width: 8, height: 8)
                            .accessibilityLabel("Unread") }
                    }
                    .frame(minHeight: 44)
                    .contentShape(Rectangle())
                }
                .accessibilityIdentifier("server-\(server.title)")
            }
        } empty: {
            "No servers yet"
        }
    }
}

/// A list anchored at the bottom.
struct BottomList<Content: View>: View {
    @Environment(AppModel.self) private var model
    @ViewBuilder let content: Content
    let empty: () -> String

    init(@ViewBuilder _ content: () -> Content, empty: @escaping () -> String) {
        self.content = content()
        self.empty = empty
    }

    var body: some View {
        // Rows rest at the bottom, where the thumb is: the content is at least as tall as the screen and starts with a spacer.
        GeometryReader { geometry in
            ScrollView {
                VStack(spacing: 0) {
                    Spacer(minLength: 0)
                    LazyVStack(alignment: .leading, spacing: 0) { content }
                        .padding(.horizontal)
                }
                .frame(minHeight: geometry.size.height)
            }
            .scrollBounceBehavior(.basedOnSize)
        }
        .background(model.theme.surface)
    }
}

struct RoomRow: View {
    let room: RoomItem
    let open: () -> Void

    var body: some View {
        Button(action: open) { RoomRowContent(room: room) }
            .accessibilityIdentifier("room-\(room.name)")
    }
}

struct RoomRowContent: View {
    @Environment(AppModel.self) private var model
    let room: RoomItem

    var body: some View {
            HStack {
                if room.isDm {
                    Avatar(name: room.name, media: room.avatar)
                } else {
                    Image(systemName: room.isVoice ? "speaker.wave.2.fill" : "number")
                        .foregroundStyle(model.theme.muted)
                        .accessibilityHidden(true)
                }
                Text(room.name)
                    .fontWeight(room.unread > 0 ? .semibold : .regular)
                    .foregroundStyle(room.muted ? model.theme.muted : model.theme.text)
                Spacer()
                if room.unread > 0 {
                    Text("\(room.unread)")
                        .font(.caption.bold())
                        .padding(.horizontal, 8).padding(.vertical, 2)
                        .foregroundStyle(model.theme.onDanger)
                        .background(model.theme.danger, in: Capsule())
                        .accessibilityLabel("\(room.unread) unread")
                }
            }
            .frame(minHeight: 44)
            .contentShape(Rectangle())
    }
}

/// A server's channels, under their category headings.
struct ChannelsView: View {
    @Environment(AppModel.self) private var model
    let key: String

    var body: some View {
        let scope = model.scope(key)
        BottomList {
            ForEach(Array((scope?.sections ?? []).enumerated()), id: \.offset) { _, section in
                if let title = section.title {
                    Text(title).font(.footnote.bold()).foregroundStyle(model.theme.muted).padding(.top, 12)
                }
                ForEach(section.rooms, id: \.id) { room in
                    NavigationLink(value: Route.room(id: room.id, name: room.name)) {
                        RoomRowContent(room: room)
                    }
                    .accessibilityIdentifier("room-\(room.name)")
                }
            }
        } empty: {
            "No channels"
        }
        .navigationTitle(scope?.title ?? "")
        .navigationBarTitleDisplayMode(.inline)
        .accessibilityIdentifier("channels")
    }
}
