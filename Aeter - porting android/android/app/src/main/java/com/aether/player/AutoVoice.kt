package com.aether.player

import android.content.Context
import android.os.Bundle
import android.provider.MediaStore

/**
 * Voice-query resolver for Gemini / Google Assistant playback requests
 * ("Hey Google, riproduci X su Aether"), shared by the MediaSession's
 * onPlayFromSearch/onPrepareFromSearch, the browse search grouping and the
 * phone-side App Actions entry point (VoicePlayActivity).
 *
 * Gemini's NLU structures the request into MediaStore EXTRA_MEDIA_FOCUS +
 * EXTRA_MEDIA_ARTIST/ALBUM/PLAYLIST/TITLE extras; this resolver maps that (or a
 * freeform query when unstructured) onto the Auto catalog snapshot and returns a
 * concrete playable mediaId in the grammar AutoCatalogNative.resolveTracks
 * understands — so the result survives the cold-start Intent hop unchanged:
 *
 *  - "<nodeId>"                 whole playlist / album / artist / liked / recent
 *  - "p#<nodeId>#track:<id>"    node queue starting at that track
 *  - "track:<id>"               single track
 *  - AutoResumeStore.RESUME_ID  resume the persisted last track
 *
 * Returns null when nothing in the library matches — the caller publishes a
 * spoken-back STATE_ERROR. Runs entirely on the catalog snapshot: no WebView, no
 * Node, safe on a cold process. Call off the main thread (first catalog load
 * parses the JSON file).
 */
object AutoVoice {

    /** Focus values Gemini/Assistant put in EXTRA_MEDIA_FOCUS. */
    private const val FOCUS_ARTIST = MediaStore.Audio.Artists.ENTRY_CONTENT_TYPE
    private const val FOCUS_ALBUM = MediaStore.Audio.Albums.ENTRY_CONTENT_TYPE
    private const val FOCUS_PLAYLIST = MediaStore.Audio.Playlists.ENTRY_CONTENT_TYPE
    private const val FOCUS_SONG = MediaStore.Audio.Media.ENTRY_CONTENT_TYPE

    /** Node-title aliases for the special root nodes, IT + EN ("riproduci i miei
     *  preferiti", "play my liked songs", …), matched on the folded query. */
    private val LIKED_HINTS = listOf("piaciut", "preferit", "liked", "favorit", "favourite")
    private val RECENT_HINTS = listOf("recent", "cronologia", "ultimi ascolt")

    fun resolve(ctx: Context, query: String?, extras: Bundle?): String? {
        val q = query?.trim() ?: ""

        // "Metti musica" / "riprendi" with no subject: resume the last track, else
        // fall back to recent listens, else the liked list. Never silence.
        if (q.isEmpty()) return resolveEmpty(ctx)

        val cat = AutoCatalogNative.load(ctx) ?: return null

        val artist = extras?.getString(MediaStore.EXTRA_MEDIA_ARTIST)?.trim().orEmpty()
        val album = extras?.getString(MediaStore.EXTRA_MEDIA_ALBUM)?.trim().orEmpty()
        val playlist = extras?.getString(MediaStore.EXTRA_MEDIA_PLAYLIST)?.trim().orEmpty()
        val title = extras?.getString(MediaStore.EXTRA_MEDIA_TITLE)?.trim().orEmpty()

        when (extras?.getString(MediaStore.EXTRA_MEDIA_FOCUS)) {
            FOCUS_PLAYLIST ->
                matchNode(cat, "playlist:", playlist.ifEmpty { q })?.let { return it.id }
            FOCUS_ARTIST ->
                matchNode(cat, "artist:", artist.ifEmpty { q })?.let { return it.id }
            FOCUS_ALBUM -> {
                // Disambiguate same-titled albums with the artist when provided.
                matchNode(cat, "album:", album.ifEmpty { q }, artist)?.let { return it.id }
            }
            FOCUS_SONG -> {
                val songQuery = listOf(title, artist).filter { it.isNotEmpty() }
                    .joinToString(" ").ifEmpty { q }
                trackHit(cat, songQuery)?.let { return it }
            }
            // Genre focus (no genre data in the catalog) and the unstructured
            // "vnd.android.cursor.item/*" both fall through to the generic path.
        }

        // Unstructured query: special roots, then named entities (a playlist or
        // artist name is a stronger signal than a token hit inside track fields),
        // then ranked track search.
        val folded = AutoCatalogNative.fold(q)
        if (LIKED_HINTS.any { folded.contains(it) } && cat.nodes.containsKey("liked")) return "liked"
        if (RECENT_HINTS.any { folded.contains(it) } && cat.nodes.containsKey("recent")) return "recent"

        matchNode(cat, "playlist:", q, minScore = SCORE_EXACT)?.let { return it.id }
        matchNode(cat, "artist:", q, minScore = SCORE_EXACT)?.let { return it.id }
        matchNode(cat, "album:", q, minScore = SCORE_EXACT)?.let { return it.id }
        trackHit(cat, q)?.let { return it }
        // Weak (all-tokens) entity matches only after tracks had their chance.
        matchNode(cat, "playlist:", q)?.let { return it.id }
        matchNode(cat, "artist:", q)?.let { return it.id }
        matchNode(cat, "album:", q)?.let { return it.id }
        return null
    }

