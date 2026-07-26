// Pure, dependency-free cover validation so it can be unit-tested in plain
// node and reused on Android (where the sharp shim can't report dimensions).

// Real cover art is never this small; anything below is a 1x1 placeholder, a
// truncated download, or an HTML/JSON error body served with a 200.
export const MIN_COVER_BYTES = 1500
export const MIN_COVER_PX = 200

// Covers are square-ish; a 16:9 frame (1.78) is a video thumbnail, not album
// art. Reject — never crop — anything outside [1/1.45, 1.45]: a cropped video
// frame is still the wrong cover, and a NULL hash lets the enrichment pass
// fetch the real one later.
export const MAX_COVER_RATIO = 1.45

/** Pure aspect-ratio gate shared by desktop (sharp) and Android (native probe). */
export function isAcceptableCoverRatio(width: number, height: number): boolean {
  if (!width || !height) return false
  const ratio = width / height
  return ratio <= MAX_COVER_RATIO && ratio >= 1 / MAX_COVER_RATIO
}

/**
 * Sniffs the magic bytes of a raster image. Dimension-independent, so it works
 * on Android where the sharp shim can't report width/height. Rejects HTML/JSON
 * error pages and other non-image payloads.
 */
export function sniffImageType(buf: Buffer): boolean {
  if (buf.length < 12) return false
  // JPEG
  if (buf[0] === 0xff && buf[1] === 0xd8 && buf[2] === 0xff) return true
  // PNG
  if (buf[0] === 0x89 && buf[1] === 0x50 && buf[2] === 0x4e && buf[3] === 0x47) return true
  // GIF
  if (buf[0] === 0x47 && buf[1] === 0x49 && buf[2] === 0x46) return true
  // BMP
  if (buf[0] === 0x42 && buf[1] === 0x4d) return true
  // WebP: "RIFF"????"WEBP"
  if (
    buf[0] === 0x52 &&
    buf[1] === 0x49 &&
    buf[2] === 0x46 &&
    buf[3] === 0x46 &&
    buf[8] === 0x57 &&
    buf[9] === 0x45 &&
    buf[10] === 0x42 &&
    buf[11] === 0x50
  )
    return true
  return false
}

/**
 * Best-effort validity check for a downloaded cover. Cheap and ICU-free: byte
 * length + image magic bytes (no decode), so it's identical on desktop and
 * Android. storeCover() additionally enforces a minimum pixel size where the
 * platform can report it.
 */
export function validateCoverBuffer(buffer: Buffer): boolean {
  return !!buffer && buffer.length >= MIN_COVER_BYTES && sniffImageType(buffer)
}
