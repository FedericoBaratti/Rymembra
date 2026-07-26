import { Howl } from 'howler'
import type { Track, RepeatMode } from '@shared/types'
import { mediaUrl, coverUrl, streamProxyUrl } from './format'
import { audioGraph } from './audio'
import { isMobile } from './platform'
import { isLanModeActive } from './lanClient'
import { NativeAudio, nativeAudioAvailable, type NativeQueueItem } from './nativeAudio'

interface Loaded {
  howl: Howl
  track: Track
}

/** A queue entry handed to the native playlist: the track plus a stable id
 *  (the JS queue index as a string) used to reconcile transitions back to the
 *  store's orderPos. */
export interface QueueEntry {
  track: Track
  mediaId: string
}

export interface PlayerEngineCallbacks {
  onEnd: () => void
  onPlay: (track: Track) => void
  onPause: () => void
  onLoadError: (track: Track, message: string) => void
  /** Native queue auto-advanced (or was seeked) to another item. mediaId is the
   *  stable per-entry id; the store maps it back to its orderPos. Mobile only. */
  onTransition: (mediaId: string, index: number) => void
}

/**
 * Public surface the player store (and a few helpers) drive. Two backends
 * implement it: HowlerEngine on desktop (Web Audio graph → EQ/ReplayGain/viz),
 * and NativeEngine on Android (Media3 ExoPlayer with a native EQ). position()
 * and duration() stay synchronous on both so the store needs no changes.
 */
export interface PlayerEngine {
  setCallbacks(cbs: PlayerEngineCallbacks): void
  /** offloadEnabled is the experimental Android DSP-offload toggle; ignored on desktop. */
  configure(opts: {
    crossfadeSec?: number
    rgEnabled?: boolean
    rgTargetDb?: number
    offloadEnabled?: boolean
  }): void
  play(track: Track): void
  /** Mobile: hand the whole play order to the native ExoPlayer playlist and start
   *  at startIndex, so it auto-advances gapless in the background. No-op on desktop. */
  setQueue(entries: QueueEntry[], startIndex: number): void
  /** Mobile: replace the items after the current one without restarting it
   *  (enqueue / play-next / reorder / shuffle). No-op on desktop. */
  updateUpcoming(entries: QueueEntry[]): void
  /** Mobile: native repeat mode. No-op on desktop (the store handles repeat). */
  setRepeatMode(mode: RepeatMode): void
  preload(track: Track | null): void
  pause(): void
  resume(): void
  stop(): void
  seek(seconds: number): void
  position(): number
  duration(): number
  isPlaying(): boolean
  currentTrackId(): number | null
  setVolume(v: number, muted: boolean): void
  setRate(rate: number): void
  fadeOutAndPause(seconds: number): void
}

function formatOf(track: Track): string {
  // strip any query string (stream URLs) before reading the extension
  const clean = track.path.split('?')[0]
  const ext = clean.split('.').pop()?.toLowerCase() ?? 'mp3'
  return ext === 'aif' ? 'aiff' : ext
}

/** Audio source for a track: a remote stream (podcast episode) when present,
 *  otherwise the local media server. Library tracks have no stream_url, so this
 *  is byte-for-byte the previous behaviour for them (zero regression). On mobile
 *  the remote stream is routed through the loopback proxy (cleartext/CORS/Range);
 *  desktop plays the remote URL directly (no cleartext block, no media server). */
function srcFor(track: Track): string {
  if (track.stream_url) return isMobile ? streamProxyUrl(track.stream_url) : track.stream_url
  return mediaUrl(track.id)
}

/** Network-fed items need ExoPlayer's WAKE_MODE_NETWORK (WifiLock): podcast/
 *  remote streams always, and — in LAN thin-client mode — every library track
 *  too, since its bytes come from the desktop over Wi-Fi. Without the WifiLock
 *  background playback stalls when the radio sleeps on screen-off. */
function isRemoteSource(track: Track): boolean {
  return !!track.stream_url || isLanModeActive()
}

/** REPLAYGAIN_TRACK_GAIN (dB) → linear multiplier, clamped like audioGraph. */
function replayGainLinear(trackGainDb: number | null, enabled: boolean, targetDb: number): number {
  let gainDb = 0
  if (enabled && trackGainDb != null) {
    gainDb = Math.max(-24, Math.min(12, trackGainDb + (targetDb - -18)))
  }
  return Math.pow(10, gainDb / 20)
}

