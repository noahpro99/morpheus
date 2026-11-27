import SwiftUI
import UIKit

/// Pictures already fetched, by the core's key for them.
private let avatarCache = NSCache<NSString, UIImage>()

/// A person's or room's picture, or their first letter on the accent while it is not there. Decorative: the name is
/// always beside it, so it is hidden from accessibility.
struct Avatar: View {
    @Environment(AppModel.self) private var model
    let name: String
    let media: MediaHandle?
    var size: CGFloat = 32
    @State private var image: UIImage?

    var body: some View {
        ZStack {
            if let image {
                Image(uiImage: image).resizable().scaledToFill()
            } else {
                model.theme.accent
                Text(String(name.trimmingCharacters(in: .whitespaces).first ?? "?").uppercased())
                    .font(.footnote.bold())
                    .foregroundStyle(model.theme.onAccent)
            }
        }
        .frame(width: size, height: size)
        .clipShape(Circle())
        .accessibilityHidden(true)
        .task(id: media?.key()) { await load() }
    }

    private func load() async {
        guard let media else { return }
        let key = media.key() as NSString
        if let hit = avatarCache.object(forKey: key) {
            image = hit
            return
        }
        let pixels = UInt32(size * 3)
        if let data = await media.thumbnail(width: pixels, height: pixels), let picture = UIImage(data: data) {
            avatarCache.setObject(picture, forKey: key)
            image = picture
        }
    }
}
