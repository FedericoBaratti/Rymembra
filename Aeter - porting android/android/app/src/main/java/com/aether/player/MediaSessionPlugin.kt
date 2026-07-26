package com.aether.player

import android.Manifest
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.support.v4.media.MediaDescriptionCompat
import android.support.v4.media.MediaMetadataCompat
import android.support.v4.media.session.MediaSessionCompat
import android.support.v4.media.session.PlaybackStateCompat
import androidx.media3.common.Player
import com.getcapacitor.JSObject
import com.getcapacitor.PermissionState
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin
import com.getcapacitor.annotation.Permission
import com.getcapacitor.annotation.PermissionCallback
import java.net.URL

/**
 * Bridges the native ExoPlayer (NativeAudioPlugin) to an Android MediaSession so
 * the media notification, lock screen, headset buttons AND Android Auto work, with
 * playback continuing in the background via a foreground service
 * (MediaPlaybackService).
 *
 * Transport actions from the notification / lock screen / headset / car drive the
 * native ExoPlayer DIRECTLY (NativeAudioHolder.player) rather than routing through
 * the WebView: when the app is backgrounded the WebView is frozen by Chromium, so a
 * JS round-trip would make the controls dead. The resulting onIsPlayingChanged /
 * onMediaItemTransition then re-publish state + metadata here, and notify the
 * (possibly frozen) renderer for later reconcile.
 *
 * The single MediaSessionCompat is owned by [Holder] and created lazily via
 * [Holder.ensureSession] so it can exist headless — AetherMediaBrowserService binds
 * it before the WebView/plugin has loaded, and the Auto play-from callbacks route to
 * [AetherAuto]. When the plugin IS loaded it registers a JS emitter (Holder.emitter)
 * so legacy "stop"/transport events still reach the renderer (src/lib/mediaSession.ts).
 */
@CapacitorPlugin(
    name = "MediaSession",
    permissions = [
        Permission(strings = [Manifest.permission.POST_NOTIFICATIONS], alias = "notifications")
    ]
)
class MediaSessionPlugin : Plugin() {

    private var durationMs: Long = 0
    // Ask for POST_NOTIFICATIONS at most once per process; granting later (via
    // Settings) is picked up by the getPermissionState check below.
    private var notifPermRequested = false

    override fun load() {
        // Route optional transport events back to JS (only while the WebView is alive).
        Holder.emitter = { action, value ->
            val data = JSObject().put("action", action)
            if (value != null) data.put("value", value)
            notifyListeners("transport", data)
        }
        // Create the shared session now so the notification/lock-screen work even if
        // the car never connects; ensureSession is idempotent with the browser service.
        Holder.ensureSession(context)
    }

    @PluginMethod
    fun setMetadata(call: PluginCall) {
        durationMs = ((call.getDouble("duration") ?: 0.0) * 1000).toLong()
        publishMetadata(
            context,
            call.getString("title") ?: "",
            call.getString("artist") ?: "",
            call.getString("album") ?: "",
            call.getString("artworkUrl") ?: "",
            durationMs
        )
        call.resolve()
    }

    @PluginMethod
    fun setPlaybackState(call: PluginCall) {
        val playing = call.getBoolean("playing") ?: false
        val positionMs = ((call.getDouble("position") ?: 0.0) * 1000).toLong()
        // publishState drives the foreground-service lifecycle (promote on play,
        // demote after a long pause) — no unconditional start here, or a paused
        // app could never be demoted.
        publishState(playing, positionMs)

        // Android 13+ requires the POST_NOTIFICATIONS runtime grant for the
        // foreground-service media notification (lock screen / shade controls) to
        // appear. Ask once, lazily, the first time playback starts — the prompt
        // then shows exactly when the user begins listening.
        if (playing
            && Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
            && getPermissionState("notifications") != PermissionState.GRANTED
            && !notifPermRequested
        ) {
            notifPermRequested = true
            requestPermissionForAlias("notifications", call, "afterNotifPerm")
            return
        }
        call.resolve()
    }

    @PermissionCallback
    private fun afterNotifPerm(call: PluginCall) {
        // Re-issue regardless of the outcome so the notification appears now that
        // the grant may exist; when denied the service still runs (background
        // audio keeps going), only the visible notification is suppressed.
        ensureForegroundService(context)
        call.resolve()
    }

    @PluginMethod
    fun stop(call: PluginCall) {
        cancelDemote()
        Holder.session?.setPlaybackState(
            PlaybackStateCompat.Builder()
                .setState(PlaybackStateCompat.STATE_STOPPED, 0, 1f)
                .build()
        )
        context.stopService(Intent(context, MediaPlaybackService::class.java))
        call.resolve()
    }

