package com.aether.player

import android.content.Context
import android.net.Uri
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import androidx.media3.common.C
import androidx.media3.common.MediaItem
import androidx.media3.common.MediaMetadata
import androidx.media3.common.PlaybackException
import androidx.media3.common.Player
import androidx.media3.common.TrackSelectionParameters.AudioOffloadPreferences
import androidx.media3.common.audio.AudioProcessor.AudioFormat
import androidx.media3.common.audio.BaseAudioProcessor
import androidx.media3.common.util.UnstableApi
import androidx.media3.exoplayer.DefaultRenderersFactory
import androidx.media3.exoplayer.ExoPlayer
import androidx.media3.exoplayer.audio.AudioSink
import androidx.media3.exoplayer.audio.DefaultAudioSink
import com.getcapacitor.JSArray
import com.getcapacitor.JSObject
import com.getcapacitor.Plugin
import com.getcapacitor.PluginCall
import com.getcapacitor.PluginMethod
import com.getcapacitor.annotation.CapacitorPlugin
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.ConcurrentHashMap
import kotlin.math.PI
import kotlin.math.cos
import kotlin.math.pow
import kotlin.math.sin
import kotlin.math.sqrt

/**
 * Native Media3/ExoPlayer audio engine for Android.
 *
 * The WebView's HTML5 <audio> playback bypasses the renderer Web Audio graph, so
 * the desktop EQ never touches the sound on mobile. This plugin moves playback
 * into an ExoPlayer that owns its own audio session and runs [BiquadEqProcessor],
 * a 10-band biquad EQ matching the desktop filter config exactly
 * (src/lib/audio.ts): band 0 low-shelf, bands 1..8 peaking (Q=1.1), band 9
 * high-shelf, ±12 dB. Renderer side: NativeEngine in src/lib/player.ts.
 *
 * ExoPlayer must be created and driven on a Looper thread, so every player
 * operation is posted to the main thread; the EQ processor is lock-free
 * (volatile coefficient array) and updated from the caller thread directly.
 */
@UnstableApi
@CapacitorPlugin(name = "NativeAudio")
class NativeAudioPlugin : Plugin() {

    private val eq = BiquadEqProcessor()
    private val main = Handler(Looper.getMainLooper())
    private var player: ExoPlayer? = null

    // Resume-entry id extraction (see catalogIdOf/coverHashOf): the loopback
    // media route is /media/<dbId>, covers are /art/<hash> (JS) or
    // <filesDir>/covers/<hash>.webp (Auto catalog).
    private val MEDIA_URL_ID = Regex("/media/(\\d+)")
    private val COVER_URL_HASH = Regex("/(?:art|covers)/([A-Za-z0-9_-]+)")

    private var userVolume = 1.0f
    private var rgLinear = 1.0f

    // Per-queue-item metadata indexed by MediaItem.mediaId (the JS queue index as
    // a string). Used on track transition to apply the correct ReplayGain and to
    // publish the right title/artist/artwork into the media notification natively.
    // ConcurrentHashMap: populated on the Capacitor thread (setQueue) and read on
    // the main thread (onMediaItemTransition).
    private val rgByMediaId = ConcurrentHashMap<String, Float>()
    private val durMsByMediaId = ConcurrentHashMap<String, Long>()
    // True for items whose bytes come from the network (podcast/remote streams,
    // proxied through the loopback /stream route): only those need the WifiLock
    // of WAKE_MODE_NETWORK. Library tracks are served from 127.0.0.1 (disk), so
    // holding the Wi-Fi radio awake for them is pure battery waste.
    private val remoteByMediaId = ConcurrentHashMap<String, Boolean>()

    // Crossfade ----------------------------------------------------------------
    // Native crossfade between automatically-advancing playlist items: a short-
    // lived secondary ExoPlayer ("tail") plays the last [crossfadeSec] seconds of
    // the outgoing track while the main player jumps early to the next item, with
    // overlapping equal-power volume ramps. Media3 has no built-in crossfade
    // (androidx/media issue #2), so this is the established two-player workaround.
    // Manual skip / seek / pause cancel the fade; podcasts and streams (unknown
    // duration) never trigger it. All fade state below is touched only on the main
    // looper, except crossfadeSec (set from the Capacitor thread in setCrossfade).
    @Volatile private var crossfadeSec = 0.0
    // Mirror of [eq] for the tail player's own sink — a BiquadEqProcessor holds
    // per-sink DF1 filter state, so the instance can't be shared between two sinks.
    private val tailEq = BiquadEqProcessor()
    private var tailPlayer: ExoPlayer? = null
    private var fadeActive = false            // overlap ramp running (handoff done)
    private var fadePrepped = false           // tail created, awaiting READY/handoff
    private var handoffDue = false            // scheduled start fired before tail READY
    private var expectFadeTransition = false  // the next onMediaItemTransition is ours
    private var fadeStartUptime = 0L
    private var fadeDurationMs = 0L
    private var fadeRgOut = 1.0f              // outgoing item's ReplayGain at handoff
    private var handoffRunnable: Runnable? = null
    private var fadeTicker: Runnable? = null
    private val fadeStepMs = 50L
    private val prepLeadMs = 700L
    private val spliceLeadMs = 60L

    /** Set by handleOnPause/handleOnResume; drives the progressTick mode. */
    @Volatile
    private var appBackgrounded = false

    // Media resumption ----------------------------------------------------------
    // Browse context of the current Auto-catalog queue ("album:<key>", "recent",
    // …) so the resumption entry can restore the whole queue, not just the track.
    // Null while a JS-driven queue plays. Set on the Capacitor/main thread in
    // playAuto, cleared by setQueue/load.
    @Volatile
    private var autoContextNodeId: String? = null

    /** Last uptime a position-only resume write happened (progressTick throttle). */
    private var lastResumeSaveUptime = 0L
    private val resumeSaveEveryMs = 5_000L

    /** Auto-catalog id ("track:<dbId>") for a playing item: catalog items carry it
     *  as their mediaId already, JS-queue items are mapped back through the
     *  loopback /media/<id> URL. Null for remote/stream items (not resumable). */
    private fun catalogIdOf(item: MediaItem?): String? {
        val id = item?.mediaId
        if (id != null && id.startsWith("track:")) return id
        val uri = item?.localConfiguration?.uri ?: return null
        if (uri.host != "127.0.0.1" && uri.host != "localhost" && uri.scheme != "file") return null
        val m = MEDIA_URL_ID.find(uri.path ?: "") ?: return null
        return "track:${m.groupValues[1]}"
    }

