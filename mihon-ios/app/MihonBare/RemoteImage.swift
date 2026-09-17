import SwiftUI
import UIKit

struct RemoteImage: View {
    let apk: URL
    let source: Int
    let url: String
    var pageUrl: String = ""
    @State private var image: UIImage?

    var body: some View {
        Group {
            if let image {
                Image(uiImage: image).resizable().scaledToFit()
            } else {
                Color.gray.opacity(0.15)
            }
        }
        .task(id: "\(source)|\(pageUrl)|\(url)") { await load() }
    }

    private func load() async {
        if url.isEmpty && pageUrl.isEmpty { return }
        let cacheKey = pageUrl.isEmpty ? url : "\(pageUrl)|\(url)"
        if let cached = ImageCache.get(cacheKey) {
            image = cached
            return
        }
        let src = source
        let loc = url
        let page = pageUrl
        let apkURL = apk
        let data: Data? = await Task.detached {
            _ = try? MihonEngine.shared.ensure(apk: apkURL)
            if let bytes = try? MihonEngine.shared.image(source: src, url: loc, pageUrl: page),
               !bytes.isEmpty
            {
                return bytes
            }
            guard let u = URL(string: loc), u.scheme == "http" || u.scheme == "https" else {
                return nil
            }
            var req = URLRequest(url: u)
            req.setValue(MihonConfig.userAgent, forHTTPHeaderField: "User-Agent")
            return try? await URLSession.shared.data(for: req).0
        }.value
        guard let data, let img = UIImage(data: data) else { return }
        ImageCache.set(cacheKey, img)
        image = img
    }
}

private enum ImageCache {
    private static let cache = NSCache<NSString, UIImage>()
    static func get(_ key: String) -> UIImage? { cache.object(forKey: key as NSString) }
    static func set(_ key: String, _ image: UIImage) { cache.setObject(image, forKey: key as NSString) }
}
