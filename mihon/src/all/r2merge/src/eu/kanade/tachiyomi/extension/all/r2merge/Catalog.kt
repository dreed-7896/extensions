package eu.kanade.tachiyomi.extension.all.r2merge

import android.util.Base64
import eu.kanade.tachiyomi.extension.all.r2merge.util.chapterNumberOf
import eu.kanade.tachiyomi.extension.all.r2merge.util.fileName
import eu.kanade.tachiyomi.extension.all.r2merge.util.isArchiveKey
import eu.kanade.tachiyomi.extension.all.r2merge.util.overlayChapterNumber
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import java.io.IOException
import kotlin.math.abs

internal data class ParsedChapter(
    val title: String,
    val number: Float,
    val url: String,
    val scanlator: String? = null,
    val dateUpload: Long = 0L,
    /** JSON `number` or a `Chapter N` title — safe to replace a folder/cbz with this number. */
    val explicitNumber: Boolean = false,
    val pageRange: PageRange? = null,
    /** Site chapter number, kept after display reindex so ranges/titles stay stable. */
    val sourceNumber: Float = 0f,
    val chapterRange: ChapterRange? = null,
    val titles: Map<Float, String> = emptyMap(),
    val titleLocked: Boolean = false,
)

internal fun ParsedChapter.resolvedSourceNumber(): Float = if (sourceNumber > 0f) sourceNumber else number

internal class JsonChapterListing(
    val chapters: List<ParsedChapter>,
)

/**
 * JSON order is source order. Series URLs expand (optional [ParsedChapter.chapterRange],
 * default all, including chapters that appear later). Singles stay one chapter.
 * Display numbers are then 1..N in that order so sources never steal each other's
 * chapter 1. Title-only / pageRange-only rows with a number patch the composed list.
 */
internal fun concatenateSources(
    specs: List<ParsedChapter>,
    fetchSeries: (String) -> List<ParsedChapter>,
): Pair<List<ParsedChapter>, List<ParsedChapter>> {
    val composed = mutableListOf<ParsedChapter>()
    val patches = mutableListOf<ParsedChapter>()
    for (spec in specs) {
        if (spec.url.isBlank()) {
            patches += spec
            continue
        }
        composed += expandSourceChapters(spec, fetchSeries)
    }
    val unique = composed.distinctBy { it.readerUrl() }
    val numbered = unique.mapIndexed { index, chapter ->
        chapter.copy(number = (index + 1).toFloat(), explicitNumber = true)
    }
    if (patches.isEmpty()) return numbered to emptyList()
    val patched = numbered.toMutableList()
    val leftover = mutableListOf<ParsedChapter>()
    for (patch in patches) {
        val idx = patched.indexOfFirst { abs(it.number - patch.number) < 1e-4f }
        if (idx >= 0 && (patch.title.isNotBlank() || patch.pageRange != null)) {
            val base = patched[idx]
            patched[idx] = base.copy(
                title = patch.title.takeIf { it.isNotBlank() } ?: base.title,
                pageRange = patch.pageRange ?: base.pageRange,
                titleLocked = patch.title.isNotBlank() || base.titleLocked,
            )
        } else {
            leftover += patch
        }
    }
    return patched to leftover
}

internal fun expandSourceChapters(
    spec: ParsedChapter,
    fetchSeries: (String) -> List<ParsedChapter>,
): List<ParsedChapter> {
    val series = isRemoteSeriesUrl(spec.url)
    val fetched = if (series) {
        fetchSeries(spec.url)
    } else {
        listOf(
            spec.copy(
                sourceNumber = spec.resolvedSourceNumber(),
                titleLocked = spec.title.isNotBlank(),
                chapterRange = null,
                titles = emptyMap(),
            ),
        )
    }
    val ranged = spec.chapterRange?.let { range ->
        fetched.filter { range.contains(it.resolvedSourceNumber()) }
    } ?: fetched
    if (series && ranged.isEmpty()) {
        val window = spec.chapterRange?.spec() ?: "all"
        throw IOException("chapterRange $window matched no chapters at ${spec.url}")
    }
    return ranged.map { chapter ->
        val sourceNum = chapter.resolvedSourceNumber()
        val renamed = spec.titles.nameFor(sourceNum)
        chapter.copy(
            title = renamed ?: chapter.title,
            titleLocked = renamed != null || chapter.titleLocked,
            pageRange = chapter.pageRange ?: spec.pageRange,
            sourceNumber = sourceNum,
            chapterRange = null,
            titles = emptyMap(),
        )
    }
}

