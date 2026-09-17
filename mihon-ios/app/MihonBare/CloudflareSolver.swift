import Foundation
import UIKit
import WebKit

enum CloudflareSolver {
    static func hasClearance(for host: String) -> Bool {
        let cookies = HTTPCookieStorage.shared.cookies ?? []
        return cookies.contains { cookie in
            guard cookie.name == "cf_clearance" else { return false }
            let domain = cookie.domain.trimmingCharacters(in: CharacterSet(charactersIn: "."))
            return host == domain || host.hasSuffix(".\(domain)") || domain.hasSuffix(host)
        }
    }

    /// Called from the engine thread. Presents a WKWebView on the main thread
    /// so Turnstile/JS challenges can be solved, then copies cookies out.
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

    @MainActor
    private static func solve(urlString: String) async -> Bool {
        guard let url = URL(string: urlString), let host = url.host else { return false }
        if hasClearance(for: host) { return true }

        let window = overlayWindow()
        let controller = ChallengeController(url: url)
        controller.heldWindow = window
        window.rootViewController = controller
        window.makeKeyAndVisible()

        let ok = await controller.waitForClearance(host: host)
        window.isHidden = true
        window.rootViewController = nil
        return ok
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
        webView.customUserAgent = MihonConfig.safariUA
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
        webView.load(URLRequest(url: target))
        startPolling()
        return await withCheckedContinuation { cont in
            continuation = cont
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
        copyCookiesThen { [weak self] ok in
            self?.complete(ok)
        }
    }

    private func copyCookiesThen(_ done: @escaping (Bool) -> Void) {
        let host = self.host
        WKWebsiteDataStore.default().httpCookieStore.getAllCookies { cookies in
            cookies.forEach { HTTPCookieStorage.shared.setCookie($0) }
            let hit = cookies.contains {
                $0.name == "cf_clearance" && ChallengeController.domainMatches($0.domain, host: host)
            }
            DispatchQueue.main.async {
                done(hit)
            }
        }
    }

    private static func domainMatches(_ cookieDomain: String, host: String) -> Bool {
        let domain = cookieDomain.trimmingCharacters(in: CharacterSet(charactersIn: "."))
        return host == domain || host.hasSuffix(".\(domain)") || domain.hasSuffix(host)
    }

    private func complete(_ ok: Bool) {
        guard let cont = continuation else { return }
        continuation = nil
        timer?.invalidate()
        timer = nil
        cont.resume(returning: ok)
    }
}
