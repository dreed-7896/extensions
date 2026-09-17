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

struct MangaListView: View {
    let source: SourceInfo
    @State private var items: [MangaEntry] = []
    @State private var query = ""
    @State private var error: String?
    @State private var loading = false

    var body: some View {
        List {
            if let error { Text(error).foregroundStyle(.red) }
            if !loading && error == nil && items.isEmpty {
                Text("no titles from this source").foregroundStyle(.secondary)
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
                            Text(manga.title).lineLimit(2)
                            Text(manga.author).font(.caption).foregroundStyle(.secondary)
                        }
                    }
                }
            }
        }
        .searchable(text: $query)
        .onSubmit(of: .search) { Task { await search() } }
        .navigationTitle(source.name)
        .overlay { if loading { ProgressView() } }
        .task { await loadPopular() }
        .refreshable { await loadPopular() }
    }

    private func loadPopular() async {
        loading = true
        error = nil
        defer { loading = false }
        do {
            items = try MihonEngine.shared.popular(source: source.index, page: 1).entries
        } catch {
            self.error = error.localizedDescription
        }
    }

    private func search() async {
        loading = true
        error = nil
        defer { loading = false }
        do {
            items = try MihonEngine.shared.search(source: source.index, query: query, page: 1).entries
        } catch {
            self.error = error.localizedDescription
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
            ForEach(chapters) { ch in
                NavigationLink(ch.name.isEmpty ? ch.url : ch.name) {
                    ReaderView(source: source, chapter: ch)
                }
            }
        }
        .navigationTitle(manga.title)
        .overlay { if loading { ProgressView() } }
        .task { await load() }
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
