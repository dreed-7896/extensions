package eu.kanade.tachiyomi.extension.all.r2merge

import android.text.InputType
import android.util.Base64
import androidx.preference.EditTextPreference
import androidx.preference.ListPreference
import androidx.preference.PreferenceScreen
import androidx.preference.SwitchPreferenceCompat
import eu.kanade.tachiyomi.extension.all.r2merge.meta.SeriesMetadata
import eu.kanade.tachiyomi.extension.all.r2merge.net.ArchiveCache
import eu.kanade.tachiyomi.extension.all.r2merge.net.ArchiveInterceptor
import eu.kanade.tachiyomi.extension.all.r2merge.net.CoverImageInterceptor
import eu.kanade.tachiyomi.extension.all.r2merge.net.R2Config
import eu.kanade.tachiyomi.extension.all.r2merge.net.S3Client
import eu.kanade.tachiyomi.extension.all.r2merge.net.S3Listing
import eu.kanade.tachiyomi.extension.all.r2merge.net.S3Object
import eu.kanade.tachiyomi.extension.all.r2merge.net.SigV4Interceptor
import eu.kanade.tachiyomi.extension.all.r2merge.net.isAbsoluteUrl
import eu.kanade.tachiyomi.extension.all.r2merge.util.CoverPageRef
import eu.kanade.tachiyomi.extension.all.r2merge.util.NaturalOrder
import eu.kanade.tachiyomi.extension.all.r2merge.util.chapterNumberOf
import eu.kanade.tachiyomi.extension.all.r2merge.util.fileName
import eu.kanade.tachiyomi.extension.all.r2merge.util.findChapterByName
import eu.kanade.tachiyomi.extension.all.r2merge.util.isArchiveKey
import eu.kanade.tachiyomi.extension.all.r2merge.util.isHiddenKey
import eu.kanade.tachiyomi.extension.all.r2merge.util.isImageKey
import eu.kanade.tachiyomi.extension.all.r2merge.util.isUnsupportedArchiveKey
import eu.kanade.tachiyomi.extension.all.r2merge.util.parseCoverPageRef
import eu.kanade.tachiyomi.extension.all.r2merge.util.pickCoverPage
import eu.kanade.tachiyomi.network.GET
import eu.kanade.tachiyomi.network.asObservableSuccess
import eu.kanade.tachiyomi.source.ConfigurableSource
import eu.kanade.tachiyomi.source.model.Filter
import eu.kanade.tachiyomi.source.model.FilterList
import eu.kanade.tachiyomi.source.model.MangasPage
import eu.kanade.tachiyomi.source.model.Page
import eu.kanade.tachiyomi.source.model.SChapter
import eu.kanade.tachiyomi.source.model.SManga
import eu.kanade.tachiyomi.source.model.UpdateStrategy
import eu.kanade.tachiyomi.source.online.HttpSource
import keiyoushi.annotation.Source
import keiyoushi.utils.getPreferencesLazy
import keiyoushi.zip.dataRange
import keiyoushi.zip.range
import keiyoushi.zip.readEntry
import keiyoushi.zip.zipDirectory
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json
import okhttp3.Headers
import okhttp3.HttpUrl.Companion.toHttpUrlOrNull
import okhttp3.Interceptor
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Request
import okhttp3.Response
import okhttp3.ResponseBody.Companion.asResponseBody
import okio.buffer
import rx.Observable
import java.io.IOException
import java.lang.String.CASE_INSENSITIVE_ORDER
import java.util.concurrent.Callable
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors

