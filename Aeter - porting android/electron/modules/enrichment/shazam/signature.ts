// Vendored port of the reverse-engineered Shazam signature algorithm
// (references: github.com/asivery/node-shazam-api, marin-m/SongRec,
// shazamio). Input: PCM s16le mono @16000 Hz. Output: the base64
// `data:audio/vnd.shazam.sig` URI the amp.shazam.com endpoint accepts.
// Zero npm dependencies (the published packages are neither Node-12-safe
// nor maintained); pure typed-array code shared desktop/Android.

import { RealFFT, hanningShazam } from './dsp'

const SAMPLE_RATE = 16000
const SAMPLE_RATE_ID_16000 = 3
const FFT_SIZE = 2048
const HOP = 128
const BINS = FFT_SIZE / 2 + 1 // 1025
const MAX_SECONDS = 12

export interface FrequencyPeak {
  fftPassNumber: number
  peakMagnitude: number
  correctedPeakFrequencyBin: number
}

const DATA_URI_PREFIX = 'data:audio/vnd.shazam.sig;base64,'

// Standard zlib CRC-32 (poly 0xEDB88320), table-based.
const CRC_TABLE = ((): Uint32Array => {
  const table = new Uint32Array(256)
  for (let n = 0; n < 256; n++) {
    let c = n
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
    table[n] = c >>> 0
  }
  return table
})()

function crc32(buf: Uint8Array): number {
  let crc = 0 ^ -1
  for (let i = 0; i < buf.length; i++) crc = (crc >>> 8) ^ CRC_TABLE[(crc ^ buf[i]) & 0xff]
  return (crc ^ -1) >>> 0
}

/** The generated signature plus the metadata the recognize request needs. */
export class ShazamSignature {
  numberSamples = 0
  /** Peaks per frequency band id (0: 250-520Hz, 1: 520-1450, 2: 1450-3500, 3: 3500-5500). */
  readonly bands = new Map<number, FrequencyPeak[]>()

  /** Sample length in ms, sent alongside the signature. */
  samplems(): number {
    return Math.round((this.numberSamples / SAMPLE_RATE) * 1000)
  }

  /**
   * Binary layout (all little-endian): 48-byte header (magic 0xCAFE2580,
   * crc32 of everything after byte 8, payload size, magic 0x94119C00, sample
   * rate id << 27, numberSamples + rate*0.24, fixed 0x7C0000), then a
   * 0x40000000 TLV wrapper and one block per band (type 0x60030040+band,
   * length, delta-encoded peaks, zero padding to 4 bytes).
   */
  encodeToBinary(): Buffer {
    const contents: number[] = []
    const bandIds = [...this.bands.keys()].sort((a, b) => a - b)
    for (const band of bandIds) {
      const peaks: number[] = []
      let fftPassNumber = 0
      for (const peak of this.bands.get(band) as FrequencyPeak[]) {
        if (peak.fftPassNumber - fftPassNumber >= 0xff) {
          peaks.push(0xff)
          pushUint32LE(peaks, peak.fftPassNumber)
          fftPassNumber = peak.fftPassNumber
        }
        peaks.push(peak.fftPassNumber - fftPassNumber)
        pushUint16LE(peaks, peak.peakMagnitude - 1)
        pushUint16LE(peaks, peak.correctedPeakFrequencyBin - 1)
        fftPassNumber = peak.fftPassNumber
      }
      pushUint32LE(contents, (0x60030040 + band) >>> 0)
      pushUint32LE(contents, peaks.length)
      contents.push(...peaks)
      const padding = 4 - (peaks.length % 4)
      if (padding < 4) for (let i = 0; i < padding; i++) contents.push(0)
    }

    const buf = Buffer.alloc(48 + 8 + contents.length)
    buf.writeUInt32LE(0xcafe2580, 0) // magic1
    // crc32 written last (offset 4)
    buf.writeUInt32LE(contents.length + 8, 8) // size minus header
    buf.writeUInt32LE(0x94119c00, 12) // magic2
    // 16..27: reserved zeros
    buf.writeUInt32LE((SAMPLE_RATE_ID_16000 << 27) >>> 0, 28)
    // 32..39: reserved zeros
    buf.writeUInt32LE(Math.round(this.numberSamples + SAMPLE_RATE * 0.24), 40)
    buf.writeUInt32LE((15 << 19) + 0x40000, 44) // 0x7C0000
    buf.writeUInt32LE(0x40000000, 48)
    buf.writeUInt32LE(contents.length + 8, 52)
    Buffer.from(contents).copy(buf, 56)
    buf.writeUInt32LE(crc32(buf.slice(8)), 4)
    return buf
  }

