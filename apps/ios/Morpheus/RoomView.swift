import Observation
import SwiftUI

final class TimelineBridge: TimelineListener, @unchecked Sendable {
    let handler: @Sendable ([TimelineRow]) -> Void
    init(_ handler: @escaping @Sendable ([TimelineRow]) -> Void) { self.handler = handler }
    func onRows(rows: [TimelineRow]) { handler(rows) }
}

@Observable
@MainActor
final class RoomModel {
    var rows: [TimelineRow] = []
    var atStart = false
    var loadingOlder = false
    var failed: String?
    private var timeline: RoomTimeline?
    private var opening: Task<Void, Never>?
    nonisolated private let latest = Latest<[TimelineRow]>()

    func open(_ morpheus: Morpheus, roomId: String) async {
        let task = Task { await self.openNow(morpheus, roomId: roomId) }
        opening = task
        await task.value
    }

    private func openNow(_ morpheus: Morpheus, roomId: String) async {
        let bridge = TimelineBridge { [weak self] rows in
            self?.latest.push(rows) { self?.rows = $0 }
        }
        do {
            timeline = try await morpheus.openRoom(roomId: roomId, listener: bridge)
            try? await timeline?.markRead()
        } catch {
            failed = "\(error)"
        }
    }

    func loadOlder() async {
        guard let timeline, !loadingOlder, !atStart else { return }
        loadingOlder = true
        if let reached = try? await timeline.loadOlder() { atStart = reached }
        loadingOlder = false
    }

    func send(_ text: String) async {
        // A message sent while the room is still opening waits for it, never dropped.
        await opening?.value
        try? await timeline?.sendText(body: text)
    }
}

struct RoomView: View {
    @Environment(AppModel.self) private var model
    @State private var room = RoomModel()
    @State private var draft = ""
    @State private var sentCount = 0
    let roomId: String
    let title: String

    var body: some View {
        VStack(spacing: 0) {
            ScrollView {
                LazyVStack(alignment: .leading, spacing: 4) {
                    if !room.atStart {
                        Button { Task { await room.loadOlder() } } label: { Text("Load earlier messages").font(.footnote) }
                            .frame(maxWidth: .infinity, minHeight: 44)
                            .accessibilityIdentifier("load-older")
                            .onAppear { Task { await room.loadOlder() } }
                    }
                    ForEach(Array(room.rows.enumerated()), id: \.offset) { _, row in
                        rowView(row)
                    }
                }
                .padding(.horizontal)
            }
            .defaultScrollAnchor(.bottom)
            .scrollDismissesKeyboard(.interactively)
            .accessibilityIdentifier("timeline")
            composer
        }
        .background(model.theme.surface)
        .navigationTitle(title)
        .navigationBarTitleDisplayMode(.inline)
        .task { await room.open(model.morpheus, roomId: roomId) }
        .sensoryFeedback(.success, trigger: sentCount)
    }

    @ViewBuilder
    private func rowView(_ row: TimelineRow) -> some View {
        switch row {
        case let .message(message):
            MessageView(message: message)
        case let .date(label, _):
            Text(label).font(.caption).foregroundStyle(model.theme.muted)
                .frame(maxWidth: .infinity).padding(.vertical, 8)
        case .start:
            Text("This is the beginning of the room.").font(.footnote).foregroundStyle(model.theme.muted)
                .frame(maxWidth: .infinity).padding(.vertical, 8)
                .accessibilityIdentifier("room-start")
        case .new:
            Text("New").font(.caption.bold()).foregroundStyle(model.theme.danger)
        }
    }

    private var composer: some View {
        HStack(alignment: .bottom, spacing: 8) {
            TextField("Message", text: $draft, axis: .vertical)
                .lineLimit(1...5)
                .padding(10)
                .background(model.theme.raised, in: RoundedRectangle(cornerRadius: 18))
                .accessibilityIdentifier("composer")
            Button(action: send) {
                Image(systemName: "arrow.up.circle.fill").font(.largeTitle)
            }
            .frame(minWidth: 44, minHeight: 44)
            .disabled(draft.trimmingCharacters(in: .whitespaces).isEmpty)
            .accessibilityLabel("Send")
            .accessibilityIdentifier("send")
        }
        .padding(.horizontal)
        .padding(.vertical, 8)
        .background(model.theme.background)
    }

    private func send() {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        draft = ""
        sentCount += 1
        Task { await room.send(text) }
    }
}

struct MessageView: View {
    @Environment(AppModel.self) private var model
    let message: MessageItem

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            if message.showHeader {
                HStack(spacing: 8) {
                    Text(message.senderName).font(.subheadline.bold()).foregroundStyle(model.theme.accent)
                    Text(message.time).font(.caption).foregroundStyle(model.theme.muted)
                }
                .padding(.top, 8)
            }
            Text(text)
                .foregroundStyle(model.theme.text)
                // The instant local echo is greyed until the server has it (#397).
                .opacity(message.sending ? 0.5 : 1)
                .accessibilityValue(message.sending ? "sending" : "sent")
                .accessibilityIdentifier("message")
        }
    }

    private var text: String {
        switch message.body {
        case let .text(text, _): text
        case let .image(caption, _, _, _): caption
        case let .file(name, _, _, _): name
        case let .other(summary): summary
        }
    }
}
