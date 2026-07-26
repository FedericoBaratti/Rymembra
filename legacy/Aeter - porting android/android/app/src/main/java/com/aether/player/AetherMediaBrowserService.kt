package com.aether.player

import android.os.Bundle
import android.support.v4.media.MediaBrowserCompat
import android.support.v4.media.MediaDescriptionCompat
import androidx.media.MediaBrowserServiceCompat
import java.util.concurrent.Executors

/**
 * Android Auto (and Assistant / system-UI) media browser. Exposes Aether's library
 * as a browse tree (Recenti, Piaciuti, Album, Artisti, Playlist) plus voice search,
 * reading the catalog snapshot written by the Node backend — so it works even when
 * the WebView/Node backend isn't running.
 *
 * It shares the ONE legacy MediaSessionCompat owned by MediaSessionPlugin (created
 * headless here if the plugin hasn't loaded yet), so transport controls, the media
 * notification and the car display all reflect the same playback. Playback itself
 * is dispatched via the session's onPlayFromMediaId/onPlayFromSearch callbacks →
 * AetherAuto → the native ExoPlayer.
 */
class AetherMediaBrowserService : MediaBrowserServiceCompat() {

    private val bg = Executors.newSingleThreadExecutor()

    override fun onCreate() {
        super.onCreate()
        sessionToken = MediaSessionPlugin.Holder.ensureSession(applicationContext).sessionToken
    }

    override fun onDestroy() {
        // Release the catalog worker thread with the service; queued loads for a
        // dead connection would only waste battery.
        bg.shutdownNow()
        super.onDestroy()
    }

    override fun onGetRoot(
        clientPackageName: String,
        clientUid: Int,
        rootHints: Bundle?
    ): BrowserRoot {
        // Media-resumption query (boot / BT reconnect / quick-settings card, and
        // Gemini "riprendi la musica"): serve the single persisted last track.
        if (rootHints?.getBoolean(BrowserRoot.EXTRA_RECENT) == true) {
            val extras = Bundle().apply { putBoolean(BrowserRoot.EXTRA_RECENT, true) }
            return BrowserRoot(RECENT_ROOT_ID, extras)
        }
        // Content-style hints give Android Auto a curated layout: a grid of album/
        // artist art, lists of tracks.
        val extras = Bundle().apply {
            putBoolean(CONTENT_STYLE_SUPPORTED, true)
            putInt(CONTENT_STYLE_BROWSABLE_HINT, CONTENT_STYLE_GRID)
            putInt(CONTENT_STYLE_PLAYABLE_HINT, CONTENT_STYLE_LIST)
        }
        // Sideload build: accept any caller (Android Auto, Assistant, system UI).
        return BrowserRoot(ROOT_ID, extras)
    }

    override fun onLoadChildren(
        parentId: String,
        result: Result<MutableList<MediaBrowserCompat.MediaItem>>
    ) {
        result.detach()
        bg.execute {
            val list = try {
                buildChildren(parentId)
            } catch (_: Exception) {
                mutableListOf()
            }
            result.sendResult(list)
        }
    }

    /**
     * Car search screen ("cerca su Aether"): grouped results — matching playlists /
     * artists / albums open as browsable entries, matching tracks play directly.
     * Group titles come from CONTENT_STYLE_GROUP_TITLE_HINT on each item.
     */
    override fun onSearch(
        query: String,
        extras: Bundle?,
        result: Result<MutableList<MediaBrowserCompat.MediaItem>>
    ) {
        result.detach()
        bg.execute {
            val out = ArrayList<MediaBrowserCompat.MediaItem>()
            try {
                AutoCatalogNative.load(applicationContext)?.let { cat ->
                    for ((prefix, group) in SEARCH_GROUPS) {
                        AutoVoice.matchNode(cat, prefix, query)?.let { n ->
                            out.add(browseItem(n.id, n.title, n.coverHash, group))
                        }
                    }
                    for (t in AutoCatalogNative.search(cat, query)) {
                        out.add(trackItem(null, t, GROUP_TRACKS))
                    }
                }
            } catch (_: Exception) {
                // best-effort search
            }
            result.sendResult(out.toMutableList())
        }
    }

