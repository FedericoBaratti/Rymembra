// Minimal DSP for the Shazam signature: an iterative radix-2 complex FFT
// (real input) and the Hanning window the algorithm expects. Pure typed-array
// code, zero dependencies, Node-12-safe (no runtime APIs beyond Math).

/**
 * The reference implementations window with numpy's `hanning(2050)[1:-1]`:
 * w[n] = 0.5 - 0.5*cos(2*pi*n/(M-1)) evaluated at n = 1..2048 with M = 2050.
 */
export function hanningShazam(size: number): Float64Array {
  const out = new Float64Array(size)
  const m = size + 2
  for (let i = 0; i < size; i++) {
    out[i] = 0.5 - 0.5 * Math.cos((2 * Math.PI * (i + 1)) / (m - 1))
  }
  return out
}

/**
 * Iterative Cooley-Tukey FFT for real input of power-of-two length. Writes the
 * first size/2+1 bins (the non-redundant half for real input) into outRe/outIm.
 * Tables (bit-reversal, twiddles) are precomputed once per instance.
 */
export class RealFFT {
  private readonly rev: Uint32Array
  private readonly cos: Float64Array
  private readonly sin: Float64Array
  private readonly re: Float64Array
  private readonly im: Float64Array

  constructor(readonly size: number) {
    if ((size & (size - 1)) !== 0 || size < 2) throw new Error(`FFT size must be a power of two, got ${size}`)
    this.rev = new Uint32Array(size)
    let bits = 0
    while (1 << bits < size) bits++
    for (let i = 0; i < size; i++) {
      let r = 0
      for (let b = 0; b < bits; b++) if (i & (1 << b)) r |= 1 << (bits - 1 - b)
      this.rev[i] = r
    }
    const half = size / 2
    this.cos = new Float64Array(half)
    this.sin = new Float64Array(half)
    for (let k = 0; k < half; k++) {
      this.cos[k] = Math.cos((2 * Math.PI * k) / size)
      this.sin[k] = Math.sin((2 * Math.PI * k) / size)
    }
    this.re = new Float64Array(size)
    this.im = new Float64Array(size)
  }

  /** input.length === size; outRe/outIm.length >= size/2+1. */
  transform(input: Float64Array, outRe: Float64Array, outIm: Float64Array): void {
    const { size, rev, cos, sin, re, im } = this
    for (let i = 0; i < size; i++) {
      re[i] = input[rev[i]]
      im[i] = 0
    }
    for (let len = 2; len <= size; len <<= 1) {
      const half = len >> 1
      const step = size / len
      for (let i = 0; i < size; i += len) {
        for (let j = 0; j < half; j++) {
          const k = j * step
          const c = cos[k]
          const s = sin[k]
          const bRe = re[i + j + half]
          const bIm = im[i + j + half]
          // twiddle e^{-2πik/N} = c - i·s
          const tRe = c * bRe + s * bIm
          const tIm = c * bIm - s * bRe
          re[i + j + half] = re[i + j] - tRe
          im[i + j + half] = im[i + j] - tIm
          re[i + j] += tRe
          im[i + j] += tIm
        }
      }
    }
    const bins = size / 2 + 1
    for (let i = 0; i < bins; i++) {
      outRe[i] = re[i]
      outIm[i] = im[i]
    }
  }
}
