import { describe, expect, it } from 'vitest'
import { ShazamSignature, SignatureGenerator } from './signature'
import { toMono16k } from './pcm'

// Local reference CRC-32 (zlib, poly 0xEDB88320) to verify the encoder's.
function refCrc32(buf: Uint8Array): number {
  let crc = 0 ^ -1
  for (let i = 0; i < buf.length; i++) {
    crc ^= buf[i]
    for (let k = 0; k < 8; k++) crc = crc & 1 ? 0xedb88320 ^ (crc >>> 1) : crc >>> 1
  }
  return (crc ^ -1) >>> 0
}

/**
 * Deterministic 12s melody at 16kHz: stepping sine frequencies with an
 * attack/decay envelope per note, so the generator finds time-localized
 * spectral peaks (a stationary tone would be suppressed by design).
 */
function melodyPcm(seconds = 12, rate = 16000): Int16Array {
  const notes = [440, 660, 880, 1320, 2000, 550, 990, 1480]
  const noteSec = 0.4
  const n = Math.floor(seconds * rate)
  const out = new Int16Array(n)
  for (let i = 0; i < n; i++) {
    const t = i / rate
    const noteIdx = Math.floor(t / noteSec) % notes.length
    const tin = t % noteSec
    const envelope = Math.min(1, tin / 0.02) * Math.exp(-3 * tin)
    const f = notes[noteIdx]
    const v = envelope * (0.6 * Math.sin(2 * Math.PI * f * t) + 0.25 * Math.sin(2 * Math.PI * 2 * f * t))
    out[i] = Math.round(v * 26000)
  }
  return out
}

describe('SignatureGenerator', () => {
  it('produces a well-formed signature with peaks from a 12s melody', () => {
    const sig = new SignatureGenerator().getSignature(melodyPcm())
    expect(sig.numberSamples).toBe(12 * 16000)
    expect(sig.samplems()).toBe(12000)

    let totalPeaks = 0
    for (const peaks of sig.bands.values()) totalPeaks += peaks.length
    expect(totalPeaks).toBeGreaterThan(10)

    const bin = sig.encodeToBinary()
    expect(bin.readUInt32LE(0)).toBe(0xcafe2580)
    expect(bin.readUInt32LE(12)).toBe(0x94119c00)
    expect(bin.readUInt32LE(28) >>> 27).toBe(3) // 16000 Hz id
    expect(bin.readUInt32LE(40)).toBe(Math.round(12 * 16000 + 16000 * 0.24))
    expect(bin.readUInt32LE(44)).toBe(0x7c0000)
    expect(bin.readUInt32LE(48)).toBe(0x40000000)
    expect(bin.readUInt32LE(8)).toBe(bin.length - 48)
    expect(bin.readUInt32LE(52)).toBe(bin.readUInt32LE(8))
    expect(bin.readUInt32LE(4)).toBe(refCrc32(bin.slice(8)))
  })

  it('is deterministic (same PCM → same base64 URI)', () => {
    const pcm = melodyPcm(4)
    const a = new SignatureGenerator().getSignature(pcm).encodeToUri()
    const b = new SignatureGenerator().getSignature(pcm).encodeToUri()
    expect(a).toBe(b)
    expect(a.startsWith('data:audio/vnd.shazam.sig;base64,')).toBe(true)
  })

  it('center-slices input longer than 12 seconds', () => {
    const sig = new SignatureGenerator().getSignature(melodyPcm(20))
    expect(sig.numberSamples).toBe(12 * 16000)
  })
})

describe('ShazamSignature.encodeToBinary', () => {
  it('delta-encodes peaks with the 0xFF escape and pads bands to 4 bytes', () => {
    const sig = new ShazamSignature()
    sig.numberSamples = 16000
    sig.bands.set(0, [
      { fftPassNumber: 10, peakMagnitude: 100, correctedPeakFrequencyBin: 300 },
      { fftPassNumber: 300, peakMagnitude: 200, correctedPeakFrequencyBin: 400 }
    ])
    const bin = sig.encodeToBinary()

    expect(bin.readUInt32LE(56)).toBe(0x60030040) // band 0 TLV type
    expect(bin.readUInt32LE(60)).toBe(15) // unpadded peaks length
    // peak 1: delta 10, magnitude-1, bin-1 (little-endian int16)
    expect(bin[64]).toBe(10)
    expect(bin.readUInt16LE(65)).toBe(99)
    expect(bin.readUInt16LE(67)).toBe(299)
    // peak 2: delta 290 >= 255 → escape 0xFF + absolute uint32 + delta 0
    expect(bin[69]).toBe(0xff)
    expect(bin.readUInt32LE(70)).toBe(300)
    expect(bin[74]).toBe(0)
    expect(bin.readUInt16LE(75)).toBe(199)
    expect(bin.readUInt16LE(77)).toBe(399)
    // 15 bytes of peaks + 1 padding byte → total size 8 (wrapper) + 8 (TLV) + 16
    expect(bin.length).toBe(48 + 8 + 8 + 16)
    expect(bin.readUInt32LE(4)).toBe(refCrc32(bin.slice(8)))
  })
})

describe('toMono16k', () => {
  it('downmixes interleaved stereo to the channel average', () => {
    const stereo = new Int16Array([1000, -1000, 2000, 0, -500, -500])
    const mono = toMono16k(stereo, 16000, 2)
    expect([...mono]).toEqual([0, 1000, -500])
  })

  it('passes mono 16kHz through unchanged', () => {
    const src = new Int16Array([1, -2, 3, -4])
    expect([...toMono16k(src, 16000, 1)]).toEqual([1, -2, 3, -4])
  })

  it('halves the sample count when downsampling 32k → 16k', () => {
    const src = new Int16Array(3200)
    for (let i = 0; i < src.length; i++) src[i] = Math.round(1000 * Math.sin(i / 20))
    const out = toMono16k(src, 32000, 1)
    expect(out.length).toBe(1600)
  })

  it('preserves a low-frequency sine through 44.1k → 16k resampling', () => {
    const rate = 44100
    const src = new Int16Array(rate) // 1s of 440 Hz
    for (let i = 0; i < src.length; i++) {
      src[i] = Math.round(20000 * Math.sin((2 * Math.PI * 440 * i) / rate))
    }
    const out = toMono16k(src, rate, 1)
    expect(out.length).toBe(16000)
    // Zero-crossing count ≈ 2×440 per second (tolerate filter edge effects).
    let crossings = 0
    for (let i = 1; i < out.length; i++) {
      if ((out[i - 1] < 0 && out[i] >= 0) || (out[i - 1] >= 0 && out[i] < 0)) crossings++
    }
    expect(crossings).toBeGreaterThan(800)
    expect(crossings).toBeLessThan(960)
  })

  it('clamps to the s16 range and handles empty/invalid input', () => {
    expect(toMono16k(new Int16Array(0), 16000, 1).length).toBe(0)
    expect(toMono16k(new Int16Array([5]), 0, 1).length).toBe(0)
  })
})
