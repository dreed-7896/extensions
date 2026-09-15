package eu.kanade.tachiyomi.extension.all.r2merge

import eu.kanade.tachiyomi.source.model.Page
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import java.io.IOException

/** 1-based inclusive page window. [end] null means through the last page. */
internal data class PageRange(
    val start: Int,
    val end: Int?,
) {
    fun spec(): String = when {
        end == null -> "$start-"
        start <= 1 -> end.toString()
        else -> "$start-$end"
    }
}

private val EMBEDDED_RANGE = Regex("""(?:#|&)r2p=([^&#]*)""")
private val RANGE_SPAN = Regex("""^(\d+)?-(\d+)?$""")

internal fun parsePageRangeSpec(raw: String?): PageRange? {
    val value = raw?.trim()?.replace(" ", "") ?: return null
    if (value.isEmpty()) return null
    RANGE_SPAN.matchEntire(value)?.let { match ->
        val start = match.groupValues[1].toIntOrNull()?.coerceAtLeast(1) ?: 1
        val end = match.groupValues[2].toIntOrNull()
        if (end != null && end < start) return null
        return PageRange(start, end)
    }
    val count = value.toIntOrNull() ?: return null
    if (count <= 0) return null
    return PageRange(1, count)
}

/** 1-based inclusive chapter window. [end] null means through the newest chapter. */
internal data class ChapterRange(
    val start: Float,
    val end: Float?,
) {
    fun spec(): String = when {
        end == null -> formatBound(start) + "-"
        start == end -> formatBound(start)
        else -> "${formatBound(start)}-${formatBound(end)}"
    }

    fun contains(number: Float): Boolean {
        if (number <= 0f) return false
        if (number + 1e-4f < start) return false
        val cap = end ?: return true
        return number <= cap + 1e-4f
    }

    private fun formatBound(value: Float): String {
        val asInt = value.toInt()
        return if (value == asInt.toFloat()) asInt.toString() else value.toString()
    }
}

private val CHAPTER_SPAN = Regex("""^(\d+(?:\.\d+)?)?-(\d+(?:\.\d+)?)?$""")

internal fun parseChapterRangeSpec(raw: String?): ChapterRange? {
    val value = raw?.trim()?.replace(" ", "") ?: return null
    if (value.isEmpty()) return null
    CHAPTER_SPAN.matchEntire(value)?.let { match ->
        val start = match.groupValues[1].toFloatOrNull()?.takeIf { it > 0f } ?: 1f
        val end = match.groupValues[2].toFloatOrNull()
        if (end != null && end < start) return null
        return ChapterRange(start, end)
    }
    val single = value.toFloatOrNull() ?: return null
    if (single <= 0f) return null
    return ChapterRange(single, single)
}

internal fun chapterRangeFromJson(obj: JsonObject?, seriesUrl: Boolean): ChapterRange? {
    if (obj == null) return null
    val keys = buildList {
        add("chapterRange")
        add("chaptersRange")
        add("chapter_range")
        if (seriesUrl) add("range")
    }
    keys.forEach { key ->
        val primitive = obj[key] as? JsonPrimitive ?: return@forEach
        primitive.contentOrNull?.let { parseChapterRangeSpec(it) }?.let { return it }
        primitive.intOrNull?.let { parseChapterRangeSpec(it.toString()) }?.let { return it }
    }
    return null
}

internal fun pageRangeFromJson(obj: JsonObject?): PageRange? {
    if (obj == null) return null
    listOf("pageRange", "pagerange", "pagesRange").forEach { key ->
        val primitive = obj[key] as? JsonPrimitive ?: return@forEach
        primitive.contentOrNull?.let { parsePageRangeSpec(it) }?.let { return it }
        primitive.intOrNull?.let { parsePageRangeSpec(it.toString()) }?.let { return it }
    }
    val start = intField(obj, "pageStart", "fromPage", "startPage")
    val end = intField(obj, "pageEnd", "toPage", "endPage", "pageLimit", "maxPages")
    if (start == null && end == null) return null
    val from = (start ?: 1).coerceAtLeast(1)
    val to = end
    if (to != null && to < from) return null
    return PageRange(from, to)
}

internal fun encodePageRange(url: String, range: PageRange?): String {
    val clean = splitPageRange(url).first
    if (range == null) return clean
    val marker = "r2p=${range.spec()}"
    return if ('#' in clean) "$clean&$marker" else "$clean#$marker"
}

internal fun splitPageRange(url: String): Pair<String, PageRange?> {
    val match = EMBEDDED_RANGE.find(url) ?: return url to null
    val range = parsePageRangeSpec(match.groupValues[1])
    var clean = url.removeRange(match.range)
    if (clean.endsWith("#") || clean.endsWith("&")) {
        clean = clean.dropLast(1)
    }
    return clean to range
}

internal fun applyPageRange(pages: List<Page>, range: PageRange?): List<Page> {
    if (range == null) return pages
    if (pages.isEmpty()) return pages
    val from = (range.start - 1).coerceAtLeast(0)
    if (from >= pages.size) {
        throw IOException("pageRange ${range.spec()} is past ${pages.size} page(s)")
    }
    val to = (range.end ?: pages.size).coerceAtMost(pages.size)
    if (from >= to) {
        throw IOException("pageRange ${range.spec()} is empty for ${pages.size} page(s)")
    }
    return pages.subList(from, to).mapIndexed { index, page ->
        Page(index, url = page.url, imageUrl = page.imageUrl)
    }
}

internal fun ParsedChapter.readerUrl(): String = encodePageRange(url, pageRange)

private fun intField(obj: JsonObject, vararg keys: String): Int? {
    keys.forEach { key ->
        val value = (obj[key] as? JsonPrimitive)?.contentOrNull?.toIntOrNull()
            ?: (obj[key] as? JsonPrimitive)?.intOrNull
        if (value != null) return value
    }
    return null
}