internal fun firstRemoteSeriesUrl(chapters: List<ParsedChapter>): String? = chapters.map { it.url }.firstOrNull { isRemoteSeriesUrl(it) }

internal fun titlesFromJson(obj: JsonObject?): Map<Float, String> {
    if (obj == null) return emptyMap()
    val node = obj["titles"] ?: obj["chapterTitles"] ?: return emptyMap()
    val mapped = when (node) {
        is JsonObject -> node.entries.mapNotNull { (key, value) ->
            val number = key.trim().toFloatOrNull() ?: overlayChapterNumber(key) ?: return@mapNotNull null
            val name = (value as? JsonPrimitive)?.contentOrNull?.trim()?.takeIf { it.isNotEmpty() }
                ?: return@mapNotNull null
            number to name
        }
        else -> emptyList()
    }
    return mapped.toMap()
}

private fun Map<Float, String>.nameFor(number: Float): String? = entries.firstOrNull { abs(it.key - number) < 1e-4f }?.value

/**
 * Gallery URL, remote/local archive, folder, id+source, or explicit page list.
 * Relative paths are resolved against the series prefix.
 */
internal fun parseChaptersJson(
    body: String,
    json: Json,
    seriesPrefix: String = "",
): List<ParsedChapter> {
    val parsed = json.parseToJsonElement(body.trim().removePrefix("\uFEFF"))
    val list: JsonArray = when (parsed) {
        is JsonArray -> parsed
        is JsonObject -> parsed["chapters"] as? JsonArray ?: JsonArray(emptyList())
        else -> return emptyList()
    }

    return list.mapIndexedNotNull { index, entry ->
        val obj = entry as? JsonObject
        val rawUrl = when (entry) {
            is JsonPrimitive -> entry.content
            else -> listOf("url", "href", "link", "archive", "file", "key").firstNotNullOfOrNull {
                (obj?.get(it) as? JsonPrimitive)?.contentOrNull
            }
        }?.trim().orEmpty()
        val pages = (obj?.get("pages") as? JsonArray)?.mapNotNull {
            (it as? JsonPrimitive)?.contentOrNull
        }?.filter { it.isNotBlank() }
        val source = listOf("source", "site", "host").firstNotNullOfOrNull {
            (obj?.get(it) as? JsonPrimitive)?.contentOrNull
        }
        val rawId = (obj?.get("id") as? JsonPrimitive)?.contentOrNull?.trim().orEmpty()
        val title = listOf("title", "name").firstNotNullOfOrNull {
            (obj?.get(it) as? JsonPrimitive)?.contentOrNull
        }?.trim().orEmpty()
        val jsonNumber = (obj?.get("number") as? JsonPrimitive)?.contentOrNull?.toFloatOrNull()
        val titles = titlesFromJson(obj)

        val chapterUrl: String
        val displayFallback: String
        val siteLabel: String?
        if (!pages.isNullOrEmpty()) {
            chapterUrl = "pages:" + Base64.encodeToString(
                json.encodeToString(ListSerializer(String.serializer()), pages).toByteArray(),
                Base64.URL_SAFE or Base64.NO_WRAP,
            )
            displayFallback = "Chapter ${index + 1}"
            siteLabel = source
        } else {
            val site = tryIdentifySite(rawUrl, source)
            when {
                site != null -> {
                    val remoteId = rawId.ifBlank { extractRemoteId(site, rawUrl) }
                    chapterUrl = if (rawUrl.isNotEmpty()) rawUrl else canonicalUrl(site, remoteId)
                    displayFallback = "${site.label()} $remoteId"
                    siteLabel = source ?: site.label()
                }
                rawUrl.isNotEmpty() -> {
                    chapterUrl = resolveChapterTarget(rawUrl, seriesPrefix)
                    displayFallback = chapterUrl.fileName().substringBeforeLast('.')
                        .ifBlank { "Chapter ${index + 1}" }
                    siteLabel = source
                }
                rawId.isNotEmpty() && source != null && tryIdentifySite("", source) != null -> {
                    val resolved = identifySite("", source)
                    chapterUrl = canonicalUrl(resolved, rawId)
                    displayFallback = "${resolved.label()} $rawId"
                    siteLabel = source
                }
                rawId.isNotEmpty() -> {
                    chapterUrl = resolveChapterTarget(rawId, seriesPrefix)
                    displayFallback = chapterUrl.fileName().substringBeforeLast('.')
                        .ifBlank { "Chapter ${index + 1}" }
                    siteLabel = source
                }
                else -> {
                    chapterUrl = ""
                    displayFallback = ""
                    siteLabel = source
                }
            }
        }

        val series = isRemoteSeriesUrl(chapterUrl)
        val pageRange = pageRangeFromJson(obj)
        val chapterRange = chapterRangeFromJson(obj, series)
        if (chapterUrl.isBlank() && pages.isNullOrEmpty()) {
            val patchNumber = jsonNumber ?: overlayChapterNumber(title)
            if (patchNumber == null && pageRange == null && title.isBlank() && titles.isEmpty()) {
                return@mapIndexedNotNull null
            }
        }

        val display = when {
            series -> ""
            title.isNotBlank() -> title
            else -> displayFallback
        }
        val titleNumber = overlayChapterNumber(display)
        val explicitNumber = !series && (jsonNumber != null || titleNumber != null)
        val number = jsonNumber
            ?: titleNumber
            ?: chapterNumberOf(display).takeIf { it >= 0f }
            ?: (index + 1).toFloat()
        val date = listOf("date", "date_upload").firstNotNullOfOrNull {
            (obj?.get(it) as? JsonPrimitive)?.contentOrNull
        }?.let { parseDate(it) } ?: 0L
        val scanlator = listOf("scanlator", "group").firstNotNullOfOrNull {
            (obj?.get(it) as? JsonPrimitive)?.contentOrNull
        } ?: siteLabel

        ParsedChapter(
            title = display,
            number = number,
            url = chapterUrl,
            scanlator = scanlator,
            dateUpload = date,
            explicitNumber = explicitNumber,
            pageRange = pageRange,
            sourceNumber = if (series) 0f else number,
            chapterRange = chapterRange,
            titles = titles,
            titleLocked = !series && title.isNotBlank(),
        )
    }
}

internal fun resolveChapterTarget(raw: String, seriesPrefix: String): String {
    val trimmed = raw.trim()
    if (trimmed.startsWith("pages:") || isAbsoluteHttpUrl(trimmed)) return trimmed
    val prefix = seriesPrefix.trimEnd('/')
    val relative = trimmed.trimStart('/')
    return if (prefix.isEmpty()) relative else "$prefix/$relative"
}

internal fun isFolderChapter(url: String): Boolean = !isAbsoluteHttpUrl(url) && !url.startsWith("pages:") && url.endsWith("/")

internal fun isBucketArchive(url: String): Boolean = !isAbsoluteHttpUrl(url) && !url.startsWith("pages:") && isArchiveKey(url)

private fun parseDate(value: String): Long {
    val asLong = value.toLongOrNull()
    if (asLong != null) {
        return if (asLong < 1_000_000_000_000L) asLong * 1000 else asLong
    }
    return runCatching { java.time.Instant.parse(value).toEpochMilli() }.getOrDefault(0L)
}