    /** Cover hash from an artwork URI — the JS /art/<hash> route or the native
     *  <filesDir>/covers/<hash>.webp file — so the resumption card has art. */
    private fun coverHashOf(item: MediaItem?): String? {
        val s = item?.mediaMetadata?.artworkUri?.toString() ?: return null
        return COVER_URL_HASH.find(s)?.groupValues?.get(1)
    }

    /** Persist the current item as the resumable "continue listening" entry. */
    private fun saveResumeEntry(item: MediaItem?, positionMs: Long) {
        val catalogId = catalogIdOf(item) ?: return
        val md = item?.mediaMetadata
        AutoResumeStore.save(
            context.applicationContext,
            AutoResumeStore.Entry(
                catalogId = catalogId,
                contextNodeId = autoContextNodeId,
                title = md?.title?.toString() ?: "",
                subtitle = md?.artist?.toString() ?: "",
                coverHash = coverHashOf(item),
                positionMs = positionMs
            )
        )
    }

    /** Mirror the ExoPlayer queue into the MediaSession so the car's queue view
     *  ("in coda") works. Main looper only (reads the player). */
    private fun publishSessionQueue(p: ExoPlayer) {
        val entries = ArrayList<MediaSessionPlugin.QueueEntry>(p.mediaItemCount)
        for (i in 0 until p.mediaItemCount) {
            val item = p.getMediaItemAt(i)
            entries.add(
                MediaSessionPlugin.QueueEntry(
                    mediaId = item.mediaId,
                    title = item.mediaMetadata.title?.toString() ?: "",
                    subtitle = item.mediaMetadata.artist?.toString() ?: ""
                )
            )
        }
        MediaSessionPlugin.publishQueue(entries)
        MediaSessionPlugin.setActiveQueueItem(p.currentMediaItemIndex)
    }

    // Audio offload (experimental, opt-in) --------------------------------------
    // Decode on the device DSP and let the CPU sleep between buffer refills —
    // the single biggest per-track energy saver ExoPlayer offers. Incompatible
    // with the EQ AudioProcessor (offload feeds the sink encoded bitstream, so
    // PCM processors never run) and with the two-player crossfade, so it only
    // engages while both are off; flipping any of the three (offload setting,
    // EQ, crossfade) rebuilds the player in the right mode (brief gap, only on
    // this opt-in path). ExoPlayer falls back to normal decoding by itself when
    // the device/format can't offload gaplessly.
    @Volatile
    private var offloadRequested = false
    /** Mode the live player was built with (main looper only). */
    private var playerOffloaded = false

    private fun offloadDesired(): Boolean =
        offloadRequested && eq.isBypassed() && crossfadeSec <= 0.0

    /**
     * Position/crossfade ticker. Foreground: 4 Hz, pushes `timeupdate` to the JS
     * store and watches for crossfade prep. Backgrounded: the WebView is frozen,
     * so `timeupdate` bridge crossings are pure battery waste — with crossfade
     * configured the tick drops to 1 Hz (crossfade prep only), without it the
     * tick stops entirely. The media notification needs neither: its scrubber
     * self-extrapolates from publishState (see MediaSessionPlugin), and track
     * transitions are event-driven. Restarted by handleOnResume, setCrossfade,
     * onIsPlayingChanged and ensurePlayer.
     */
    private val progressTick = object : Runnable {
        override fun run() {
            val bg = appBackgrounded
            // Thermal pressure: 4 Hz of bridge crossings is the one steady CPU
            // cost of playback — at warning+ drop to the same 1 Hz used in the
            // background (position UI updates once a second; nothing else cares).
            val hot = ThermalMonitor.currentLevel != ThermalMonitor.LEVEL_NORMAL
            val p = player
            if (p != null && p.isPlaying) {
                if (!bg) {
                    notifyListeners("timeupdate", JSObject().put("position", p.currentPosition / 1000.0))
                }
                maybePrepCrossfade(p)
                // Keep the resumption entry's position fresh (throttled — prefs
                // write) so a process kill mid-track resumes close to where it was.
                val now = SystemClock.uptimeMillis()
                if (now - lastResumeSaveUptime >= resumeSaveEveryMs) {
                    lastResumeSaveUptime = now
                    catalogIdOf(p.currentMediaItem)?.let {
                        AutoResumeStore.updatePosition(context.applicationContext, it, p.currentPosition)
                    }
                }
            }
            if (bg && crossfadeSec <= 0.0) return // nothing to tick for; see restarts above
            main.postDelayed(this, if (bg || hot) 1000L else 250L)
        }
    }

    /** Re-arm the ticker in the current mode (Handler is thread-safe). */
    private fun rescheduleProgressTick() {
        main.removeCallbacks(progressTick)
        main.post(progressTick)
    }

    // --- Wake mode (main looper only) ------------------------------------------

    private var currentWakeMode = C.WAKE_MODE_LOCAL

    /** Fallback when an item isn't in remoteByMediaId (e.g. plain load()):
     *  network bytes = http(s) to a non-loopback host, or the loopback /stream
     *  proxy (whose upstream fetch in node needs Wi-Fi awake just the same). */
    private fun looksRemote(uri: Uri?): Boolean {
        val scheme = uri?.scheme ?: return false // file:// and null → local
        if (scheme != "http" && scheme != "https") return false
        val loopback = uri.host == "127.0.0.1" || uri.host == "localhost"
        return !loopback || uri.path?.startsWith("/stream") == true
    }

    private fun applyWakeMode(p: ExoPlayer, remote: Boolean) {
        val mode = if (remote) C.WAKE_MODE_NETWORK else C.WAKE_MODE_LOCAL
        if (mode != currentWakeMode) {
            currentWakeMode = mode
            p.setWakeMode(mode)
        }
    }

    override fun handleOnPause() {
        appBackgrounded = true
        rescheduleProgressTick()
    }

    override fun handleOnResume() {
        appBackgrounded = false
        // Runs the tick immediately: the first pass pushes a `timeupdate` so the
        // JS store resyncs its position right as the WebView thaws.
        rescheduleProgressTick()
    }

