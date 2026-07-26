import { useState } from 'react'

/**
 * Shared artwork <img>: renders `fallback` when there is no src OR the load
 * fails (purged cover cache, stale hash), instead of the broken-image glyph.
 * Lazy + non-draggable by default, with a light fade-in once decoded so grids
 * don't pop. Call sites keep their own sized/rounded container — this renders
 * only the <img> (or the fallback node), so the DOM stays as before.
 */
export default function CoverImage({
  src,
  alt = '',
  className,
  fallback = null,
  eager = false
}: {
  src: string | null | undefined
  alt?: string
  className?: string
  fallback?: React.ReactNode
  eager?: boolean
}): React.JSX.Element {
  // Keyed by src so a track/cover change on a reused <img> retries and
  // re-fades instead of sticking to the previous src's state.
  const [failedSrc, setFailedSrc] = useState<string | null>(null)
  const [loadedSrc, setLoadedSrc] = useState<string | null>(null)

  if (!src || src === failedSrc) return <>{fallback}</>

  return (
    <img
      src={src}
      alt={alt}
      className={className}
      draggable={false}
      loading={eager ? undefined : 'lazy'}
      decoding="async"
      style={{ opacity: src === loadedSrc ? 1 : 0, transition: 'opacity 200ms ease' }}
      onLoad={() => setLoadedSrc(src)}
      onError={() => setFailedSrc(src)}
    />
  )
}
