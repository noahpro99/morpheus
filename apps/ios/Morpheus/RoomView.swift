import Observation
import SwiftUI
import UIKit

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

    func reply(_ text: String, to eventId: String) async {
        await opening?.value
        try? await timeline?.sendReply(body: text, replyTo: eventId)
    }

    func edit(_ eventId: String, to text: String) async {
        try? await timeline?.editMessage(eventIdOf: eventId, body: text)
    }

    func delete(_ eventId: String) async {
        try? await timeline?.deleteMessage(eventIdOf: eventId)
    }

    func react(_ eventId: String, _ key: String) async {
        try? await timeline?.toggleReaction(eventIdOf: eventId, key: key)
    }
}

/// What the composer is doing: a new message, a reply to one, or a replacement of one of mine.
enum ComposerMode {
    case message
    case reply(MessageItem)
    case edit(MessageItem)
}

struct RoomView: View {
    @Environment(AppModel.self) private var model
    @State private var room = RoomModel()
    @State private var draft = ""
    @State private var mode = ComposerMode.message
    @State private var sentCount = 0
    @FocusState private var composing: Bool
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
            MessageView(message: message, actions: actions)
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

    private var actions: MessageActions {
        MessageActions(
            reply: { message in mode = .reply(message); composing = true },
            edit: { message in
                mode = .edit(message)
                draft = message.plainText
                composing = true
            },
            delete: { id in Task { await room.delete(id) } },
            react: { id, key in Task { await room.react(id, key) } }
        )
    }

    private var composer: some View {
        VStack(spacing: 0) {
            if let banner = modeBanner {
                HStack {
                    Text(banner).font(.footnote).foregroundStyle(model.theme.muted).lineLimit(1)
                        .accessibilityIdentifier("composer-mode")
                    Spacer()
                    Button { cancelMode() } label: { Image(systemName: "xmark.circle.fill").accessibilityHidden(true) }
                        .frame(minWidth: 44, minHeight: 44)
                        .accessibilityLabel("Cancel")
                        .accessibilityIdentifier("composer-cancel")
                }
                .padding(.horizontal)
            }
            HStack(alignment: .bottom, spacing: 8) {
                TextField("Message", text: $draft, axis: .vertical)
                    .lineLimit(1...5)
                    .focused($composing)
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
        }
        .background(model.theme.background)
    }

    private var modeBanner: String? {
        switch mode {
        case .message: nil
        case let .reply(message): "Replying to \(message.senderName)"
        case .edit: "Editing message"
        }
    }

    private func cancelMode() {
        if case .edit = mode { draft = "" }
        mode = .message
    }

    private func send() {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty else { return }
        draft = ""
        sentCount += 1
        let current = mode
        mode = .message
        Task {
            switch current {
            case .message: await room.send(text)
            case let .reply(message):
                if let id = message.eventId { await room.reply(text, to: id) } else { await room.send(text) }
            case let .edit(message):
                if let id = message.eventId { await room.edit(id, to: text) }
            }
        }
    }
}

/// What a long press on a message can do.
struct MessageActions {
    var reply: (MessageItem) -> Void
    var edit: (MessageItem) -> Void
    var delete: (String) -> Void
    var react: (String, String) -> Void
}

extension MessageItem {
    /// The text a message carries, for edit and copy.
    var plainText: String {
        switch body {
        case let .text(text, _): text
        case let .image(caption, _, _, _): caption
        case let .file(name, _, _, _): name
        case let .other(summary): summary
        }
    }
}

struct MessageView: View {
    @Environment(AppModel.self) private var model
    let message: MessageItem
    let actions: MessageActions

    /// The quick reactions of the long-press menu.
    private let quick = ["\u{1F44D}", "\u{2764}\u{FE0F}", "\u{1F602}", "\u{1F389}"]

    var body: some View {
        VStack(alignment: .leading, spacing: 2) {
            if message.showHeader {
                HStack(spacing: 8) {
                    Avatar(name: message.senderName, media: message.senderAvatar, size: 24)
                    Text(message.senderName).font(.subheadline.bold()).foregroundStyle(model.theme.accent)
                    Text(message.time).font(.caption).foregroundStyle(model.theme.muted)
                }
                .padding(.top, 8)
            }
            if let reply = message.replyTo {
                HStack(spacing: 6) {
                    Capsule().fill(model.theme.accent).frame(width: 3)
                    Text("\(reply.senderName ?? "") \(reply.summary ?? "")")
                        .font(.footnote).foregroundStyle(model.theme.muted).lineLimit(1)
                }
                .fixedSize(horizontal: false, vertical: true)
                .accessibilityIdentifier("reply-quote")
            }
            if case let .image(caption, media, width, height) = message.body {
                MediaImage(media: media, caption: caption, width: width, height: height)
            } else {
            Group {
                if message.edited {
                    Text("\(message.plainText) ") + Text("(edited)").font(.caption).foregroundStyle(model.theme.muted)
                } else {
                    Text(message.plainText)
                }
            }
            .foregroundStyle(model.theme.text)
            // The instant local echo is greyed until the server has it (#397).
            .opacity(message.sending ? 0.5 : 1)
            .accessibilityValue(message.sending ? "sending" : "sent")
            .accessibilityIdentifier("message")
            }
            if !message.reactions.isEmpty { reactions }
        }
        .contextMenu { menu }
    }

    private var reactions: some View {
        HStack(spacing: 6) {
            ForEach(message.reactions, id: \.key) { reaction in
                Button {
                    if let id = message.eventId { actions.react(id, reaction.key) }
                } label: {
                    Text("\(reaction.key) \(reaction.count)").font(.footnote)
                        .padding(.horizontal, 10).padding(.vertical, 4)
                        .background(reaction.mine ? model.theme.accent.opacity(0.3) : model.theme.raised, in: Capsule())
                        .foregroundStyle(model.theme.text)
                }
                .frame(minHeight: 44)
                .accessibilityIdentifier("reaction")
            }
        }
    }

    @ViewBuilder
    private var menu: some View {
        if let id = message.eventId {
            Button { actions.reply(message) } label: { Label("Reply", systemImage: "arrowshape.turn.up.left") }
            ForEach(quick, id: \.self) { key in
                Button(key) { actions.react(id, key) }
            }
            Button { UIPasteboard.general.string = message.plainText } label: { Label("Copy", systemImage: "doc.on.doc") }
            if message.canEdit {
                Button { actions.edit(message) } label: { Label("Edit", systemImage: "pencil") }
            }
            if message.canDelete {
                Button(role: .destructive) { actions.delete(id) } label: { Label("Delete", systemImage: "trash") }
            }
        }
    }
}
