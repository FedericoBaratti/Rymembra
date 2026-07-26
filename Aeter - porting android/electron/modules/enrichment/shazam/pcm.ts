// Platform seam for PCM decoding (mirrors setTagWriteBack / setYtdlpRunner):
// desktop defaults to ffmpeg (pcmFfmpeg.ts), Android installs a MediaCodec
// decoder via node-backend. Plus the pure mono/16kHz conversion the Shazam
// signature needs, kept here so it is unit-testable without any decoder.

export interface PcmResult {
  /** Interleaved s16le samples at the decoder's native rate/channels. */
  samples: Int16Array
  sampleRate: number
  channels: number
}

/** Decodes `durationSec` seconds of audio starting at `offsetSec`. Null = unavailable. */
export type PcmDecoder = (
  path: string,
  offsetSec: number,
  durationSec: number
) => Promise<PcmResult | null>

let decoder: PcmDecoder | null = null

export function setPcmDecoder(fn: PcmDecoder | null): void {
  decoder = fn
}

export function getPcmDecoder(): PcmDecoder | null {
  return decoder
}

const TARGET_RATE = 16000

/**
 * Downmix interleaved multi-channel s16le to mono and resample to 16000 Hz
 * (moving-average low-pass sized to the decimation ratio, then linear
 * interpolation). Good enough for fingerprinting: Shazam's peaks live well
 * below the Nyquist of 16kHz and survive mild aliasing.
 */
export function toMono16k(samples: Int16Array, srcRate: number, channels: number): Int16Array {
  if (!srcRate || srcRate <= 0) return new Int16Array(0)
  let mono: Float32Array
  if (channels <= 1) {
    mono = new Float32Array(samples.length)
    for (let i = 0; i < samples.length; i++) mono[i] = samples[i]
  } else {
    const frames = Math.floor(samples.length / channels)
    mono = new Float32Array(frames)
    for (let i = 0; i < frames; i++) {
      let acc = 0
      const base = i * channels
      for (let c = 0; c < channels; c++) acc += samples[base + c]
      mono[i] = acc / channels
    }
  }

  if (srcRate === TARGET_RATE) return clampToInt16(mono)

  // Anti-alias low-pass before downsampling: moving average over the ratio.
  const win = Math.floor(srcRate / TARGET_RATE)
  if (win > 1) {
    const filtered = new Float32Array(mono.length)
    let acc = 0
    for (let i = 0; i < mono.length; i++) {
      acc += mono[i]
      if (i >= win) acc -= mono[i - win]
      filtered[i] = acc / Math.min(i + 1, win)
    }
    mono = filtered
  }

  const outLen = Math.floor((mono.length * TARGET_RATE) / srcRate)
  const out = new Int16Array(outLen)
  const step = srcRate / TARGET_RATE
  for (let i = 0; i < outLen; i++) {
    const pos = i * step
    const i0 = Math.floor(pos)
    const frac = pos - i0
    const next = i0 + 1 < mono.length ? mono[i0 + 1] : mono[i0]
    const s = mono[i0] * (1 - frac) + next * frac
    out[i] = s > 32767 ? 32767 : s < -32768 ? -32768 : Math.round(s)
  }
  return out
}

function clampToInt16(mono: Float32Array): Int16Array {
  const out = new Int16Array(mono.length)
  for (let i = 0; i < mono.length; i++) {
    const s = mono[i]
    out[i] = s > 32767 ? 32767 : s < -32768 ? -32768 : Math.round(s)
  }
  return out
}
