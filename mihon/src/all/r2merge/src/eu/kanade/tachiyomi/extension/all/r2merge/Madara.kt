package eu.kanade.tachiyomi.extension.all.r2merge

import eu.kanade.tachiyomi.extension.all.r2merge.meta.SeriesMetadata
import eu.kanade.tachiyomi.extension.all.r2merge.util.chapterNumberOf
import eu.kanade.tachiyomi.extension.all.r2merge.util.hasChapterPart
import eu.kanade.tachiyomi.extension.all.r2merge.util.leadingChapterNumber
import eu.kanade.tachiyomi.extension.all.r2merge.util.overlayChapterNumber
import eu.kanade.tachiyomi.extension.all.r2merge.util.siteListIndex
import eu.kanade.tachiyomi.network.GET
import eu.kanade.tachiyomi.source.model.Page
import okhttp3.FormBody
import okhttp3.Headers
import okhttp3.OkHttpClient
import okhttp3.Request
import okhttp3.RequestBody.Companion.toRequestBody
import org.jsoup.Jsoup
import java.io.IOException

internal const val NOVELCROW_BASE = "https://novelcrow.com"
internal const val ALLPORNCOMIC_BASE = "https://allporncomic.com"

private val MADARA_SERIES_PATH = Regex(
    """^https?://(?:www\.)?(novelcrow\.com|allporncomic\.com|allporncomics\.com)/""" +
        """(comic|manga|porncomic)/([^/?#]+)/?$""",
    RegexOption.IGNORE_CASE,
)

internal fun isMadaraSeriesUrl(url: String): Boolean {
    val path = url.trim().substringBefore('?').substringBefore('#')
    return MADARA_SERIES_PATH.containsMatchIn(path)
}

internal fun isNovelCrowSeriesUrl(url: String): Boolean = isMadaraSeriesUrl(url) && hostOf(url).contains("novelcrow")

internal fun isRemoteSeriesUrl(url: String): Boolean = isMadaraSeriesUrl(url) || isMangaDexSeriesUrl(url)

internal fun madaraOrigin(url: String): String {
    val match = Regex("""^(https?://[^/?#]+)""", RegexOption.IGNORE_CASE).find(url.trim())
    return match?.groupValues?.get(1)?.trimEnd('/') ?: NOVELCROW_BASE
}

internal fun madaraHostLabel(url: String): String {
    val host = hostOf(url)
    return when {
        host.contains("allporncomic") -> "AllPornComic"
        host.contains("novelcrow") -> "NovelCrow"
        host.isNotBlank() -> host
        else -> "Madara"
    }
}

internal fun madaraRelativePath(url: String): String {
    val path = url.trim()
        .replace(Regex("""[?#].*$"""), "")
        .trimEnd('/')
        .replace(Regex("""^https?://[^/]+""", RegexOption.IGNORE_CASE), "")
        .trim('/')
    return path
}

internal fun parseMadaraChapterList(html: String, baseUrl: String, scanlator: String): List<ParsedChapter> {
    val document = Jsoup.parse(html, baseUrl)
    val items = document.select("li.wp-manga-chapter").ifEmpty {
        document.select("li.wp-manga-chapter a, .wp-manga-chapter > a")
    }
    val parsed = items.mapNotNull { item ->
        val link = madaraChapterAnchor(item) ?: return@mapNotNull null
        val href = madaraHref(link)
        if (href.isBlank() || isMadaraSeriesUrl(href)) return@mapNotNull null
        val title = madaraChapterTitle(link)
        if (title.isBlank()) return@mapNotNull null
        // Link text only — the row's thumbnail `<a>` often repeats the latest chapter's `2 . …`.
        val context = listOf(
            madaraRowText(link),
            item.ownText(),
            item.selectFirst(".chapter-num, .chapternum, .wp-manga-chapter-num, [class*=chapter-num]")?.text().orEmpty(),
        ).filter { it.isNotBlank() }.joinToString("\n")
        val attrNumber = listOf("data-num", "data-chapter-number")
            .firstNotNullOfOrNull { item.attr(it).trim().toFloatOrNull() }
        val number = madaraChapterNumber(title, href, attrNumber, context)
        ParsedChapter(
            title = title,
            number = number,
            url = href.substringBefore('?').trimEnd('/') + "/",
            scanlator = scanlator,
            explicitNumber = number > 0f,
            sourceNumber = number,
        )
    }
    // Thumbnail + title `<a>` share a URL; keep the title link, not the latest-chapter caption.
    val unique = LinkedHashMap<String, ParsedChapter>()
    for (chapter in parsed) {
        val prev = unique[chapter.url]
        unique[chapter.url] = if (prev == null) chapter else betterParsedChapter(prev, chapter)
    }
    return unique.values.toList()
}