@Source
class R2Merge(
    override val name: String,
    override val lang: String,
    override val baseUrl: String,
    override val id: Long,
) : HttpSource(),
    ConfigurableSource {

    override val supportsLatest = true

    private val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
    }
    private val preferences by getPreferencesLazy()
    private val hitomiGg = HitomiGg()
    private var cachedNhServer: String? = null

    private fun config(): R2Config? {
        val accountId = preferences.getString(PREF_ACCOUNT, "")!!.trim()
        val endpointRaw = preferences.getString(PREF_ENDPOINT, "")!!.trim().ifBlank { accountId }
        val endpoint = R2Config.parseEndpoint(endpointRaw) ?: return null
        val accessKeyId = preferences.getString(PREF_ACCESS_KEY, "")!!.trim()
        val secret = preferences.getString(PREF_SECRET, "")!!.trim()
        val bucket = preferences.getString(PREF_BUCKET, "")!!.trim()
        if (accessKeyId.isEmpty() || secret.isEmpty() || bucket.isEmpty()) return null
        return R2Config(
            endpoint = endpoint,
            bucket = bucket,
            accessKeyId = accessKeyId,
            secretAccessKey = secret,
            region = preferences.getString(PREF_REGION, DEFAULT_REGION)!!.ifBlank { DEFAULT_REGION },
            rootPrefix = R2Config.normalizePrefix(preferences.getString(PREF_PREFIX, "")!!),
            publicBaseUrl = R2Config.parsePublicBase(preferences.getString(PREF_PUBLIC_BASE, "")!!),
        )
    }

    private fun requireConfig(): R2Config = config() ?: throw IOException(SETUP_MESSAGE)

    private val signer = SigV4Interceptor(::config)

    private val signedClient: OkHttpClient by lazy {
        network.client.newBuilder().addInterceptor(signer).build()
    }

    private val archives: ArchiveCache by lazy { ArchiveCache(signedClient, ::config) }

    override val client: OkHttpClient by lazy {
        network.client.newBuilder()
            .addInterceptor(::chaikaZipInterceptor)
            .addInterceptor(ArchiveInterceptor(archives))
            .addInterceptor(CoverImageInterceptor)
            .addInterceptor(::imageRefererInterceptor)
            .addInterceptor(::ehentaiBackupInterceptor)
            .addInterceptor(::ehentaiCookieInterceptor)
            .addNetworkInterceptor(::ehentaiCookieInterceptor)
            .addInterceptor(signer)
            .apply {
                // Source interceptors (fake hosts, Referer) must run before CF.
                interceptors().apply {
                    val cloudflare = firstOrNull { it.javaClass.simpleName == "CloudflareInterceptor" }
                    if (cloudflare != null) {
                        remove(cloudflare)
                        add(cloudflare)
                    }
                }
            }
            .build()
    }

    private val s3: S3Client by lazy { S3Client(signedClient) }

    private val coverPool by lazy {
        Executors.newFixedThreadPool(COVER_THREADS) { runnable ->
            Thread(runnable, "r2library-cover").apply { isDaemon = true }
        }
    }

    private class Cached<T>(val value: T, val storedAt: Long)

    @Volatile
    private var seriesCache: Cached<List<Series>>? = null

    @Volatile
    private var recentCache: Cached<List<Series>>? = null

    private val shallowCache = ListingCache(64)
    private val treeCache = ListingCache(8)
    private val coverCache = ConcurrentHashMap<String, String>()
    private val titleCache = ConcurrentHashMap<String, String>()
    private val seriesChapterCache = ConcurrentHashMap<String, List<ParsedChapter>>()
    private val remoteMetaCache = ConcurrentHashMap<String, SeriesMetadata>()

    private fun invalidateCaches() {
        seriesCache = null
        recentCache = null
        shallowCache.clear()
        treeCache.clear()
        coverCache.clear()
        titleCache.clear()
        seriesChapterCache.clear()
        remoteMetaCache.clear()
        archives.clear()
    }

    private fun <T> Cached<T>?.freshValue(): T? {
        val cached = this ?: return null
        val ttl = cacheTtlMs
        if (ttl > 0 && System.currentTimeMillis() - cached.storedAt > ttl) return null
        return cached.value
    }

    private val cacheTtlMs: Long
        get() = preferences.getString(PREF_CACHE_TTL, null)?.toLongOrNull()?.times(1000L)
            ?: DEFAULT_CACHE_TTL_SECONDS * 1000L

    private data class Series(val name: String, val prefix: String)

    override fun fetchPopularManga(page: Int): Observable<MangasPage> = Observable.fromCallable { browse(page, "", FilterList()) }

    override fun fetchSearchManga(page: Int, query: String, filters: FilterList): Observable<MangasPage> = Observable.fromCallable { browse(page, query, filters) }

    override fun fetchLatestUpdates(page: Int): Observable<MangasPage> = Observable.fromCallable { paginate(recentSeries(requireConfig()), page) }

    private fun browse(page: Int, query: String, filters: FilterList): MangasPage {
        val config = requireConfig()
        var series = allSeries(config)
        if (query.isNotBlank()) {
            val needle = query.trim()
            series = series.filter {
                it.name.contains(needle, ignoreCase = true) ||
                    (titleCache[it.prefix] ?: it.name).contains(needle, ignoreCase = true)
            }
        }
        val ascending = filters.filterIsInstance<SortFilter>()
            .firstOrNull()?.state?.ascending ?: true
        if (!ascending) series = series.asReversed()
        return paginate(series, page)
    }

    private fun paginate(series: List<Series>, page: Int): MangasPage {
        val from = (page - 1) * PAGE_SIZE
        val slice = series.drop(from).take(PAGE_SIZE)
        return MangasPage(withCovers(requireConfig(), slice), series.size > from + slice.size)
    }

    private fun allSeries(config: R2Config): List<Series> {
        seriesCache.freshValue()?.let { return it }
        var listing = s3.listAll(config, config.rootPrefix, DELIMITER)
        var root = config.rootPrefix
        if (listing.prefixes.isEmpty() && root.isNotEmpty()) {
            listing = s3.listAll(config, "", DELIMITER)
            root = ""
        }
        val series = listing.prefixes
            .map { Series(name = it.removePrefix(root).trimEnd('/'), prefix = it) }
            .filter { it.name.isNotEmpty() && !it.name.startsWith(".") && !it.name.equals("upload", true) }
            .sortedWith(compareBy(NaturalOrder) { it.name })
        if (series.isEmpty()) throw IOException(emptyLibraryMessage(config, listing))
        seriesCache = Cached(series, System.currentTimeMillis())
        return series
    }

    private fun recentSeries(config: R2Config): List<Series> {
        recentCache.freshValue()?.let { return it }
        val maxPages = preferences.getString(PREF_LATEST_PAGES, null)?.toIntOrNull()
            ?.coerceIn(1, 100) ?: DEFAULT_LATEST_PAGES
        val listing = s3.listAll(config, config.rootPrefix, delimiter = null, maxPages = maxPages)
        val newest = HashMap<String, Long>()
        for (obj in listing.objects) {
            if (isHiddenKey(obj.key)) continue
            val name = obj.key.removePrefix(config.rootPrefix).substringBefore('/', "")
            if (name.isEmpty() || name.equals("upload", true)) continue
            val current = newest[name]
            if (current == null || obj.lastModified > current) newest[name] = obj.lastModified
        }
        val series = newest.entries
            .sortedByDescending { it.value }
            .map { Series(it.key, "${config.rootPrefix}${it.key}/") }
        if (series.isEmpty()) throw IOException(emptyLibraryMessage(config, listing))
        recentCache = Cached(series, System.currentTimeMillis())
        return series
    }

    private data class SeriesCard(val title: String, val coverUrl: String?)

    private fun withCovers(config: R2Config, series: List<Series>): List<SManga> {
        if (series.isEmpty()) return emptyList()
        val pending = series.map { entry ->
            coverPool.submit(
                Callable {
                    runCatching {
                        val listing = shallowListing(config, entry.prefix)
                        val metadata = readMetadata(config, listing)
                        val title = titleCache[entry.prefix] ?: (metadata?.title?.trim()?.takeIf { it.isNotEmpty() } ?: entry.name)
                            .also { titleCache[entry.prefix] = it }
                        SeriesCard(title, coverUrl(config, entry.prefix, listing, metadata))
                    }.getOrNull()
                },
            )
        }
        return series.mapIndexed { index, entry ->
            val card = runCatching { pending[index].get() }.getOrNull()
            SManga.create().apply {
                url = entry.name
                title = card?.title ?: entry.name
                thumbnail_url = card?.coverUrl
                update_strategy = UpdateStrategy.ALWAYS_UPDATE
            }
        }
    }

    private fun seriesPrefix(config: R2Config, mangaUrl: String): String {
        val id = mangaUrl.trim('/')
        return if (id.contains('/')) {
            if (id.endsWith("/")) id else "$id/"
        } else {
            "${config.rootPrefix}$id/"
        }
    }

    override fun fetchMangaDetails(manga: SManga): Observable<SManga> = Observable.fromCallable {
        val config = requireConfig()
        val prefix = seriesPrefix(config, manga.url)
        val listing = shallowListing(config, prefix)
        val metadata = resolvedMetadata(config, prefix, listing)
        SManga.create().apply {
            url = manga.url
            title = metadata?.title?.trim()?.takeIf { it.isNotEmpty() } ?: prefix.fileName()
            author = metadata?.author
            artist = metadata?.artist
            description = metadata?.description
            genre = metadata?.genre
            status = metadata?.status ?: SManga.UNKNOWN
            thumbnail_url = runCatching { coverUrl(config, prefix, listing, metadata) }.getOrNull()
            update_strategy = UpdateStrategy.ALWAYS_UPDATE
            initialized = true
        }.also { details ->
            details.title.trim().takeIf { it.isNotEmpty() }?.let { titleCache[prefix] = it }
        }
    }

    private fun readMetadata(config: R2Config, listing: S3Listing): SeriesMetadata? {
        val byName = listing.objects.associateBy { it.key.fileName().lowercase() }
        byName[SeriesMetadata.DETAILS_JSON]?.let { obj ->
            fetchText(config, obj)?.let { text ->
                SeriesMetadata.fromDetailsJson(text)?.let { return it }
            }
        }
        byName[SeriesMetadata.COMIC_INFO_XML]?.let { obj ->
            fetchText(config, obj)?.let { text ->
                SeriesMetadata.fromComicInfo(text)?.let { return it }
            }
        }
        return null
    }

    private fun resolvedMetadata(
        config: R2Config,
        seriesPrefix: String,
        listing: S3Listing,
    ): SeriesMetadata? {
        val local = readMetadata(config, listing)
        val seriesUrl = firstRemoteSeriesUrl(jsonListedChapters(config, listing, seriesPrefix).chapters)
            ?: return local
        val remote = remoteMetaCache.getOrPut(normalizeSeriesCacheKey(seriesUrl)) {
            runCatching { fetchRemoteSeriesMetadata(seriesUrl) }.getOrNull() ?: SeriesMetadata()
        }.takeIf { it.title != null || it.cover != null || it.author != null || it.description != null }
        return when {
            local != null -> local.overlay(remote)
            else -> remote
        }
    }

    private fun fetchRemoteSeriesMetadata(url: String): SeriesMetadata? = when {
        isMadaraSeriesUrl(url) -> fetchMadaraSeriesMetadata(client, headers, url)
        isMangaDexSeriesUrl(url) -> fetchMangaDexMetadata(client, headers, url, json)
        else -> null
    }

    private fun fetchText(config: R2Config, obj: S3Object): String? {
        if (obj.size > MAX_METADATA_BYTES) return null
        return runCatching {
            signedClient.newCall(GET(config.objectUrl(obj.key), headers)).execute().use { response ->
                if (response.isSuccessful) response.body.string() else null
            }
        }.getOrNull()
    }

    private fun coverUrl(
        config: R2Config,
        seriesPrefix: String,
        listing: S3Listing,
        metadata: SeriesMetadata? = null,
    ): String? {
        val coverSpec = metadata?.cover?.trim()?.takeIf { it.isNotEmpty() }
        val cacheKey = coverCacheKey(seriesPrefix, coverSpec)
        coverCache[cacheKey]?.let { return it }
        val fromDetails = coverSpec?.let { cover ->
            runCatching { resolveDetailsCover(config, seriesPrefix, listing, cover) }.getOrNull()
        }
        if (fromDetails != null) {
            val published = CoverImageInterceptor.wrapIfNeeded(fromDetails)
            coverCache[cacheKey] = published
            return published
        }
        val directImages = listing.objects.filter { isImageKey(it.key) && it.key.isChildOf(seriesPrefix) }
        val url = directImages.firstOrNull { it.key.isCoverFile() }?.let { config.imageUrl(it.key).toString() }
            ?: directImages.minWithOrNull(compareBy(NaturalOrder) { it.key })
                ?.let { config.imageUrl(it.key).toString() }
            ?: firstPageUrl(config, listing)
        // Don't pin a fallback while a cover ref is set — retry until the chapter resolves.
        if (url != null && coverSpec == null) coverCache[cacheKey] = url
        return url
    }

    private fun coverCacheKey(seriesPrefix: String, coverSpec: String?): String = "$seriesPrefix\u0000${coverSpec.orEmpty()}"

    /**
     * `details.json` cover: absolute URL, `Chapter 1_1` (name + 1-based index),
     * `chapter 4_24.png` (name + page file), or a relative image key.
     */
    private fun resolveDetailsCover(
        config: R2Config,
        seriesPrefix: String,
        listing: S3Listing,
        cover: String,
    ): String? {
        if (cover.startsWith("data:", ignoreCase = true) || isAbsoluteHttpUrl(cover)) return cover
        parseCoverPageRef(cover)?.let { ref ->
            resolveCoverPageRef(config, seriesPrefix, listing, ref)?.let { return it }
        }
        val key = if (cover.startsWith(seriesPrefix)) {
            cover
        } else {
            "${seriesPrefix.trimEnd('/')}/${cover.trimStart('/')}"
        }
        return if (isImageKey(key)) config.imageUrl(key).toString() else null
    }

    private fun resolveCoverPageRef(
        config: R2Config,
        seriesPrefix: String,
        listing: S3Listing,
        ref: CoverPageRef,
    ): String? {
        val folder = findChapterByName(listing.prefixes, ref.chapter) { it }
        if (folder != null) {
            val images = s3.listAll(config, folder, delimiter = null)
                .objects
                .filter { isImageKey(it.key) }
            pickCoverPage(images, ref.page) { it.key }?.let { page ->
                return config.imageUrl(page.key).toString()
            }
        }
        val archiveObjects = listing.objects.filter {
            isArchiveKey(it.key) && it.key.isChildOf(seriesPrefix)
        }
        val archive = findChapterByName(archiveObjects, ref.chapter) { it.key }
        if (archive != null) {
            val entries = archives.zipFor(archive.key).entries()
                .filter { isImageKey(it.name) && !isHiddenKey(it.name) }
            pickCoverPage(entries, ref.page) { it.name }?.let { entry ->
                return ArchiveInterceptor.pageUrl(archive.key, entry.name)
            }
        }
        return chaptersJsonCoverPage(config, seriesPrefix, listing, ref)
    }

    private fun jsonListedChapters(
        config: R2Config,
        listing: S3Listing,
        seriesPrefix: String,
    ): JsonChapterListing {
        val direct = listing.objects.filter { it.key.isChildOf(seriesPrefix) }
        val chapters = mutableListOf<ParsedChapter>()
        direct.firstOrNull { it.key.fileName().equals("details.json", true) }?.let { obj ->
            fetchText(config, obj)?.let { body ->
                chapters += parseChaptersJson(body, json, seriesPrefix)
            }
        }
        direct.firstOrNull { isChaptersJson(it.key) }?.let { obj ->
            fetchText(config, obj)?.let { body ->
                chapters += parseChaptersJson(body, json, seriesPrefix)
            }
        }
        return JsonChapterListing(chapters = chapters)
    }

    /** Expand each source in list order, then concatenate. */
    private fun composeJsonChapters(listed: JsonChapterListing): List<ParsedChapter> = concatenateSources(listed.chapters, ::fetchSeriesChapters).first

    private fun fetchSeriesChapters(url: String): List<ParsedChapter> {
        val key = normalizeSeriesCacheKey(url)
        return seriesChapterCache.getOrPut(key) {
            when {
                isMadaraSeriesUrl(url) -> fetchMadaraChapters(client, headers, url)
                isMangaDexSeriesUrl(url) -> fetchMangaDexChapters(client, headers, url, json)
                else -> emptyList()
            }
        }
    }

    private fun normalizeSeriesCacheKey(url: String): String = url.trim().substringBefore('#').substringBefore('?').trimEnd('/') + "/"

    private fun chaptersJsonCoverPage(
        config: R2Config,
        seriesPrefix: String,
        listing: S3Listing,
        ref: CoverPageRef,
    ): String? {
        val chapters = composeJsonChapters(jsonListedChapters(config, listing, seriesPrefix))
        val chapter = findChapterByName(chapters, ref.chapter) { it.title } ?: return null
        val pages = pagesForCover(chapter.readerUrl())
        val imageUrl = pickCoverPage(pages, ref.page, preserveOrder = true) { it.imageUrl.orEmpty() }
            ?.imageUrl
            ?: return null
        return CoverImageInterceptor.wrapIfNeeded(imageUrl)
    }

    private fun isChaptersJson(key: String): Boolean {
        val name = key.fileName().lowercase()
        return name == "chapters.json" || name == "chapter-list.json" || name == "chapter_list.json"
    }

    private fun pagesForCover(url: String): List<Page> {
        val chapter = SChapter.create().apply { this.url = url }
        return fetchPageList(chapter).toBlocking().first()
    }

    private fun firstPageUrl(config: R2Config, listing: S3Listing): String? {
        listing.prefixes.minWithOrNull(NaturalOrder)?.let { firstFolder ->
            val page = s3.listAll(config, firstFolder, delimiter = null, maxPages = 1)
                .objects
                .filter { isImageKey(it.key) }
                .minWithOrNull(compareBy(NaturalOrder) { it.key })
            if (page != null) return config.imageUrl(page.key).toString()
        }
        val archive = listing.objects
            .filter { isArchiveKey(it.key) }
            .minWithOrNull(compareBy(NaturalOrder) { it.key })
            ?: return null
        val entry = archives.zipFor(archive.key).entries()
            .filter { isImageKey(it.name) }
            .minWithOrNull(compareBy(NaturalOrder) { it.name })
            ?: return null
        return ArchiveInterceptor.pageUrl(archive.key, entry.name)
    }

    override fun fetchChapterList(manga: SManga): Observable<List<SChapter>> = Observable.fromCallable {
        val config = requireConfig()
        val prefix = seriesPrefix(config, manga.url)
        val tree = treeListing(config, prefix)
        val seriesName = prefix.fileName()
        val folderDates = LinkedHashMap<String, Long>()
        val archiveObjects = mutableListOf<S3Object>()
        val looseImages = mutableListOf<S3Object>()
        var unsupported = 0
        var chaptersJson: S3Object? = null
        var detailsJson: S3Object? = null

        for (obj in tree.objects) {
            if (isHiddenKey(obj.key)) continue
            val name = obj.key.fileName()
            when {
                name.equals("details.json", true) && obj.key.isChildOf(prefix) -> detailsJson = obj
                isChaptersJson(obj.key) && obj.key.isChildOf(prefix) -> chaptersJson = obj
                isArchiveKey(obj.key) -> archiveObjects += obj
                isUnsupportedArchiveKey(obj.key) -> unsupported++
                !isImageKey(obj.key) -> Unit
                obj.key.isChildOf(prefix) -> looseImages += obj
                else -> {
                    val folder = "${obj.key.substringBeforeLast('/')}/"
                    val previous = folderDates[folder]
                    if (previous == null || obj.lastModified > previous) {
                        folderDates[folder] = obj.lastModified
                    }
                }
            }
        }

        val chapters = mutableListOf<ParsedChapter>()
        folderDates.forEach { (folder, date) ->
            val label = folder.removePrefix(prefix).trimEnd('/').replace("/", " – ")
            chapters += ParsedChapter(
                title = label,
                number = chapterNumberOf(label, seriesName).takeIf { it >= 0f } ?: 0f,
                url = folder,
                dateUpload = date,
            )
        }
        archiveObjects.forEach { obj ->
            val label = obj.key.removePrefix(prefix).substringBeforeLast('.').replace("/", " – ")
            chapters += ParsedChapter(
                title = label,
                number = chapterNumberOf(label, seriesName).takeIf { it >= 0f } ?: 0f,
                url = obj.key,
                dateUpload = obj.lastModified,
            )
        }
        val flatPages = pagesOf(looseImages).filterNot { it.key.isCoverFile() }
        if (chapters.isEmpty() && flatPages.isNotEmpty()) {
            chapters += ParsedChapter(
                title = seriesName,
                number = 1f,
                url = prefix,
                dateUpload = flatPages.maxOfOrNull { it.lastModified } ?: 0L,
            )
        }

        val listed = jsonListedChapters(config, tree, prefix)
        val composed = composeJsonChapters(listed)
        // Non-empty JSON chapters replace folder/cbz for this series.
        val merged = if (composed.isNotEmpty()) composed else chapters

        if (merged.isEmpty()) {
            throw IOException(
                emptySeriesMessage(
                    seriesName,
                    unsupported,
                    detailsJson != null || chaptersJson != null,
                ),
            )
        }

        merged
            .sortedWith(compareBy<ParsedChapter> { it.number }.thenBy(NaturalOrder) { it.title })
            .map { chapter ->
                SChapter.create().apply {
                    url = chapter.readerUrl()
                    name = chapter.title
                    date_upload = chapter.dateUpload
                    scanlator = chapter.scanlator
                    chapter_number = chapter.number.takeIf { it > 0f }
                        ?: chapterNumberOf(chapter.title, seriesName).takeIf { it >= 0f }
                        ?: 1f
                }
            }
            .asReversed()
    }

    override fun fetchPageList(chapter: SChapter): Observable<List<Page>> {
        val (url, range) = splitPageRange(chapter.url)
        val slice = { pages: List<Page> -> applyPageRange(pages, range) }
        if (url.startsWith("pages:")) {
            val raw = String(
                Base64.decode(url.removePrefix("pages:"), Base64.URL_SAFE),
                Charsets.UTF_8,
            )
            val urls = json.decodeFromString(ListSerializer(String.serializer()), raw)
            return Observable.just(slice(urls.mapIndexed { index, pageUrl -> Page(index, imageUrl = pageUrl) }))
        }
        if (tryIdentifySite(url) != null) {
            val cleaned = SChapter.create().apply { this.url = url }
            return client.newCall(pageListRequest(cleaned)).asObservableSuccess().map { slice(pageListParse(it)) }
        }
        return Observable.fromCallable { slice(localOrArchivePages(url)) }
    }

    override fun pageListRequest(chapter: SChapter): Request {
        val url = splitPageRange(chapter.url).first
        if (url.startsWith("pages:")) {
            throw Exception("Static page chapters must be opened through fetchPageList")
        }
        val site = identifySite(url)
        if (isRemoteSeriesUrl(url)) {
            throw Exception("Series URLs expand when you refresh the chapter list.")
        }
        val remoteId = extractRemoteId(site, url)
        val target = pageListUrl(site, remoteId, url)
        val builder = headers.newBuilder().set(
            "Referer",
            if (site == SiteId.NovelCrow || site == SiteId.AllPornComic) {
                "${madaraOrigin(url)}/"
            } else {
                siteReferer(site)
            },
        )
        if (site == SiteId.Hitomi) builder.set("Origin", HITOMI_BASE)
        if (site == SiteId.MangaDex) {
            builder.set("User-Agent", MANGADEX_USER_AGENT).set("Accept", "application/json")
        }
        return GET(target, builder.build())
    }

    override fun pageListParse(response: Response): List<Page> {
        val requestUrl = response.request.url.toString()
        val body = response.use { it.body.string() }
        val host = hostOf(requestUrl)
        return when {
            host.contains("nhentai.net") && requestUrl.contains("/api/v2/galleries/") -> {
                val server = cachedNhServer ?: run {
                    val cfg = runCatching {
                        client.newCall(GET("https://nhentai.net/api/v2/config", headers)).execute()
                            .use { it.body.string() }
                    }.getOrNull()
                    (cfg?.let { pickNhServer(it, json) } ?: pickNhServer("{}", json)).also {
                        cachedNhServer = it
                    }
                }
                parseNhentaiPages(body, json, server)
            }
            host.contains("hentairead.com") -> parseHentaiReadPages(body, requestUrl, json)
            host.contains("hentainexus.com") -> parseHentaiNexusPages(body, json)
            host.contains("hentai2read.com") -> parseHentai2ReadPages(body)
            host.contains("panda.chaika.moe") || host.contains("chaika.moe") -> parseChaikaPages(body)
            isEHentaiHost(host) -> parseEhentaiPages(body, requestUrl)
            host.contains(HITOMI_CDN) || host.contains("hitomi.la") -> parseHitomiPages(body)
            host.contains("novelcrow.com") || host.contains("allporncomic") ->
                parseMadaraPages(body, requestUrl)
            host.contains("api.mangadex.org") -> parseMangaDexAtHome(body, json, requestUrl)
            else -> throw Exception("Don't know how to parse pages from $host")
        }
    }

    private fun localOrArchivePages(rawTarget: String): List<Page> {
        val target = splitPageRange(rawTarget).first
        val config = requireConfig()
        val pages = when {
            isAbsoluteUrl(target) || isBucketArchive(target) -> archivePages(target)
            isFolderChapter(target) || target.endsWith("/") -> {
                val images = s3.listAll(config, target, delimiter = null)
                    .objects
                    .filter { isImageKey(it.key) && it.key.isChildOf(target) }
                pagesOf(images).mapIndexed { index, obj ->
                    Page(index, imageUrl = config.imageUrl(obj.key).toString())
                }
            }
            else -> archivePages(target)
        }
        if (pages.isEmpty()) throw IOException("No images found in \"${target.fileName()}\".")
        return pages
    }

    private fun archivePages(target: String): List<Page> = archives.zipFor(target).entries()
        .filter { isImageKey(it.name) && !isHiddenKey(it.name) }
        .sortedWith(compareBy(NaturalOrder) { it.name })
        .mapIndexed { index, entry ->
            Page(index, imageUrl = ArchiveInterceptor.pageUrl(target, entry.name))
        }

    private fun pagesOf(images: List<S3Object>): List<S3Object> {
        val sorted = images.sortedWith(compareBy(NaturalOrder) { it.key })
        if (sorted.size <= 1) return sorted
        return sorted.filterNot { it.key.isCoverFile() }
    }

    private fun shallowListing(config: R2Config, seriesPrefix: String): S3Listing = shallowCache.get(seriesPrefix) { s3.listAll(config, seriesPrefix, DELIMITER) }

    private fun treeListing(config: R2Config, seriesPrefix: String): S3Listing = treeCache.get(seriesPrefix) { s3.listAll(config, seriesPrefix, delimiter = null) }

    private fun String.isChildOf(prefix: String): Boolean = startsWith(prefix) && !removePrefix(prefix).contains('/')

    private fun String.isCoverFile(): Boolean = fileName().substringBeforeLast('.').equals("cover", ignoreCase = true)

    override fun imageUrlParse(response: Response): String {
        val requestUrl = response.request.url.toString()
        if (!isEHentaiHost(hostOf(requestUrl))) throw UnsupportedOperationException()
        val html = response.use { it.body.string() }
        return parseEhentaiImageUrl(html, requestUrl, includeBackup = true)
    }

    override fun imageRequest(page: Page): Request {
        val url = page.imageUrl ?: throw Exception("Missing page URL")
        val builder = headers.newBuilder()
            .set("Accept", "image/avif,image/webp,image/apng,image/*,*/*;q=0.8")
        when {
            isChaikaLoopback(url) -> builder.set("Referer", "$CHAIKA_BASE/")
            url.contains(CoverImageInterceptor.HOST, ignoreCase = true) -> {
                url.toHttpUrlOrNull()?.queryParameter("u")?.let { real ->
                    refererForImage(real)?.let { builder.set("Referer", it) }
                }
            }
            page.url.contains("e-hentai.org") || page.url.contains("exhentai.org") ->
                builder.set("Referer", page.url)
            page.url.contains("novelcrow.com") || url.contains("novelcrow.com") ->
                builder.set("Referer", "$NOVELCROW_BASE/")
            page.url.contains("allporncomic") || url.contains("allporncomic") ->
                builder.set("Referer", "${madaraOrigin(page.url.ifBlank { url })}/")
            else -> refererForImage(url)?.let { builder.set("Referer", it) }
        }
        return GET(url, builder.build())
    }

    private fun imageRefererInterceptor(chain: Interceptor.Chain): Response {
        val request = chain.request()
        if (!request.method.equals("GET", ignoreCase = true) ||
            !request.header("Referer").isNullOrEmpty()
        ) {
            return chain.proceed(request)
        }
        val referer = refererForImage(request.url.toString()) ?: return chain.proceed(request)
        return chain.proceed(
            request.newBuilder()
                .header("Referer", referer)
                .header("Accept", "image/avif,image/webp,image/apng,image/*,*/*;q=0.8")
                .build(),
        )
    }

    private fun parseEhentaiPages(firstHtml: String, requestUrl: String): List<Page> {
        val urls = linkedSetOf<String>()
        var html = firstHtml
        var currentUrl = requestUrl
        var listingPages = 0
        while (true) {
            urls += parseEhentaiThumbLinks(html, currentUrl)
            val next = parseEhentaiNextPage(html, currentUrl) ?: break
            if (++listingPages > 200) break
            currentUrl = next
            html = client.newCall(GET(next, ehHeaders(next))).execute().use { response ->
                if (!response.isSuccessful) throw Exception("E-Hentai listing failed (${response.code})")
                response.body.string()
            }
        }
        if (urls.isEmpty()) throw Exception("E-Hentai: no pages")
        return urls.mapIndexed { index, viewerUrl -> Page(index, viewerUrl) }
    }

    private fun parseHitomiPages(body: String): List<Page> {
        val hashes = parseHitomiHashes(body, json)
        val ggHeaders = headers.newBuilder()
            .set("Referer", "$HITOMI_BASE/")
            .set("Origin", HITOMI_BASE)
            .build()
        return hashes.mapIndexed { index, hash ->
            Page(index, imageUrl = hitomiGg.imageUrl(client, ggHeaders, hash))
        }
    }

    private fun parseChaikaPages(apiBody: String): List<Page> {
        val zipUrl = parseChaikaDownloadPath(apiBody, json)
        val zipHeaders = headers.newBuilder().set("Referer", "$CHAIKA_BASE/").build()
        val dir = client.zipDirectory(zipUrl, zipHeaders)
        val entries = dir.entries
            .filter { isChaikaImageEntry(it.name) }
            .ifEmpty {
                dir.entries.filter { entry ->
                    !entry.name.endsWith("/") && !entry.name.startsWith("__MACOSX/")
                }
            }
            .sortedWith(compareBy(CASE_INSENSITIVE_ORDER) { it.name })
        if (entries.isEmpty()) throw Exception("PandaChaika: archive has no pages")
        return entries.mapIndexed { index, entry ->
            val payload = json.encodeToString(
                ChaikaZipImage.serializer(),
                ChaikaZipImage(
                    url = zipUrl,
                    name = entry.name,
                    offset = entry.localHeaderOffset,
                    compressedSize = entry.compressedSize,
                    method = entry.method,
                ),
            )
            Page(index, imageUrl = "https://127.0.0.1/#$payload")
        }
    }

    private fun ehHeaders(pageUrl: String): Headers {
        val origin = if (hostOf(pageUrl).contains("exhentai")) EXHENTAI_BASE else EHENTAI_BASE
        return headers.newBuilder().set("Referer", "$origin/").build()
    }

    private fun ehentaiCookieInterceptor(chain: Interceptor.Chain): Response {
        val request = chain.request()
        if (!isEHentaiHost(request.url.host)) return chain.proceed(request)
        val cookie = request.header("Cookie").orEmpty()
        if (cookie.contains("nw=")) return chain.proceed(request)
        val merged = listOf(cookie.takeIf { it.isNotBlank() }, "nw=1", "uconfig=prn_n")
            .filterNotNull()
            .joinToString("; ")
        return chain.proceed(request.newBuilder().header("Cookie", merged).build())
    }

    private fun ehentaiBackupInterceptor(chain: Interceptor.Chain): Response {
        val request = chain.request()
        val bakUrl = request.url.fragment?.takeIf { it.startsWith("http") }
            ?: return chain.proceed(request)
        val result = runCatching { chain.proceed(request) }
        if (!result.isFailure && result.getOrNull()?.isSuccessful == true) {
            return result.getOrThrow()
        }
        result.getOrNull()?.close()
        val bakResponse = chain.proceed(GET(bakUrl, ehHeaders(bakUrl)))
        val newImageUrl = bakResponse.use { response ->
            if (!response.isSuccessful) throw Exception("E-Hentai backup failed (${response.code})")
            parseEhentaiImageUrl(response.body.string(), bakUrl, includeBackup = false)
        }
        return chain.proceed(request.newBuilder().url(newImageUrl).build())
    }

    private fun chaikaZipInterceptor(chain: Interceptor.Chain): Response {
        val request = chain.request()
        if (request.url.host != "127.0.0.1") return chain.proceed(request)
        val fragment = request.url.fragment ?: return chain.proceed(request)
        val data = runCatching {
            json.decodeFromString(ChaikaZipImage.serializer(), fragment)
        }.getOrNull() ?: return chain.proceed(request)

        val rangeRequest = request.newBuilder()
            .url(data.url)
            .range(dataRange(data.offset, data.compressedSize))
            .header("Referer", "$CHAIKA_BASE/")
            .build()
        val response = chain.proceed(rangeRequest)
        if (!response.isSuccessful) return response
        val image = readEntry(response.body.source(), data.compressedSize, data.method).buffer()
        var type = data.name.substringAfterLast('.').lowercase()
        if (type == "jpg") type = "jpeg"
        return response.newBuilder()
            .removeHeader("Content-Range")
            .removeHeader("Content-Length")
            .code(200)
            .message("OK")
            .protocol(Protocol.HTTP_1_1)
            .body(image.asResponseBody("image/$type".toMediaType()))
            .build()
    }

    private class SortFilter : Filter.Sort("Sort", arrayOf("Title"), Filter.Sort.Selection(0, ascending = true))

    override fun getFilterList() = FilterList(
        Filter.Header("Search matches folder names in your bucket."),
        SortFilter(),
    )

    override fun setupPreferenceScreen(screen: PreferenceScreen) {
        val context = screen.context
        fun field(
            key: String,
            title: String,
            summary: String,
            default: String = "",
            secret: Boolean = false,
        ) {
            EditTextPreference(context).apply {
                this.key = key
                this.title = title
                this.summary = summary
                setDefaultValue(default)
                if (secret) {
                    setOnBindEditTextListener { edit ->
                        edit.inputType = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
                    }
                }
                setOnPreferenceChangeListener { _, _ ->
                    invalidateCaches()
                    true
                }
            }.also(screen::addPreference)
        }

        field(PREF_ACCOUNT, "Account ID", "Cloudflare account id")
        field(PREF_ACCESS_KEY, "Access Key ID", "R2 API token access key id")
        field(PREF_SECRET, "Secret Access Key", "R2 API token secret", secret = true)
        field(PREF_BUCKET, "Bucket", "R2 bucket name", default = "manga")
        field(PREF_ENDPOINT, "S3 Endpoint (optional)", "Blank = https://<accountId>.r2.cloudflarestorage.com")
        field(PREF_PREFIX, "Root Prefix", "Empty if title folders sit at bucket root")
        field(
            PREF_PUBLIC_BASE,
            "Public image URL (optional)",
            "r2.dev or custom domain. Images load unsigned when set.",
        )
        field(PREF_REGION, "Region", "Leave as auto", default = DEFAULT_REGION)

        val ttlLabels = arrayOf("1 minute", "5 minutes", "15 minutes", "1 hour", "Never cache")
        val ttlValues = arrayOf("60", "300", "900", "3600", "0")
        val ttlDefault = DEFAULT_CACHE_TTL_SECONDS.toString()
        screen.addPreference(
            ListPreference(context).apply {
                key = PREF_CACHE_TTL
                title = "Re-read the bucket every"
                entries = arrayOf<CharSequence>(*ttlLabels)
                entryValues = arrayOf<CharSequence>(*ttlValues)
                setDefaultValue(ttlDefault)
                setOnPreferenceChangeListener { _, _ ->
                    invalidateCaches()
                    true
                }
            },
        )
        field(
            PREF_LATEST_PAGES,
            "Latest updates scan limit",
            "How many 1000-object pages to scan for Latest.",
            default = DEFAULT_LATEST_PAGES.toString(),
        )
        screen.addPreference(
            SwitchPreferenceCompat(context).apply {
                key = PREF_CLEAR_CACHE
                title = "Clear cached listings"
                summary = "Flip to re-read the bucket immediately"
                setDefaultValue(false)
                setOnPreferenceChangeListener { _, _ ->
                    invalidateCaches()
                    true
                }
            },
        )
    }

    private fun emptyLibraryMessage(config: R2Config, listing: S3Listing): String {
        val where = if (config.rootPrefix.isEmpty()) {
            "bucket \"${config.bucket}\""
        } else {
            "\"${config.rootPrefix}\" in bucket \"${config.bucket}\""
        }
        return if (listing.objects.isEmpty() && listing.prefixes.isEmpty()) {
            "No files found in $where. Upload series folders, then pull to refresh."
        } else {
            "No series folders found in $where. Expected <series>/<chapter>/… or <series>/chapter.cbz."
        }
    }

    private fun emptySeriesMessage(seriesName: String, unsupported: Int, hasChapterList: Boolean): String = when {
        hasChapterList ->
            "\"$seriesName\" has details.json/chapters.json but no readable chapters. " +
                "Each entry needs a url (gallery or .cbz), id+source, or pages array."
        unsupported > 0 ->
            "\"$seriesName\" only contains archive formats this source cannot open " +
                "($unsupported file(s)). Convert them to .cbz, or upload loose images."
        else ->
            "No chapters found in \"$seriesName\". Expected chapter folders, .cbz files, " +
                "or a details.json / chapters.json with gallery URLs / remote archives / page lists."
    }

    override fun popularMangaRequest(page: Int) = throw UnsupportedOperationException()
    override fun popularMangaParse(response: Response) = throw UnsupportedOperationException()
    override fun latestUpdatesRequest(page: Int) = throw UnsupportedOperationException()
    override fun latestUpdatesParse(response: Response) = throw UnsupportedOperationException()
    override fun searchMangaRequest(page: Int, query: String, filters: FilterList) = throw UnsupportedOperationException()
    override fun searchMangaParse(response: Response) = throw UnsupportedOperationException()
    override fun mangaDetailsRequest(manga: SManga) = throw UnsupportedOperationException()
    override fun mangaDetailsParse(response: Response) = throw UnsupportedOperationException()
    override fun chapterListRequest(manga: SManga) = throw UnsupportedOperationException()
    override fun chapterListParse(response: Response) = throw UnsupportedOperationException()
    override fun getMangaUrl(manga: SManga): String = ""
    override fun getChapterUrl(chapter: SChapter): String = ""

    companion object {
        private const val PAGE_SIZE = 30
        private const val COVER_THREADS = 6
        private const val DELIMITER = "/"
        private const val DEFAULT_REGION = "auto"
        private const val DEFAULT_CACHE_TTL_SECONDS = 300L
        private const val DEFAULT_LATEST_PAGES = 10
        private const val MAX_METADATA_BYTES = 1024L * 1024
        private const val PREF_ACCOUNT = "accountId"
        private const val PREF_ACCESS_KEY = "accessKeyId"
        private const val PREF_SECRET = "secretAccessKey"
        private const val PREF_BUCKET = "bucket"
        private const val PREF_ENDPOINT = "endpoint"
        private const val PREF_PREFIX = "prefix"
        private const val PREF_PUBLIC_BASE = "publicBaseUrl"
        private const val PREF_REGION = "region"
        private const val PREF_CACHE_TTL = "cacheTtl"
        private const val PREF_LATEST_PAGES = "latestPages"
        private const val PREF_CLEAR_CACHE = "clearCache"
        private const val SETUP_MESSAGE =
            "Open R2 Library settings and set Account ID, Access Key, Secret, and Bucket"
    }
}

private class ListingCache(private val maxSize: Int) {
    private val entries = object : LinkedHashMap<String, S3Listing>(16, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, S3Listing>) = size > maxSize
    }

    fun get(key: String, load: () -> S3Listing): S3Listing {
        synchronized(entries) { entries[key] }?.let { return it }
        val value = load()
        synchronized(entries) { entries[key] = value }
        return value
    }

    fun clear() = synchronized(entries) { entries.clear() }
}