    override fun handleOnDestroy() {
        // Keep the shared session alive for the process lifetime — Android Auto may
        // still be browsing headless after the Activity/WebView is torn down. Only
        // drop the JS emitter bridge tied to this (now dead) WebView.
        Holder.emitter = null
    }

    /**
     * Owns the single MediaSessionCompat shared by the plugin, the foreground
     * service and the Android Auto browser service. [ensureSession] creates it lazily
     * so it can exist without a live WebView/plugin (headless car browse). [emitter]
     * forwards legacy transport events to JS and is null when the WebView is gone.
     */
    object Holder {
        @Volatile
        var session: MediaSessionCompat? = null

        @Volatile
        var emitter: ((action: String, value: Double?) -> Unit)? = null

        /** Application context captured by ensureSession so the static
         *  publishState can drive the foreground service without a plugin. */
        @Volatile
        var appContext: Context? = null

        fun ensureSession(ctx: Context): MediaSessionCompat {
            appContext = ctx.applicationContext
            session?.let { return it }
            synchronized(this) {
                session?.let { return it }
                val s = MediaSessionCompat(ctx.applicationContext, "AetherMediaSession")
                s.setCallback(AetherSessionCallback(ctx.applicationContext))
                s.isActive = true
                session = s
                return s
            }
        }
    }

    /** One session-queue row (car display "coda di riproduzione"). */
    data class QueueEntry(val mediaId: String, val title: String, val subtitle: String)

    /**
     * Static publishers + helpers so NativeAudioPlugin can update the media
     * notification directly (natively) on track transitions and play/pause, without
     * a WebView round-trip — the renderer may be frozen in the background.
     */
    companion object {
        /** How long playback may sit paused before the foreground service is
         *  demoted and the process becomes a freezable cached app. Long enough
         *  that a normal "pause, do something, resume" never hits it. */
        private const val PAUSE_DEMOTE_GRACE_MS = 10 * 60_000L

        private val demoteHandler = Handler(Looper.getMainLooper())
        private val demoteRunnable = Runnable { MediaPlaybackService.demote() }

        private fun scheduleDemote() {
            demoteHandler.removeCallbacks(demoteRunnable)
            demoteHandler.postDelayed(demoteRunnable, PAUSE_DEMOTE_GRACE_MS)
        }

        fun cancelDemote() {
            demoteHandler.removeCallbacks(demoteRunnable)
        }

        private fun stateActions(): Long =
            PlaybackStateCompat.ACTION_PLAY or
                PlaybackStateCompat.ACTION_PAUSE or
                PlaybackStateCompat.ACTION_PLAY_PAUSE or
                PlaybackStateCompat.ACTION_SKIP_TO_NEXT or
                PlaybackStateCompat.ACTION_SKIP_TO_PREVIOUS or
                PlaybackStateCompat.ACTION_SEEK_TO or
                PlaybackStateCompat.ACTION_STOP or
                PlaybackStateCompat.ACTION_PLAY_FROM_MEDIA_ID or
                PlaybackStateCompat.ACTION_PLAY_FROM_SEARCH or
                PlaybackStateCompat.ACTION_PREPARE_FROM_MEDIA_ID or
                PlaybackStateCompat.ACTION_PREPARE or
                PlaybackStateCompat.ACTION_PREPARE_FROM_SEARCH or
                PlaybackStateCompat.ACTION_SKIP_TO_QUEUE_ITEM or
                PlaybackStateCompat.ACTION_SET_SHUFFLE_MODE or
                PlaybackStateCompat.ACTION_SET_REPEAT_MODE

        /** Run the foreground service while there is anything to control, so the
         *  native player keeps running in the background. */
        fun ensureForegroundService(ctx: Context) {
            val intent = Intent(ctx, MediaPlaybackService::class.java)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                ctx.startForegroundService(intent)
            } else {
                ctx.startService(intent)
            }
        }

        /** Publish playing/paused + position; the system extrapolates the scrubber
         *  from here using the 1.0 playback speed, so per-second updates aren't
         *  needed while the WebView is frozen.
         *
         *  Also the single driver of the foreground-service lifecycle: playing
         *  promotes (startForegroundService is legal here — every play originates
         *  from a media-button / notification / UI interaction, all exempt from
         *  the Android 12+ background-start restriction), a sustained pause
         *  demotes after [PAUSE_DEMOTE_GRACE_MS] so the process stops being
         *  pinned in memory just because a track is paused. */
        fun publishState(playing: Boolean, positionMs: Long) {
            val session = Holder.session ?: return
            session.setPlaybackState(
                PlaybackStateCompat.Builder()
                    .setActions(stateActions())
                    .setActiveQueueItemId(activeQueueItemId)
                    .setState(
                        if (playing) PlaybackStateCompat.STATE_PLAYING else PlaybackStateCompat.STATE_PAUSED,
                        positionMs,
                        1f
                    )
                    .build()
            )
            if (playing) {
                cancelDemote()
                Holder.appContext?.let { ensureForegroundService(it) }
            } else {
                scheduleDemote()
            }
        }

