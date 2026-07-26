import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Search, Check, Music2, Plus } from 'lucide-react'
import type { Track } from '@shared/types'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'
import { coverUrl, formatDuration } from '@/lib/format'
import { isMobile } from '@/lib/platform'
import BottomSheet from '@/components/ui/BottomSheet'
import CoverImage from '@/components/ui/CoverImage'

/**
 * Track picker to add songs to a playlist from within the playlist detail page
 * (previously only reachable through the library context menu). Searches the
 * library (FTS when a term is typed, recent tracks otherwise), supports
 * multi-selection, then calls addToPlaylist. Bottom sheet on mobile, modal on
 * desktop.
 */
function PickerContent({
  playlistId,
  onAdded,
  onClose
}: {
  playlistId: number
  onAdded: () => void
  onClose: () => void
}): React.JSX.Element {
  const { t } = useTranslation()
  const [term, setTerm] = useState('')
  const [results, setResults] = useState<Track[]>([])
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [busy, setBusy] = useState(false)

  useEffect(() => {
    let cancelled = false
    const handle = setTimeout(() => {
      const load = term.trim()
        ? window.aether.search(term.trim()).then((r) => r.tracks)
        : window.aether.getTracks({ sortBy: 'date_added', sortDir: 'desc', limit: 100 })
      void load
        .then((tracks) => {
          if (!cancelled) setResults(tracks)
        })
        .catch(() => {
          // Backend reject (booting / dead): show the empty state, not a hang.
          if (!cancelled) setResults([])
        })
    }, 250)
    return () => {
      cancelled = true
      clearTimeout(handle)
    }
  }, [term])

  const toggle = (id: number): void => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const confirm = async (): Promise<void> => {
    if (selected.size === 0) return
    setBusy(true)
    try {
      await window.aether.addToPlaylist(playlistId, [...selected])
      toast.success(t('playlists.tracks_added', { count: selected.size }))
      onAdded()
      onClose()
    } catch (e) {
      toast.error(ipcErrorMessage(e))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="px-4 pb-2">
        <div className="relative">
          <Search size={15} className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-text-3" />
          <input
            autoFocus={!isMobile}
            type="search"
            enterKeyHint="search"
            className="field-input h-10 w-full pl-9 text-[13px]"
            placeholder={t('search.placeholder')}
            value={term}
            onChange={(e) => setTerm(e.target.value)}
          />
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-2">
        {results.map((tr) => {
          const checked = selected.has(tr.id)
          const thumb = coverUrl(tr.cover_art_hash, true)
          return (
            <button
              key={tr.id}
              className={`flex w-full items-center gap-2.5 rounded-lg px-2 text-left text-[13px] transition-colors ${
                isMobile ? 'py-2.5' : 'py-2'
              } ${checked ? 'bg-[var(--accent-soft)]' : 'hover:bg-white/[0.045]'}`}
              onClick={() => toggle(tr.id)}
            >
              <span
                className={`flex h-5 w-5 shrink-0 items-center justify-center rounded-md border ${
                  checked ? 'border-[var(--accent)] bg-[var(--accent)] text-white' : 'border-white/25'
                }`}
              >
                {checked && <Check size={13} />}
              </span>
              <div className={`${isMobile ? 'h-11 w-11' : 'h-9 w-9'} shrink-0 overflow-hidden rounded bg-surface-3`}>
                <CoverImage
                  src={thumb}
                  className="h-full w-full object-cover"
                  fallback={
                    <div className="flex h-full w-full items-center justify-center">
                      <Music2 size={13} className="text-text-3" />
                    </div>
                  }
                />
              </div>
              <div className="min-w-0 flex-1">
                <div className="truncate font-medium">{tr.title}</div>
                <div className="truncate text-[11.5px] text-text-3">
                  {tr.artist} — {tr.album}
                </div>
              </div>
              <span className="tnum text-[11.5px] text-text-3">{formatDuration(tr.duration)}</span>
            </button>
          )
        })}
        {results.length === 0 && (
          <div className="px-3 py-8 text-center text-[12.5px] text-text-3">{t('playlists.no_results')}</div>
        )}
      </div>

      <div className="flex shrink-0 items-center justify-between gap-3 px-4 pt-3">
        <span className="text-[12px] text-text-3">{t('playlists.selected_count', { count: selected.size })}</span>
        <button
          className="btn-accent flex items-center gap-1.5 rounded-lg px-4 py-2 text-[13px]"
          onClick={() => void confirm()}
          disabled={busy || selected.size === 0}
        >
          <Plus size={15} /> {t('playlists.add_tracks')}
        </button>
      </div>
    </div>
  )
}

export default function AddTracksPicker({
  playlistId,
  onAdded,
  onClose
}: {
  playlistId: number
  onAdded: () => void
  onClose: () => void
}): React.JSX.Element {
  const { t } = useTranslation()
  const trapRef = useFocusTrap<HTMLDivElement>(!isMobile, onClose)

  if (isMobile) {
    return (
      <BottomSheet open onClose={onClose} title={t('playlists.add_tracks')}>
        <PickerContent playlistId={playlistId} onAdded={onAdded} onClose={onClose} />
      </BottomSheet>
    )
  }

  return (
    <div
      className="overlay-in fixed inset-0 z-50 flex items-center justify-center bg-black/55 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        ref={trapRef}
        role="dialog"
        aria-modal="true"
        aria-label={t('playlists.add_tracks')}
        className="glass-modal scale-in flex max-h-[80vh] w-[min(560px,calc(100vw-48px))] flex-col overflow-hidden rounded-2xl py-4"
        style={{ boxShadow: '0 24px 80px rgba(0,0,0,0.7)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="px-4 pb-2 text-[15px] font-bold">{t('playlists.add_tracks')}</div>
        <PickerContent playlistId={playlistId} onAdded={onAdded} onClose={onClose} />
      </div>
    </div>
  )
}