/**
 * Desktop engine: owns Howl instances; the play queue lives in the Zustand
 * store, which calls into this engine. Supports crossfade and gapless preload
 * of the next track, and feeds Howler's master gain into the Web Audio graph.
 */
class HowlerEngine implements PlayerEngine {
  private current: Loaded | null = null
  private preloaded: Loaded | null = null
  private cbs: PlayerEngineCallbacks | null = null
  private volume = 0.8
  private muted = false
  private rate = 1
  private crossfadeSec = 0
  private rgEnabled = false
  private rgTargetDb = -18
  private fadeOutTimer: number | null = null

  setCallbacks(cbs: PlayerEngineCallbacks): void {
    this.cbs = cbs
  }

  configure(opts: {
    crossfadeSec?: number
    rgEnabled?: boolean
    rgTargetDb?: number
    offloadEnabled?: boolean
  }): void {
    // offloadEnabled is native-only (Android DSP): nothing to do here.
    if (opts.crossfadeSec != null) this.crossfadeSec = opts.crossfadeSec
    if (opts.rgEnabled != null) this.rgEnabled = opts.rgEnabled
    if (opts.rgTargetDb != null) this.rgTargetDb = opts.rgTargetDb
    this.applyReplayGain()
  }

  private applyReplayGain(): void {
    audioGraph.setReplayGain(
      this.current?.track.replaygain_track_gain ?? null,
      this.rgEnabled,
      this.rgTargetDb
    )
  }

  private createHowl(track: Track, autoplay: boolean): Loaded {
    const howl = new Howl({
      src: [srcFor(track)],
      format: [formatOf(track)],
      // Desktop keeps Web Audio (html5:false) so the EQ/ReplayGain/visualizer
      // graph still applies. Remote streams (podcasts) use HTML5 audio instead,
      // which streams progressively and avoids the Web Audio CORS fetch.
      html5: !!track.stream_url,
      autoplay,
      volume: autoplay && this.crossfadeSec > 0 ? 0 : this.effectiveVolume(),
      rate: this.rate,
      onplay: () => {
        audioGraph.ensure()
        if (this.current?.howl === howl) this.cbs?.onPlay(track)
      },
      onpause: () => {
        if (this.current?.howl === howl) this.cbs?.onPause()
      },
      onend: () => {
        if (this.current?.howl === howl) this.cbs?.onEnd()
      },
      onloaderror: (_id, err) => {
        this.cbs?.onLoadError(track, String(err))
      },
      onplayerror: (_id, err) => {
        this.cbs?.onLoadError(track, String(err))
      }
    })
    return { howl, track }
  }

  private effectiveVolume(): number {
    return this.muted ? 0 : this.volume
  }

  play(track: Track): void {
    if (this.fadeOutTimer != null) {
      window.clearTimeout(this.fadeOutTimer)
      this.fadeOutTimer = null
    }

    const old = this.current

    if (this.preloaded?.track.id === track.id) {
      this.current = this.preloaded
      this.preloaded = null
      this.current.howl.volume(this.crossfadeSec > 0 && old ? 0 : this.effectiveVolume())
      this.current.howl.rate(this.rate)
      this.current.howl.play()
    } else {
      this.discard(this.preloaded)
      this.preloaded = null
      this.current = this.createHowl(track, true)
    }

    if (old) {
      if (this.crossfadeSec > 0) {
        const ms = this.crossfadeSec * 1000
        old.howl.fade(old.howl.volume() as number, 0, ms)
        this.current.howl.fade(0, this.effectiveVolume(), ms)
        const toDiscard = old
        this.fadeOutTimer = window.setTimeout(() => this.discard(toDiscard), ms + 100)
      } else {
        this.discard(old)
      }
    }

    this.applyReplayGain()
  }

  // Desktop has no native playlist: the Zustand store drives the queue and calls
  // play()/preload() per track, so these are no-ops here.
  setQueue(): void {}
  updateUpcoming(): void {}
  setRepeatMode(): void {}

  preload(track: Track | null): void {
    if (!track) {
      this.discard(this.preloaded)
      this.preloaded = null
      return
    }
    if (this.preloaded?.track.id === track.id) return
    this.discard(this.preloaded)
    this.preloaded = this.createHowl(track, false)
  }

  private discard(loaded: Loaded | null | undefined): void {
    if (!loaded) return
    loaded.howl.off()
    loaded.howl.unload()
  }