    /**
     * A renderers factory whose audio sink runs [processor] (the 10-band biquad
     * EQ). The main player uses [eq]; the crossfade tail player uses [tailEq].
     */
    private fun buildRenderers(ctx: Context, processor: BiquadEqProcessor): DefaultRenderersFactory =
        object : DefaultRenderersFactory(ctx) {
            override fun buildAudioSink(
                context: Context,
                enableFloatOutput: Boolean,
                enableAudioTrackPlaybackParams: Boolean
            ): AudioSink {
                return DefaultAudioSink.Builder(context)
                    .setEnableFloatOutput(enableFloatOutput)
                    .setEnableAudioTrackPlaybackParams(enableAudioTrackPlaybackParams)
                    .setAudioProcessors(arrayOf(processor))
                    .build()
            }
        }

    private fun ensurePlayer() {
        if (player != null) return
        val ctx: Context = context
        val offload = offloadDesired()
        // Offload needs a plain sink: a custom AudioProcessor in the chain would
        // never run (encoded passthrough) and can prevent offload from engaging.
        val p = if (offload) {
            ExoPlayer.Builder(ctx, DefaultRenderersFactory(ctx)).build()
        } else {
            ExoPlayer.Builder(ctx, buildRenderers(ctx, eq)).build()
        }
        if (offload) {
            p.trackSelectionParameters = p.trackSelectionParameters
                .buildUpon()
                .setAudioOffloadPreferences(
                    AudioOffloadPreferences.Builder()
                        .setAudioOffloadMode(AudioOffloadPreferences.AUDIO_OFFLOAD_MODE_ENABLED)
                        // Never trade gapless for offload: fall back to normal
                        // decoding when the device can't do both.
                        .setIsGaplessSupportRequired(true)
                        .build()
                )
                .build()
        }
        playerOffloaded = offload
        // Hold a partial wake lock (CPU) while playing so background playback
        // doesn't stall when the device dozes / the screen is off. Only needs the
        // WAKE_LOCK manifest permission — works without running inside a Service
        // (the foreground MediaPlaybackService keeps the process alive). LOCAL by
        // default (library tracks stream from 127.0.0.1 = disk); applyWakeMode
        // upgrades to NETWORK (adds a WifiLock) only while a remote item plays.
        p.setWakeMode(C.WAKE_MODE_LOCAL)
        currentWakeMode = C.WAKE_MODE_LOCAL
        // Pause when headphones are unplugged / audio becomes "noisy".
        p.setHandleAudioBecomingNoisy(true)
        p.addListener(object : Player.Listener {
            override fun onIsPlayingChanged(isPlaying: Boolean) {
                notifyListeners(if (isPlaying) "play" else "pause", JSObject())
                // Keep the media notification correct even if the WebView is frozen.
                MediaSessionPlugin.publishState(isPlaying, p.currentPosition)
                // Playback may start while backgrounded (notification / Auto) with
                // the ticker stopped — re-arm it in the mode we're in.
                if (isPlaying) rescheduleProgressTick()
                // A pause is the last reliable moment before the process may be
                // frozen/killed: pin the resume position now.
                if (!isPlaying) {
                    catalogIdOf(p.currentMediaItem)?.let {
                        AutoResumeStore.updatePosition(context.applicationContext, it, p.currentPosition)
                    }
                }
            }

            override fun onPlayWhenReadyChanged(playWhenReady: Boolean, reason: Int) {
                // A genuine pause (plugin, notification, headphones unplugged / audio
                // becoming noisy) clears the playback intent — a buffer stall does not.
                // Abandon any crossfade so we never resume into a double-audio state.
                if (!playWhenReady && (fadeActive || fadePrepped)) cancelCrossfade(snapVolume = true)
            }

            override fun onPositionDiscontinuity(
                oldPosition: Player.PositionInfo,
                newPosition: Player.PositionInfo,
                reason: Int
            ) {
                // An in-item seek (scrubber / seek()) during a fade invalidates it. A
                // seek that changes the media item (notification next/prev) is handled
                // in onMediaItemTransition instead. Our own handoff seek is excluded via
                // expectFadeTransition (covers the single-item repeat-all case, where
                // the index doesn't change) and, defensively, via the index check.
                if ((fadeActive || fadePrepped) &&
                    !expectFadeTransition &&
                    reason == Player.DISCONTINUITY_REASON_SEEK &&
                    oldPosition.mediaItemIndex == newPosition.mediaItemIndex
                ) {
                    cancelCrossfade(snapVolume = true)
                }
            }

            override fun onMediaItemTransition(mediaItem: MediaItem?, reason: Int) {
                val id = mediaItem?.mediaId
                // Apply the new track's ReplayGain.
                rgLinear = id?.let { rgByMediaId[it] } ?: 1.0f
                // WifiLock only while the current item actually streams from the
                // network; local library tracks run on the CPU wakelock alone.
                applyWakeMode(
                    p,
                    id?.let { remoteByMediaId[it] }
                        ?: looksRemote(mediaItem?.localConfiguration?.uri)
                )
                if (expectFadeTransition) {
                    // Our own crossfade handoff advanced the item early; the ramp owns
                    // p.volume until it completes, so don't snap it here.
                    expectFadeTransition = false
                } else {
                    // External transition (plain auto-advance, or notification next/
                    // prev landing mid-fade): drop any fade and snap to the new item.
                    cancelCrossfade(snapVolume = false)
                    p.volume = effectiveVolume()
                }
                // Publish the new track's metadata into the media notification
                // natively — no WebView round-trip needed (it may be frozen).
                val md = mediaItem?.mediaMetadata
                if (md != null) {
                    MediaSessionPlugin.publishMetadata(
                        ctx,
                        md.title?.toString() ?: "",
                        md.artist?.toString() ?: "",
                        md.albumTitle?.toString() ?: "",
                        md.artworkUri?.toString() ?: "",
                        id?.let { durMsByMediaId[it] } ?: 0L,
                        id ?: ""
                    )
                }
                // Active-row id rides inside PlaybackState: set it first so the
                // publishState below carries the new index to the car display.
                MediaSessionPlugin.setActiveQueueItem(p.currentMediaItemIndex)
                MediaSessionPlugin.publishState(p.isPlaying, 0L)
                // Every transition re-anchors the "continue listening" entry (car
                // resumption card, Gemini "riprendi la musica").
                saveResumeEntry(mediaItem, 0L)
                // Notify the renderer so the JS player store can reconcile its
                // queue position / scrobble when it next runs (possibly on resume).
                val data = JSObject().put("index", p.currentMediaItemIndex)
                if (id != null) data.put("mediaId", id)
                notifyListeners("transition", data)
            }

            override fun onPlaybackStateChanged(state: Int) {
                when (state) {
                    Player.STATE_ENDED -> notifyListeners("ended", JSObject())
                    Player.STATE_READY -> {
                        val d = p.duration
                        if (d != C.TIME_UNSET) {
                            notifyListeners("loaded", JSObject().put("duration", d / 1000.0))
                        }
                    }
                }
            }

            override fun onPlayerError(error: PlaybackException) {
                notifyListeners("loaderror", JSObject().put("message", error.message ?: "playback error"))
            }
        })
        p.volume = effectiveVolume()
        player = p
        NativeAudioHolder.player = p
        rescheduleProgressTick()
    }

