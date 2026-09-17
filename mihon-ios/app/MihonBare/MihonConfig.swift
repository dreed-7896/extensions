import Foundation

enum MihonConfig {
    static let repoIndex = URL(
        string: "https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.pb"
    )!
    /// Mihon default desktop Chrome UA. Used for both WKWebView CF solves and
    /// every URLSession request — clearance cookies are bound to this string,
    /// and mobile HTML breaks desktop CSS selectors in extensions.
    static let userAgent =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
}