  pause(): void {
    this.current?.howl.pause()
  }

  resume(): void {
    this.current?.howl.play()
  }

  stop(): void {
    if (this.fadeOutTimer != null) window.clearTimeout(this.fadeOutTimer)
    this.discard(this.current)
    this.discard(this.preloaded)
    this.current = null
    this.preloaded = null
  }

  seek(seconds: number): void {
    this.current?.howl.seek(seconds)
  }

  position(): number {
    const pos = this.current?.howl.seek()
    return typeof pos === 'number' ? pos : 0
  }

  duration(): number {
    return this.current?.howl.duration() ?? this.current?.track.duration ?? 0
  }

  isPlaying(): boolean {
    return this.current?.howl.playing() ?? false
  }

  currentTrackId(): number | null {
    return this.current?.track.id ?? null
  }

  setVolume(v: number, muted: boolean): void {
    this.volume = v
    this.muted = muted
    this.current?.howl.volume(this.effectiveVolume())
  }

  setRate(rate: number): void {
    this.rate = rate
    this.current?.howl.rate(rate)
  }

  /** Fade out over `seconds`, then pause (sleep timer). */
  fadeOutAndPause(seconds: number): void {
    const howl = this.current?.howl
    if (!howl) return
    howl.fade(howl.volume() as number, 0, seconds * 1000)
    this.fadeOutTimer = window.setTimeout(() => {
      howl.pause()
      howl.volume(this.effectiveVolume())
    }, seconds * 1000 + 50)
  }
}

/**
 * Android engine: delegates playback to the native Media3/ExoPlayer plugin so
 * the audio runs through a native session with a real 10-band EQ. Position and
 * duration are cached from native `timeupdate`/`loaded` events so position()/
 * duration() stay synchronous (the store calls them directly). Crossfade on
 * automatic track transitions runs natively (tail-player overlap in the plugin);
 * ReplayGain is applied as a volume factor.
 */
class NativeEngine implements PlayerEngine {
  private current: Track | null = null
  private cbs: PlayerEngineCallbacks | null = null
  private volume = 0.8
  private muted = false
  private rate = 1
  private rgEnabled = false
  private rgTargetDb = -18
  private cachedPosition = 0
  private cachedDuration = 0
  private playing = false
  private fadeTimer: number | null = null

  constructor() {
    if (!nativeAudioAvailable()) return
    void NativeAudio.addListener('play', () => {
      this.playing = true
      if (this.current) this.cbs?.onPlay(this.current)
    })
    void NativeAudio.addListener('pause', () => {
      this.playing = false
      this.cbs?.onPause()
    })
    void NativeAudio.addListener('ended', () => {
      // STATE_ENDED only fires at the END of the whole native playlist (auto-
      // advance between items does not). Let the store finalize / stop.
      this.playing = false
      this.cbs?.onEnd()
    })
    void NativeAudio.addListener('transition', (d) => {
      // The native ExoPlayer queue advanced (or was seeked) to another item —
      // possibly while the WebView was frozen in the background. Reset the cached
      // position and let the store reconcile its orderPos / scrobbling by mediaId.
      this.cachedPosition = 0
      this.playing = true
      if (d.mediaId != null) this.cbs?.onTransition(d.mediaId, d.index ?? -1)
    })
    void NativeAudio.addListener('timeupdate', (d) => {
      if (typeof d.position === 'number') this.cachedPosition = d.position
    })
    void NativeAudio.addListener('loaded', (d) => {
      if (typeof d.duration === 'number' && d.duration > 0) this.cachedDuration = d.duration
    })
    void NativeAudio.addListener('loaderror', (d) => {
      if (this.current) this.cbs?.onLoadError(this.current, d.message ?? 'load error')
    })
  }

  setCallbacks(cbs: PlayerEngineCallbacks): void {
    this.cbs = cbs
  }

  configure(opts: {
    crossfadeSec?: number
    rgEnabled?: boolean
    rgTargetDb?: number
    offloadEnabled?: boolean
  }): void {
    // Native crossfade (tail-player overlap) applies to automatic track transitions.
    if (opts.crossfadeSec != null) void NativeAudio.setCrossfade({ seconds: opts.crossfadeSec })
    if (opts.offloadEnabled != null) void NativeAudio.setOffload({ enabled: opts.offloadEnabled })
    if (opts.rgEnabled != null) this.rgEnabled = opts.rgEnabled
    if (opts.rgTargetDb != null) this.rgTargetDb = opts.rgTargetDb
    this.applyReplayGain()
  }

