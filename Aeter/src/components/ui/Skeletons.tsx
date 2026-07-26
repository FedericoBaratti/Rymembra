import MediaGrid from '@/components/ui/MediaGrid'

/**
 * Shared loading placeholders built on the `.skeleton` (shimmer) class from
 * global.css. Shown while the library store has `loaded === false` so the
 * first paint is structured instead of a blank page or a misleading
 * EmptyState (see useLibraryStore.loaded). Purely visual, no data.
 */

/** Card grid (Albums/Playlists): square cover + two text lines. */
export function GridSkeleton({ count = 12, dense = false }: { count?: number; dense?: boolean }): React.JSX.Element {
  return (
    <div className="min-h-0 flex-1 overflow-hidden px-[var(--content-x)] pt-1" aria-hidden>
      <MediaGrid dense={dense}>
        {Array.from({ length: count }).map((_, i) => (
          <div key={i} className="flex w-full flex-col gap-2 p-3">
            <div className="skeleton aspect-square w-full rounded-lg" />
            <div className="skeleton h-3.5 w-3/4 rounded" />
            <div className="skeleton h-3 w-1/2 rounded" />
          </div>
        ))}
      </MediaGrid>
    </div>
  )
}

/** Track row list: cover chip + two text lines. */
export function ListSkeleton({ count = 12 }: { count?: number }): React.JSX.Element {
  return (
    <div className="flex flex-col gap-2 px-[var(--content-x)] pt-1" aria-hidden>
      {Array.from({ length: count }).map((_, i) => (
        <div key={i} className="flex items-center gap-3">
          <div className="skeleton h-9 w-9 shrink-0 rounded" />
          <div className="flex min-w-0 flex-1 flex-col gap-1.5">
            <div className="skeleton h-3.5 w-1/2 rounded" />
            <div className="skeleton h-3 w-1/3 rounded" />
          </div>
        </div>
      ))}
    </div>
  )
}