        /**
         * Voice feedback for a failed play-from-search: Gemini/Assistant read the
         * error message aloud ("Nessun risultato…") instead of failing silently.
         * Transient — the next publishState (any successful play) replaces it.
         */
        fun publishError(message: String) {
            Holder.session?.setPlaybackState(
                PlaybackStateCompat.Builder()
                    .setActions(stateActions())
                    .setState(PlaybackStateCompat.STATE_ERROR, 0, 1f)
                    .setErrorMessage(PlaybackStateCompat.ERROR_CODE_APP_ERROR, message)
                    .build()
            )
        }

        /** Android Auto's queue view can't page, and giant queues risk the binder
         *  transaction limit — publish at most this many rows. */
        private const val QUEUE_PUBLISH_LIMIT = 300

        /** Mirror the ExoPlayer queue into the session so the car shows "up next".
         *  QueueItem ids are the player indices, which onSkipToQueueItem seeks to. */
        fun publishQueue(entries: List<QueueEntry>) {
            val session = Holder.session ?: return
            val items = ArrayList<MediaSessionCompat.QueueItem>(minOf(entries.size, QUEUE_PUBLISH_LIMIT))
            for ((i, e) in entries.withIndex()) {
                if (i >= QUEUE_PUBLISH_LIMIT) break
                val desc = MediaDescriptionCompat.Builder()
                    .setMediaId(e.mediaId)
                    .setTitle(e.title)
                    .setSubtitle(e.subtitle)
                    .build()
                items.add(MediaSessionCompat.QueueItem(desc, i.toLong()))
            }
            session.setQueue(items)
        }

        /** Active row id (= player index, see publishQueue). The framework carries
         *  it inside PlaybackState, so it's stored here and stamped onto every
         *  publishState — call BEFORE publishState on a transition. */
        @Volatile
        private var activeQueueItemId = -1L

        fun setActiveQueueItem(index: Int) {
            activeQueueItemId = index.toLong()
        }

        /** Publish track metadata; artwork loads from the URL (local media server
         *  http://127.0.0.1 or a file:// cover) on a background thread and
         *  re-publishes when ready. */
        fun publishMetadata(
            ctx: Context,
            title: String,
            artist: String,
            album: String,
            artworkUrl: String,
            durationMs: Long,
            mediaId: String = ""
        ) {
            val session = Holder.session ?: return
            val builder = MediaMetadataCompat.Builder()
                .putString(MediaMetadataCompat.METADATA_KEY_TITLE, title)
                .putString(MediaMetadataCompat.METADATA_KEY_ARTIST, artist)
                .putString(MediaMetadataCompat.METADATA_KEY_ALBUM, album)
                .putLong(MediaMetadataCompat.METADATA_KEY_DURATION, durationMs)
            if (mediaId.isNotEmpty()) {
                builder.putString(MediaMetadataCompat.METADATA_KEY_MEDIA_ID, mediaId)
            }
            session.setMetadata(builder.build())
            if (artworkUrl.isNotEmpty()) {
                Thread {
                    try {
                        val bmp: Bitmap? =
                            URL(artworkUrl).openStream().use { BitmapFactory.decodeStream(it) }
                        if (bmp != null) {
                            builder.putBitmap(MediaMetadataCompat.METADATA_KEY_ALBUM_ART, bmp)
                            session.setMetadata(builder.build())
                            MediaPlaybackService.refresh(ctx)
                        }
                    } catch (_: Exception) {
                        /* artwork is best-effort */
                    }
                }.start()
            }
        }
    }
}

/**
 * MediaSession transport callback. Drives the native ExoPlayer directly so the
 * controls work even while the WebView (and its JS player store) is frozen in the
 * background; falls back to the optional JS emitter when the player isn't up yet.
 * The play-from callbacks power Android Auto browse + voice playback via AetherAuto.
 */
