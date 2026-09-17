import Compression
import Foundation

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
        guard data.count > 18, data[0] == 0x1f, data[1] == 0x8b else { return data }
        guard let deflate = gzipPayload(data) else { return data }
        var cap = max(deflate.count * 8, 256 * 1024)
        for _ in 0..<4 {
            var out = Data(count: cap)
            let n = out.withUnsafeMutableBytes { dst -> Int in
                deflate.withUnsafeBytes { src -> Int in
                    guard let d = dst.bindMemory(to: UInt8.self).baseAddress,
                          let s = src.bindMemory(to: UInt8.self).baseAddress
                    else { return -1 }
                    return compression_decode_buffer(d, cap, s, deflate.count, nil, COMPRESSION_ZLIB)
                }
            }
            if n > 0 { return out.prefix(n) }
            cap *= 2
        }
        return data
    }

    private static func gzipPayload(_ data: Data) -> Data? {
        guard data.count > 18, data[2] == 8 else { return nil }
        var i = 10
        let flags = data[3]
        if flags & 4 != 0 {
            guard i + 2 <= data.count else { return nil }
            let xlen = Int(data[i]) | (Int(data[i + 1]) << 8)
            i += 2 + xlen
        }
        if flags & 8 != 0 {
            while i < data.count, data[i] != 0 { i += 1 }
            i += 1
        }
        if flags & 16 != 0 {
            while i < data.count, data[i] != 0 { i += 1 }
            i += 1
        }
        if flags & 2 != 0 { i += 2 }
        guard i + 8 < data.count else { return nil }
        return data.subdata(in: i..<(data.count - 8))
    }

    private static func parseProtobuf(_ data: Data) -> [RepoExtension] {
        var cur = ProtoCursor(data)
        while let (num, wire, bytes, _) = cur.next() {
            if num == 101 && wire == 2 {
                return parseExtensionList(bytes)
            }
        }
        return parseExtensionList(data)
    }

    private static func parseExtensionList(_ data: Data) -> [RepoExtension] {
        var cur = ProtoCursor(data)
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
        var cur = ProtoCursor(data)
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
        return RepoExtension(name: name, pkg: pkg, apk: apk, apkUrl: apkUrl, lang: lang)
    }

    private static func parseResources(_ data: Data) -> String? {
        var cur = ProtoCursor(data)
        while let (num, wire, bytes, _) = cur.next() {
            if num == 1 && wire == 2 {
                let url = string(bytes)
                if url.hasSuffix(".apk") { return url }
            }
        }
        return nil
    }

    private static func parseSourceLang(_ data: Data) -> String? {
        var cur = ProtoCursor(data)
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