    private fun effectiveVolume(): Float = (userVolume * rgLinear).coerceIn(0f, 1f)

    /**
     * When the desired offload mode no longer matches the mode the live player
     * was built with, rebuild it in place, carrying over queue / position /
     * play-state. Causes a short audible gap — acceptable for an explicit
     * settings change on the experimental offload path, and a no-op for
     * everyone else (offloadRequested defaults to false). Main looper only.
     */
    private fun maybeRebuildForOffload() {
        val p = player ?: return // next ensurePlayer picks the right mode
        if (offloadDesired() == playerOffloaded) return
        cancelCrossfade(snapVolume = false)
        val items = ArrayList<MediaItem>(p.mediaItemCount)
        for (i in 0 until p.mediaItemCount) items.add(p.getMediaItemAt(i))
        val index = p.currentMediaItemIndex
        val positionMs = p.currentPosition
        val wasPlaying = p.playWhenReady
        val wasIdle = p.playbackState == Player.STATE_IDLE
        val repeat = p.repeatMode
        val speed = p.playbackParameters.speed
        p.release()
        player = null
        NativeAudioHolder.player = null
        ensurePlayer()
        player?.apply {
            repeatMode = repeat
            setPlaybackSpeed(speed)
            if (items.isNotEmpty()) {
                // onMediaItemTransition re-applies ReplayGain/wake mode and
                // re-publishes the notification metadata for the current item.
                setMediaItems(items, index.coerceIn(0, items.size - 1), positionMs)
                if (!wasIdle) {
                    prepare()
                    playWhenReady = wasPlaying
                }
                publishSessionQueue(this)
            }
        }
    }

    /**
     * Experimental audio-offload master switch (Settings → Playback, mobile
     * only; default off). See [offloadDesired] for when it actually engages.
     */
    @PluginMethod
    fun setOffload(call: PluginCall) {
        offloadRequested = call.getBoolean("enabled") ?: false
        main.post { maybeRebuildForOffload() }
        call.resolve()
    }

    /**
     * Register the Android Auto warm-play hook while this plugin is alive: the car
     * can start catalog playback straight on this ExoPlayer (full EQ / crossfade /
     * ReplayGain), reading file paths from the catalog snapshot — no WebView or Node.
     * When this plugin isn't loaded the hook is null and AetherAuto launches the app.
     */
    override fun load() {
        NativeAudioHolder.autoPlay = { mediaId -> playAuto(mediaId) }
        // Idempotent (ThermalPlugin also starts it); guarantees the level is live
        // even if plugin load order changes. Re-arming the ticker on transitions
        // moves between the 4 Hz and 1 Hz cadence without waiting a full period.
        ThermalMonitor.start(context)
        offThermal = ThermalMonitor.addListener { main.post { rescheduleProgressTick() } }
    }

    private var offThermal: (() -> Unit)? = null

    /**
     * Android Auto (browse / voice / cold-start deep link) → build a file-path queue
     * from the catalog snapshot and play it on the shared ExoPlayer. Safe to call off
     * the main thread: the per-item RG/duration maps are populated first, then the
     * player ops are posted to the looper.
     */
    private fun playAuto(mediaId: String) {
        val ctx: Context = context.applicationContext
        val catalog = AutoCatalogNative.load(ctx) ?: return

        // The resumption entry replays its saved browse context (album/playlist)
        // seeked to the saved position; a lone track otherwise.
        var target = mediaId
        var startPosMs = 0L
        if (mediaId == AutoResumeStore.RESUME_ID) {
            val entry = AutoResumeStore.load(ctx) ?: return
            val ctxNode = entry.contextNodeId?.takeIf { catalog.nodes.containsKey(it) }
            target = if (ctxNode != null) "p#$ctxNode#${entry.catalogId}" else entry.catalogId
            startPosMs = entry.positionMs
        }

        val resolved = AutoCatalogNative.resolveTracks(catalog, target)
        if (resolved.tracks.isEmpty()) {
            // The id was resolved against an older snapshot (voice, resumption
            // card): tell the car/assistant instead of failing silently.
            MediaSessionPlugin.publishError("Contenuto non più disponibile")
            return
        }
        rgByMediaId.clear()
        durMsByMediaId.clear()
        val items = ArrayList<MediaItem>(resolved.tracks.size)
        for (t in resolved.tracks) items.add(buildAutoItem(ctx, t))
        val start = resolved.startIndex.coerceIn(0, items.size - 1)
        // Remember the browse context for the next resumption entry.
        autoContextNodeId = when {
            target.startsWith("p#") -> {
                val cut = target.lastIndexOf('#')
                if (cut > 2) target.substring(2, cut) else null
            }
            catalog.nodes.containsKey(target) -> target
            else -> null
        }
        // The car drives the shared MediaSession; make sure it and the foreground
        // service (which shows the notification) exist even on a cold plugin load.
        MediaSessionPlugin.Holder.ensureSession(ctx)
        val posMs = startPosMs
        main.post {
            cancelCrossfade(snapVolume = true)
            ensurePlayer()
            player?.apply {
                setMediaItems(items, start, posMs)
                prepare()
                playWhenReady = true
                publishSessionQueue(this)
            }
        }
        MediaSessionPlugin.ensureForegroundService(ctx)
    }

