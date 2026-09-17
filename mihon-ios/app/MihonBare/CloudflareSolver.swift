import Foundation
import UIKit
import WebKit

struct WebFetchResult {
    var code: Int
    var message: String
    var headers: [(String, String)]
    var bodyB64: String
}

enum CloudflareSolver {
    private static let lock = NSLock()
    private static var webViews: [String: WKWebView] = [:]
    private static var webHosts = Set<String>()

    static func hasClearance(for host: String) -> Bool {
        let cookies = HTTPCookieStorage.shared.cookies ?? []
        return cookies.contains { cookie in
            guard cookie.name == "cf_clearance" else { return false }
            return domainMatches(cookie.domain, host: host)
        }
    }

    static func usesWeb(_ host: String) -> Bool {
        lock.lock()
        defer { lock.unlock() }
        return webHosts.contains(host)
    }

    static func domainMatches(_ cookieDomain: String, host: String) -> Bool {
        let domain = cookieDomain.trimmingCharacters(in: CharacterSet(charactersIn: "."))
        return host == domain || host.hasSuffix(".\(domain)") || domain.hasSuffix(host)
    }

    /// Called from the engine thread. Presents a WKWebView on the main thread
    /// so Turnstile/JS challenges can be solved, then keeps that WebView for
    /// later fetches (same TLS stack as cf_clearance — URLSession 403s after).
    static func solveBlocking(urlString: String) -> Bool {
        let box = NSMutableArray()
        let sem = DispatchSemaphore(value: 0)
        DispatchQueue.main.async {
            Task { @MainActor in
                let ok = await solve(urlString: urlString)
                box.add(ok)
                sem.signal()
            }
        }
        _ = sem.wait(timeout: .now() + 180)
        return (box.firstObject as? Bool) ?? false
    }

    static func fetchBlocking(
        method: String,
        url: URL,
        headers: [(String, String)],
        body: String?
    ) -> WebFetchResult? {
        let box = FetchBox()
        let sem = DispatchSemaphore(value: 0)
        DispatchQueue.main.async {
            Task { @MainActor in
                box.result = await fetch(method: method, url: url, headers: headers, body: body)
                sem.signal()
            }
        }
        _ = sem.wait(timeout: .now() + 40)
        return box.result
    }

    @MainActor
    private static func solve(urlString: String) async -> Bool {
        guard let url = URL(string: urlString), let host = url.host else { return false }
        let window = overlayWindow()
        let controller = ChallengeController(url: url)
        controller.heldWindow = window
        window.rootViewController = controller
        window.makeKeyAndVisible()

        let ok = await controller.waitForClearance(host: host)
        if let webView = controller.takeWebView() {
            adopt(host: host, webView: webView)
        }
        window.isHidden = true
        window.rootViewController = nil
        return ok
    }

    @MainActor
    fileprivate static func adopt(host: String, webView: WKWebView) {
        webView.removeFromSuperview()
        webView.navigationDelegate = nil
        lock.lock()
        webViews[host] = webView
        webHosts.insert(host)
        lock.unlock()
    }

    @MainActor
    private static func view(for host: String) -> WKWebView {
        lock.lock()
        let existing = webViews[host]
        lock.unlock()
        if let existing { return existing }
        let config = WKWebViewConfiguration()
        config.websiteDataStore = .default()
        let webView = WKWebView(frame: CGRect(x: 0, y: 0, width: 1, height: 1), configuration: config)
        webView.customUserAgent = MihonConfig.userAgent
        lock.lock()
        webViews[host] = webView
        webHosts.insert(host)
        lock.unlock()
        return webView
    }

