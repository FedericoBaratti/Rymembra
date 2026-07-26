import { useTranslation } from 'react-i18next'
import { Music2, Play, Download, Check, Loader2 } from 'lucide-react'
import type { Track, ExternalRecoTrack } from '@shared/types'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useExternalDownload } from '@/hooks/useExternalDownload'
import { coverUrl, remoteImageUrl } from '@/lib/format'
import { tap } from '@/lib/haptics'
import { isMobile } from '@/lib/platform'
import CoverImage from '@/components/ui/CoverImage'

/** Horizontal, snap-scrolling row used by every Home section. */
function HorizontalRow({ children }: { children: React.ReactNode }): React.JSX.Element {
  return (
    // overscroll-x-contain: a fine riga il gesto non deve propagarsi alla
    // pagina (o al back-swipe di sistema su Android).
    <div className="flex gap-3 overflow-x-auto overscroll-x-contain pb-2 [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
      {children}
    </div>
  )
}

export function SectionHeading({ title }: { title: string }): React.JSX.Element {
  return <h2 className="section-heading mb-2 px-[var(--content-x)] text-[15px] font-bold tracking-tight">{title}</h2>
}

function TrackCard({
  track,
  onPlay
}: {
  track: Track
  onPlay: () => void
}): React.JSX.Element {
  const cover = coverUrl(track.cover_art_hash)
  return (
    <button
      className="card-lift group fade-in flex w-[140px] shrink-0 flex-col gap-2 rounded-xl p-2 text-left hover:bg-white/[0.05]"
      onClick={() => {
        tap()
        onPlay()
      }}
    >
      <div className="relative aspect-square w-full overflow-hidden rounded-lg bg-surface-3 shadow-lg">
        <CoverImage
          src={cover}
          className="h-full w-full object-cover"
          fallback={
            <div className="flex h-full w-full items-center justify-center">
              <Music2 size={28} className="text-text-3" />
            </div>
          }
        />
        <div className="absolute inset-0 flex items-end justify-end p-2">
          <span
            className="card-play flex h-9 w-9 items-center justify-center rounded-full text-white"
            style={{ background: 'var(--accent)', boxShadow: '0 4px 20px var(--accent-glow)' }}
          >
            <Play size={15} fill="currentColor" className="ml-0.5" />
          </span>
        </div>
      </div>
      <div>
        <div className="truncate text-[12.5px] font-semibold">{track.title}</div>
        <div className="truncate text-[11.5px] text-text-3">{track.artist}</div>
      </div>
    </button>
  )
}

/** A row of owned tracks; clicking a card plays the row from that position. */
export function TrackCarousel({ tracks }: { tracks: Track[] }): React.JSX.Element {
  const playTracks = usePlayerStore((s) => s.playTracks)
  return (
    <HorizontalRow>
      {tracks.map((track, i) => (
        <TrackCard key={track.id} track={track} onPlay={() => playTracks(tracks, i)} />
      ))}
    </HorizontalRow>
  )
}

function ExternalCard({ track }: { track: ExternalRecoTrack }): React.JSX.Element {
  const { t } = useTranslation()
  // Icon tracks the real queue status (not fire-and-forget): a failed download
  // flips back to downloadable; useExternalDownload also owns the error toasts.
  const { state, start } = useExternalDownload(track)

  const download = async (): Promise<void> => {
    tap()
    await start()
  }

  const cover = track.coverUrl ? remoteImageUrl(track.coverUrl) : null
  return (
    <div className="card-lift fade-in flex w-[140px] shrink-0 flex-col gap-2 rounded-xl p-2">
      <div className="relative flex aspect-square w-full items-center justify-center overflow-hidden rounded-lg bg-surface-3 shadow-lg">
        <CoverImage
          src={cover}
          className="h-full w-full object-cover"
          fallback={<Music2 size={28} className="text-text-3" />}
        />
        <button
          className={`card-play absolute bottom-2 right-2 flex items-center justify-center rounded-full text-white disabled:opacity-70 ${
            isMobile ? 'h-11 w-11' : 'h-9 w-9'
          }`}
          style={{ background: 'var(--accent)', boxShadow: '0 4px 20px var(--accent-glow)' }}
          onClick={() => void download()}
          disabled={state !== 'idle'}
          aria-label={t('discover.download')}
        >
          {state === 'busy' ? (
            <Loader2 size={15} className="animate-spin" />
          ) : state === 'done' ? (
            <Check size={15} />
          ) : (
            <Download size={15} />
          )}
        </button>
      </div>
      <div>
        <div className="truncate text-[12.5px] font-semibold">{track.title}</div>
        <div className="truncate text-[11.5px] text-text-3">{track.artist}</div>
      </div>
    </div>
  )
}

/** A row of external (downloadable) recommendations. */
export function ExternalCarousel({ tracks }: { tracks: ExternalRecoTrack[] }): React.JSX.Element {
  return (
    <HorizontalRow>
      {tracks.map((track, i) => (
        <ExternalCard key={`${track.artist}-${track.title}-${i}`} track={track} />
      ))}
    </HorizontalRow>
  )
}
