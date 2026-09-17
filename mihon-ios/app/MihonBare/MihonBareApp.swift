import SwiftUI

@main
struct MihonBareApp: App {
    init() {
        HostHTTP.install()
        HostJS.install()
    }

    var body: some Scene {
        WindowGroup {
            NavigationStack {
                RepoView()
            }
        }
    }
}
