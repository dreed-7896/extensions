import Foundation

/// Thin Swift wrapper around the dexvm C ABI.
/// All calls hop onto `queue` — the VM is not thread-safe.
final class MihonEngine: @unchecked Sendable {
    static let shared = MihonEngine()
    private let queue = DispatchQueue(label: "mihon.engine")
    private var handle: OpaquePointer?

    var isOpen: Bool { queue.sync { handle != nil } }

    func open(apk: URL) throws {
        try queue.sync {
            if let handle {
                mihon_close(handle)
                self.handle = nil
            }
            var err: UnsafeMutablePointer<CChar>?
            let opened = apk.path.withCString { path in
                mihon_open_file(path, &err)
            }
            if let opened {
                handle = opened
                return
            }
            throw MihonError.message(Self.take(err) ?? "failed to open apk")
        }
    }

    func close() {
        queue.sync {
            if let handle {
                mihon_close(handle)
                self.handle = nil
            }
        }
    }

    func sources() throws -> [SourceInfo] {
        try decode(SourcesPayload.self, op: "sources", args: [:]).sources
    }

    func popular(source: Int, page: Int) throws -> BrowsePayload {
        try decode(BrowsePayload.self, op: "popular", args: ["source": source, "page": page])
    }

    func latest(source: Int, page: Int) throws -> BrowsePayload {
        try decode(BrowsePayload.self, op: "latest", args: ["source": source, "page": page])
    }

    func search(source: Int, query: String, page: Int) throws -> BrowsePayload {
        try decode(
            BrowsePayload.self,
            op: "search",
            args: ["source": source, "page": page, "query": query]
        )
    }

    func chapters(source: Int, url: String, title: String) throws -> [ChapterEntry] {
        try decode(
            ChaptersPayload.self,
            op: "chapters",
            args: ["source": source, "url": url, "title": title]
        ).chapters
    }

    func pages(source: Int, url: String, name: String) throws -> [PageEntry] {
        try decode(
            PagesPayload.self,
            op: "pages",
            args: ["source": source, "url": url, "name": name]
        ).pages
    }

    func image(source: Int, url: String, pageUrl: String = "") throws -> Data {
        var args: [String: Any] = ["source": source, "url": url]
        if !pageUrl.isEmpty {
            args["pageUrl"] = pageUrl
        }
        let payload = try decode(ImagePayload.self, op: "image", args: args)
        guard let data = Data(base64Encoded: payload.bodyB64) else {
            throw MihonError.message("bad image payload")
        }
        return data
    }

    private func decode<T: Decodable>(_ type: T.Type, op: String, args: [String: Any]) throws -> T {
        try queue.sync {
            guard let handle else { throw MihonError.message("no apk loaded") }
            let argsData = try JSONSerialization.data(withJSONObject: args)
            let argsJson = String(data: argsData, encoding: .utf8) ?? "{}"
            var err: UnsafeMutablePointer<CChar>?
            let raw = op.withCString { opPtr in
                argsJson.withCString { argsPtr in
                    mihon_call(handle, opPtr, argsPtr, &err)
                }
            }
            guard let raw else {
                throw MihonError.message(Self.take(err) ?? "engine returned null")
            }
            defer { mihon_string_free(raw) }
            let json = String(cString: raw)
            guard let data = json.data(using: .utf8) else {
                throw MihonError.message("invalid utf8 from engine")
            }
            return try JSONDecoder().decode(T.self, from: data)
        }
    }

    private static func take(_ err: UnsafeMutablePointer<CChar>?) -> String? {
        guard let err else { return nil }
        let msg = String(cString: err)
        mihon_string_free(err)
        return msg
    }
}

enum MihonError: Error, LocalizedError {
    case message(String)
    var errorDescription: String? {
        switch self {
        case let .message(s): s
        }
    }
}