    @MainActor
    private static func fetch(
        method: String,
        url: URL,
        headers: [(String, String)],
        body: String?
    ) async -> WebFetchResult? {
        guard let host = url.host else { return nil }
        let webView = view(for: host)
        await seedCookies(into: webView)
        if webView.url?.host == nil {
            _ = await load(webView, URL(string: "https://\(host)/") ?? url)
        }
        var hdrs: [String: String] = [:]
        for (k, v) in headers {
            let key = k.lowercased()
            if key == "cookie" || key == "user-agent" || key == "host" { continue }
            hdrs[k] = v
        }
        let js = """
        const init = { method: method, credentials: 'include', redirect: 'follow', headers: headers || {} };
        if (body) { init.body = body; }
        const r = await fetch(url, init);
        const buf = await r.arrayBuffer();
        const bytes = new Uint8Array(buf);
        let bin = '';
        const chunk = 32768;
        for (let i = 0; i < bytes.length; i += chunk) {
            bin += String.fromCharCode.apply(null, bytes.subarray(i, i + chunk));
        }
        const out = [];
        r.headers.forEach((v, k) => out.push([k, v]));
        return { code: r.status, message: r.statusText || '', headers: out, bodyB64: btoa(bin) };
        """
        var args: [String: Any] = [
            "url": url.absoluteString,
            "method": method,
            "headers": hdrs,
        ]
        if let body { args["body"] = body }
        do {
            let raw = try await webView.callAsyncJavaScript(
                js,
                arguments: args,
                in: nil,
                in: .page
            )
            return parseJS(raw)
        } catch {
            return nil
        }
    }

    @MainActor
    private static func seedCookies(into webView: WKWebView) async {
        let cookies = HTTPCookieStorage.shared.cookies ?? []
        guard !cookies.isEmpty else { return }
        let store = webView.configuration.websiteDataStore.httpCookieStore
        for cookie in cookies {
            await withCheckedContinuation { (cont: CheckedContinuation<Void, Never>) in
                store.setCookie(cookie) { cont.resume() }
            }
        }
    }

    @MainActor
    private static func load(_ webView: WKWebView, _ url: URL) async -> Bool {
        var req = URLRequest(url: url)
        req.setValue(MihonConfig.userAgent, forHTTPHeaderField: "User-Agent")
        webView.load(req)
        try? await Task.sleep(nanoseconds: 1_200_000_000)
        return true
    }

    private static func parseJS(_ raw: Any?) -> WebFetchResult? {
        guard let dict = raw as? [String: Any] else { return nil }
        let code = (dict["code"] as? Int) ?? (dict["code"] as? Double).map { Int($0) } ?? 0
        let message = dict["message"] as? String ?? ""
        let bodyB64 = dict["bodyB64"] as? String ?? ""
        var headers: [(String, String)] = []
        if let rows = dict["headers"] as? [[Any]] {
            for row in rows {
                if row.count >= 2, let k = row[0] as? String, let v = row[1] as? String {
                    headers.append((k, v))
                }
            }
        }
        return WebFetchResult(code: code, message: message, headers: headers, bodyB64: bodyB64)
    }

    @MainActor
    private static func overlayWindow() -> UIWindow {
        let scene = UIApplication.shared.connectedScenes
            .compactMap { $0 as? UIWindowScene }
            .first { $0.activationState == .foregroundActive }
            ?? UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }.first
        let window: UIWindow
        if let scene {
            window = UIWindow(windowScene: scene)
        } else {
            window = UIWindow(frame: UIScreen.main.bounds)
        }
        window.windowLevel = .alert + 1
        window.backgroundColor = .systemBackground
        return window
    }
}

private final class FetchBox: @unchecked Sendable {
    var result: WebFetchResult?
}

@MainActor
private final class ChallengeController: UIViewController, WKNavigationDelegate {
    private let target: URL
    private var webView: WKWebView!
    private var continuation: CheckedContinuation<Bool, Never>?
    private var timer: Timer?
    private var host: String = ""
    var heldWindow: UIWindow?

    init(url: URL) {
        self.target = url
        super.init(nibName: nil, bundle: nil)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) { nil }

    func takeWebView() -> WKWebView? {
        let view = webView
        webView = nil
        return view
    }

