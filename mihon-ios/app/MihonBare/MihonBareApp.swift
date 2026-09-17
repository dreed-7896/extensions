import SwiftUI

@main
struct MihonBareApp: App {
    init() {
        HostHTTP.install()
    }

    var body: some Scene {
        WindowGroup {
            NavigationStack {
                RepoView()
            }
        }
    }
}
