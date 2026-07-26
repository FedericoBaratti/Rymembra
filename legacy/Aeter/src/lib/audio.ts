import { Howler } from 'howler'

export const EQ_FREQUENCIES = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000]

export const EQ_PRESETS: Record<string, number[]> = {
  Flat: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  'Bass Boost': [6, 5, 4, 2.5, 1, 0, 0, 0, 0, 0],
  'Treble Boost': [0, 0, 0, 0, 0, 1, 2.5, 4, 5, 6],
  Vocal: [-2, -1, 0, 2, 4, 4, 3, 1, 0, -1],
  Classical: [0, 0, 0, 0, 0, 0, -2, -2, -2, -3],
  Electronic: [4.5, 4, 1.5, 0, -1.5, 1.5, 1, 1.5, 4, 4.5],
  Rock: [4, 3, 1, -1.5, -2.5, -1, 1.5, 3, 3.5, 3.5],
  Pop: [-1, 1, 3, 4, 3, 0, -1, -1, 1, 2]
}

/**
 * Shared Web Audio graph appended after Howler's master gain:
 *   Howler.masterGain -> EQ x10 -> replay gain -> analyser -> destination
 */
class AudioGraph {
  private filters: BiquadFilterNode[] = []
  private rgGain: GainNode | null = null
  private analyserNode: AnalyserNode | null = null
  private eqGains: number[] = new Array(10).fill(0)
  private eqEnabled = false
  private built = false

  private build(): void {
    if (this.built) return
    const ctx = Howler.ctx
    const master = Howler.masterGain
    if (!ctx || !master) return

    this.filters = EQ_FREQUENCIES.map((freq, i) => {
      const f = ctx.createBiquadFilter()
      if (i === 0) f.type = 'lowshelf'
      else if (i === EQ_FREQUENCIES.length - 1) f.type = 'highshelf'
      else {
        f.type = 'peaking'
        f.Q.value = 1.1
      }
      f.frequency.value = freq
      f.gain.value = 0
      return f
    })

    this.rgGain = ctx.createGain()
    this.analyserNode = ctx.createAnalyser()
    this.analyserNode.fftSize = 2048
    this.analyserNode.smoothingTimeConstant = 0.8

    master.disconnect()
    let node: AudioNode = master
    for (const f of this.filters) {
      node.connect(f)
      node = f
    }
    node.connect(this.rgGain)
    this.rgGain.connect(this.analyserNode)
    this.analyserNode.connect(ctx.destination)
    this.built = true
    this.applyEq()
  }

  /** Must be called after the first Howl is created (Howler.ctx exists). */
  ensure(): void {
    this.build()
  }

  get analyser(): AnalyserNode | null {
    this.build()
    return this.analyserNode
  }

  setEq(gains: number[], enabled: boolean): void {
    this.eqGains = gains.slice(0, 10)
    this.eqEnabled = enabled
    this.applyEq()
  }

  private applyEq(): void {
    if (!this.built) return
    const ctx = Howler.ctx!
    this.filters.forEach((f, i) => {
      const target = this.eqEnabled ? (this.eqGains[i] ?? 0) : 0
      f.gain.setTargetAtTime(target, ctx.currentTime, 0.05)
    })
  }

  /**
   * Apply ReplayGain. trackGainDb is the REPLAYGAIN_TRACK_GAIN value (dB, ref 89 dB / -18 LUFS).
   * targetDb shifts the reference (e.g. -18 keeps tag as-is, -14 adds +4 dB).
   */
  setReplayGain(trackGainDb: number | null, enabled: boolean, targetDb: number): void {
    this.build()
    if (!this.rgGain) return
    let gainDb = 0
    if (enabled && trackGainDb != null) {
      gainDb = Math.max(-24, Math.min(12, trackGainDb + (targetDb - -18)))
    }
    const linear = Math.pow(10, gainDb / 20)
    this.rgGain.gain.setTargetAtTime(linear, Howler.ctx!.currentTime, 0.1)
  }
}

export const audioGraph = new AudioGraph()