private fun madaraHref(link: org.jsoup.nodes.Element): String = link.absUrl("href").ifBlank { link.attr("href") }.trim()

/** Title link, not a thumbnail `<a><img>` that often repeats the latest chapter's label. */
private fun madaraChapterAnchor(item: org.jsoup.nodes.Element): org.jsoup.nodes.Element? {
    if (item.tagName() == "a") return item
    val anchors = item.select("a[href]").filter { a ->
        val href = madaraHref(a)
        href.isNotBlank() && !isMadaraSeriesUrl(href)
    }
    if (anchors.isEmpty()) return null
    return anchors.maxByOrNull { madaraAnchorScore(it) }
}

private fun madaraAnchorScore(a: org.jsoup.nodes.Element): Int {
    val text = madaraChapterTitle(a)
    val img = a.selectFirst("img") != null
    val numbered = leadingChapterNumber(text) != null || overlayChapterNumber(text) != null
    val titled = text.isNotBlank()
    val rank = when {
        numbered && !img -> 6
        titled && !img -> 5
        numbered && img -> 3
        titled && img -> 2
        !img -> 1
        else -> 0
    }
    return rank * 1_000 + text.length.coerceAtMost(200)
}

private fun betterParsedChapter(a: ParsedChapter, b: ParsedChapter): ParsedChapter {
    val score = { chapter: ParsedChapter ->
        val text = chapter.title
        val numbered = leadingChapterNumber(text) != null || overlayChapterNumber(text) != null
        (
            if (numbered) {
                2
            } else if (text.isNotBlank()) {
                1
            } else {
                0
            }
            ) * 1_000 + text.length.coerceAtMost(200)
    }
    return if (score(b) > score(a)) b else a
}

private fun madaraChapterTitle(link: org.jsoup.nodes.Element): String {
    val named = link.selectFirst(".chapter-manhwa-title, .chapternum, .chapter-title, .chapter-name")
        ?.text()?.trim().orEmpty()
    if (named.isNotEmpty()) return named
    val own = link.ownText().trim()
    if (own.isNotEmpty()) return own
    val nodes = link.textNodes().joinToString(" ") { it.text().trim() }.trim()
    if (nodes.isNotEmpty()) return nodes
    val copy = link.clone()
    copy.select("img, .chapter-release-date, .c-new-tag, time").remove()
    return copy.text().trim()
}

/**
 * Site chapter number for `titles` / `chapterRange`.
 * `1.6. … Chapter 1 Part 7` is 1.6, not 1. Slug `/1-6-…` is 1.6.
 * `Chapter N` must not win while `Part` is in the title.
 */
internal fun madaraChapterNumber(
    title: String,
    href: String,
    attrNumber: Float? = null,
    context: String = "",
): Float {
    siteListIndex(title)?.takeIf { it > 0f }?.let { return it }
    siteListIndex(context)?.takeIf { it > 0f }?.let { return it }
    hyphenDecimalSlug(href)?.takeIf { it > 0f }?.let { return it }
    dottedDecimalSlug(href)?.takeIf { it > 0f }?.let { return it }
    attrNumber?.takeIf { it > 0f && it < 1_000f }?.let { return it }
    overlayChapterNumber(title)?.takeIf { !hasChapterPart(title) }?.let { return it }
    overlayChapterNumber(context)?.takeIf { context.isNotBlank() && !hasChapterPart(context) }?.let { return it }
    plainNumericSlug(href)?.takeIf { it > 0f }?.let { return it }
    chapterNumberOf(title).takeIf { it >= 0f && !hasChapterPart(title) }?.let { return it }
    return 0f
}

private val SLUG_DECIMAL = Regex("""^(\d+)-(\d+)(?:-|$)""")
private val SLUG_DOTTED = Regex("""^(\d+\.\d+)(?:-|$)""")
private val SLUG_PLAIN = Regex("""^(\d+(?:\.\d+)?)$""")
private val SLUG_PREFIX = Regex("""^(?:ch(?:apter)?|ep(?:isode)?)-?""", RegexOption.IGNORE_CASE)

private fun madaraSlug(url: String): String {
    val seg = url.trim().substringBefore('?').substringBefore('#').trimEnd('/')
        .substringAfterLast('/').replace('_', '-')
    return seg.replace(SLUG_PREFIX, "")
}

