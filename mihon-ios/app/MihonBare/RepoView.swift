import SwiftUI

struct RepoView: View {
    @State private var query = ""
    @State private var extensions: [RepoExtension] = []
    @State private var error: String?
    @State private var loading = false

    var filtered: [RepoExtension] {
        let q = query.lowercased()
        if q.isEmpty { return extensions }
        return extensions.filter {
            $0.name.lowercased().contains(q) || $0.pkg.lowercased().contains(q)
        }
    }

    var body: some View {
        List {
            Section("Keiyoushi") {
                TextField("search extensions", text: $query)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            }
            if let error {
                Section { Text(error).foregroundStyle(.red) }
            }
            Section("Extensions (\(extensions.count))") {
                ForEach(filtered) { ext in
                    NavigationLink {
                        InstallView(repoURL: MihonConfig.repoIndex.absoluteString, ext: ext)
                    } label: {
                        VStack(alignment: .leading, spacing: 2) {
                            Text(ext.name).font(.headline)
                            Text("\(ext.lang ?? "") · \(ext.apk)")
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                }
            }
        }
        .navigationTitle("MihonBare")
        .overlay { if loading { ProgressView() } }
        .task { await loadIndex() }
        .refreshable { await loadIndex() }
    }

    private func loadIndex() async {
        loading = true
        error = nil
        defer { loading = false }
        do {
            let (data, _) = try await URLSession.shared.data(from: MihonConfig.repoIndex)
            let parsed = RepoCatalog.parse(data)
            if parsed.isEmpty {
                error = "could not parse index"
            } else {
                extensions = parsed
            }
        } catch {
            self.error = error.localizedDescription
        }
    }
}

struct InstallView: View {
    let repoURL: String
    let ext: RepoExtension
    @State private var error: String?
    @State private var sources: [SourceInfo] = []
    @State private var loading = true

    var body: some View {
        Group {
            if loading {
                ProgressView("Loading \(ext.apk)")
            } else if let error {
                Text(error).padding()
            } else {
                List(sources) { src in
                    NavigationLink("\(src.name) (\(src.lang))") {
                        MangaListView(source: src)
                    }
                }
            }
        }
        .navigationTitle(ext.name)
        .task { await install() }
    }

    private func install() async {
        loading = true
        defer { loading = false }
        do {
            let apk = try await ExtensionStore.shared.download(ext: ext, repoURL: repoURL)
            try MihonEngine.shared.open(apk: apk)
            sources = try MihonEngine.shared.sources()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
