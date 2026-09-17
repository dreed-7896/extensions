import SwiftUI

struct SourceBrowseView: View {
    let apkName: String
    let pkg: String
    @State private var sources: [SourceInfo] = []
    @State private var error: String?
    @State private var loading = false

    var body: some View {
        List {
            if let error { Text(error).foregroundStyle(.red) }
            ForEach(sources) { src in
                NavigationLink(src.name + " (\(src.lang))") {
                    MangaListView(source: src)
                }
            }
        }
        .navigationTitle(apkName)
        .overlay { if loading { ProgressView() } }
        .task { await load() }
    }

    private func load() async {
        loading = true
        defer { loading = false }
        do {
            sources = try MihonEngine.shared.sources()
        } catch {
            self.error = error.localizedDescription
        }
    }
}

private enum BrowseTab: String, CaseIterable, Identifiable {
    case popular = "Popular"
    case latest = "Latest"
    case search = "Search"
    var id: String { rawValue }
}

struct MangaListView: View {
    let source: SourceInfo
    @State private var tab: BrowseTab = .popular
    @State private var items: [MangaEntry] = []
    @State private var query = ""
    @State private var page = 1
    @State private var hasNext = false
    @State private var error: String?
    @State private var loading = false
    @State private var loadingMore = false

    private var tabs: [BrowseTab] {
        source.supports_latest
            ? BrowseTab.allCases
            : BrowseTab.allCases.filter { $0 != .latest }
    }

    var body: some View {
        List {
            if let error { Text(error).foregroundStyle(.red) }
            if !loading && error == nil && items.isEmpty {
                Text(emptyCaption).foregroundStyle(.secondary)
            }
            ForEach(items) { manga in
                NavigationLink {
                    ChapterListView(source: source, manga: manga)
                } label: {
                    HStack {
                        RemoteImage(url: manga.thumbnail_url)
                            .frame(width: 48, height: 64)
                            .clipped()
                        VStack(alignment: .leading) {
                            Text(manga.title.isEmpty ? manga.url : manga.title).lineLimit(2)
                            Text(manga.author).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
            }
            if hasNext {
                Button {
                    Task { await loadMore() }
                } label: {
                    if loadingMore {
                        ProgressView()
                    } else {
                        Text("Load more")
                    }
                }
            }
        }
        .navigationTitle(source.name)
        .safeAreaInset(edge: .top) {
            VStack(spacing: 8) {
                Picker("browse", selection: $tab) {
                    ForEach(tabs) { t in
                        Text(t.rawValue).tag(t)
                    }
                }
                .pickerStyle(.segmented)
                if tab == .search {
                    HStack {
                        TextField("search this source", text: $query)
                            .textInputAutocapitalization(.never)
                            .autocorrectionDisabled()
                            .onSubmit { Task { await reload() } }
                        Button("Go") { Task { await reload() } }
                    }
                }
            }
            .padding(.horizontal)
            .padding(.vertical, 8)
            .background(.bar)
        }
        .overlay { if loading { ProgressView() } }
        .task { await reload() }
        .refreshable { await reload() }
        .onChange(of: tab) { _ in
            Task { await reload() }
        }
    }

    private var emptyCaption: String {
        if tab == .search && query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return "type a query"
        }
        return "no titles from this source"
    }

    private func reload() async {
        page = 1
        hasNext = false
        items = []
        error = nil
        if tab == .search && query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return
        }
        await fetch(reset: true)
    }

    private func loadMore() async {
        guard hasNext, !loading, !loadingMore else { return }
        await fetch(reset: false)
    }

    private func fetch(reset: Bool) async {
        if reset { loading = true } else { loadingMore = true }
        defer {
            if reset { loading = false } else { loadingMore = false }
        }
        do {
            let pageToLoad = reset ? 1 : page + 1
            let result: BrowsePayload
            switch tab {
            case .popular:
                result = try MihonEngine.shared.popular(source: source.index, page: pageToLoad)
            case .latest:
                result = try MihonEngine.shared.latest(source: source.index, page: pageToLoad)
            case .search:
                result = try MihonEngine.shared.search(
                    source: source.index,
                    query: query,
                    page: pageToLoad
                )
            }
            if reset {
                items = result.entries
            } else {
                let seen = Set(items.map(\.url))
                items.append(contentsOf: result.entries.filter { !seen.contains($0.url) })
            }
            page = pageToLoad
            hasNext = result.hasNext
            error = nil
        } catch {
            self.error = error.localizedDescription
            if reset { items = [] }
        }
    }
}

struct ChapterListView: View {
    let source: SourceInfo
    let manga: MangaEntry
    @State private var chapters: [ChapterEntry] = []
    @State private var error: String?
    @State private var loading = false

    var body: some View {
        List {
            if let error { Text(error).foregroundStyle(.red) }
            if !loading && error == nil && chapters.isEmpty {
                Text("no chapters from this source").foregroundStyle(.secondary)
            }
            ForEach(chapters) { ch in
                NavigationLink(ch.name.isEmpty ? ch.url : ch.name) {
                    ReaderView(source: source, chapter: ch)
                }
            }
        }
        .navigationTitle(manga.title.isEmpty ? manga.url : manga.title)
        .overlay { if loading { ProgressView() } }
        .task { await load() }
        .refreshable { await load() }
    }

    private func load() async {
        loading = true
        defer { loading = false }
        do {
            chapters = try MihonEngine.shared.chapters(
                source: source.index,
                url: manga.url,
                title: manga.title
            )
            error = nil
        } catch {
            self.error = error.localizedDescription
        }
    }
}

struct ReaderView: View {
    let source: SourceInfo
    let chapter: ChapterEntry
    @State private var pages: [PageEntry] = []
    @State private var error: String?
    @State private var loading = false

    var body: some View {
        Group {
            if let error {
                Text(error).padding()
            } else if !loading && pages.isEmpty {
                Text("no pages").padding().foregroundStyle(.secondary)
            } else {
                ScrollView {
                    LazyVStack(spacing: 0) {
                        ForEach(pages) { page in
                            let raw = page.image_url.isEmpty ? page.url : page.image_url
                            RemoteImage(url: raw)
                        }
                    }
                }
            }
        }
        .navigationTitle(chapter.name)
        .navigationBarTitleDisplayMode(.inline)
        .overlay { if loading { ProgressView() } }
        .task { await load() }
    }

    private func load() async {
        loading = true
        defer { loading = false }
        do {
            pages = try MihonEngine.shared.pages(
                source: source.index,
                url: chapter.url,
                name: chapter.name
            )
        } catch {
            self.error = error.localizedDescription
        }
    }
}
