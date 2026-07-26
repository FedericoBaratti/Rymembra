import { Howl } from 'howler'
import type { Track } from '@shared/types'
import { mediaUrl } from './format'
import { audioGraph } from './audio'

interface Loaded {
  howl: Howl
  track: Track
}

export interface PlayerEngineCallbacks {
  onEnd: () => void
  onPlay: (track: Track) => void
  onPause: () => void
  onLoadError: (track: Track, message: string) => void
}

function formatOf(track: Track): string {
  // strip any query string (stream URLs) before reading the extension
  const clean = (track.stream_url ?? track.path).split('?')[0]
  const ext = clean.split('.').pop()?.toLowerCase() ?? 'mp3'
  return ext === 'aif' ? 'aiff' : ext
}

/** Audio source for a track: a remote stream (podcast episode) when present,
 *  otherwise the local media server. Library tracks have no stream_url, so this
 *  is byte-for-byte the previous behaviour for them. Desktop plays the remote URL
 *  directly (no cleartext block, no proxy). */
function srcFor(track: Track): string {
  return track.stream_url ?? mediaUrl(track.id)
}

/**
 * Owns Howl instances; the play queue lives in the Zustand store, which calls
 * into this engine. Supports crossfade and gapless preload of the next track.
 */
class PlayerEngine {
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

  configure(opts: { crossfadeSec?: number; rgEnabled?: boolean; rgTargetDb?: number }): void {
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
      // Remote streams (podcast episodes) must use the HTML5 Audio path for
      // progressive streaming + CORS; local tracks keep Web Audio (html5:false)
      // so the EQ/ReplayGain/visualizer graph stays intact.
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

  /** Load and play a track, crossfading from the previous one if configured. */
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

  /** Preload the next track without playing it (gapless). */
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

export const playerEngine = new PlayerEngine()
