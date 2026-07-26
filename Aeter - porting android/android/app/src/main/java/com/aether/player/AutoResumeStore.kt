package com.aether.player

import android.content.Context

/**
 * Persists the last locally-played library track (id + position) so playback can
 * resume with no WebView/Node running:
 *
 *  - Android's media-resumption card ("continue listening" in the car / quick
 *    settings, after reboot or BT reconnect) — AetherMediaBrowserService serves it
 *    when the system asks for the EXTRA_RECENT root;
 *  - Gemini/Assistant "riprendi la musica" with an empty/resume voice query
 *    (AutoVoice) and the session's onPrepare/onPlay on a cold session.
 *
 * Written by NativeAudioPlugin on track transitions (any queue: the Auto catalog
 * uses "track:<id>" mediaIds directly, JS queues are mapped back through the
 * loopback /media/<id> URL) and throttled position saves. Remote items (podcast
 * streams, LAN thin-client) are skipped — they aren't in the catalog snapshot, so
 * the last local track stays resumable instead.
 *
 * The special mediaId [RESUME_ID] is resolved by NativeAudioPlugin.playAuto into
 * the stored track (with its browse context when it came from the catalog) seeked
 * to the stored position.
 */
object AutoResumeStore {

    /** Catalog-independent mediaId meaning "resume the persisted entry". */
    const val RESUME_ID = "resume"

    private const val PREFS = "aether_auto_resume"

    data class Entry(
        /** Catalog track id ("track:<dbId>"). */
        val catalogId: String,
        /** Browse context ("album:<key>", "playlist:<id>", "recent", …) when the
         *  play originated from the Auto catalog; null for JS-queue playback. */
        val contextNodeId: String?,
        val title: String,
        val subtitle: String,
        val coverHash: String?,
        val positionMs: Long
    )

    fun save(ctx: Context, entry: Entry) {
        ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit()
            .putString("catalogId", entry.catalogId)
            .putString("contextNodeId", entry.contextNodeId)
            .putString("title", entry.title)
            .putString("subtitle", entry.subtitle)
            .putString("coverHash", entry.coverHash)
            .putLong("positionMs", entry.positionMs)
            .apply()
    }

    /** Cheap position-only update for the currently-persisted track. */
    fun updatePosition(ctx: Context, catalogId: String, positionMs: Long) {
        val p = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        if (p.getString("catalogId", null) != catalogId) return
        p.edit().putLong("positionMs", positionMs).apply()
    }

    fun load(ctx: Context): Entry? {
        val p = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val catalogId = p.getString("catalogId", null) ?: return null
        return Entry(
            catalogId = catalogId,
            contextNodeId = p.getString("contextNodeId", null),
            title = p.getString("title", "") ?: "",
            subtitle = p.getString("subtitle", "") ?: "",
            coverHash = p.getString("coverHash", null),
            positionMs = p.getLong("positionMs", 0L)
        )
    }
}
