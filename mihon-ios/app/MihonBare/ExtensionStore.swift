import Foundation

final class ExtensionStore: Sendable {
    static let shared = ExtensionStore()

    func download(ext: RepoExtension, repoURL: String) async throws -> URL {
        let dir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("apks", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let dest = dir.appendingPathComponent(ext.apk)
        if FileManager.default.fileExists(atPath: dest.path) {
            return dest
        }

        var candidates: [String] = []
        if let apkUrl = ext.apkUrl { candidates.append(apkUrl) }
        let base = repoURL
            .replacingOccurrences(of: "index.min.json", with: "")
            .replacingOccurrences(of: "index.json", with: "")
        let slash = base.hasSuffix("/") ? "" : "/"
        candidates.append(contentsOf: [
            "\(base)\(slash)apk/\(ext.apk)",
            "\(base)\(slash)\(ext.apk)",
        ])

        var last: Error = MihonError.message("no apk url worked")
        for urlString in candidates {
            guard let url = URL(string: urlString) else { continue }
            do {
                let (data, resp) = try await URLSession.shared.data(from: url)
                if let http = resp as? HTTPURLResponse, http.statusCode == 200, data.count > 1000 {
                    try data.write(to: dest, options: .atomic)
                    return dest
                }
            } catch {
                last = error
            }
        }
        throw last
    }
}
