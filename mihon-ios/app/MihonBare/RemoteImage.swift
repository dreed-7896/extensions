import SwiftUI
import UIKit

struct RemoteImage: View {
    let url: String
    @State private var image: UIImage?

    var body: some View {
        Group {
            if let image {
                Image(uiImage: image).resizable().scaledToFit()
            } else {
                Color.gray.opacity(0.15)
            }
        }
        .task(id: url) { await load() }
    }

    private func load() async {
        guard let loc = URL(string: url) else { return }
        var req = URLRequest(url: loc)
        if let host = loc.host, CloudflareSolver.hasClearance(for: host) {
            req.setValue(MihonConfig.safariUA, forHTTPHeaderField: "User-Agent")
        }
        if let (data, _) = try? await URLSession.shared.data(for: req),
           let img = UIImage(data: data) {
            image = img
        }
    }
}