    /** Build a file-path MediaItem for an Auto catalog track (mediaId "track:<id>"),
     *  populating the per-item ReplayGain/duration maps like a JS queue entry. */
    private fun buildAutoItem(ctx: Context, t: AutoCatalogNative.AutoTrack): MediaItem {
        rgByMediaId[t.mediaId] = t.replayGain
        durMsByMediaId[t.mediaId] = t.durationMs
        remoteByMediaId[t.mediaId] = false // Auto catalog plays local files
        val md = MediaMetadata.Builder()
            .setTitle(t.title)
            .setArtist(t.subtitle)
        AutoCatalogNative.coverFileUri(ctx, t.coverHash)?.let { md.setArtworkUri(it) }
        return MediaItem.Builder()
            .setMediaId(t.mediaId)
            .setUri(Uri.fromFile(File(t.path)))
            .setMediaMetadata(md.build())
            .build()
    }

    @PluginMethod
    fun load(call: PluginCall) {
        val url = call.getString("url")
        if (url == null) {
            call.reject("url required")
            return
        }
        val autoplay = call.getBoolean("autoplay") ?: true
        val remote = call.getBoolean("remote") ?: looksRemote(Uri.parse(url))
        autoContextNodeId = null
        main.post {
            cancelCrossfade(snapVolume = true)
            ensurePlayer()
            player?.apply {
                applyWakeMode(this, remote)
                setMediaItem(MediaItem.fromUri(url))
                prepare()
                playWhenReady = autoplay
                publishSessionQueue(this)
            }
        }
        call.resolve()
    }

    /** Build a MediaItem carrying mediaId + metadata + ReplayGain for a queue entry. */
    private fun buildItem(o: JSObject): MediaItem {
        val url = o.getString("url") ?: ""
        val mediaId = o.getString("mediaId") ?: url
        val artworkUrl = o.getString("artworkUrl") ?: ""
        // optDouble (not getDouble) returns the default when the key is absent
        // instead of throwing — getDouble is org.json's non-null/throwing variant.
        val durationMs = (o.optDouble("duration", 0.0) * 1000).toLong()
        val rg = o.optDouble("replayGain", 1.0).toFloat()
        rgByMediaId[mediaId] = rg
        durMsByMediaId[mediaId] = durationMs
        remoteByMediaId[mediaId] = o.optBoolean("remote", looksRemote(Uri.parse(url)))
        val mdBuilder = MediaMetadata.Builder()
            .setTitle(o.getString("title") ?: "")
            .setArtist(o.getString("artist") ?: "")
            .setAlbumTitle(o.getString("album") ?: "")
        if (artworkUrl.isNotEmpty()) mdBuilder.setArtworkUri(Uri.parse(artworkUrl))
        return MediaItem.Builder()
            .setMediaId(mediaId)
            .setUri(url)
            .setMediaMetadata(mdBuilder.build())
            .build()
    }

    private fun parseItems(arr: JSArray): ArrayList<MediaItem> {
        val items = ArrayList<MediaItem>(arr.length())
        for (i in 0 until arr.length()) {
            items.add(buildItem(JSObject.fromJSONObject(arr.getJSONObject(i))))
        }
        return items
    }

    /**
     * Replace the whole playlist and start at [startIndex]. ExoPlayer owns the
     * queue from here and auto-advances (gapless, pre-buffered) in the background
     * without any WebView involvement — the fix for "next track doesn't play when
     * backgrounded".
     */
    @PluginMethod
    fun setQueue(call: PluginCall) {
        val arr = call.getArray("items") ?: JSArray()
        val startIndex = call.getInt("startIndex") ?: 0
        val autoplay = call.getBoolean("autoplay") ?: true
        // Reset stale entries first so the per-item maps don't grow unbounded
        // across queues; parseItems() then repopulates them for this queue.
        rgByMediaId.clear()
        durMsByMediaId.clear()
        remoteByMediaId.clear()
        val items = try {
            parseItems(arr)
        } catch (e: Exception) {
            call.reject("invalid items: ${e.message}")
            return
        }
        // A JS-driven queue has no Auto browse context for the resumption entry.
        autoContextNodeId = null
        main.post {
            cancelCrossfade(snapVolume = true)
            ensurePlayer()
            player?.apply {
                val safeStart = if (items.isEmpty()) 0 else startIndex.coerceIn(0, items.size - 1)
                setMediaItems(items, safeStart, 0L)
                prepare()
                playWhenReady = autoplay
                publishSessionQueue(this)
            }
        }
        call.resolve()
    }

    /**
     * Replace the upcoming items (everything after the currently playing item)
     * without touching the current one — used for enqueue / play-next / reorder /
     * shuffle so the current track keeps playing uninterrupted. The cut point is
     * derived natively from currentMediaItemIndex, so the renderer never has to
     * track the live native index.
     */
    @PluginMethod
    fun updateUpcoming(call: PluginCall) {
        val arr = call.getArray("items") ?: JSArray()
        val items = try {
            parseItems(arr)
        } catch (e: Exception) {
            call.reject("invalid items: ${e.message}")
            return
        }
        main.post {
            // Not a crossfade-cancel point: this only replaces items *after* the
            // current one. A prepped/active fade plays the outgoing track regardless
            // of what "next" becomes, and post-handoff the current item is already the
            // incoming track — so the fade stays valid.
            val p = player ?: return@post
            val from = (p.currentMediaItemIndex + 1).coerceIn(0, p.mediaItemCount)
            if (p.mediaItemCount > from) p.removeMediaItems(from, p.mediaItemCount)
            p.addMediaItems(items)
            publishSessionQueue(p)
        }
        call.resolve()
    }

    @PluginMethod
    fun setRepeatMode(call: PluginCall) {
        val mode = call.getString("mode") ?: "off"
        main.post {
            player?.repeatMode = when (mode) {
                "one" -> Player.REPEAT_MODE_ONE
                "all" -> Player.REPEAT_MODE_ALL
                else -> Player.REPEAT_MODE_OFF
            }
            // Repeat-one loops the same item via seek-to-0; a crossfade into itself
            // isn't meaningful, so drop any pending fade.
            if (mode == "one") cancelCrossfade(snapVolume = true)
        }
        call.resolve()
    }

    @PluginMethod
    fun play(call: PluginCall) {
        main.post { player?.play() }
        call.resolve()
    }

    @PluginMethod
    fun pause(call: PluginCall) {
        main.post { player?.pause() }
        call.resolve()
    }

    @PluginMethod
    fun seek(call: PluginCall) {
        val position = call.getDouble("position") ?: 0.0
        main.post { player?.seekTo((position * 1000).toLong()) }
        call.resolve()
    }

