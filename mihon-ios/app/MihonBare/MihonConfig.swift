import Foundation

struct RepoSpec: Identifiable, Hashable {
    let id: String
    let name: String
    let index: URL
}

enum MihonConfig {
    static let repos: [RepoSpec] = [
        RepoSpec(
            id: "keiyoushi",
            name: "Keiyoushi",
            index: URL(string: "https://raw.githubusercontent.com/keiyoushi/extensions/repo/index.pb")!
        ),
        RepoSpec(
            id: "cursed",
            name: "Cursed",
            index: URL(string: "https://github.com/yuzono/cursed-manga-repo/raw/repo/index.pb")!
        ),
    ]

    /// Mihon default desktop Chrome UA. Used for both WKWebView CF solves and
    /// every URLSession request — clearance cookies are bound to this string,
    /// and mobile HTML breaks desktop CSS selectors in extensions.
    static let userAgent =
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36"
}
