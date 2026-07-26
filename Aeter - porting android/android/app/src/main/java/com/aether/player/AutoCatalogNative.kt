package com.aether.player

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.util.LruCache
import org.json.JSONArray
import org.json.JSONObject
import java.io.File
import java.text.Normalizer

/**
 * Native reader for the Android Auto browse-catalog snapshot that the Node backend
 * writes (electron/modules/auto/autoCatalog.ts) to <filesDir>/auto/catalog.json.
 *
 * The car can bind the MediaBrowserService with the WebView/Node backend NOT
 * running, so browse + playback here go through this file, never through the DB
 * (which lives inside nodejs-mobile). app.getPath('userData') maps to
 * context.filesDir on Android (node-backend/runtime.ts), so covers live at
 * <filesDir>/covers/<hash>.webp and the catalog at <filesDir>/auto/catalog.json.
 *
 * The parsed catalog is cached in memory and only re-read when the file's
 * mtime/length changes. Cover bitmaps are downscaled (≤320px) and LRU-cached.
 */
object AutoCatalogNative {

    data class AutoTrack(
        val mediaId: String,
        val title: String,
        val subtitle: String,
        val path: String,
        val durationMs: Long,
        val coverHash: String?,
        val replayGain: Float,
        val fold: String,
        /** Folded title alone, so voice matches can rank title hits above
         *  artist/album hits (the snapshot's fold field mixes all three). */
        val titleFold: String
    )

    data class AutoNode(
        val id: String,
        val title: String,
        val coverHash: String?,
        val childIds: List<String>,
        val trackIds: List<String>,
        /** Folded title for voice/browse entity matching (playlist/artist/album
         *  names) — computed at parse, the snapshot schema is unchanged. */
        val fold: String
    )

    class Catalog(
        val root: List<String>,
        val nodes: Map<String, AutoNode>,
        val tracks: Map<String, AutoTrack>
    )

    /** A resolved play request: the queue to load and where to start. */
    data class Resolved(val tracks: List<AutoTrack>, val startIndex: Int)

    private const val MAX_COVER_PX = 320
    private const val SEARCH_LIMIT = 60
    private const val SEARCH_PREFIX = "search#"
    private const val NODE_PLAY_PREFIX = "p#"

    @Volatile private var cached: Catalog? = null
    @Volatile private var cachedMtime: Long = -1L
    @Volatile private var cachedLen: Long = -1L

    private val coverCache = object : LruCache<String, Bitmap>(8 * 1024 * 1024) {
        override fun sizeOf(key: String, value: Bitmap): Int = value.byteCount
    }

    private fun catalogFile(ctx: Context): File = File(File(ctx.filesDir, "auto"), "catalog.json")
    private fun coverFile(ctx: Context, hash: String): File =
        File(File(ctx.filesDir, "covers"), "$hash.webp")

    /** Load (or return the cached) catalog. Null when the snapshot doesn't exist yet. */
    fun load(ctx: Context): Catalog? {
        val f = catalogFile(ctx)
        if (!f.exists()) return null
        val mtime = f.lastModified()
        val len = f.length()
        val c = cached
        if (c != null && mtime == cachedMtime && len == cachedLen) return c
        return try {
            val parsed = parse(f.readText(Charsets.UTF_8))
            cached = parsed
            cachedMtime = mtime
            cachedLen = len
            parsed
        } catch (_: Exception) {
            null
        }
    }

    private fun parse(json: String): Catalog {
        val o = JSONObject(json)

        val root = strList(o.optJSONArray("root"))

        val nodes = HashMap<String, AutoNode>()
        o.optJSONObject("nodes")?.let { nodesObj ->
            val it = nodesObj.keys()
            while (it.hasNext()) {
                val id = it.next()
                val n = nodesObj.getJSONObject(id)
                val title = n.optString("title", "")
                nodes[id] = AutoNode(
                    id = id,
                    title = title,
                    coverHash = optHash(n, "coverHash"),
                    childIds = strList(n.optJSONArray("children")),
                    trackIds = strList(n.optJSONArray("trackIds")),
                    fold = fold(title)
                )
            }
        }

        val tracks = HashMap<String, AutoTrack>()
        o.optJSONObject("tracks")?.let { tracksObj ->
            val it = tracksObj.keys()
            while (it.hasNext()) {
                val id = it.next()
                val t = tracksObj.getJSONObject(id)
                val title = t.optString("title", "")
                tracks[id] = AutoTrack(
                    mediaId = id,
                    title = title,
                    subtitle = t.optString("subtitle", ""),
                    path = t.optString("path", ""),
                    durationMs = t.optLong("durationMs", 0L),
                    coverHash = optHash(t, "coverHash"),
                    replayGain = t.optDouble("replayGain", 1.0).toFloat(),
                    fold = t.optString("fold", ""),
                    titleFold = fold(title)
                )
            }
        }

        return Catalog(root, nodes, tracks)
    }

    private fun optHash(o: JSONObject, key: String): String? {
        if (!o.has(key) || o.isNull(key)) return null
        val s = o.optString(key, "")
        return if (s.isEmpty()) null else s
    }

    private fun strList(arr: JSONArray?): List<String> {
        if (arr == null) return emptyList()
        val out = ArrayList<String>(arr.length())
        for (i in 0 until arr.length()) out.add(arr.getString(i))
        return out
    }

