import { registerPlugin, Capacitor } from '@capacitor/core'

/**
 * Bridge to the native Media3/ExoPlayer audio engine (NativeAudioPlugin.kt).
 *
 * On Android the WebView's HTML5 <audio> playback bypasses the renderer's Web
 * Audio graph, so the desktop EQ/ReplayGain (src/lib/audio.ts) never touches the
 * sound. To get a *real* equalizer on mobile we move playback into a native
 * ExoPlayer that owns its own audio session and runs a 10-band biquad EQ
 * AudioProcessor (mirroring the desktop filter config exactly).
 *
 * Renderer side: NativeEngine in src/lib/player.ts drives playback; the EQ panel
 * routes band gains here via applyEq() in src/lib/audio.ts. Desktop never calls
 * into this module (guarded by isMobile + isAvailable()).
 */
/** One entry in the native ExoPlayer playlist. */
export interface NativeQueueItem {
  /** Stable per-queue-entry id (the JS queue index as a string) for reconcile. */
  mediaId: string
  url: string
  title: string
  artist: string
  album: string
  artworkUrl: string
  /** Seconds. */
  duration: number
  /** Linear ReplayGain multiplier applied on transition (1 = no change). */
  replayGain: number
  /** True when the bytes come from the network (podcast/remote stream via the
   *  loopback /stream proxy). Drives the wake mode: remote items hold a WifiLock
   *  (WAKE_MODE_NETWORK), local library tracks only the CPU lock (LOCAL). */
  remote: boolean
}

export interface NativeAudioPlugin {
  /** Load a single media URL (legacy/fallback); prefer setQueue on mobile. */
  load(o: { url: string; autoplay: boolean; remote?: boolean }): Promise<void>
  /**
   * Replace the whole native playlist and start at startIndex. ExoPlayer then
   * owns the queue and auto-advances (gapless) in the background without the
   * WebView — the core of reliable background "next track" playback.
   */
  setQueue(o: { items: NativeQueueItem[]; startIndex: number; autoplay: boolean }): Promise<void>
  /** Replace the items after the current one (cut point derived natively). */
  updateUpcoming(o: { items: NativeQueueItem[] }): Promise<void>
  /** Native repeat mode: 'off' | 'all' | 'one'. */
  setRepeatMode(o: { mode: string }): Promise<void>
  play(): Promise<void>
  pause(): Promise<void>
  seek(o: { position: number }): Promise<void>
  /** User volume 0..1 (multiplied by the ReplayGain linear factor natively). */
  setVolume(o: { volume: number }): Promise<void>
  setRate(o: { rate: number }): Promise<void>
  stop(): Promise<void>
  /** Hint the next track URL for warm buffering (null clears it). */
  preload(o: { url: string | null }): Promise<void>
  /** 10 band gains in dB (-12..12) + master enable for the EQ AudioProcessor. */
  setEq(o: { gains: number[]; enabled: boolean }): Promise<void>
  /** Linear ReplayGain multiplier (1 = no change). */
  setReplayGain(o: { linear: number }): Promise<void>
  /**
   * Crossfade duration in seconds (0..12; 0 disables) for automatic track
   * transitions. A short-lived second ExoPlayer overlaps the outgoing track's
   * tail with equal-power volume ramps; manual skip/seek/pause cancel it and
   * podcasts/streams (unknown duration) never trigger it.
   */
  setCrossfade(o: { seconds: number }): Promise<void>
  /**
   * Experimental DSP audio offload (default off). Only engages while the EQ is
   * off/flat and crossfade is 0; the native side rebuilds the player when the
   * effective mode changes (brief gap on toggle).
   */
  setOffload(o: { enabled: boolean }): Promise<void>

  addListener(
    event: 'play' | 'pause' | 'ended' | 'timeupdate' | 'loaded' | 'loaderror' | 'transition',
    cb: (data: {
      position?: number
      duration?: number
      message?: string
      index?: number
      mediaId?: string
    }) => void
  ): Promise<{ remove: () => Promise<void> }>
}

export const NativeAudio = registerPlugin<NativeAudioPlugin>('NativeAudio')

/** True only when the native NativeAudio plugin is registered in this APK. */
export function nativeAudioAvailable(): boolean {
  try {
    return Capacitor.isPluginAvailable('NativeAudio')
  } catch {
    return false
  }
}