/** `/0-2/` `/chapter-0-2/` `/1-6-the-snap-…` → 0.2 / 1.6. Not `2-the-snap-chapter-2`. */
private fun hyphenDecimalSlug(url: String): Float? {
    val match = SLUG_DECIMAL.find(madaraSlug(url)) ?: return null
    return "${match.groupValues[1]}.${match.groupValues[2]}".toFloatOrNull()
}

/** `/1.6/` or `/1.6-the-snap/` → 1.6. */
private fun dottedDecimalSlug(url: String): Float? =
    SLUG_DOTTED.find(madaraSlug(url))?.groupValues?.get(1)?.toFloatOrNull()

/** `/2/` or `/chapter-2/` → 2. */
private fun plainNumericSlug(url: String): Float? = SLUG_PLAIN.matchEntire(madaraSlug(url))?.groupValues?.get(1)?.toFloatOrNull()

private fun madaraRowText(item: org.jsoup.nodes.Element): String {
    val copy = item.clone()
    copy.select("img, .chapter-release-date, .c-new-tag, time").remove()
    return copy.text().trim()
}

internal fun parseMadaraPages(html: String, pageUrl: String): List<Page> {
    val document = Jsoup.parse(html, pageUrl)
    val fromBreaks = document.select("div.page-break img, li.blocks-gallery-item img")
        .mapNotNull { madaraImageUrl(it) }
    val urls = fromBreaks.ifEmpty {
        document.select("div.reading-content img, img.wp-manga-chapter-img")
            .mapNotNull { madaraImageUrl(it) }
    }.ifEmpty {
        Regex("""chapter_preloaded_images\s*=\s*(\[[^\]]+\])""")
            .find(html)
            ?.groupValues
            ?.get(1)
            ?.let { blob ->
                Regex("""https?:\\?/\\?/[^"'\\s]+""").findAll(blob).map { match ->
                    match.value.replace("\\/", "/")
                }.toList()
            }
            .orEmpty()
    }.filter { url ->
        val skip = Regex("avatar|logo|icon|ads|banner|emoji|spinner|loading", RegexOption.IGNORE_CASE)
        val keep = Regex("wp-content/uploads|manga|chapter|comic|/wp-content/", RegexOption.IGNORE_CASE)
        keep.containsMatchIn(url) || !skip.containsMatchIn(url)
    }.distinct()
    if (urls.isEmpty()) {
        throw IOException("${madaraHostLabel(pageUrl)}: no pages at $pageUrl")
    }
    return urls.mapIndexed { index, url -> Page(index, url = pageUrl, imageUrl = url) }
}

internal fun parseMadaraSeriesMetadata(html: String, pageUrl: String): SeriesMetadata {
    val document = Jsoup.parse(html, pageUrl)
    val title = listOf(
        "div.post-title h1",
        ".post-title h1",
        "h1.entry-title",
        "h1",
    ).firstNotNullOfOrNull { selector ->
        document.selectFirst(selector)?.text()?.trim()?.takeIf { it.isNotEmpty() }
    }
    val coverEl = document.selectFirst("div.summary_image img, .summary_image img, meta[property=og:image]")
    val cover = when {
        coverEl == null -> null
        coverEl.tagName() == "meta" -> coverEl.attr("abs:content").ifBlank { coverEl.attr("content") }
            .takeIf { it.startsWith("http") }
        else -> madaraImageUrl(coverEl)
    }
    val author = document.select(".author-content a, .manga-authors a")
        .eachText()
        .map { it.trim() }
        .filter { it.isNotEmpty() }
        .distinct()
        .joinToString()
        .takeIf { it.isNotEmpty() }
    val artist = document.select(".artist-content a")
        .eachText()
        .map { it.trim() }
        .filter { it.isNotEmpty() }
        .distinct()
        .joinToString()
        .takeIf { it.isNotEmpty() }
    val description = document.selectFirst(
        "div.description-summary .summary__content, .summary__content, div.summary__content",
    )?.text()?.trim()?.takeIf { it.isNotEmpty() }
    val genre = document.select(".genres-content a")
        .eachText()
        .map { it.trim() }
        .filter { it.isNotEmpty() }
        .distinct()
        .joinToString(", ")
        .takeIf { it.isNotEmpty() }
    val statusRaw = document.select(".post-content_item").firstOrNull { item ->
        item.selectFirst("h5, .summary-heading")?.text().orEmpty().contains("status", ignoreCase = true)
    }?.selectFirst(".summary-content")?.text()
        ?: document.selectFirst(".post-status .summary-content")?.text()
    return SeriesMetadata(
        title = title,
        author = author,
        artist = artist,
        description = description,
        genre = genre,
        status = SeriesMetadata.statusOf(statusRaw),
        cover = cover,
    )
}

