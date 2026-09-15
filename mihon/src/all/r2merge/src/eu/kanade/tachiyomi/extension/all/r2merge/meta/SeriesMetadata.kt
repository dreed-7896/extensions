package eu.kanade.tachiyomi.extension.all.r2merge.meta

import eu.kanade.tachiyomi.source.model.SManga
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import org.jsoup.Jsoup
import org.jsoup.nodes.Element
import org.jsoup.parser.Parser

/**
 * Optional per-series metadata. Both supported files are hand-written by users,
 * so parsing is deliberately forgiving: an unexpected shape drops one field
 * rather than failing the whole series.
 */
data class SeriesMetadata(
    val title: String? = null,
    val author: String? = null,
    val artist: String? = null,
    val description: String? = null,
    val genre: String? = null,
    val status: Int = SManga.UNKNOWN,
    val cover: String? = null,
) {
    /** Local details.json / ComicInfo wins; remote series pages fill blanks. */
    fun overlay(remote: SeriesMetadata?): SeriesMetadata {
        if (remote == null) return this
        return copy(
            title = title ?: remote.title,
            author = author ?: remote.author,
            artist = artist ?: remote.artist,
            description = description ?: remote.description,
            genre = genre ?: remote.genre,
            status = if (status != SManga.UNKNOWN) status else remote.status,
            cover = cover ?: remote.cover,
        )
    }

    companion object {
        const val DETAILS_JSON = "details.json"
        const val COMIC_INFO_XML = "comicinfo.xml"

        private val json = Json {
            ignoreUnknownKeys = true
            isLenient = true
        }

        /** Tachiyomi's local-source `details.json`. */
        fun fromDetailsJson(raw: String): SeriesMetadata? = runCatching {
            val root = json.parseToJsonElement(raw) as? JsonObject ?: return null
            val credits = root["credits"] as? JsonArray ?: JsonArray(emptyList())
            val authors = credits.mapNotNull { credit ->
                val obj = credit as? JsonObject ?: return@mapNotNull null
                val role = obj.string("role")?.lowercase().orEmpty()
                val name = obj.string("name") ?: return@mapNotNull null
                if (role.isEmpty() || role.contains("author") || role.contains("story")) name else null
            }
            val artists = credits.mapNotNull { credit ->
                val obj = credit as? JsonObject ?: return@mapNotNull null
                val role = obj.string("role")?.lowercase().orEmpty()
                val name = obj.string("name") ?: return@mapNotNull null
                if (role.contains("art") || role.contains("artist")) name else null
            }
            SeriesMetadata(
                title = root.string("title"),
                author = root.string("author") ?: authors.joinToString().takeIf { it.isNotEmpty() },
                artist = root.string("artist") ?: (artists.ifEmpty { authors }).joinToString()
                    .takeIf { it.isNotEmpty() },
                description = root.string("description") ?: root.string("summary"),
                genre = root.stringList("genre") ?: root.stringList("genres"),
                status = statusOf(root.string("status")),
                cover = root.string("cover"),
            )
        }.getOrNull()

        /** ComicRack `ComicInfo.xml`, as written by Mihon, Komga, Kavita and friends. */
        fun fromComicInfo(raw: String): SeriesMetadata? = runCatching {
            val doc = Jsoup.parse(raw, "", Parser.xmlParser())

            val writers = listOfNotNull(doc.value("Writer"), doc.value("Penciller"), doc.value("Inker"))
            val artists = listOfNotNull(doc.value("CoverArtist"), doc.value("Penciller"), doc.value("Letterer"))
            val genres = listOfNotNull(doc.value("Genre"), doc.value("Tags"))

            SeriesMetadata(
                title = doc.value("Series") ?: doc.value("Title"),
                author = writers.firstOrNull(),
                artist = artists.firstOrNull(),
                description = doc.value("Summary"),
                genre = genres.flatMap { it.split(',') }
                    .map { it.trim() }
                    .filter { it.isNotEmpty() }
                    .distinct()
                    .joinToString(", ")
                    .takeIf { it.isNotEmpty() },
                // Mihon writes its publishing status into this custom element.
                status = statusOf(
                    doc.value("PublishingStatusTachiyomi") ?: doc.value("Status"),
                ),
            )
        }.getOrNull()

        /** Accepts both the numeric constants and the human-readable names. */
        fun statusOf(raw: String?): Int = when (raw?.trim()?.lowercase()?.replace('_', ' ')) {
            null, "", "0", "unknown" -> SManga.UNKNOWN
            "1", "ongoing", "publishing", "continuing" -> SManga.ONGOING
            "2", "completed", "complete", "finished", "ended" -> SManga.COMPLETED
            "3", "licensed" -> SManga.LICENSED
            "4", "publishing finished" -> SManga.PUBLISHING_FINISHED
            "5", "cancelled", "canceled", "abandoned" -> SManga.CANCELLED
            "6", "on hiatus", "hiatus" -> SManga.ON_HIATUS
            else -> SManga.UNKNOWN
        }

        private fun JsonObject.string(key: String): String? = (this[key] as? JsonPrimitive)?.content?.trim()?.takeIf { it.isNotEmpty() && it != "null" }

        /** `genre` may be a list or an already-joined string. */
        private fun JsonObject.stringList(key: String): String? {
            val element = this[key] ?: return null
            val values = when (element) {
                is JsonArray -> element.mapNotNull {
                    (it as? JsonPrimitive)?.content?.trim()?.takeIf(String::isNotEmpty)
                }
                is JsonPrimitive -> element.content.split(',').map { it.trim() }.filter { it.isNotEmpty() }
                else -> emptyList()
            }
            return values.distinct().joinToString(", ").takeIf { it.isNotEmpty() }
        }

        private fun Element.value(tag: String): String? = getElementsByTag(tag).firstOrNull()?.wholeText()?.trim()?.takeIf { it.isNotEmpty() }
    }
}