    private fun buildChildren(parentId: String): MutableList<MediaBrowserCompat.MediaItem> {
        if (parentId == RECENT_ROOT_ID) {
            // Resumption root: the single "continue listening" entry, playable via
            // the RESUME_ID grammar (NativeAudioPlugin seeks to the saved position).
            val e = AutoResumeStore.load(applicationContext) ?: return mutableListOf()
            val desc = MediaDescriptionCompat.Builder()
                .setMediaId(AutoResumeStore.RESUME_ID)
                .setTitle(e.title)
                .setSubtitle(e.subtitle)
            AutoCatalogNative.coverBitmap(applicationContext, e.coverHash)?.let { desc.setIconBitmap(it) }
            return mutableListOf(
                MediaBrowserCompat.MediaItem(desc.build(), MediaBrowserCompat.MediaItem.FLAG_PLAYABLE)
            )
        }
        val cat = AutoCatalogNative.load(applicationContext) ?: return mutableListOf()
        val out = ArrayList<MediaBrowserCompat.MediaItem>()
        if (parentId == ROOT_ID) {
            for (id in cat.root) {
                val n = cat.nodes[id] ?: continue
                out.add(browseItem(n.id, n.title, n.coverHash))
            }
            return out.toMutableList()
        }
        val node = cat.nodes[parentId] ?: return mutableListOf()
        if (node.childIds.isNotEmpty()) {
            for (childId in node.childIds) {
                val c = cat.nodes[childId] ?: continue
                out.add(browseItem(c.id, c.title, c.coverHash))
            }
        } else {
            for (tid in node.trackIds) {
                val t = cat.tracks[tid] ?: continue
                out.add(trackItem(node.id, t))
            }
        }
        return out.toMutableList()
    }

    private fun browseItem(
        id: String,
        title: String,
        coverHash: String?,
        group: String? = null
    ): MediaBrowserCompat.MediaItem {
        val desc = MediaDescriptionCompat.Builder().setMediaId(id).setTitle(title)
        AutoCatalogNative.coverBitmap(applicationContext, coverHash)?.let { desc.setIconBitmap(it) }
        group?.let { desc.setExtras(Bundle().apply { putString(CONTENT_STYLE_GROUP_TITLE_HINT, it) }) }
        return MediaBrowserCompat.MediaItem(desc.build(), MediaBrowserCompat.MediaItem.FLAG_BROWSABLE)
    }

    private fun trackItem(
        nodeId: String?,
        t: AutoCatalogNative.AutoTrack,
        group: String? = null
    ): MediaBrowserCompat.MediaItem {
        // Carry the parent node so playing a track starts the whole album/playlist at
        // that point ("p#<nodeId>#track:<id>"); search hits have no node → single track.
        val mediaId = if (nodeId != null) "p#$nodeId#${t.mediaId}" else t.mediaId
        val desc = MediaDescriptionCompat.Builder()
            .setMediaId(mediaId)
            .setTitle(t.title)
            .setSubtitle(t.subtitle)
        AutoCatalogNative.coverBitmap(applicationContext, t.coverHash)?.let { desc.setIconBitmap(it) }
        group?.let { desc.setExtras(Bundle().apply { putString(CONTENT_STYLE_GROUP_TITLE_HINT, it) }) }
        return MediaBrowserCompat.MediaItem(desc.build(), MediaBrowserCompat.MediaItem.FLAG_PLAYABLE)
    }

    companion object {
        private const val ROOT_ID = "aether_root"
        private const val RECENT_ROOT_ID = "aether_recent_root"
        // Android Auto content-style extras (com.google.android.gms.car media browse).
        private const val CONTENT_STYLE_SUPPORTED = "android.media.browse.CONTENT_STYLE_SUPPORTED"
        private const val CONTENT_STYLE_BROWSABLE_HINT = "android.media.browse.CONTENT_STYLE_BROWSABLE_HINT"
        private const val CONTENT_STYLE_PLAYABLE_HINT = "android.media.browse.CONTENT_STYLE_PLAYABLE_HINT"
        private const val CONTENT_STYLE_GROUP_TITLE_HINT = "android.media.browse.CONTENT_STYLE_GROUP_TITLE_HINT"
        private const val CONTENT_STYLE_LIST = 1
        private const val CONTENT_STYLE_GRID = 2
        // Search-result sections (shown as group headers on the car screen).
        private const val GROUP_TRACKS = "Brani"
        private val SEARCH_GROUPS = listOf(
            "playlist:" to "Playlist",
            "artist:" to "Artisti",
            "album:" to "Album"
        )
    }
}