    /** Empty voice query → resume > recent > liked > null. */
    private fun resolveEmpty(ctx: Context): String? {
        if (AutoResumeStore.load(ctx) != null) return AutoResumeStore.RESUME_ID
        val cat = AutoCatalogNative.load(ctx) ?: return null
        if (cat.nodes.containsKey("recent")) return "recent"
        if (cat.nodes.containsKey("liked")) return "liked"
        return cat.root.firstOrNull()
    }

    /** Best track for a song query, queued inside its album when it has one so
     *  playback continues naturally (Google's "populate the queue" guidance). */
    private fun trackHit(cat: AutoCatalogNative.Catalog, query: String): String? {
        val t = AutoCatalogNative.search(cat, query).firstOrNull() ?: return null
        val album = AutoCatalogNative.nodeContaining(cat, t.mediaId, "album:")
        return if (album != null) "p#${album.id}#${t.mediaId}" else t.mediaId
    }

    const val SCORE_EXACT = 3
    private const val SCORE_PREFIX = 2
    private const val SCORE_TOKENS = 1

    /**
     * Best node whose id starts with [prefix] matching [name] on the folded
     * title: exact > prefix > all-tokens-contained. [secondary] (e.g. the artist
     * for an album query) breaks ties when it appears in the node's tracks.
     * [minScore] raises the bar for the unstructured pass (exact-only there, so
     * a stray token can't hijack a song query into a whole album).
     */
    fun matchNode(
        cat: AutoCatalogNative.Catalog,
        prefix: String,
        name: String,
        secondary: String = "",
        minScore: Int = SCORE_TOKENS
    ): AutoCatalogNative.AutoNode? {
        val folded = AutoCatalogNative.fold(name).trim()
        if (folded.isEmpty()) return null
        val tokens = folded.split(' ').filter { it.isNotEmpty() }
        val secondaryFold = AutoCatalogNative.fold(secondary).trim()
        var best: AutoCatalogNative.AutoNode? = null
        var bestScore = minScore - 1
        for (n in cat.nodes.values) {
            if (!n.id.startsWith(prefix)) continue
            var score = when {
                n.fold == folded -> SCORE_EXACT
                n.fold.startsWith(folded) -> SCORE_PREFIX
                tokens.all { n.fold.contains(it) } -> SCORE_TOKENS
                else -> continue
            }
            if (secondaryFold.isNotEmpty() &&
                n.trackIds.firstOrNull()?.let { cat.tracks[it]?.fold?.contains(secondaryFold) } == true
            ) {
                score += 1
            }
            // Prefer the shorter title on equal score (closest to the spoken name).
            val prev = best
            if (score > bestScore || (prev != null && score == bestScore && n.title.length < prev.title.length)) {
                best = n
                bestScore = score
            }
        }
        return best
    }
}