    @PluginMethod
    fun setVolume(call: PluginCall) {
        userVolume = (call.getDouble("volume") ?: 1.0).toFloat()
        // During a crossfade the ramp ticker owns player.volume and reads userVolume
        // live, so it retargets on its own; skip the direct set to avoid a jump.
        main.post { if (!fadeActive) player?.volume = effectiveVolume() }
        call.resolve()
    }

    @PluginMethod
    fun setReplayGain(call: PluginCall) {
        rgLinear = (call.getDouble("linear") ?: 1.0).toFloat()
        main.post { if (!fadeActive) player?.volume = effectiveVolume() }
        call.resolve()
    }

    @PluginMethod
    fun setRate(call: PluginCall) {
        val rate = (call.getDouble("rate") ?: 1.0).toFloat()
        main.post {
            player?.setPlaybackSpeed(rate)
            tailPlayer?.setPlaybackSpeed(rate)
        }
        call.resolve()
    }

    @PluginMethod
    fun stop(call: PluginCall) {
        main.post {
            cancelCrossfade(snapVolume = true)
            player?.apply {
                stop()
                clearMediaItems()
                publishSessionQueue(this)
            }
        }
        call.resolve()
    }

    @PluginMethod
    fun preload(call: PluginCall) {
        // Reserved for native gapless (ExoPlayer media-item queue). The queue is
        // currently driven from JS via the `ended` event, so this is a no-op hint.
        call.resolve()
    }

    @PluginMethod
    fun setEq(call: PluginCall) {
        val arr = call.getArray("gains")
        val enabled = call.getBoolean("enabled") ?: false
        val gains = DoubleArray(BiquadEqProcessor.BANDS)
        if (arr != null) {
            for (i in 0 until minOf(arr.length(), BiquadEqProcessor.BANDS)) {
                gains[i] = arr.optDouble(i, 0.0)
            }
        }
        eq.setGains(gains, enabled)
        tailEq.setGains(gains, enabled)
        // An offloaded player has no EQ processor in its sink: turning the EQ
        // on (or off again) while offload is opted in must swap the player.
        main.post { maybeRebuildForOffload() }
        call.resolve()
    }

    @PluginMethod
    fun setCrossfade(call: PluginCall) {
        crossfadeSec = (call.getDouble("seconds") ?: 0.0).coerceIn(0.0, 12.0)
        if (crossfadeSec <= 0.0) main.post { cancelCrossfade(snapVolume = true) }
        // The background ticker stops entirely when crossfade is 0; enabling
        // crossfade must bring the 1 Hz prep watcher back.
        else rescheduleProgressTick()
        // Crossfade and offload are mutually exclusive (two-player overlap needs
        // PCM decoding on the CPU): re-evaluate the player mode if opted in.
        main.post { maybeRebuildForOffload() }
        call.resolve()
    }

    // --- Crossfade internals (main looper only) -------------------------------

    /** Watcher from [progressTick]: prep a fade as the current item nears its end. */
    private fun maybePrepCrossfade(p: ExoPlayer) {
        // Critically hot: a second decoding ExoPlayer is the last thing the
        // device needs. Tracks splice without fade until it cools (self-healing:
        // the next prep window after the level drops fades again). An already
        // prepped/running fade is left alone — cancelling mid-ramp is jarring.
        if (ThermalMonitor.currentLevel == ThermalMonitor.LEVEL_CRITICAL) return
        if (crossfadeSec <= 0.0 || fadeActive || fadePrepped) return
        if (p.repeatMode == Player.REPEAT_MODE_ONE) return
        if (!p.hasNextMediaItem() || p.isCurrentMediaItemLive) return
        val dur = p.duration
        if (dur == C.TIME_UNSET || dur <= 0L) return          // podcasts / streams: excluded
        val speed = p.playbackParameters.speed.coerceAtLeast(0.01f)
        val effFadeMs = minOf((crossfadeSec * 1000).toLong(), dur / 2)
        if (effFadeMs <= 0L) return
        // At the 1 Hz background tick the next pass may land after the ideal prep
        // point — widen the window by one tick so the prep is never missed. The
        // handoff time itself stays exact (startDelayMs is computed, not polled).
        val slackMs = if (appBackgrounded) 1000L else 0L
        val remainingWallMs = ((dur - p.currentPosition) / speed).toLong()
        if (remainingWallMs <= effFadeMs + prepLeadMs + slackMs) {
            prepCrossfade(p, dur, effFadeMs, (remainingWallMs - effFadeMs).coerceAtLeast(0L))
        }
    }

    /** Create the silent tail player, pre-buffered around the handoff point. */
    private fun prepCrossfade(p: ExoPlayer, durMs: Long, effFadeMs: Long, startDelayMs: Long) {
        val uri = p.currentMediaItem?.localConfiguration?.uri ?: return
        fadePrepped = true
        fadeDurationMs = effFadeMs
        fadeRgOut = rgLinear
        val ctx: Context = context
        val tail = ExoPlayer.Builder(ctx, buildRenderers(ctx, tailEq)).build()
        // Never contend with the main player for audio focus; it lives a few seconds.
        tail.setAudioAttributes(tail.audioAttributes, /* handleAudioFocus = */ false)
        tail.setPlaybackSpeed(p.playbackParameters.speed)
        tail.volume = 0f
        // Fresh MediaItem with no mediaId: nothing keyed by mediaId (the RG map, the
        // JS reconcile) ever sees the tail.
        tail.setMediaItem(MediaItem.fromUri(uri))
        tail.addListener(object : Player.Listener {
            override fun onPlaybackStateChanged(state: Int) {
                if (tailPlayer !== tail) return
                when (state) {
                    Player.STATE_READY -> if (handoffDue) startHandoff()
                    Player.STATE_ENDED -> releaseTail() // main ramp finishes on its own
                    else -> {}
                }
            }
            override fun onPlayerError(error: PlaybackException) {
                // A tail failure must never break main playback — just drop the fade.
                if (tailPlayer === tail) cancelCrossfade(snapVolume = true)
            }
        })
        tail.seekTo((durMs - effFadeMs).coerceAtLeast(0L))
        tail.prepare() // playWhenReady stays false until the handoff
        tailPlayer = tail
        handoffRunnable = Runnable { startHandoff() }.also { main.postDelayed(it, startDelayMs) }
    }

