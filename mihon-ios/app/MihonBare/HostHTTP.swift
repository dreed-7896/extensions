import Foundation
import Darwin

enum HostHTTP {
    private static let session: URLSession = {
        let config = URLSessionConfiguration.default
        config.httpCookieStorage = .shared
        config.httpShouldSetCookies = true
        config.httpCookieAcceptPolicy = .always
        config.timeoutIntervalForRequest = 30
        config.requestCachePolicy = .reloadIgnoringLocalCacheData
        return URLSession(configuration: config)
    }()

    static func install() {
        mihon_set_http(mihonSwiftHttp)
    }

    static func handle(_ reqJson: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
        guard let reqJson else { return strdupJson(fail(0, "null request")) }
        let raw = String(cString: reqJson)
        guard let data = raw.data(using: .utf8),
              let req = try? JSONDecoder().decode(EngineReq.self, from: data),
              let url = URL(string: req.url)
        else {
            return strdupJson(fail(0, "bad request"))
        }

        var result = fetch(req, url: url)
        if isCloudflare(result), CloudflareSolver.solveBlocking(urlString: req.url) {
            result = fetch(req, url: url)
        }
        return strdupJson(result)
    }

    private static func fetch(_ req: EngineReq, url: URL) -> EngineResp {
        var request = URLRequest(url: url)
        request.httpMethod = req.method
        for (k, v) in req.headers {
            request.setValue(v, forHTTPHeaderField: k)
        }
        if let host = url.host, CloudflareSolver.hasClearance(for: host) {
            request.setValue(MihonConfig.safariUA, forHTTPHeaderField: "User-Agent")
        }
        if let body = req.body {
            request.httpBody = Data(body.utf8)
        }

        let sem = DispatchSemaphore(value: 0)
        var out = EngineResp(code: 0, message: "no response", headers: [], bodyB64: "")
        session.dataTask(with: request) { data, response, error in
            if let error {
                out = EngineResp(code: 0, message: error.localizedDescription, headers: [], bodyB64: "")
            } else if let http = response as? HTTPURLResponse {
                var headers: [String: String] = [:]
                for (k, v) in http.allHeaderFields {
                    if let ks = k as? String, let vs = v as? String { headers[ks] = vs }
                }
                out = EngineResp(
                    code: http.statusCode,
                    message: HTTPURLResponse.localizedString(forStatusCode: http.statusCode),
                    headers: headers.map { ($0.key, $0.value) },
                    bodyB64: data?.base64EncodedString() ?? ""
                )
            }
            sem.signal()
        }.resume()
        _ = sem.wait(timeout: .now() + 35)
        return out
    }

    private static func isCloudflare(_ r: EngineResp) -> Bool {
        guard r.code == 403 || r.code == 503 else { return false }
        let headers = Dictionary(uniqueKeysWithValues: r.headers.map { ($0.0.lowercased(), $0.1) })
        if (headers["cf-mitigated"] ?? "").lowercased() == "challenge" { return true }
        let server = (headers["server"] ?? "").lowercased()
        let body = Data(base64Encoded: r.bodyB64).flatMap { String(data: $0.prefix(8000), encoding: .utf8) }?
            .lowercased() ?? ""
        if server.contains("cloudflare") { return true }
        return body.contains("just a moment")
            || body.contains("challenge-platform")
            || body.contains("cf-chl")
            || body.contains("_cf_chl")
            || body.contains("enable javascript and cookies to continue")
    }

    private static func fail(_ code: Int, _ message: String) -> EngineResp {
        EngineResp(code: code, message: message, headers: [], bodyB64: "")
    }

    private static func strdupJson(_ resp: EngineResp) -> UnsafeMutablePointer<CChar>? {
        let rust: [String: Any] = [
            "code": resp.code,
            "message": resp.message,
            "headers": resp.headers.map { [$0.0, $0.1] },
            "bodyB64": resp.bodyB64,
        ]
        guard let data = try? JSONSerialization.data(withJSONObject: rust),
              let s = String(data: data, encoding: .utf8)
        else {
            return strdup(#"{"code":0,"message":"encode failed","headers":[],"bodyB64":""}"#)
        }
        return strdup(s)
    }
}

private struct EngineReq: Decodable {
    let method: String
    let url: String
    let headers: [(String, String)]
    let body: String?

    enum CodingKeys: String, CodingKey {
        case method, url, headers, body
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        method = try c.decode(String.self, forKey: .method)
        url = try c.decode(String.self, forKey: .url)
        body = try c.decodeIfPresent(String.self, forKey: .body)
        if let dict = try? c.decode([String: String].self, forKey: .headers) {
            headers = dict.map { ($0.key, $0.value) }
        } else if let pairs = try? c.decode([[String]].self, forKey: .headers) {
            headers = pairs.compactMap { p in
                guard p.count >= 2 else { return nil }
                return (p[0], p[1])
            }
        } else {
            headers = []
        }
    }
}

private struct EngineResp {
    var code: Int
    var message: String
    var headers: [(String, String)]
    var bodyB64: String
}

private let mihonSwiftHttp: @convention(c) (UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? = { req in
    HostHTTP.handle(req)
}
