import Foundation

struct RepoExtension: Identifiable, Decodable {
    var id: String { pkg }
    let name: String
    let pkg: String
    let apk: String
    let apkUrl: String?
    let lang: String?

    init(name: String, pkg: String, apk: String, apkUrl: String?, lang: String?) {
        self.name = name
        self.pkg = pkg
        self.apk = apk
        self.apkUrl = apkUrl
        self.lang = lang
    }

    private enum CodingKeys: String, CodingKey {
        case name, pkg, apk, lang, packageName, resources, sources
    }

    private struct Resources: Decodable {
        let apkUrl: String?
    }

    private struct Src: Decodable {
        let language: String?
        let lang: String?
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        name = try c.decode(String.self, forKey: .name)
        pkg = try c.decodeIfPresent(String.self, forKey: .pkg)
            ?? c.decode(String.self, forKey: .packageName)
        let resources = try c.decodeIfPresent(Resources.self, forKey: .resources)
        apkUrl = resources?.apkUrl
        if let apk = try c.decodeIfPresent(String.self, forKey: .apk) {
            self.apk = apk
        } else if let url = apkUrl, let last = URL(string: url)?.lastPathComponent {
            self.apk = last
        } else {
            self.apk = pkg + ".apk"
        }
        if let lang = try c.decodeIfPresent(String.self, forKey: .lang) {
            self.lang = lang
        } else {
            let srcs = try c.decodeIfPresent([Src].self, forKey: .sources)
            self.lang = srcs?.first?.language ?? srcs?.first?.lang
        }
    }
}

struct SourceInfo: Identifiable, Decodable {
    var id: Int { index }
    let index: Int
    let name: String
    let lang: String
    let supports_latest: Bool
    let base_url: String?

    private enum CodingKeys: String, CodingKey {
        case index, name, lang, supports_latest, base_url
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        index = try c.decode(Int.self, forKey: .index)
        name = try c.decode(String.self, forKey: .name)
        lang = try c.decode(String.self, forKey: .lang)
        supports_latest = try c.decode(Bool.self, forKey: .supports_latest)
        base_url = try c.decodeIfPresent(String.self, forKey: .base_url)
    }
}

struct MangaEntry: Identifiable, Decodable, Hashable {
    var id: String { url }
    let title: String
    let url: String
    let thumbnail_url: String
    let author: String
    let artist: String
    let description: String
    let genre: String
    let status: Int
}

struct ChapterEntry: Identifiable, Decodable, Hashable {
    var id: String { url }
    let name: String
    let url: String
    let date_upload: Int64
    let scanlator: String
}

struct PageEntry: Identifiable, Decodable {
    var id: Int { index }
    let index: Int
    let url: String
    let image_url: String
}

struct SourcesPayload: Decodable { let sources: [SourceInfo] }
struct BrowsePayload: Decodable {
    let entries: [MangaEntry]
    let hasNext: Bool
}
struct ChaptersPayload: Decodable { let chapters: [ChapterEntry] }
struct PagesPayload: Decodable { let pages: [PageEntry] }
struct ImagePayload: Decodable { let bodyB64: String }

struct NewKeiyoushiIndex: Decodable {
    struct List: Decodable { let extensions: [RepoExtension] }
    let extensionList: List?
}
