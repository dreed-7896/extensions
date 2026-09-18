import Foundation
import zlib

/// TachiManga/Suwayomi `JsDelivrFallback`: GitHub raw often 403s, so retry
/// `raw.githubusercontent.com` / `github.com/.../raw/` via jsDelivr.
enum GitHubFallback {
    static func urls(_ original: String) -> [String] {
        var out = [original]
        if let alt = jsDelivr(original), alt != original {
            out.append(alt)
        }
        return out
    }

    static func fetch(_ urlString: String) async throws -> Data {
        var last: Error = MihonError.message("fetch failed")
        for candidate in urls(urlString) {
            guard let url = URL(string: candidate) else { continue }
            do {
                var req = URLRequest(url: url)
                req.setValue(MihonConfig.userAgent, forHTTPHeaderField: "User-Agent")
                if candidate.lowercased().contains(".apk") {
                    req.setValue(
                        "application/vnd.android.package-archive,*/*",
                        forHTTPHeaderField: "Accept"
                    )
                }
                let (data, resp) = try await URLSession.shared.data(for: req)
                let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
                if code == 200, data.count > 32 {
                    if candidate.lowercased().contains(".apk"),
                       !(data.count > 1000 && data[0] == 0x50 && data[1] == 0x4B)
                    {
                        last = MihonError.message("not an apk \(candidate)")
                        continue
                    }
                    return data
                }
                last = MihonError.message("HTTP \(code) \(candidate)")
            } catch {
                last = error
            }
        }
        throw last
    }

    private static func jsDelivr(_ url: String) -> String? {
        let rawPrefix = "https://raw.githubusercontent.com/"
        if url.hasPrefix(rawPrefix) {
            let parts = String(url.dropFirst(rawPrefix.count))
                .split(separator: "/", maxSplits: 3, omittingEmptySubsequences: false)
                .map(String.init)
            guard parts.count == 4 else { return nil }
            return "https://cdn.jsdelivr.net/gh/\(parts[0])/\(parts[1])@\(parts[2])/\(parts[3])"
        }
        let ghPrefix = "https://github.com/"
        if url.hasPrefix(ghPrefix) {
            let parts = String(url.dropFirst(ghPrefix.count))
                .split(separator: "/", maxSplits: 4, omittingEmptySubsequences: false)
                .map(String.init)
            guard parts.count == 5, parts[2] == "raw" else { return nil }
            return "https://cdn.jsdelivr.net/gh/\(parts[0])/\(parts[1])@\(parts[3])/\(parts[4])"
        }
        return nil
    }
}

enum RepoCatalog {
    static func parse(_ data: Data) -> [RepoExtension] {
        if let list = parseJSON(data), !list.isEmpty, !isStub(list) {
            return list
        }
        let payload = gunzipIfNeeded(data)
        if let list = parseJSON(payload), !list.isEmpty, !isStub(list) {
            return list
        }
        let proto = parseProtobuf(payload)
        if !proto.isEmpty { return proto }
        return parseJSON(data) ?? []
    }

    private static func isStub(_ list: [RepoExtension]) -> Bool {
        list.count <= 2 && list.contains { $0.pkg.contains("keiyoushi") && $0.name.lowercased().contains("outdated") }
    }

    private static func parseJSON(_ data: Data) -> [RepoExtension]? {
        if let modern = try? JSONDecoder().decode(NewKeiyoushiIndex.self, from: data),
           let list = modern.extensionList?.extensions, !list.isEmpty {
            return list
        }
        if let list = try? JSONDecoder().decode([RepoExtension].self, from: data) {
            return list
        }
        return nil
    }

    private static func gunzipIfNeeded(_ data: Data) -> Data {
        guard data.count > 2, data[0] == 0x1f, data[1] == 0x8b else { return data }
        return inflateGzip(data) ?? data
    }