    override func viewDidLoad() {
        super.viewDidLoad()
        view.backgroundColor = .systemBackground

        let label = UILabel()
        label.text = "Cloudflare challenge — complete it if asked"
        label.font = .preferredFont(forTextStyle: .headline)
        label.textAlignment = .center
        label.numberOfLines = 0
        label.translatesAutoresizingMaskIntoConstraints = false

        let config = WKWebViewConfiguration()
        config.websiteDataStore = .default()
        webView = WKWebView(frame: .zero, configuration: config)
        webView.customUserAgent = MihonConfig.userAgent
        webView.navigationDelegate = self
        webView.translatesAutoresizingMaskIntoConstraints = false

        let done = UIButton(type: .system)
        done.setTitle("Done", for: .normal)
        done.addTarget(self, action: #selector(finishTapped), for: .touchUpInside)
        done.translatesAutoresizingMaskIntoConstraints = false

        view.addSubview(label)
        view.addSubview(webView)
        view.addSubview(done)
        NSLayoutConstraint.activate([
            label.topAnchor.constraint(equalTo: view.safeAreaLayoutGuide.topAnchor, constant: 12),
            label.leadingAnchor.constraint(equalTo: view.leadingAnchor, constant: 16),
            label.trailingAnchor.constraint(equalTo: view.trailingAnchor, constant: -16),
            done.bottomAnchor.constraint(equalTo: view.safeAreaLayoutGuide.bottomAnchor, constant: -8),
            done.centerXAnchor.constraint(equalTo: view.centerXAnchor),
            webView.topAnchor.constraint(equalTo: label.bottomAnchor, constant: 8),
            webView.leadingAnchor.constraint(equalTo: view.leadingAnchor),
            webView.trailingAnchor.constraint(equalTo: view.trailingAnchor),
            webView.bottomAnchor.constraint(equalTo: done.topAnchor, constant: -8),
        ])
    }

    func waitForClearance(host: String) async -> Bool {
        self.host = host
        seedCookiesThenLoad()
        startPolling()
        return await withCheckedContinuation { cont in
            continuation = cont
        }
    }

    private func seedCookiesThenLoad() {
        let store = webView.configuration.websiteDataStore.httpCookieStore
        let cookies = HTTPCookieStorage.shared.cookies ?? []
        guard !cookies.isEmpty else {
            var req = URLRequest(url: target)
            req.setValue(MihonConfig.userAgent, forHTTPHeaderField: "User-Agent")
            webView.load(req)
            return
        }
        let group = DispatchGroup()
        for cookie in cookies {
            group.enter()
            store.setCookie(cookie) { group.leave() }
        }
        group.notify(queue: .main) { [weak self] in
            guard let self else { return }
            var req = URLRequest(url: self.target)
            req.setValue(MihonConfig.userAgent, forHTTPHeaderField: "User-Agent")
            self.webView.load(req)
        }
    }

    private func startPolling() {
        timer?.invalidate()
        timer = Timer.scheduledTimer(
            timeInterval: 0.6,
            target: self,
            selector: #selector(pollCookies),
            userInfo: nil,
            repeats: true
        )
    }

    @objc private func pollCookies() {
        copyCookiesThen { [weak self] ok in
            if ok { self?.complete(true) }
        }
    }

    @objc private func finishTapped() {
        copyCookiesThen { [weak self] _ in
            self?.complete(true)
        }
    }

    private func copyCookiesThen(_ done: @escaping (Bool) -> Void) {
        let host = self.host
        WKWebsiteDataStore.default().httpCookieStore.getAllCookies { cookies in
            HTTPCookieStorage.shared.setCookies(
                cookies,
                for: URL(string: "https://\(host)/"),
                mainDocumentURL: URL(string: "https://\(host)/")
            )
            cookies.forEach { HTTPCookieStorage.shared.setCookie($0) }
            let hit = cookies.contains {
                ($0.name == "cf_clearance" || $0.name == "__cf_bm")
                    && CloudflareSolver.domainMatches($0.domain, host: host)
            }
            DispatchQueue.main.async {
                done(hit)
            }
        }
    }

    private func complete(_ ok: Bool) {
        guard let cont = continuation else { return }
        continuation = nil
        timer?.invalidate()
        timer = nil
        cont.resume(returning: ok)
    }
}
