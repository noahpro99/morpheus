import SwiftUI
import UIKit

private let imageCache = NSCache<NSString, UIImage>()

/// A picture message: the thumbnail from the core at the width of the screen, with the shape kept while it loads so the
/// timeline does not jump. The caption is its accessibility label.
struct MediaImage: View {
    @Environment(AppModel.self) private var model
    let media: MediaHandle
    let caption: String
    let width: UInt32?
    let height: UInt32?
    @State private var image: UIImage?

    private var ratio: CGFloat {
        guard let width, let height, width > 0, height > 0 else { return 4 / 3 }
        return CGFloat(width) / CGFloat(height)
    }

    var body: some View {
        ZStack {
            if let image {
                Image(uiImage: image).resizable().scaledToFit()
            } else {
                model.theme.raised
                ProgressView()
            }
        }
        .aspectRatio(ratio, contentMode: .fit)
        .frame(maxWidth: 320)
        .clipShape(RoundedRectangle(cornerRadius: 12))
        .accessibilityElement(children: .ignore)
        .accessibilityLabel(caption)
        .accessibilityAddTraits(.isImage)
        .accessibilityIdentifier("image-message")
        .task(id: media.key()) { await load() }
    }

    private func load() async {
        let key = media.key() as NSString
        if let hit = imageCache.object(forKey: key) {
            image = hit
            return
        }
        if let data = await media.thumbnail(width: 960, height: 960), let picture = UIImage(data: data) {
            imageCache.setObject(picture, forKey: key)
            image = picture
        }
    }
}