internal fun fetchMadaraChapters(
    client: OkHttpClient,
    headers: Headers,
    seriesUrl: String,
): List<ParsedChapter> {
    val label = madaraHostLabel(seriesUrl)
    val origin = madaraOrigin(seriesUrl)
    val series = seriesUrl.trim().substringBefore('#').substringBefore('?').trimEnd('/') + "/"
    val pageHeaders = headers.newBuilder()
        .set("Referer", "$origin/")
        .set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .build()
    var html = fetchMadaraHttp(client, GET(series, pageHeaders), label)
    var chapters = parseMadaraChapterList(html, series, label)
    if (chapters.isEmpty()) {
        val ajaxHeaders = pageHeaders.newBuilder()
            .set("X-Requested-With", "XMLHttpRequest")
            .set("Referer", series)
            .build()
        val ajaxUrls = listOf("${series}ajax/chapters/", "${series}ajax/chapters")
        for (ajaxUrl in ajaxUrls) {
            html = runCatching {
                fetchMadaraHttp(
                    client,
                    Request.Builder()
                        .url(ajaxUrl)
                        .headers(ajaxHeaders)
                        .post(ByteArray(0).toRequestBody(null))
                        .build(),
                    label,
                )
            }.getOrNull() ?: continue
            chapters = parseMadaraChapterList(html, series, label)
            if (chapters.isNotEmpty()) break
        }
    }
    if (chapters.isEmpty()) {
        val postId = Regex(
            """id=["']manga-chapters-holder["'][^>]*data-id=["'](\d+)["']""",
            RegexOption.IGNORE_CASE,
        ).find(html)?.groupValues?.get(1)
            ?: Regex(
                """data-id=["'](\d+)["'][^>]*id=["']manga-chapters-holder""",
                RegexOption.IGNORE_CASE,
            ).find(html)?.groupValues?.get(1)
        if (!postId.isNullOrBlank()) {
            val form = FormBody.Builder()
                .add("action", "manga_get_chapters")
                .add("manga", postId)
                .build()
            html = fetchMadaraHttp(
                client,
                Request.Builder()
                    .url("$origin/wp-admin/admin-ajax.php")
                    .headers(
                        pageHeaders.newBuilder()
                            .set("X-Requested-With", "XMLHttpRequest")
                            .set("Referer", series)
                            .build(),
                    )
                    .post(form)
                    .build(),
                label,
            )
            chapters = parseMadaraChapterList(html, series, label)
        }
    }
    if (chapters.isEmpty()) {
        throw IOException(
            "No $label chapters at $series — open a $label page to pass Cloudflare, then refresh.",
        )
    }
    return chapters.sortedWith(compareBy { it.number })
}

internal fun fetchMadaraSeriesMetadata(
    client: OkHttpClient,
    headers: Headers,
    seriesUrl: String,
): SeriesMetadata {
    val origin = madaraOrigin(seriesUrl)
    val series = seriesUrl.trim().substringBefore('#').substringBefore('?').trimEnd('/') + "/"
    val pageHeaders = headers.newBuilder()
        .set("Referer", "$origin/")
        .set("Accept", "text/html,application/xhtml+xml,application/xml;q=0.9,*/*;q=0.8")
        .build()
    val html = fetchMadaraHttp(client, GET(series, pageHeaders), madaraHostLabel(seriesUrl))
    return parseMadaraSeriesMetadata(html, series)
}

internal fun fetchNovelCrowChapters(
    client: OkHttpClient,
    headers: Headers,
    seriesUrl: String,
): List<ParsedChapter> = fetchMadaraChapters(client, headers, seriesUrl)

private fun madaraImageUrl(element: org.jsoup.nodes.Element): String? {
    val url = listOf("data-src", "data-lazy-src", "data-cfsrc", "src")
        .firstNotNullOfOrNull { attr ->
            element.absUrl(attr).ifBlank { element.attr(attr) }.takeIf { it.startsWith("http") }
        }
        ?: return null
    if (url.startsWith("data:")) return null
    return url
}

private fun fetchMadaraHttp(client: OkHttpClient, request: Request, label: String): String {
    val response = client.newCall(request).execute()
    val body = response.use { it.body.string() }
    if (!response.isSuccessful) {
        throw IOException("HTTP ${response.code} from ${request.url}")
    }
    if (body.contains("Just a moment", ignoreCase = true) &&
        (body.contains("cf-", ignoreCase = true) || body.contains("challenge-platform"))
    ) {
        throw IOException(
            "Cloudflare blocked $label. Open any $label page, solve it, then refresh.",
        )
    }
    return body
}