    private static func inflateGzip(_ data: Data) -> Data? {
        data.withUnsafeBytes { raw -> Data? in
            guard let inPtr = raw.bindMemory(to: Bytef.self).baseAddress else { return nil }
            var stream = z_stream()
            stream.next_in = UnsafeMutablePointer(mutating: inPtr)
            stream.avail_in = uInt(raw.count)
            let initRc = inflateInit2_(
                &stream,
                16 + MAX_WBITS,
                ZLIB_VERSION,
                Int32(MemoryLayout<z_stream>.size)
            )
            guard initRc == Z_OK else { return nil }
            defer { inflateEnd(&stream) }

            let chunk = 64 * 1024
            var out = Data()
            var buf = [UInt8](repeating: 0, count: chunk)
            while true {
                let rc = buf.withUnsafeMutableBytes { dst -> Int32 in
                    stream.next_out = dst.bindMemory(to: Bytef.self).baseAddress
                    stream.avail_out = uInt(dst.count)
                    return inflate(&stream, Z_NO_FLUSH)
                }
                let produced = chunk - Int(stream.avail_out)
                if produced > 0 {
                    out.append(contentsOf: buf.prefix(produced))
                }
                if rc == Z_STREAM_END { return out }
                if rc != Z_OK { return nil }
            }
        }
    }

    private static func parseProtobuf(_ data: Data) -> [RepoExtension] {
        var cur = ProtoCursor(data: data)
        while let (num, wire, bytes, _) = cur.next() {
            if num == 101 && wire == 2 {
                return parseExtensionList(bytes)
            }
        }
        return parseExtensionList(data)
    }

    private static func parseExtensionList(_ data: Data) -> [RepoExtension] {
        var cur = ProtoCursor(data: data)
        var out: [RepoExtension] = []
        while let (num, wire, bytes, _) = cur.next() {
            if (num == 1 || num == 101) && wire == 2 {
                if let ext = parseExtension(bytes) {
                    out.append(ext)
                }
            }
        }
        return out
    }

    private static func parseExtension(_ data: Data) -> RepoExtension? {
        var cur = ProtoCursor(data: data)
        var name = ""
        var pkg = ""
        var apkUrl: String?
        var lang: String?
        while let (num, wire, bytes, varint) = cur.next() {
            switch (num, wire) {
            case (1, 2): name = string(bytes)
            case (2, 2): pkg = string(bytes)
            case (3, 2):
                if apkUrl == nil { apkUrl = parseResources(bytes) }
            case (6, 2): break
            case (8, 2):
                if lang == nil { lang = parseSourceLang(bytes) }
            default:
                _ = varint
            }
        }
        guard !pkg.isEmpty || !name.isEmpty else { return nil }
        let apk = apkUrl.flatMap { URL(string: $0)?.lastPathComponent } ?? "\(pkg).apk"
        return RepoExtension(
            name: name,
            pkg: pkg,
            apk: apk,
            apkUrl: apkUrl,
            lang: lang
        )
    }

    private static func parseResources(_ data: Data) -> String? {
        var cur = ProtoCursor(data: data)
        while let (num, wire, bytes, _) = cur.next() {
            if num == 1 && wire == 2 {
                let url = string(bytes)
                if url.hasSuffix(".apk") { return url }
            }
        }
        return nil
    }

    private static func parseSourceLang(_ data: Data) -> String? {
        var cur = ProtoCursor(data: data)
        while let (num, wire, bytes, _) = cur.next() {
            if num == 3 && wire == 2 {
                let s = string(bytes)
                if !s.isEmpty { return s }
            }
        }
        return nil
    }

    private static func string(_ data: Data) -> String {
        String(data: data, encoding: .utf8) ?? ""
    }
}

private struct ProtoCursor {
    let data: Data
    var i = 0

    init(data: Data) {
        self.data = data
        self.i = 0
    }

    mutating func next() -> (Int, Int, Data, UInt64)? {
        guard i < data.count else { return nil }
        let key = varint()
        let num = Int(key >> 3)
        let wire = Int(key & 7)
        switch wire {
        case 0:
            let v = varint()
            return (num, wire, Data(), v)
        case 1:
            guard i + 8 <= data.count else { return nil }
            i += 8
            return (num, wire, Data(), 0)
        case 5:
            guard i + 4 <= data.count else { return nil }
            i += 4
            return (num, wire, Data(), 0)
        case 2:
            let n = Int(varint())
            guard i + n <= data.count else { return nil }
            let slice = data.subdata(in: i..<(i + n))
            i += n
            return (num, wire, slice, 0)
        default:
            return nil
        }
    }

    mutating func varint() -> UInt64 {
        var r: UInt64 = 0
        var shift = 0
        while i < data.count {
            let b = data[i]
            i += 1
            r |= UInt64(b & 0x7f) << shift
            if b & 0x80 == 0 { break }
            shift += 7
            if shift > 63 { break }
        }
        return r
    }
}