    /** Splice the tail in for the outgoing track and advance the main player early. */
    private fun startHandoff() {
        if (!fadePrepped || fadeActive) return
        val p = player ?: return
        val tail = tailPlayer ?: return
        if (tail.playbackState != Player.STATE_READY) { handoffDue = true; return }
        handoffDue = false
        val pos = p.currentPosition
        // If the handoff is running late (slow tail buffering), shorten the fade to
        // the time actually left so it never overruns the outgoing track's end.
        if (p.duration != C.TIME_UNSET) {
            val speed = p.playbackParameters.speed.coerceAtLeast(0.01f)
            val remainingWallMs = ((p.duration - pos) / speed).toLong()
            fadeDurationMs = minOf(fadeDurationMs, remainingWallMs).coerceAtLeast(1L)
        }
        tail.seekTo(pos + spliceLeadMs)
        tail.volume = (userVolume * fadeRgOut).coerceIn(0f, 1f)
        tail.play()
        expectFadeTransition = true
        p.seekToNextMediaItem() // fires onMediaItemTransition (RG switches to incoming)
        p.volume = 0f
        fadeActive = true
        fadePrepped = false
        fadeStartUptime = SystemClock.uptimeMillis()
        startFadeTicker()
    }

    /** Equal-power volume ramp; progress is read from wall-clock so it's drift-free. */
    private fun startFadeTicker() {
        fadeTicker?.let { main.removeCallbacks(it) }
        val ticker = object : Runnable {
            override fun run() {
                if (!fadeActive) return
                val q = ((SystemClock.uptimeMillis() - fadeStartUptime).toDouble() / fadeDurationMs)
                    .coerceIn(0.0, 1.0)
                val phase = q * (PI / 2)
                // Incoming reads effectiveVolume() live (rgLinear already switched to
                // the incoming item), so setVolume / setReplayGain retarget the ramp.
                player?.volume = (sin(phase).toFloat() * effectiveVolume()).coerceIn(0f, 1f)
                tailPlayer?.volume = (cos(phase).toFloat() * userVolume * fadeRgOut).coerceIn(0f, 1f)
                if (q >= 1.0) cancelCrossfade(snapVolume = true) else main.postDelayed(this, fadeStepMs)
            }
        }
        fadeTicker = ticker
        main.postDelayed(ticker, fadeStepMs)
    }

    /** Single idempotent teardown for every fade exit (completion or cancellation). */
    private fun cancelCrossfade(snapVolume: Boolean) {
        handoffRunnable?.let { main.removeCallbacks(it) }
        handoffRunnable = null
        fadeTicker?.let { main.removeCallbacks(it) }
        fadeTicker = null
        releaseTail()
        fadeActive = false
        fadePrepped = false
        handoffDue = false
        expectFadeTransition = false
        if (snapVolume) player?.volume = effectiveVolume()
    }

    private fun releaseTail() {
        tailPlayer?.release()
        tailPlayer = null
    }

    override fun handleOnDestroy() {
        NativeAudioHolder.autoPlay = null
        offThermal?.invoke()
        offThermal = null
        main.removeCallbacks(progressTick)
        main.post {
            cancelCrossfade(snapVolume = false)
            player?.release(); player = null; NativeAudioHolder.player = null
        }
    }
}

/**
 * Shares the active player with MediaSessionPlugin so notification / lock-screen
 * transport actions can drive it directly (no WebView round-trip, essential while
 * the renderer is frozen in the background). Top-level (not nested in the
 * @UnstableApi plugin) and typed as the stable [Player] interface, so
 * MediaSessionPlugin needn't opt into any @UnstableApi symbol.
 */
object NativeAudioHolder {
    @Volatile
    var player: Player? = null

    /**
     * Android Auto play hook. Set by NativeAudioPlugin.load() while the app is
     * "warm" (bridge/plugin alive); null when cold. Given a catalog mediaId
     * ("track:<id>", a node id, "p#<nodeId>#track:<id>" or "search#<query>") it
     * builds a file-path queue from the Auto catalog snapshot and plays it on the
     * existing ExoPlayer — no WebView/Node needed. When null, AetherAuto launches
     * MainActivity with EXTRA_PLAY_MEDIA_ID and re-dispatches once the plugin loads.
     */
    @Volatile
    var autoPlay: ((mediaId: String) -> Unit)? = null
}

/**
 * Cascade of 10 biquad bands per channel, applied to interleaved PCM (16-bit or
 * float). Coefficients use the Audio EQ Cookbook (RBJ) to match the WebView's
 * BiquadFilterNode: band 0 low-shelf, bands 1..8 peaking with Q=1.1, band 9
 * high-shelf; shelves use slope S=1 (Web Audio default). Disabling the EQ (or
 * setting every gain to 0 dB) engages a true bypass that skips the per-sample
 * cascade entirely (see [bypass]).
 */
@UnstableApi
class BiquadEqProcessor : BaseAudioProcessor() {

    companion object {
        const val BANDS = 10
        private val FREQS = doubleArrayOf(32.0, 64.0, 125.0, 250.0, 500.0, 1000.0, 2000.0, 4000.0, 8000.0, 16000.0)
        private const val PEAK_Q = 1.1
        private const val COEFFS_PER_BAND = 5 // b0, b1, b2, a1, a2 (a0 normalised to 1)
    }

    @Volatile
    private var gains = DoubleArray(BANDS)
    @Volatile
    private var enabled = false
    /** Flattened [BANDS * 5] coefficient array, swapped atomically on update. */
    @Volatile
    private var coeffs: FloatArray = identityCoeffs()
    /**
     * True bypass: when the EQ is disabled (or every gain is 0 dB) the biquads
     * are all identity, so skip the per-sample cascade entirely instead of
     * multiplying every frame through 10 unity filters (~1M mults/sec saved
     * during playback). Deliberately NOT isActive()=false: Media3 re-evaluates
     * chain activity only on sink reconfiguration, so a mid-track EQ enable
     * would silently not apply until the next track.
     */
    @Volatile
    private var bypass = true
    // Audio-thread-only mirror of `bypass`, to reset filter state on the
    // bypass→active edge from the thread that owns the state arrays.
    private var wasBypassed = true

    private var channelCount = 0
    private var encoding = C.ENCODING_PCM_16BIT
    private var sampleRate = 0
    // DF1 state: [channel * BANDS + band]
    private var x1 = FloatArray(0)
    private var x2 = FloatArray(0)
    private var y1 = FloatArray(0)
    private var y2 = FloatArray(0)