    /**
     * Resolve a play mediaId into a queue + start index:
     *  - "search#<query>"          → voice/search results
     *  - "p#<nodeId>#track:<id>"    → the node's list, starting on that track
     *  - "track:<id>"              → that single track
     *  - "<nodeId>"                → the whole node list (album/playlist/liked/recent)
     */
    fun resolveTracks(cat: Catalog, mediaId: String): Resolved {
        if (mediaId.startsWith(SEARCH_PREFIX)) {
            return Resolved(search(cat, mediaId.substring(SEARCH_PREFIX.length)), 0)
        }
        if (mediaId.startsWith(NODE_PLAY_PREFIX)) {
            val body = mediaId.substring(NODE_PLAY_PREFIX.length)
            // trackId ("track:<n>") never contains '#', so the LAST '#' splits it off
            // regardless of any '#' inside the node id (artist names, album keys).
            val cut = body.lastIndexOf('#')
            if (cut > 0) {
                val nodeId = body.substring(0, cut)
                val trackId = body.substring(cut + 1)
                val node = cat.nodes[nodeId]
                if (node != null && node.trackIds.isNotEmpty()) {
                    val list = node.trackIds.mapNotNull { cat.tracks[it] }
                    val idx = list.indexOfFirst { it.mediaId == trackId }.let { if (it < 0) 0 else it }
                    return Resolved(list, idx)
                }
                cat.tracks[trackId]?.let { return Resolved(listOf(it), 0) }
            }
        }
        if (mediaId.startsWith("track:")) {
            cat.tracks[mediaId]?.let { return Resolved(listOf(it), 0) }
        }
        val node = cat.nodes[mediaId]
        if (node != null) {
            val list = flattenTracks(cat, node)
            if (list.isNotEmpty()) return Resolved(list, 0)
        }
        return Resolved(emptyList(), 0)
    }

    /**
     * All playable tracks under a node, in browse order. Children-only nodes
     * (an artist = list of albums) flatten one level down so "riproduci <artista>"
     * plays the whole discography instead of nothing.
     */
    fun flattenTracks(cat: Catalog, node: AutoNode): List<AutoTrack> {
        if (node.trackIds.isNotEmpty()) return node.trackIds.mapNotNull { cat.tracks[it] }
        if (node.childIds.isEmpty()) return emptyList()
        val out = ArrayList<AutoTrack>()
        for (childId in node.childIds) {
            val child = cat.nodes[childId] ?: continue
            for (tid in child.trackIds) cat.tracks[tid]?.let { out.add(it) }
        }
        return out
    }

    /** Accent-insensitive track search — all query tokens must appear in a track's
     *  folded "title artist album" field (built by foldText in the snapshot).
     *  Results are ranked: exact title > title contains all tokens > anywhere, so
     *  a Gemini/Assistant song query lands on the intended track first. */
    fun search(cat: Catalog, rawQuery: String): List<AutoTrack> {
        val folded = fold(rawQuery).trim()
        val tokens = folded.split(' ').filter { it.isNotEmpty() }
        if (tokens.isEmpty()) return emptyList()
        val scored = ArrayList<Pair<Int, AutoTrack>>()
        for (t in cat.tracks.values) {
            if (!tokens.all { t.fold.contains(it) }) continue
            val score = when {
                t.titleFold == folded -> 3
                tokens.all { t.titleFold.contains(it) } -> 2
                else -> 1
            }
            scored.add(score to t)
        }
        scored.sortByDescending { it.first }
        return scored.take(SEARCH_LIMIT).map { it.second }
    }

    /** First node with the given id prefix that contains [trackId] — used to give
     *  a voice-matched song its album/playlist as playback context and queue. */
    fun nodeContaining(cat: Catalog, trackId: String, prefix: String): AutoNode? {
        for (n in cat.nodes.values) {
            if (n.id.startsWith(prefix) && n.trackIds.contains(trackId)) return n
        }
        return null
    }

    /** Diacritic-insensitive fold — mirrors foldText() in shared/text.ts. */
    fun fold(s: String): String {
        val n = Normalizer.normalize(s, Normalizer.Form.NFD)
        val sb = StringBuilder(n.length)
        for (ch in n) {
            val c = ch.code
            if (c in 0x0300..0x036f) continue
            sb.append(ch)
        }
        return sb.toString().lowercase()
    }

    /** file:// Uri of a cover for ExoPlayer artwork metadata, or null if absent. */
    fun coverFileUri(ctx: Context, hash: String?): Uri? {
        if (hash.isNullOrEmpty()) return null
        val f = coverFile(ctx, hash)
        return if (f.exists()) Uri.fromFile(f) else null
    }

    /** Downscaled, LRU-cached cover bitmap for a browse MediaItem icon, or null. */
    fun coverBitmap(ctx: Context, hash: String?): Bitmap? {
        if (hash.isNullOrEmpty()) return null
        coverCache.get(hash)?.let { return it }
        val f = coverFile(ctx, hash)
        if (!f.exists()) return null
        return try {
            val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
            BitmapFactory.decodeFile(f.absolutePath, bounds)
            val opts = BitmapFactory.Options().apply {
                inSampleSize = sampleSize(bounds.outWidth, bounds.outHeight, MAX_COVER_PX)
            }
            val bmp = BitmapFactory.decodeFile(f.absolutePath, opts) ?: return null
            coverCache.put(hash, bmp)
            bmp
        } catch (_: Exception) {
            null
        }
    }

    private fun sampleSize(w: Int, h: Int, target: Int): Int {
        if (w <= 0 || h <= 0) return 1
        var s = 1
        val max = maxOf(w, h)
        while (max / (s * 2) >= target) s *= 2
        return s
    }
}