private class AetherSessionCallback(
    private val appContext: Context
) : MediaSessionCompat.Callback() {

    private fun emit(action: String, value: Double? = null) {
        MediaSessionPlugin.Holder.emitter?.invoke(action, value)
    }

    override fun onPlay() {
        val p = NativeAudioHolder.player
        if (p != null) p.play() else emit("play")
    }

    override fun onPause() {
        val p = NativeAudioHolder.player
        if (p != null) p.pause() else emit("pause")
    }

    override fun onSkipToNext() {
        val p = NativeAudioHolder.player
        if (p != null) p.seekToNext() else emit("next")
    }

    override fun onSkipToPrevious() {
        // ExoPlayer's seekToPrevious() restarts the current item when the position is
        // past maxSeekToPreviousPositionMs (3s default), else moves to the previous
        // item — exactly the desired UX.
        val p = NativeAudioHolder.player
        if (p != null) p.seekToPrevious() else emit("previous")
    }

    override fun onStop() {
        NativeAudioHolder.player?.stop()
        emit("stop")
    }

    override fun onSeekTo(pos: Long) {
        val p = NativeAudioHolder.player
        if (p != null) p.seekTo(pos) else emit("seek", pos.toDouble())
    }

    override fun onSkipToQueueItem(id: Long) {
        // QueueItem ids are ExoPlayer indices (MediaSessionPlugin.publishQueue).
        val p = NativeAudioHolder.player ?: return
        if (id in 0 until p.mediaItemCount) {
            p.seekTo(id.toInt(), 0L)
            p.play()
        }
    }

    override fun onSetShuffleMode(shuffleMode: Int) {
        // Native-first (the WebView is typically frozen while driving): ExoPlayer
        // owns the queue, so its shuffle order is what actually plays. The JS
        // store keeps itself consistent by reconciling transitions by mediaId;
        // the emit lets a live renderer mirror the flag in the UI if it wants.
        val on = shuffleMode != PlaybackStateCompat.SHUFFLE_MODE_NONE
        NativeAudioHolder.player?.shuffleModeEnabled = on
        MediaSessionPlugin.Holder.session?.setShuffleMode(shuffleMode)
        emit("shuffle", if (on) 1.0 else 0.0)
    }

    override fun onSetRepeatMode(repeatMode: Int) {
        NativeAudioHolder.player?.repeatMode = when (repeatMode) {
            PlaybackStateCompat.REPEAT_MODE_ONE -> Player.REPEAT_MODE_ONE
            PlaybackStateCompat.REPEAT_MODE_ALL,
            PlaybackStateCompat.REPEAT_MODE_GROUP -> Player.REPEAT_MODE_ALL
            else -> Player.REPEAT_MODE_OFF
        }
        MediaSessionPlugin.Holder.session?.setRepeatMode(repeatMode)
        emit("repeat", repeatMode.toDouble())
    }

    // --- Android Auto: browse + voice playback --------------------------------

    override fun onPlayFromMediaId(mediaId: String?, extras: Bundle?) {
        if (!mediaId.isNullOrEmpty()) AetherAuto.play(appContext, mediaId)
    }

    override fun onPrepareFromMediaId(mediaId: String?, extras: Bundle?) {
        if (!mediaId.isNullOrEmpty()) AetherAuto.play(appContext, mediaId)
    }

    /**
     * Gemini / Assistant voice playback. Resolution happens here, BEFORE the
     * warm/cold dispatch, so the resolved id survives the cold-start Intent hop
     * and a miss can be spoken back. Off the callback thread: the first
     * AutoCatalogNative.load parses the snapshot file.
     */
    override fun onPlayFromSearch(query: String?, extras: Bundle?) {
        voiceExec.execute {
            val resolved = try {
                AutoVoice.resolve(appContext, query, extras)
            } catch (_: Exception) {
                null
            }
            if (resolved != null) {
                AetherAuto.play(appContext, resolved)
            } else {
                MediaSessionPlugin.publishError(
                    if (query.isNullOrBlank()) "Nessun brano da riprodurre"
                    else "Nessun risultato per \"$query\""
                )
            }
        }
    }

    // Prepare-then-play: Gemini sometimes prepares first; treating it as an
    // immediate play matches onPrepareFromMediaId above and is what a car
    // "riproduci" ultimately wants.
    override fun onPrepareFromSearch(query: String?, extras: Bundle?) {
        onPlayFromSearch(query, extras)
    }

    /** Bare prepare (media resumption): stage the persisted last track. */
    override fun onPrepare() {
        if (AutoResumeStore.load(appContext) != null) {
            AetherAuto.play(appContext, AutoResumeStore.RESUME_ID)
        }
    }

    private companion object {
        /** Single lane for voice resolutions — they're rare and must not race. */
        val voiceExec: java.util.concurrent.ExecutorService =
            java.util.concurrent.Executors.newSingleThreadExecutor()
    }
}
