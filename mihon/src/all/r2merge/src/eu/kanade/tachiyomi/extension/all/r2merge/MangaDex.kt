package eu.kanade.tachiyomi.extension.all.r2merge

import eu.kanade.tachiyomi.extension.all.r2merge.meta.SeriesMetadata
import eu.kanade.tachiyomi.network.GET
import eu.kanade.tachiyomi.source.model.Page
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import okhttp3.Headers
import okhttp3.HttpUrl.Companion.toHttpUrl
import okhttp3.OkHttpClient
import java.io.IOException
import java.time.Instant

internal const val MANGADEX_SITE = "https://mangadex.org"
internal const val MANGADEX_API = "https://api.mangadex.org"
internal const val MANGADEX_USER_AGENT =
    "R2Library/1.4.13 (https://github.com/raahat-hossain/extensions)"

private val UUID_RE = Regex(
    """[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}""",
    RegexOption.IGNORE_CASE,
)

private val SERIES_PATH = Regex(
    """^https?://(?:www\.|api\.)?mangadex\.org/(?:title|manga)/""" +
        """([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})""" +
        """(?:/[^/?#]*)?/?$""",
    RegexOption.IGNORE_CASE,
)

private val CHAPTER_PATH = Regex(
    """^https?://(?:www\.|api\.)?mangadex\.org/chapter/""" +
        """([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})""" +
        """(?:/[^/?#]*)?/?$""",
    RegexOption.IGNORE_CASE,
)

internal fun isMangaDexSeriesUrl(url: String): Boolean {
    val path = url.trim().substringBefore('?').substringBefore('#')
    return SERIES_PATH.containsMatchIn(path)
}

internal fun isMangaDexChapterUrl(url: String): Boolean {
    val path = url.trim().substringBefore('?').substringBefore('#')
    return CHAPTER_PATH.containsMatchIn(path)
}

internal fun mangaDexIdFromUrl(url: String): String {
    val path = url.trim().substringBefore('?').substringBefore('#')
    return CHAPTER_PATH.find(path)?.groupValues?.get(1)
        ?: SERIES_PATH.find(path)?.groupValues?.get(1)
        ?: UUID_RE.find(path)?.value
        ?: throw Exception("Could not parse mangadex id from $url")
}

internal fun mangaDexHeaders(headers: Headers): Headers = headers.newBuilder()
    .set("User-Agent", MANGADEX_USER_AGENT)
    .set("Accept", "application/json")
    .set("Referer", "$MANGADEX_SITE/")
    .build()

internal fun fetchMangaDexChapters(
    client: OkHttpClient,
    headers: Headers,
    seriesUrl: String,
    json: Json,
): List<ParsedChapter> {
    val mangaId = SERIES_PATH.find(
        seriesUrl.trim().substringBefore('?').substringBefore('#'),
    )?.groupValues?.get(1)
        ?: mangaDexIdFromUrl(seriesUrl)
    val mdHeaders = mangaDexHeaders(headers)
    val collected = mutableListOf<MdFeedChapter>()
    var offset = 0
    val limit = 100
    var total = Int.MAX_VALUE
    var pages = 0
    while (offset < total && pages < 50) {
        val body = fetchMangaDex(client, feedUrl(mangaId, offset, limit), mdHeaders)
        val page = parseMangaDexFeed(body, json)
        collected += page.chapters
        total = page.total
        offset += page.chapters.size
        pages++
        if (page.chapters.isEmpty()) break
    }
    val chapters = dedupeMangaDexChapters(collected)
    if (chapters.isEmpty()) {
        throw IOException("No MangaDex chapters at $seriesUrl")
    }
    return chapters.sortedWith(compareBy { it.number })
}

internal fun parseMangaDexAtHome(body: String, json: Json, requestUrl: String): List<Page> {
    val root = json.parseToJsonElement(body) as? JsonObject
        ?: throw IOException("MangaDex at-home: not JSON")
    val result = (root["result"] as? JsonPrimitive)?.contentOrNull
    if (result != null && result != "ok") {
        throw IOException("MangaDex at-home $result for $requestUrl")
    }
    val baseUrl = (root["baseUrl"] as? JsonPrimitive)?.contentOrNull?.trimEnd('/')
        ?: throw IOException("MangaDex at-home missing baseUrl")
    val chapter = root["chapter"] as? JsonObject
        ?: throw IOException("MangaDex at-home missing chapter")
    val hash = (chapter["hash"] as? JsonPrimitive)?.contentOrNull
        ?: throw IOException("MangaDex at-home missing hash")
    val files = (chapter["data"] as? JsonArray)?.mapNotNull {
        (it as? JsonPrimitive)?.contentOrNull?.takeIf { name -> name.isNotBlank() }
    }.orEmpty()
    if (files.isEmpty()) {
        throw IOException("MangaDex chapter has no pages")
    }
    return files.mapIndexed { index, file ->
        Page(index, url = requestUrl, imageUrl = "$baseUrl/data/$hash/$file")
    }
}

