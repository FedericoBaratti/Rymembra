import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { Search, Music2, Disc3, Users } from 'lucide-react'
import type { SearchResults } from '@shared/types'
import { useUiStore } from '@/store/useUiStore'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import { coverUrl, formatDuration } from '@/lib/format'

function Highlight({ text, term }: { text: string; term: string }): React.JSX.Element {
  if (!term) return <>{text}</>
  const idx = text.toLowerCase().indexOf(term.toLowerCase())
  if (idx < 0) return <>{text}</>
  return (
    <>
      {text.slice(0, idx)}
      <mark className="rounded-sm bg-[var(--accent-soft)] px-0.5 text-[var(--accent)]">
        {text.slice(idx, idx + term.length)}
      </mark>
      {text.slice(idx + term.length)}
    </>
  )
}

export default function SearchOverlay(): React.JSX.Element | null {
  const { t } = useTranslation()
  const navigate = useNavigate()
  const open = useUiStore((s) => s.searchOpen)
  const setOpen = useUiStore((s) => s.setSearchOpen)
  const playTracks = usePlayerStore((s) => s.playTracks)
  const [term, setTerm] = useState('')
  const [results, setResults] = useState<SearchResults | null>(null)
  const inputRef = useRef<HTMLInputElement>(null)
  const debounce = useRef<number | null>(null)

  useEffect(() => {
    if (open) {
      setTerm('')
      setResults(null)
      setTimeout(() => inputRef.current?.focus(), 30)
    }
  }, [open])

  useEffect(() => {
    if (!open) return
    if (debounce.current) window.clearTimeout(debounce.current)
    if (!term.trim()) {
      setResults(null)
      return
    }
    debounce.current = window.setTimeout(() => {
      void window.aether.search(term).then(setResults)
    }, 180)
  }, [term, open])

  const trapRef = useFocusTrap<HTMLDivElement>(open, () => setOpen(false))

  if (!open) return null

  const close = (): void => setOpen(false)
  const hasResults =
    results && (results.tracks.length > 0 || results.albums.length > 0 || results.artists.length > 0)

  return (
    <div
      className="overlay-in fixed inset-0 z-50 flex items-start justify-center bg-black/55 pt-[10vh] backdrop-blur-sm"
      onClick={close}
    >
      <div
        ref={trapRef}
        role="dialog"
        aria-modal="true"
        aria-label={t('search.placeholder')}
        className="glass-modal scale-in flex max-h-[70vh] w-[min(640px,calc(100vw-48px))] flex-col overflow-hidden rounded-2xl"
        style={{ boxShadow: '0 24px 80px rgba(0,0,0,0.7)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center gap-3 border-b px-4" style={{ borderColor: 'var(--hairline)' }}>
          <Search size={17} className="text-text-3" />
          <input
            ref={inputRef}
            className="h-12 flex-1 bg-transparent text-[15px] outline-none placeholder:text-text-3"
            placeholder={t('search.placeholder')}
            value={term}
            onChange={(e) => setTerm(e.target.value)}
          />
          <kbd className="rounded border px-1.5 py-0.5 text-[10px] text-text-3" style={{ borderColor: 'var(--hairline)' }}>
            ESC
          </kbd>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {results && !hasResults && (
            <div className="px-3 py-8 text-center text-[13px] text-text-3">
              {t('search.no_results', { term })}
            </div>
          )}

          {results && results.tracks.length > 0 && (
            <section className="mb-2">
              <h3 className="flex items-center gap-1.5 px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-text-3">
                <Music2 size={11} /> {t('search.tracks')}
              </h3>
              {results.tracks.slice(0, 8).map((track, i) => {
                const thumb = coverUrl(track.cover_art_hash, true)
                return (
                  <button
                    key={track.id}
                    className="flex w-full items-center gap-3 rounded-lg px-3 py-1.5 text-left transition-colors hover:bg-white/[0.06]"
                    onDoubleClick={() => {
                      playTracks(results.tracks, i)
                      close()
                    }}
                    onClick={() => {
                      playTracks(results.tracks, i)
                      close()
                    }}
                  >
                    <div className="h-8 w-8 shrink-0 overflow-hidden rounded bg-surface-3">
                      {thumb && <img src={thumb} alt="" className="h-full w-full object-cover" />}
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-[13px] font-medium">
                        <Highlight text={track.title} term={term} />
                      </div>
                      <div className="truncate text-[11.5px] text-text-3">
                        <Highlight text={track.artist} term={term} /> —{' '}
                        <Highlight text={track.album} term={term} />
                      </div>
                    </div>
                    <span className="tnum text-[11px] text-text-3">
                      {formatDuration(track.duration)}
                    </span>
                  </button>
                )
              })}
            </section>
          )}

          {results && results.albums.length > 0 && (
            <section className="mb-2">
              <h3 className="flex items-center gap-1.5 px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-text-3">
                <Disc3 size={11} /> {t('search.albums')}
              </h3>
              {results.albums.slice(0, 5).map((album) => {
                const thumb = coverUrl(album.cover_art_hash, true)
                return (
                  <button
                    key={album.id}
                    className="flex w-full items-center gap-3 rounded-lg px-3 py-1.5 text-left transition-colors hover:bg-white/[0.06]"
                    onClick={() => {
                      navigate(`/albums/${album.id}`)
                      close()
                    }}
                  >
                    <div className="h-8 w-8 shrink-0 overflow-hidden rounded bg-surface-3">
                      {thumb && <img src={thumb} alt="" className="h-full w-full object-cover" />}
                    </div>
                    <div className="min-w-0 flex-1">
                      <div className="truncate text-[13px] font-medium">
                        <Highlight text={album.title} term={term} />
                      </div>
                      <div className="truncate text-[11.5px] text-text-3">
                        <Highlight text={album.artist} term={term} />
                        {album.year ? ` · ${album.year}` : ''}
                      </div>
                    </div>
                  </button>
                )
              })}
            </section>
          )}

          {results && results.artists.length > 0 && (
            <section>
              <h3 className="flex items-center gap-1.5 px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-text-3">
                <Users size={11} /> {t('search.artists')}
              </h3>
              {results.artists.slice(0, 5).map((artist) => (
                <button
                  key={artist.id}
                  className="flex w-full items-center gap-3 rounded-lg px-3 py-1.5 text-left transition-colors hover:bg-white/[0.06]"
                  onClick={() => {
                    navigate(`/artists/${encodeURIComponent(artist.name)}`)
                    close()
                  }}
                >
                  <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full bg-surface-3">
                    <Users size={13} className="text-text-3" />
                  </div>
                  <div className="truncate text-[13px] font-medium">
                    <Highlight text={artist.name} term={term} />
                  </div>
                </button>
              ))}
            </section>
          )}
        </div>
      </div>
    </div>
  )
}
