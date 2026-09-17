import Combine
import Foundation

struct InstalledExt: Codable, Identifiable, Hashable {
    var id: String { "\(repoID)|\(pkg)" }
    let repoID: String
    let repoName: String
    let repoIndex: String
    let name: String
    let pkg: String
    let apk: String
    let lang: String
    let apkPath: String

    var apkURL: URL { URL(fileURLWithPath: apkPath) }
}

final class ExtensionStore: ObservableObject {
    static let shared = ExtensionStore()
    private let defaultsKey = "mihon.installed.v1"
    @Published private(set) var installed: [InstalledExt] = []

    private init() {
        load()
    }

    func installed(_ ext: RepoExtension) -> InstalledExt? {
        installed.first { $0.id == ext.id }
    }

    func download(ext: RepoExtension) async throws -> InstalledExt {
        if let existing = installed(ext),
           FileManager.default.fileExists(atPath: existing.apkPath)
        {
            return existing
        }
        let dir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
            .appendingPathComponent("apks", isDirectory: true)
        try FileManager.default.createDirectory(at: dir, withIntermediateDirectories: true)
        let dest = dir.appendingPathComponent("\(ext.repoID)-\(ext.apk)")
        if !FileManager.default.fileExists(atPath: dest.path) {
            try await fetchApk(ext: ext, dest: dest)
        }
        let row = InstalledExt(
            repoID: ext.repoID,
            repoName: ext.repoName,
            repoIndex: ext.repoIndex,
            name: ext.name,
            pkg: ext.pkg,
            apk: ext.apk,
            lang: ext.lang ?? "",
            apkPath: dest.path
        )
        await MainActor.run {
            installed.removeAll { $0.id == row.id }
            installed.append(row)
            installed.sort { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
            persist()
        }
        return row
    }

    private func fetchApk(ext: RepoExtension, dest: URL) async throws {
        var candidates: [String] = []
        if let apkUrl = ext.apkUrl { candidates.append(apkUrl) }
        let base = ext.repoIndex
            .replacingOccurrences(of: "index.min.json", with: "")
            .replacingOccurrences(of: "index.json", with: "")
            .replacingOccurrences(of: "index.pb", with: "")
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
                    return
                }
            } catch {
                last = error
            }
        }
        throw last
    }

    private func load() {
        guard let data = UserDefaults.standard.data(forKey: defaultsKey),
              let rows = try? JSONDecoder().decode([InstalledExt].self, from: data)
        else { return }
        installed = rows.filter { FileManager.default.fileExists(atPath: $0.apkPath) }
    }

    private func persist() {
        if let data = try? JSONEncoder().encode(installed) {
            UserDefaults.standard.set(data, forKey: defaultsKey)
        }
    }
}