internal fun parseMangaDexFeed(body: String, json: Json): MdFeedPage {
    val root = json.parseToJsonElement(body) as? JsonObject
        ?: throw IOException("MangaDex feed: not JSON")
    val result = (root["result"] as? JsonPrimitive)?.contentOrNull
    if (result != null && result != "ok") {
        throw IOException("MangaDex feed $result")
    }
    val total = (root["total"] as? JsonPrimitive)?.intOrNull ?: 0
    val data = root["data"] as? JsonArray ?: JsonArray(emptyList())
    val chapters = data.mapNotNull { entry ->
        val obj = entry as? JsonObject ?: return@mapNotNull null
        val id = (obj["id"] as? JsonPrimitive)?.contentOrNull ?: return@mapNotNull null
        val attrs = obj["attributes"] as? JsonObject ?: return@mapNotNull null
        val pages = (attrs["pages"] as? JsonPrimitive)?.intOrNull ?: 0
        if (pages <= 0) return@mapNotNull null
        if (!(attrs["externalUrl"] as? JsonPrimitive)?.contentOrNull.isNullOrBlank()) {
            return@mapNotNull null
        }
        val chapter = (attrs["chapter"] as? JsonPrimitive)?.contentOrNull?.trim()
        val title = (attrs["title"] as? JsonPrimitive)?.contentOrNull?.trim()
        val lang = (attrs["translatedLanguage"] as? JsonPrimitive)?.contentOrNull?.lowercase().orEmpty()
        val published = (attrs["publishAt"] as? JsonPrimitive)?.contentOrNull
            ?: (attrs["readableAt"] as? JsonPrimitive)?.contentOrNull
        val group = scanlationGroupName(obj["relationships"] as? JsonArray)
        MdFeedChapter(
            id = id,
            chapter = chapter,
            title = title,
            language = lang,
            pages = pages,
            group = group,
            dateUpload = parseMangaDexDate(published),
        )
    }
    return MdFeedPage(total = total, chapters = chapters)
}

internal class MdFeedPage(
    val total: Int,
    val chapters: List<MdFeedChapter>,
)

internal class MdFeedChapter(
    val id: String,
    val chapter: String?,
    val title: String?,
    val language: String,
    val pages: Int,
    val group: String?,
    val dateUpload: Long,
)

private fun dedupeMangaDexChapters(chapters: List<MdFeedChapter>): List<ParsedChapter> {
    val grouped = linkedMapOf<String, MutableList<MdFeedChapter>>()
    for (chapter in chapters) {
        val key = chapterKey(chapter.chapter).ifBlank { "id:${chapter.id}" }
        grouped.getOrPut(key) { mutableListOf() } += chapter
    }
    return grouped.values.map { group ->
        val best = group.minWith(
            compareBy<MdFeedChapter> { languageRank(it.language) }
                .thenByDescending { it.pages },
        )
        val number = best.chapter?.toFloatOrNull()?.takeIf { it > 0f } ?: 0f
        val extra = best.title?.takeIf { it.isNotBlank() }
        val display = when {
            number > 0f && extra != null -> "Chapter ${formatChapterNumber(number)} - $extra"
            number > 0f -> "Chapter ${formatChapterNumber(number)}"
            extra != null -> extra
            else -> "Oneshot"
        }
        ParsedChapter(
            title = display,
            number = number,
            url = "$MANGADEX_SITE/chapter/${best.id}",
            scanlator = best.group?.takeIf { it.isNotBlank() } ?: "MangaDex",
            dateUpload = best.dateUpload,
            explicitNumber = number > 0f,
            sourceNumber = number,
        )
    }
}

private fun languageRank(lang: String): Int = when (lang) {
    "en" -> 0
    "en-us" -> 0
    else -> 1
}

private fun chapterKey(raw: String?): String {
    val value = raw?.trim().orEmpty()
    if (value.isEmpty()) return ""
    return value.toFloatOrNull()?.toString() ?: value.lowercase()
}

private fun formatChapterNumber(number: Float): String {
    val asInt = number.toInt()
    return if (number == asInt.toFloat()) asInt.toString() else number.toString()
}

private fun scanlationGroupName(relationships: JsonArray?): String? {
    if (relationships == null) return null
    for (entry in relationships) {
        val obj = entry as? JsonObject ?: continue
        if ((obj["type"] as? JsonPrimitive)?.contentOrNull != "scanlation_group") continue
        val name = ((obj["attributes"] as? JsonObject)?.get("name") as? JsonPrimitive)?.contentOrNull
        if (!name.isNullOrBlank()) return name
    }
    return null
}

private fun parseMangaDexDate(value: String?): Long {
    if (value.isNullOrBlank()) return 0L
    return runCatching { Instant.parse(value).toEpochMilli() }.getOrDefault(0L)
}

