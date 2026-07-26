/**
 * `sharp` stand-in for nodejs-mobile.
 *
 * sharp depends on libvips (impractical to cross-compile for android-arm64).
 * It is used only by electron/modules/coverArt.ts to resize cover art to
 * 512px/64px WebP. This shim records the requested resize/encode and delegates
 * the actual work to the native ImageResize plugin (Android Bitmap → WebP) via
 * the reverse-RPC layer (callNative → src/lib/nativeRpc.ts → ImageResizePlugin.kt).
 *
 * If no resize was requested the original bytes are returned unchanged. The
 * node-backend Vite build aliases `sharp` → this file.
 */
import { callNative } from './runtime'
import { join } from 'node:path'
import { tmpdir } from 'node:os'
import { writeFile, readFile, unlink } from 'node:fs/promises'
import { randomBytes } from 'node:crypto'

interface SharpLike {
  metadata(): Promise<{ width?: number; height?: number }>
  resize(width?: number, height?: number, opts?: unknown): SharpLike
  webp(opts?: { quality?: number }): SharpLike
  png(...args: unknown[]): SharpLike
  jpeg(...args: unknown[]): SharpLike
  toBuffer(): Promise<Buffer>
}

function sharp(input: Buffer): SharpLike {
  let width = 0
  let height = 0
  let quality = 80
  const self: SharpLike = {
    async metadata() {
      // Real dimensions via the native probe (inJustDecodeBounds — no full
      // decode) so coverArt's size/aspect gate works on Android too. Fail-open:
      // a probe hiccup returns {} and must not discard an otherwise valid cover.
      const id = randomBytes(16).toString('hex')
      const srcPath = join(tmpdir(), `probe_${id}.tmp`)
      try {
        await writeFile(srcPath, input)
        const res = (await callNative('imageProbe', { srcPath })) as {
          width?: number
          height?: number
        } | null
        if (res && res.width && res.height) return { width: res.width, height: res.height }
        return {}
      } catch (err) {
        // stderr reaches logcat (console.log does not)
        console.error('[sharp-shim] imageProbe fallito, gate dimensioni saltato:', err)
        return {}
      } finally {
        await unlink(srcPath).catch(() => {})
      }
    },
    resize(w?: number, h?: number) {
      if (w) width = w
      if (h) height = h
      return self
    },
    webp(opts?: { quality?: number }) {
      if (opts?.quality != null) quality = opts.quality
      return self
    },
    png() {
      return self
    },
    jpeg() {
      return self
    },
    async toBuffer() {
      if (!width && !height) return input
      
      // nodejs-mobile's Node lacks crypto.randomUUID; randomBytes is available
      // and a hex token is unique enough for a temp filename.
      const id = randomBytes(16).toString('hex')
      const srcPath = join(tmpdir(), `resize_in_${id}.tmp`)
      const destPath = join(tmpdir(), `resize_out_${id}.webp`)
      
      await writeFile(srcPath, input)
      try {
        await callNative('imageResize', {
          srcPath,
          destPath,
          width: width || height,
          height: height || width,
          quality
        })
        return await readFile(destPath)
      } finally {
        await unlink(srcPath).catch(() => {})
        await unlink(destPath).catch(() => {})
      }
    }
  }
  return self
}

export default sharp