  encodeToUri(): string {
    return DATA_URI_PREFIX + this.encodeToBinary().toString('base64')
  }
}

function pushUint16LE(arr: number[], v: number): void {
  arr.push(v & 0xff, (v >>> 8) & 0xff)
}

function pushUint32LE(arr: number[], v: number): void {
  arr.push(v & 0xff, (v >>> 8) & 0xff, (v >>> 16) & 0xff, (v >>> 24) & 0xff)
}

const pyMod = (a: number, b: number): number => (a % b >= 0 ? a % b : b + (a % b))

/**
 * Spectral-peak extraction over a sliding 2048-sample FFT with a 128-sample
 * hop. Faithful port of the reference generator: squared-magnitude spectra in
 * a 256-entry ring, peak spreading over frequency (3 bins) and time (-1/-3/-6
 * frames), then peak recognition 46 frames back with neighborhood suppression.
 */
export class SignatureGenerator {
  private readonly fft = new RealFFT(FFT_SIZE)
  private readonly window = hanningShazam(FFT_SIZE)
  private readonly fftRe = new Float64Array(BINS)
  private readonly fftIm = new Float64Array(BINS)
  private readonly windowed = new Float64Array(FFT_SIZE)

  private samplesRing!: Float64Array
  private samplesPos!: number
  private fftOutputs!: Float64Array[]
  private fftPos!: number
  private spreadOutputs!: Float64Array[]
  private spreadPos!: number
  private numSpreadFftsDone!: number
  private signature!: ShazamSignature

  constructor() {
    this.reset()
  }

  private reset(): void {
    this.samplesRing = new Float64Array(FFT_SIZE)
    this.samplesPos = 0
    this.fftOutputs = Array.from({ length: 256 }, () => new Float64Array(BINS))
    this.fftPos = 0
    this.spreadOutputs = Array.from({ length: 256 }, () => new Float64Array(BINS))
    this.spreadPos = 0
    this.numSpreadFftsDone = 0
    this.signature = new ShazamSignature()
  }

  /** Consumes up to 12s of s16le mono 16kHz samples (center-sliced if longer). */
  getSignature(s16leMonoSamples: Int16Array): ShazamSignature {
    let samples = s16leMonoSamples
    const maxSamples = MAX_SECONDS * SAMPLE_RATE
    if (samples.length > maxSamples) {
      const middle = Math.floor(samples.length / 2)
      samples = samples.subarray(middle - maxSamples / 2, middle + maxSamples / 2)
    }

    this.signature.numberSamples += samples.length
    const wholeHops = samples.length - (samples.length % HOP)
    for (let i = 0; i < wholeHops; i += HOP) {
      this.doFFT(samples, i)
      this.doPeakSpreading()
      this.numSpreadFftsDone++
      if (this.numSpreadFftsDone >= 46) this.doPeakRecognition()
    }
    const result = this.signature
    this.reset()
    return result
  }

  private doFFT(samples: Int16Array, offset: number): void {
    const ring = this.samplesRing
    for (let i = 0; i < HOP; i++) ring[this.samplesPos + i] = samples[offset + i]
    this.samplesPos = (this.samplesPos + HOP) % FFT_SIZE

    const windowed = this.windowed
    const pos = this.samplesPos
    for (let i = 0; i < FFT_SIZE; i++) {
      windowed[i] = ring[(pos + i) % FFT_SIZE] * this.window[i]
    }
    this.fft.transform(windowed, this.fftRe, this.fftIm)

    const out = this.fftOutputs[pyMod(this.fftPos++, 256)]
    for (let i = 0; i < BINS; i++) {
      const e = (this.fftRe[i] * this.fftRe[i] + this.fftIm[i] * this.fftIm[i]) / (1 << 17)
      out[i] = Math.max(0.0000000001, e)
    }
  }