private fun feedUrl(mangaId: String, offset: Int, limit: Int): String {
    val builder = "$MANGADEX_API/manga/$mangaId/feed".toHttpUrl().newBuilder()
        .addQueryParameter("limit", limit.toString())
        .addQueryParameter("offset", offset.toString())
        .addQueryParameter("order[chapter]", "asc")
        .addQueryParameter("includeEmptyPages", "0")
        .addQueryParameter("includeFuturePublishAt", "0")
        .addQueryParameter("includeExternalUrl", "0")
        .addQueryParameter("includes[]", "scanlation_group")
    for (rating in listOf("safe", "suggestive", "erotica", "pornographic")) {
        builder.addQueryParameter("contentRating[]", rating)
    }
    return builder.build().toString()
}

internal fun fetchMangaDexMetadata(
    client: OkHttpClient,
    headers: Headers,
    seriesUrl: String,
    json: Json,
): SeriesMetadata {
    val mangaId = SERIES_PATH.find(
        seriesUrl.trim().substringBefore('?').substringBefore('#'),
    )?.groupValues?.get(1)
        ?: mangaDexIdFromUrl(seriesUrl)
    val body = fetchMangaDex(
        client,
        mangaDexMangaUrl(mangaId),
        mangaDexHeaders(headers),
    )
    return parseMangaDexManga(body, json, mangaId)
}

internal fun parseMangaDexManga(body: String, json: Json, mangaId: String): SeriesMetadata {
    val root = json.parseToJsonElement(body) as? JsonObject
        ?: throw IOException("MangaDex manga: not JSON")
    val result = (root["result"] as? JsonPrimitive)?.contentOrNull
    if (result != null && result != "ok") {
        throw IOException("MangaDex manga $result")
    }
    val data = root["data"] as? JsonObject ?: throw IOException("MangaDex manga missing data")
    val attrs = data["attributes"] as? JsonObject
    val titleMap = attrs?.get("title") as? JsonObject
    val title = localizedMangaDexText(titleMap)
        ?: localizedMangaDexText(attrs?.get("altTitles") as? JsonArray)
    val description = localizedMangaDexText(attrs?.get("description") as? JsonObject)
    val status = SeriesMetadata.statusOf((attrs?.get("status") as? JsonPrimitive)?.contentOrNull)
    val author = relationshipNames(data["relationships"] as? JsonArray, "author")
    val artist = relationshipNames(data["relationships"] as? JsonArray, "artist")
    val coverFile = coverFileName(data["relationships"] as? JsonArray)
    val cover = coverFile?.let { "https://uploads.mangadex.org/covers/$mangaId/$it" }
    return SeriesMetadata(
        title = title,
        author = author,
        artist = artist,
        description = description,
        status = status,
        cover = cover,
    )
}

private fun mangaDexMangaUrl(mangaId: String): String {
    val builder = "$MANGADEX_API/manga/$mangaId".toHttpUrl().newBuilder()
        .addQueryParameter("includes[]", "cover_art")
        .addQueryParameter("includes[]", "author")
        .addQueryParameter("includes[]", "artist")
    return builder.build().toString()
}

private fun localizedMangaDexText(map: JsonObject?): String? {
    if (map == null) return null
    listOf("en", "en-us", "ja-ro", "ja", "ko", "zh").forEach { lang ->
        (map[lang] as? JsonPrimitive)?.contentOrNull?.trim()?.takeIf { it.isNotEmpty() }?.let { return it }
    }
    return map.values.firstOrNull { it is JsonPrimitive }
        ?.let { (it as JsonPrimitive).contentOrNull?.trim()?.takeIf { text -> text.isNotEmpty() } }
}

private fun localizedMangaDexText(alts: JsonArray?): String? {
    if (alts == null) return null
    for (entry in alts) {
        localizedMangaDexText(entry as? JsonObject)?.let { return it }
    }
    return null
}

private fun relationshipNames(relationships: JsonArray?, type: String): String? {
    if (relationships == null) return null
    val names = relationships.mapNotNull { entry ->
        val obj = entry as? JsonObject ?: return@mapNotNull null
        if ((obj["type"] as? JsonPrimitive)?.contentOrNull != type) return@mapNotNull null
        ((obj["attributes"] as? JsonObject)?.get("name") as? JsonPrimitive)?.contentOrNull
            ?.trim()?.takeIf { it.isNotEmpty() }
    }
    return names.joinToString().takeIf { it.isNotEmpty() }
}

private fun coverFileName(relationships: JsonArray?): String? {
    if (relationships == null) return null
    for (entry in relationships) {
        val obj = entry as? JsonObject ?: continue
        if ((obj["type"] as? JsonPrimitive)?.contentOrNull != "cover_art") continue
        val name = ((obj["attributes"] as? JsonObject)?.get("fileName") as? JsonPrimitive)?.contentOrNull
        if (!name.isNullOrBlank()) return name
    }
    return null
}

private fun fetchMangaDex(client: OkHttpClient, url: String, headers: Headers): String {
    val response = client.newCall(GET(url, headers)).execute()
    val body = response.use { it.body.string() }
    if (!response.isSuccessful) {
        throw IOException("HTTP ${response.code} from $url")
    }
    return body
}
