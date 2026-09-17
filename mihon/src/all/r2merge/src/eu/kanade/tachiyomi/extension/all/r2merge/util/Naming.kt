package eu.kanade.tachiyomi.extension.all.r2merge.util

private val IMAGE_EXTENSIONS = setOf(
    "jpg", "jpeg", "png", "webp", "gif", "avif", "bmp", "jxl", "heif", "heic",
)

private val ARCHIVE_EXTENSIONS = setOf("cbz", "zip")

/** Archive formats a pure-HTTP source cannot open, kept apart so we can say why. */
private val UNSUPPORTED_ARCHIVE_EXTENSIONS = setOf("cbr", "rar", "cb7", "7z", "cbt", "tar", "pdf", "epub")

fun String.fileExtension(): String = substringAfterLast('.', "").lowercase()

fun String.fileName(): String = trimEnd('/').substringAfterLast('/')

fun isImageKey(key: String): Boolean = !isHiddenKey(key) && key.fileExtension() in IMAGE_EXTENSIONS

fun isArchiveKey(key: String): Boolean = !isHiddenKey(key) && key.fileExtension() in ARCHIVE_EXTENSIONS

fun isUnsupportedArchiveKey(key: String): Boolean = !isHiddenKey(key) && key.fileExtension() in UNSUPPORTED_ARCHIVE_EXTENSIONS

/** Skips dotfiles and the junk macOS and Windows leave inside uploads. */
fun isHiddenKey(key: String): Boolean {
    val name = key.fileName()
    return name.startsWith(".") ||
        name.equals("Thumbs.db", true) ||
        key.contains("__MACOSX/")
}

fun mediaTypeForName(name: String): String = when (name.fileExtension()) {
    "png" -> "image/png"
    "webp" -> "image/webp"
    "gif" -> "image/gif"
    "avif" -> "image/avif"
    "bmp" -> "image/bmp"
    "jxl" -> "image/jxl"
    "heif", "heic" -> "image/heif"
    else -> "image/jpeg"
}

/**
 * Orders strings the way a human reads them, so `Chapter 2` sorts before
 * `Chapter 10` instead of after it.
 */
object NaturalOrder : Comparator<String> {
    override fun compare(a: String, b: String): Int {
        var i = 0
        var j = 0
        while (i < a.length && j < b.length) {
            val ca = a[i]
            val cb = b[j]
            if (ca.isDigit() && cb.isDigit()) {
                // Skip leading zeros so 007 and 7 compare equal; a run of only
                // zeros collapses onto its last digit rather than vanishing.
                var si = i
                while (si < a.length - 1 && a[si] == '0' && a[si + 1].isDigit()) si++
                var sj = j
                while (sj < b.length - 1 && b[sj] == '0' && b[sj + 1].isDigit()) sj++

                var ei = si
                while (ei < a.length && a[ei].isDigit()) ei++
                var ej = sj
                while (ej < b.length && b[ej].isDigit()) ej++

                val lenA = ei - si
                val lenB = ej - sj
                if (lenA != lenB) return lenA - lenB
                for (k in 0 until lenA) {
                    val diff = a[si + k] - b[sj + k]
                    if (diff != 0) return diff
                }
                i = ei
                j = ej
            } else {
                val diff = ca.lowercaseChar().compareTo(cb.lowercaseChar())
                if (diff != 0) return diff
                i++
                j++
            }
        }
        return (a.length - i) - (b.length - j)
    }
}

private val VOLUME_TOKEN = Regex("""\b(?:v|vol|volume)[\s._-]*\d+(?:\.\d+)?\b""", RegexOption.IGNORE_CASE)
private val CHAPTER_TOKEN = Regex("""\b(?:ch|chap|chapter|episode|ep|#)[\s._-]*(\d+(?:\.\d+)?)""", RegexOption.IGNORE_CASE)
private val STANDALONE_NUMBER = Regex("""(?:^|[\s._\-\[(])(\d+(?:\.\d+)?)(?=$|[\s._\-\])])""")
private val ANY_NUMBER = Regex("""(\d+(?:\.\d+)?)""")
private val LEADING_CHAPTER = Regex(
    """^\s*(\d+(?:\.\d+)?)(?!\d)(?:\s*[.。．\-–—:)|]|\s+|$)""",
)
private val SITE_INDEX = Regex(
    """(?:^|[\n\r>|])\s*(\d+(?:\.\d+)?)\s*[.。．]\s+\S""",
)
private val PART_TOKEN = Regex("""\bpart\b""", RegexOption.IGNORE_CASE)

/**
 * Madara/APC list titles look like `0.2 . The Snap - OLD - Chapter 2`.
 * Prefer that leading index over a later `Chapter 2` token.
 */
fun leadingChapterNumber(rawName: String): Float? {
    val name = rawName.trim()
    if (name.isEmpty()) return null
    return LEADING_CHAPTER.find(name)?.groupValues?.get(1)?.toFloatOrNull()?.takeIf { it >= 0f }
}

/**
 * Site list index from a Madara row: `1.6. Title`, or the same token after a
 * thumbnail caption. `Chapter 1 Part 7` is not an index.
 */
fun siteListIndex(rawName: String): Float? {
    val name = rawName.trim()
    if (name.isEmpty()) return null
    leadingChapterNumber(name)?.let { return it }
    name.lineSequence().forEach { line ->
        leadingChapterNumber(line)?.let { return it }
    }
    return SITE_INDEX.find(name)?.groupValues?.get(1)?.toFloatOrNull()?.takeIf { it >= 0f }
}

fun hasChapterPart(rawName: String): Boolean = PART_TOKEN.containsMatchIn(rawName)

/**
 * Chapter number taken only from `Chapter 4` / `Ch.4` / `Ep 4` style tokens.
 * Used as the overlay key so gallery ids like `nhentai 289857` are not treated
 * as "replace chapter 289857".
 */
fun overlayChapterNumber(rawName: String): Float? {
    val name = rawName.trimEnd('/').substringAfterLast('/')
    return CHAPTER_TOKEN.find(name)?.groupValues?.get(1)?.toFloatOrNull()
}

/**
 * Best-effort chapter number for a folder or archive name. Returns -1 when the
 * name carries no number at all, which the app treats as "unknown".
 */
fun chapterNumberOf(rawName: String, seriesTitle: String? = null): Float {
    var name = rawName.trimEnd('/').substringAfterLast('/')
    // Drop the archive suffix but leave folder names alone.
    if (isArchiveKey(name)) name = name.substringBeforeLast('.')

    if (!seriesTitle.isNullOrBlank()) {
        name = name.replace(seriesTitle, " ", ignoreCase = true)
    }
    // A volume marker would otherwise be mistaken for the chapter number.
    name = VOLUME_TOKEN.replace(name, " ")

    CHAPTER_TOKEN.find(name)?.let { return it.groupValues[1].toFloatOrNull() ?: -1f }
    STANDALONE_NUMBER.find(name)?.let { return it.groupValues[1].toFloatOrNull() ?: -1f }
    ANY_NUMBER.find(name)?.let { return it.groupValues[1].toFloatOrNull() ?: -1f }
    return -1f
}