    fun setGains(newGains: DoubleArray, newEnabled: Boolean) {
        gains = newGains.copyOf(BANDS)
        enabled = newEnabled
        if (sampleRate > 0) coeffs = computeCoeffs(sampleRate)
        bypass = !newEnabled || newGains.all { it == 0.0 }
    }

    /** True while the cascade is a no-op (EQ off or all gains 0 dB) — the
     *  audio-offload gate reads this to decide the player mode. */
    fun isBypassed(): Boolean = bypass

    override fun onConfigure(inputAudioFormat: AudioFormat): AudioFormat {
        // Pass through unsupported encodings unchanged (still "active" so the
        // chain stays valid, but the per-sample loop only runs for PCM 16/float).
        channelCount = inputAudioFormat.channelCount
        encoding = inputAudioFormat.encoding
        sampleRate = inputAudioFormat.sampleRate
        val n = channelCount * BANDS
        x1 = FloatArray(n); x2 = FloatArray(n); y1 = FloatArray(n); y2 = FloatArray(n)
        coeffs = computeCoeffs(sampleRate)
        return inputAudioFormat
    }

    override fun queueInput(inputBuffer: ByteBuffer) {
        val limit = inputBuffer.limit()
        val size = limit - inputBuffer.position()
        if (size == 0) return
        val out = replaceOutputBuffer(size)
        val supported = encoding == C.ENCODING_PCM_16BIT || encoding == C.ENCODING_PCM_FLOAT
        val skip = bypass
        if (!skip && wasBypassed) resetState() // fresh filter state on bypass→active
        wasBypassed = skip
        if (skip || !supported || channelCount == 0) {
            out.put(inputBuffer)
            out.flip()
            return
        }
        inputBuffer.order(ByteOrder.LITTLE_ENDIAN)
        out.order(ByteOrder.LITTLE_ENDIAN)
        val c = coeffs
        if (encoding == C.ENCODING_PCM_16BIT) {
            val frames = size / (2 * channelCount)
            for (f in 0 until frames) {
                for (ch in 0 until channelCount) {
                    val sample = inputBuffer.short / 32768.0f
                    val y = process(sample, ch, c)
                    out.putShort((y.coerceIn(-1f, 1f) * 32767.0f).toInt().toShort())
                }
            }
        } else { // ENCODING_PCM_FLOAT
            val frames = size / (4 * channelCount)
            for (f in 0 until frames) {
                for (ch in 0 until channelCount) {
                    val sample = inputBuffer.float
                    val y = process(sample, ch, c)
                    out.putFloat(y.coerceIn(-1f, 1f))
                }
            }
        }
        inputBuffer.position(limit)
        out.flip()
    }

    private fun process(input: Float, ch: Int, c: FloatArray): Float {
        var s = input
        val base = ch * BANDS
        for (b in 0 until BANDS) {
            val ci = b * COEFFS_PER_BAND
            val si = base + b
            val x = s
            val y = c[ci] * x + c[ci + 1] * x1[si] + c[ci + 2] * x2[si] - c[ci + 3] * y1[si] - c[ci + 4] * y2[si]
            x2[si] = x1[si]; x1[si] = x
            y2[si] = y1[si]; y1[si] = y
            s = y
        }
        return s
    }

    override fun onFlush() = resetState()
    override fun onReset() = resetState()

    private fun resetState() {
        x1.fill(0f); x2.fill(0f); y1.fill(0f); y2.fill(0f)
    }

    private fun computeCoeffs(fs: Int): FloatArray {
        val out = FloatArray(BANDS * COEFFS_PER_BAND)
        for (b in 0 until BANDS) {
            val gainDb = if (enabled) gains[b] else 0.0
            val w0 = 2.0 * PI * FREQS[b] / fs
            val cosW = cos(w0)
            val sinW = sin(w0)
            val a = 10.0.pow(gainDb / 40.0)
            val b0: Double; val b1: Double; val b2: Double
            val a0: Double; val a1: Double; val a2: Double
            when (b) {
                0 -> { // low-shelf (S = 1)
                    val alpha = sinW / 2.0 * sqrt(2.0)
                    val twoSqrtAAlpha = 2.0 * sqrt(a) * alpha
                    b0 = a * ((a + 1) - (a - 1) * cosW + twoSqrtAAlpha)
                    b1 = 2 * a * ((a - 1) - (a + 1) * cosW)
                    b2 = a * ((a + 1) - (a - 1) * cosW - twoSqrtAAlpha)
                    a0 = (a + 1) + (a - 1) * cosW + twoSqrtAAlpha
                    a1 = -2 * ((a - 1) + (a + 1) * cosW)
                    a2 = (a + 1) + (a - 1) * cosW - twoSqrtAAlpha
                }
                BANDS - 1 -> { // high-shelf (S = 1)
                    val alpha = sinW / 2.0 * sqrt(2.0)
                    val twoSqrtAAlpha = 2.0 * sqrt(a) * alpha
                    b0 = a * ((a + 1) + (a - 1) * cosW + twoSqrtAAlpha)
                    b1 = -2 * a * ((a - 1) + (a + 1) * cosW)
                    b2 = a * ((a + 1) + (a - 1) * cosW - twoSqrtAAlpha)
                    a0 = (a + 1) - (a - 1) * cosW + twoSqrtAAlpha
                    a1 = 2 * ((a - 1) - (a + 1) * cosW)
                    a2 = (a + 1) - (a - 1) * cosW - twoSqrtAAlpha
                }
                else -> { // peaking
                    val alpha = sinW / (2.0 * PEAK_Q)
                    b0 = 1 + alpha * a
                    b1 = -2 * cosW
                    b2 = 1 - alpha * a
                    a0 = 1 + alpha / a
                    a1 = -2 * cosW
                    a2 = 1 - alpha / a
                }
            }
            val ci = b * COEFFS_PER_BAND
            out[ci] = (b0 / a0).toFloat()
            out[ci + 1] = (b1 / a0).toFloat()
            out[ci + 2] = (b2 / a0).toFloat()
            out[ci + 3] = (a1 / a0).toFloat()
            out[ci + 4] = (a2 / a0).toFloat()
        }
        return out
    }
}

private fun identityCoeffs(): FloatArray {
    val out = FloatArray(BiquadEqProcessor.BANDS * 5)
    for (b in 0 until BiquadEqProcessor.BANDS) out[b * 5] = 1.0f // b0 = 1, rest 0
    return out
}