  private applyReplayGain(): void {
    const linear = replayGainLinear(
      this.current?.replaygain_track_gain ?? null,
      this.rgEnabled,
      this.rgTargetDb
    )
    void NativeAudio.setReplayGain({ linear })
  }

  private effectiveVolume(): number {
    return this.muted ? 0 : this.volume
  }

  play(track: Track): void {
    this.clearFade()
    this.current = track
    this.cachedPosition = 0
    this.cachedDuration = track.duration ?? 0
    this.playing = true
    void NativeAudio.setVolume({ volume: this.effectiveVolume() })
    void NativeAudio.setRate({ rate: this.rate })
    void NativeAudio.load({ url: srcFor(track), autoplay: true, remote: isRemoteSource(track) })
    this.applyReplayGain()
  }

  private toNativeItem(entry: QueueEntry): NativeQueueItem {
    const t = entry.track
    return {
      mediaId: entry.mediaId,
      url: srcFor(t),
      title: t.title,
      artist: t.artist,
      album: t.album,
      artworkUrl: t.stream_cover_url ?? coverUrl(t.cover_art_hash) ?? '',
      duration: t.duration ?? 0,
      // ReplayGain is applied per-item natively on each transition.
      replayGain: replayGainLinear(t.replaygain_track_gain ?? null, this.rgEnabled, this.rgTargetDb),
      remote: isRemoteSource(t)
    }
  }

  setQueue(entries: QueueEntry[], startIndex: number): void {
    this.clearFade()
    const first = entries[startIndex]?.track ?? entries[0]?.track ?? null
    this.current = first
    this.cachedPosition = 0
    this.cachedDuration = first?.duration ?? 0
    this.playing = true
    void NativeAudio.setVolume({ volume: this.effectiveVolume() })
    void NativeAudio.setRate({ rate: this.rate })
    void NativeAudio.setQueue({
      items: entries.map((e) => this.toNativeItem(e)),
      startIndex: Math.max(0, startIndex),
      autoplay: true
    })
  }

  updateUpcoming(entries: QueueEntry[]): void {
    void NativeAudio.updateUpcoming({ items: entries.map((e) => this.toNativeItem(e)) })
  }

  setRepeatMode(mode: RepeatMode): void {
    void NativeAudio.setRepeatMode({ mode })
  }

  preload(track: Track | null): void {
    void NativeAudio.preload({ url: track ? srcFor(track) : null })
  }

  pause(): void {
    this.playing = false
    void NativeAudio.pause()
  }

  resume(): void {
    this.playing = true
    void NativeAudio.play()
  }

  stop(): void {
    this.clearFade()
    this.current = null
    this.playing = false
    this.cachedPosition = 0
    this.cachedDuration = 0
    void NativeAudio.stop()
  }

  seek(seconds: number): void {
    this.cachedPosition = seconds
    void NativeAudio.seek({ position: seconds })
  }

  position(): number {
    return this.cachedPosition
  }

  duration(): number {
    return this.cachedDuration || this.current?.duration || 0
  }

  isPlaying(): boolean {
    return this.playing
  }

  currentTrackId(): number | null {
    return this.current?.id ?? null
  }

  setVolume(v: number, muted: boolean): void {
    this.volume = v
    this.muted = muted
    void NativeAudio.setVolume({ volume: this.effectiveVolume() })
  }

  setRate(rate: number): void {
    this.rate = rate
    void NativeAudio.setRate({ rate })
  }

  /** JS-side volume ramp (native has no fade), then pause (sleep timer). */
  fadeOutAndPause(seconds: number): void {
    this.clearFade()
    const steps = Math.max(1, Math.round(seconds * 10))
    let step = 0
    const startVol = this.effectiveVolume()
    this.fadeTimer = window.setInterval(() => {
      step++
      const v = startVol * (1 - step / steps)
      void NativeAudio.setVolume({ volume: Math.max(0, v) })
      if (step >= steps) {
        this.clearFade()
        void NativeAudio.pause()
        void NativeAudio.setVolume({ volume: this.effectiveVolume() })
      }
    }, 100)
  }

  private clearFade(): void {
    if (this.fadeTimer != null) {
      window.clearInterval(this.fadeTimer)
      this.fadeTimer = null
    }
  }
}

export const playerEngine: PlayerEngine = isMobile ? new NativeEngine() : new HowlerEngine()