  private doPeakSpreading(): void {
    const originLastFFT = this.fftOutputs[pyMod(this.fftPos - 1, 256)]
    const spreadLastFFT = this.spreadOutputs[pyMod(this.spreadPos, 256)]
    spreadLastFFT.set(originLastFFT)

    for (let position = 0; position <= 1022; position++) {
      spreadLastFFT[position] = Math.max(
        spreadLastFFT[position],
        spreadLastFFT[position + 1],
        spreadLastFFT[position + 2]
      )
    }
    for (const formerFftNum of [-1, -3, -6]) {
      const former = this.spreadOutputs[pyMod(this.spreadPos + formerFftNum, 256)]
      for (let position = 0; position < BINS; position++) {
        former[position] = Math.max(former[position], spreadLastFFT[position])
      }
    }
    this.spreadPos++
  }

  private doPeakRecognition(): void {
    const fftMinus46 = this.fftOutputs[pyMod(this.fftPos - 46, 256)]
    const fftMinus49 = this.spreadOutputs[pyMod(this.spreadPos - 49, 256)]

    for (let binPosition = 10; binPosition <= 1014; binPosition++) {
      if (fftMinus46[binPosition] < 1 / 64 || fftMinus46[binPosition] < fftMinus49[binPosition - 1]) {
        continue
      }

      let maxNeighborInFftMinus49 = 0
      for (const neighborOffset of [-10, -7, -4, -3, 1, 2, 5, 8]) {
        maxNeighborInFftMinus49 = Math.max(fftMinus49[binPosition + neighborOffset], maxNeighborInFftMinus49)
      }
      if (fftMinus46[binPosition] <= maxNeighborInFftMinus49) continue

      let maxNeighborInOtherAdjacentFFTs = maxNeighborInFftMinus49
      for (const otherOffset of [-53, -45, 165, 172, 179, 186, 193, 200, 214, 221, 228, 235, 242, 249]) {
        const other = this.spreadOutputs[pyMod(this.spreadPos + otherOffset, 256)]
        maxNeighborInOtherAdjacentFFTs = Math.max(other[binPosition - 1], maxNeighborInOtherAdjacentFFTs)
      }
      if (fftMinus46[binPosition] <= maxNeighborInOtherAdjacentFFTs) continue

      const fftNumber = this.numSpreadFftsDone - 46
      const peakMagnitude = Math.log(Math.max(1 / 64, fftMinus46[binPosition])) * 1477.3 + 6144
      const peakMagnitudeBefore = Math.log(Math.max(1 / 64, fftMinus46[binPosition - 1])) * 1477.3 + 6144
      const peakMagnitudeAfter = Math.log(Math.max(1 / 64, fftMinus46[binPosition + 1])) * 1477.3 + 6144

      const peakVariation1 = peakMagnitude * 2 - peakMagnitudeBefore - peakMagnitudeAfter
      if (peakVariation1 <= 0) continue
      const peakVariation2 = ((peakMagnitudeAfter - peakMagnitudeBefore) * 32) / peakVariation1
      const correctedPeakFrequencyBin = ((binPosition * 64 + peakVariation2) & 0xffff) >>> 0

      const frequencyHz = correctedPeakFrequencyBin * (SAMPLE_RATE / 2 / 1024 / 64)
      let band: number
      if (frequencyHz < 250) continue
      else if (frequencyHz < 520) band = 0
      else if (frequencyHz < 1450) band = 1
      else if (frequencyHz < 3500) band = 2
      else if (frequencyHz <= 5500) band = 3
      else continue

      let peaks = this.signature.bands.get(band)
      if (!peaks) {
        peaks = []
        this.signature.bands.set(band, peaks)
      }
      peaks.push({
        fftPassNumber: fftNumber,
        peakMagnitude: Math.round(peakMagnitude) & 0xffff,
        correctedPeakFrequencyBin: Math.round(correctedPeakFrequencyBin)
      })
    }
  }
}
