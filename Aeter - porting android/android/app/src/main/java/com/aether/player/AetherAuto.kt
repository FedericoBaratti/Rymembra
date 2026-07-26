package com.aether.player

import android.content.Context
import android.content.Intent
import android.os.Handler
import android.os.Looper

/**
 * Android Auto playback dispatcher, bridging the MediaSession's play-from callbacks
 * (and the MainActivity cold-start deep link) to the native ExoPlayer.
 *
 * "Warm-first + launch on cold play": while the NativeAudio plugin is alive the car
 * can start catalog playback directly on the existing ExoPlayer (full EQ / crossfade
 * / ReplayGain, no WebView or Node needed) via [NativeAudioHolder.autoPlay]. When the
 * app is cold that hook is null, so we launch MainActivity carrying the mediaId; once
 * the plugin loads, MainActivity re-dispatches through [playWhenReady].
 */
object AetherAuto {

    /** Intent extra carrying the catalog mediaId to play after a cold-start launch. */
    const val EXTRA_PLAY_MEDIA_ID = "aether.auto.playMediaId"

    private val main = Handler(Looper.getMainLooper())

    /** Warm: play now on the shared ExoPlayer. Cold: launch the app to play. */
    @JvmStatic
    fun play(ctx: Context, mediaId: String) {
        val hook = NativeAudioHolder.autoPlay
        if (hook != null) hook(mediaId) else launchForPlay(ctx, mediaId)
    }

    /** Launch MainActivity with the mediaId stashed for post-boot dispatch. */
    @JvmStatic
    fun launchForPlay(ctx: Context, mediaId: String) {
        val i = Intent(ctx, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            .putExtra(EXTRA_PLAY_MEDIA_ID, mediaId)
        ctx.startActivity(i)
    }

    /**
     * Called from MainActivity after a cold-play launch: poll for the NativeAudio
     * plugin to finish loading (which sets the play hook), then play — never
     * re-launches, so there is no launch loop.
     */
    @JvmStatic
    @JvmOverloads
    fun playWhenReady(mediaId: String, remainingMs: Long = 12_000L) {
        val hook = NativeAudioHolder.autoPlay
        if (hook != null) {
            hook(mediaId)
            return
        }
        if (remainingMs <= 0L) return
        main.postDelayed({ playWhenReady(mediaId, remainingMs - 200L) }, 200L)
    }
}
