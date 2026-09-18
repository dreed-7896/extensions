import SwiftUI

private enum CatalogTab: String, CaseIterable, Identifiable {
    case installed = "Installed"
    case all = "All"
    case keiyoushi = "Keiyoushi"
    case cursed = "Cursed"
    var id: String { rawValue }
}

struct RepoView: View {
    @ObservedObject private var store = ExtensionStore.shared
    @State private var tab: CatalogTab = .installed
    @State private var query = ""
    @State private var catalog: [RepoExtension] = []
    @State private var error: String?
    @State private var loading = false

    private var visible: [RepoExtension] {
        let q = query.lowercased()
        let base: [RepoExtension]
        switch tab {
        case .installed:
            return []
        case .all:
            base = catalog
        case .keiyoushi:
            base = catalog.filter { $0.repoID == "keiyoushi" }
        case .cursed:
            base = catalog.filter { $0.repoID == "cursed" }
        }
        if q.isEmpty { return base }
        return base.filter {
            $0.name.lowercased().contains(q) || $0.pkg.lowercased().contains(q)
        }
    }

    private var installedFiltered: [InstalledExt] {
        let q = query.lowercased()
        if q.isEmpty { return store.installed }
        return store.installed.filter {
            $0.name.lowercased().contains(q) || $0.pkg.lowercased().contains(q)
        }
    }

    var body: some View {
        List {
            Section {
                Picker("tab", selection: $tab) {
                    ForEach(CatalogTab.allCases) { t in
                        Text(t.rawValue).tag(t)
                    }
                }
                .pickerStyle(.segmented)
                TextField("search", text: $query)
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
            }
            if let error {
                Section { Text(error).foregroundStyle(.red) }
            }
            if tab == .installed {
                Section("Installed (\(store.installed.count))") {
                    if store.installed.isEmpty {
                        Text("Install an APK from Keiyoushi or Cursed. Same name in both repos = pick which one.")
                            .foregroundStyle(.secondary)
                    }
                    ForEach(installedFiltered) { ext in
                        NavigationLink {
                            SourceListView(apk: ext.apkURL, title: ext.name)
                        } label: {
                            VStack(alignment: .leading, spacing: 2) {
                                Text(ext.name).font(.headline)
                                Text("\(ext.lang) · \(ext.repoName) · \(ext.apk)")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }
                }
            } else {
                Section("\(tab.rawValue) (\(visible.count))") {
                    ForEach(visible) { ext in
                        NavigationLink {
                            InstallView(ext: ext)
                        } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(ext.name).font(.headline)
                                    Text("\(ext.lang ?? "") · \(ext.repoName) · \(ext.apk)")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                                Spacer()
                                if store.installed(ext) != nil {
                                    Text("Installed")
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                        }
                    }
                }
            }
        }
        .navigationTitle("MihonBare")
        .overlay { if loading { ProgressView() } }
        .task { await loadIndexes() }
        .refreshable { await loadIndexes() }
    }

    private func loadIndexes() async {
        loading = true
        error = nil
        defer { loading = false }
        var rows: [RepoExtension] = []
        var errors: [String] = []
        await withTaskGroup(of: (String, Result<[RepoExtension], Error>).self) { group in
            for repo in MihonConfig.repos {
                group.addTask {
                    do {
                        let data = try await GitHubFallback.fetch(repo.index.absoluteString)
                        return (repo.id, .success(RepoCatalog.parse(data).map { $0.tagged(repo: repo) }))
                    } catch {
                        return (repo.id, .failure(error))
                    }
                }
            }
            for await (id, result) in group {
                switch result {
                case let .success(list):
                    rows.append(contentsOf: list)
                case let .failure(err):
                    errors.append("\(id): \(err.localizedDescription)")
                }
            }
        }
        rows.sort {
            $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending
        }
        catalog = rows
        if rows.isEmpty {
            error = errors.joined(separator: "\n")
        } else if !errors.isEmpty {
            error = errors.joined(separator: "\n")
        }
    }
}

struct InstallView: View {
    let ext: RepoExtension
    @ObservedObject private var store = ExtensionStore.shared
    @State private var error: String?
    @State private var sources: [SourceInfo] = []
    @State private var apk: URL?
    @State private var loading = true

    var body: some View {
        Group {
            if loading {
                ProgressView("Installing \(ext.apk)")
            } else if let error {
                Text(error).padding()
            } else if let apk {
                List(sources) { src in
                    NavigationLink("\(src.name) (\(src.lang))") {
                        MangaListView(apk: apk, source: src)
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
            let row = try await store.download(ext: ext)
            try MihonEngine.shared.ensure(apk: row.apkURL)
            apk = row.apkURL
            sources = try MihonEngine.shared.sources()
        } catch {
            self.error = error.localizedDescription
        }
    }
}

struct SourceListView: View {
    let apk: URL
    let title: String
    @State private var sources: [SourceInfo] = []
    @State private var error: String?
    @State private var loading = false

    var body: some View {
        List {
            if let error { Text(error).foregroundStyle(.red) }
            ForEach(sources) { src in
                NavigationLink("\(src.name) (\(src.lang))") {
                    MangaListView(apk: apk, source: src)
                }
            }
        }
        .navigationTitle(title)
        .overlay { if loading { ProgressView() } }
        .task { await load() }
    }

    private func load() async {
        loading = true
        defer { loading = false }
        do {
            try MihonEngine.shared.ensure(apk: apk)
            sources = try MihonEngine.shared.sources()
        } catch {
            self.error = error.localizedDescription
        }
    }
}
