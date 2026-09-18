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
        if #available(iOS 13.0, *) {
            config.tlsMinimumSupportedProtocolVersion = .TLSv12
        }
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
        // TachiManga NativeNet: NSURLSession for every request. WKWebView is
        // only the CF solver (McCookieJar copies cf_clearance, then retry).
        // JS fetch() is CORS-bound to the solved origin and breaks api/cdn.
        if isCloudflare(result) {
            let solved = CloudflareSolver.solveBlocking(urlString: req.url)
            if solved {
                CloudflareSolver.exportCookiesBlocking()
                result = fetch(req, url: url)
                if isCloudflare(result) {
                    Thread.sleep(forTimeInterval: 0.4)
                    CloudflareSolver.exportCookiesBlocking()
                    result = fetch(req, url: url)
                }
                if isCloudflare(result),
                   let web = CloudflareSolver.fetchBlocking(
                    method: req.method,
                    url: url,
                    headers: req.headers,
                    body: req.body
                   )
                {
                    result = EngineResp(
                        code: web.code,
                        message: web.message,
                        headers: web.headers,
                        bodyB64: web.bodyB64
                    )
                }
            }
            if isCloudflare(result) {
                result.code = 403
                result.message = "cloudflare challenge"
            }
        }
        return strdupJson(result)
    }

    private static func fetch(_ req: EngineReq, url: URL) -> EngineResp {
        var request = URLRequest(url: url)
        request.httpMethod = req.method
        for (k, v) in req.headers {
            request.setValue(v, forHTTPHeaderField: k)
        }
        request.setValue(MihonConfig.userAgent, forHTTPHeaderField: "User-Agent")
        applyCookies(&request, url: url)
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

    /// TachiManga `McCookieJar.directLoadForRequest`: merge every cookie
    /// whose domain matches the host (parent domains included). Last-wins
    /// per name so `cf_clearance` from WKWebView replaces a stale jar.
    private static func applyCookies(_ request: inout URLRequest, url: URL) {
        var parts: [(String, String)] = []
        let put = { (raw: String) in
            for item in raw.split(separator: ";") {
                let p = item.trimmingCharacters(in: .whitespaces)
                guard let eq = p.firstIndex(of: "=") else { continue }
                let name = String(p[..<eq])
                let value = String(p[p.index(after: eq)...])
                if let i = parts.firstIndex(where: { $0.0.caseInsensitiveCompare(name) == .orderedSame }) {
                    parts[i] = (name, value)
                } else {
                    parts.append((name, value))
                }
            }
        }
        if let existing = request.value(forHTTPHeaderField: "Cookie") {
            put(existing)
        }
        let host = url.host ?? ""
        let cookies = (HTTPCookieStorage.shared.cookies ?? []).filter {
            CloudflareSolver.domainMatches($0.domain, host: host)
        }
        if !cookies.isEmpty {
            put(cookies.map { "\($0.name)=\($0.value)" }.joined(separator: "; "))
        }
        if parts.isEmpty { return }
        request.setValue(
            parts.map { "\($0.0)=\($0.1)" }.joined(separator: "; "),
            forHTTPHeaderField: "Cookie"
        )
    }

    private static func isCloudflare(_ r: EngineResp) -> Bool {
        let headers = Dictionary(
            r.headers.map { ($0.0.lowercased(), $0.1) },
            uniquingKeysWith: { _, last in last }
        )
        if (headers["cf-mitigated"] ?? "").lowercased() == "challenge" { return true }
        let server = (headers["server"] ?? "").lowercased()
        let body = Data(base64Encoded: r.bodyB64).flatMap { String(data: $0.prefix(16000), encoding: .utf8) }?
            .lowercased() ?? ""
        let challenged = body.contains("just a moment")
            || body.contains("challenge-platform")
            || body.contains("cf-chl")
            || body.contains("_cf_chl")
            || body.contains("enable javascript and cookies to continue")
            || body.contains("checking your browser before accessing")
            || body.contains("cdn-cgi/challenge-platform")
            || body.contains("challenge-error-title")
            || body.contains("cf-browser-verification")
            || (body.contains("cf-turnstile") && body.contains("cdn-cgi"))
        if challenged { return true }
        if r.code == 403 || r.code == 503 || r.code == 429 {
            return server.contains("cloudflare") || server.contains("ddos-guard") || body.contains("cloudflare") || body.contains("ddos-guard")
        }
        return false
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
