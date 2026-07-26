import { useEffect, useRef, useState } from 'react'
import { useNavigate } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import {
  Search,
  Music2,
  Disc3,
  Users,
  Globe,
  Download,
  Loader2,
  Check,
  RefreshCw,
  X,
  Play,
  ListStart,
  ListEnd,
  ListMusic,
  Heart
} from 'lucide-react'
import type { SearchResults, ExternalRecoTrack, Track } from '@shared/types'
import { useUiStore } from '@/store/useUiStore'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useLibraryStore } from '@/store/useLibraryStore'
import { toast } from '@/store/useToastStore'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import { useLongPress } from '@/hooks/useLongPress'
import { useExternalDownload } from '@/hooks/useExternalDownload'
import { coverUrl, remoteImageUrl, formatDuration } from '@/lib/format'
import { isMobile } from '@/lib/platform'
import { impact } from '@/lib/haptics'
import FullScreenSheet from '@/components/ui/FullScreenSheet'
import BottomSheet from '@/components/ui/BottomSheet'
import SheetAction from '@/components/ui/SheetAction'
import CoverImage from '@/components/ui/CoverImage'

/** A web (Deezer) result the user can download into the library with one tap.
 *  The icon tracks the REAL queue status (via useExternalDownload), so a
 *  download that fails later flips back to downloadable instead of lying
 *  with a green check. */
function ExternalRow({ track }: { track: ExternalRecoTrack }): React.JSX.Element {
  const { t } = useTranslation()
  const { state, start } = useExternalDownload(track)
  const cover = track.coverUrl ? remoteImageUrl(track.coverUrl) : null

  return (
    <div className="flex w-full items-center gap-3 rounded-lg px-3 py-1.5">
      <div className="flex h-8 w-8 shrink-0 items-center justify-center overflow-hidden rounded bg-surface-3">
        <CoverImage
          src={cover}
          className="h-full w-full object-cover"
          fallback={<Music2 size={13} className="text-text-3" />}
        />
      </div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px] font-medium">{track.title}</div>
        <div className="truncate text-[11.5px] text-text-3">{track.artist}</div>
      </div>
      <button
        className={`icon-btn shrink-0 disabled:opacity-60 ${isMobile ? 'h-11 w-11' : 'h-8 w-8'}`}
        onClick={() => void start()}
        disabled={state !== 'idle'}
        aria-label={t('discover.download')}
      >
        {state === 'busy' ? <Loader2 size={15} className="animate-spin" /> : state === 'done' ? <Check size={15} /> : <Download size={15} />}
      </button>
    </div>
  )
}

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
  const playNext = usePlayerStore((s) => s.playNext)
  const enqueue = usePlayerStore((s) => s.enqueue)
  const playlists = useLibraryStore((s) => s.playlists).filter((p) => !p.is_smart)
  const [term, setTerm] = useState('')
  const [results, setResults] = useState<SearchResults | null>(null)
  const [localLoading, setLocalLoading] = useState(false)
  // Long-press context menu on local track rows (mobile): same actions as the
  // library list, so search results are first-class touch citizens.
  const [menuTrack, setMenuTrack] = useState<Track | null>(null)
  const [menuPlaylists, setMenuPlaylists] = useState(false)
  const pressTrack = useRef<Track | null>(null)
  const longPress = useLongPress(() => {
    if (pressTrack.current) {
      impact()
      setMenuPlaylists(false)
      setMenuTrack(pressTrack.current)
    }
  })
  const [external, setExternal] = useState<ExternalRecoTrack[] | null>(null)
  const [externalLoading, setExternalLoading] = useState(false)
  // Web search failed (network/backend): shown with a retry button — an error
  // must never masquerade as "no results". Bumping extRetry re-runs the effect.
  const [externalError, setExternalError] = useState(false)
  const [extRetry, setExtRetry] = useState(0)
  const inputRef = useRef<HTMLInputElement>(null)
  const debounce = useRef<number | null>(null)
  const extDebounce = useRef<number | null>(null)
  // Monotonic request ids: on variable mobile latency an older response can
  // resolve AFTER a newer one — only the latest request may touch state.
  const localReq = useRef(0)
  const extReq = useRef(0)

  useEffect(() => {
    if (open) {
      setTerm('')
      setResults(null)
      setExternal(null)
      setExternalError(false)
      setMenuTrack(null)
      setTimeout(() => inputRef.current?.focus(), 30)
    }
  }, [open])

  // Web catalogue search (keyless Deezer): lets the user find & download ANY
  // song, not only what's already in the library. Slower than local search, so
  // it runs on a longer debounce and renders below the local results.
  useEffect(() => {
    const req = ++extReq.current // invalidate any in-flight response
    if (!open) return
    if (extDebounce.current) window.clearTimeout(extDebounce.current)
    setExternalError(false)
    if (!term.trim()) {
      setExternal(null)
      setExternalLoading(false)
      return
    }
    setExternalLoading(true)
    extDebounce.current = window.setTimeout(() => {
      void window.aether
        .searchExternalCatalog(term)
        .then((rows) => {
          if (req === extReq.current) setExternal(rows.filter((r) => !r.owned))
        })
        .catch(() => {
          // Backend threw (EXT_SEARCH_FAILED / transport down): surface it with
          // a retry affordance instead of pretending there are no results.
          if (req === extReq.current) {
            setExternal(null)
            setExternalError(true)
          }
        })
        .finally(() => {
          if (req === extReq.current) setExternalLoading(false)
        })
    }, 500)
  }, [term, open, extRetry])

  useEffect(() => {
    const req = ++localReq.current // invalidate any in-flight response
    if (!open) return
    if (debounce.current) window.clearTimeout(debounce.current)
    if (!term.trim()) {
      setResults(null)
      setLocalLoading(false)
      return
    }
    // Local search on the mobile sql.js backend is not instant: show a loading
    // state instead of a blank area (the web section already has one).
    setLocalLoading(true)
    debounce.current = window.setTimeout(() => {
      void window.aether
        .search(term)
        .then((r) => {
          if (req === localReq.current) setResults(r)
        })
        // Never leave results === null on a backend error: that renders as a
        // bare search bar with no feedback. Show the empty-state instead.
        .catch(() => {
          if (req === localReq.current) setResults({ tracks: [], albums: [], artists: [] })
        })
        .finally(() => {
          if (req === localReq.current) setLocalLoading(false)
        })
    }, 180)
  }, [term, open])

  // Focus trap only on the desktop dialog: the mobile sheet is full-screen.
  const trapRef = useFocusTrap<HTMLDivElement>(open && !isMobile, () => setOpen(false))

  if (!open) return null

  const close = (): void => setOpen(false)
  const hasResults =
    results && (results.tracks.length > 0 || results.albums.length > 0 || results.artists.length > 0)

  // Righe risultato: su touch salgono a ≥52px (M3), sul desktop restano dense.
  const rowCls = `flex w-full items-center gap-3 rounded-lg px-3 py-1.5 text-left transition-colors hover:bg-white/[0.06] ${
    isMobile ? 'min-h-[52px]' : ''
  }`

  const body = (
    <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {localLoading && !results && (
            <div className="flex flex-col gap-2 p-3">
              {Array.from({ length: 3 }).map((_, i) => (
                <div key={i} className="skeleton h-11" />
              ))}
            </div>
          )}
          {results && !hasResults && !localLoading && !externalLoading && !externalError && (external == null || external.length === 0) && (
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
                    className={rowCls}
                    onDoubleClick={() => {
                      playTracks(results.tracks, i)
                      close()
                    }}
                    onClick={() => {
                      playTracks(results.tracks, i)
                      close()
                    }}
                    onPointerDown={isMobile ? (e) => {
                      pressTrack.current = track
                      longPress.onPointerDown(e)
                    } : undefined}
                    onPointerMove={isMobile ? longPress.onPointerMove : undefined}
                    onPointerUp={isMobile ? longPress.onPointerUp : undefined}
                    onPointerCancel={isMobile ? longPress.onPointerCancel : undefined}
                  >
                    <div className="h-8 w-8 shrink-0 overflow-hidden rounded bg-surface-3">
                      <CoverImage src={thumb} className="h-full w-full object-cover" />
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
                    className={rowCls}
                    onClick={() => {
                      navigate(`/albums/${album.id}`)
                      close()
                    }}
                  >
                    <div className="h-8 w-8 shrink-0 overflow-hidden rounded bg-surface-3">
                      <CoverImage src={thumb} className="h-full w-full object-cover" />
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
                  className={rowCls}
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

          {/* Web catalogue: download anything that isn't in the library yet. */}
          {term.trim() && (externalLoading || externalError || (external && external.length > 0)) && (
            <section className="mt-1">
              <h3 className="flex items-center gap-1.5 px-3 py-1.5 text-[11px] font-semibold uppercase tracking-wide text-text-3">
                <Globe size={11} /> {t('search.from_web')}
                {externalLoading && <Loader2 size={11} className="animate-spin" />}
              </h3>
              {externalError && !externalLoading ? (
                <div className="flex items-center gap-3 px-3 py-2">
                  <span className="flex-1 text-[12.5px] text-text-3">{t('search.web_error')}</span>
                  <button
                    className="flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-[12px] transition-colors hover:bg-white/[0.06]"
                    style={{ borderColor: 'var(--hairline)' }}
                    onClick={() => setExtRetry((n) => n + 1)}
                  >
                    <RefreshCw size={12} /> {t('search.web_retry')}
                  </button>
                </div>
              ) : (
                (external ?? []).slice(0, 8).map((track, i) => (
                  <ExternalRow key={`${track.artist}-${track.title}-${i}`} track={track} />
                ))
              )}
            </section>
          )}
    </div>
  )

  // Mobile: full-screen search view (M3 Search) — input pinned in the app bar,
  // keyboard-aware via FullScreenSheet's --kb-height padding.
  if (isMobile) {
    const menuIndex = menuTrack && results ? results.tracks.findIndex((tr) => tr.id === menuTrack.id) : -1
    return (
      <>
        <FullScreenSheet
          open
          onClose={close}
          title={t('search.placeholder')}
          headerContent={
            <>
              <input
                ref={inputRef}
                className="h-12 min-w-0 flex-1 bg-transparent text-[16px] outline-none placeholder:text-text-3"
                placeholder={t('search.placeholder')}
                value={term}
                onChange={(e) => setTerm(e.target.value)}
                inputMode="search"
                enterKeyHint="search"
                autoCapitalize="none"
                autoCorrect="off"
              />
              {term && (
                <button
                  className="icon-btn h-12 w-12 shrink-0"
                  onClick={() => {
                    setTerm('')
                    inputRef.current?.focus()
                  }}
                  aria-label={t('search.clear')}
                >
                  <X size={20} />
                </button>
              )}
            </>
          }
        >
          {body}
        </FullScreenSheet>

        {/* Long-press actions on a local result (parity with the library list). */}
        {menuTrack && (
          <BottomSheet open onClose={() => setMenuTrack(null)} title={menuTrack.title}>
            <div className="px-2 pb-2">
              {menuPlaylists ? (
                <>
                  {playlists.length === 0 && (
                    <div className="px-4 py-3 text-[13px] text-text-3">—</div>
                  )}
                  {playlists.map((p) => (
                    <SheetAction
                      key={p.id}
                      icon={ListMusic}
                      label={p.name}
                      onClick={() => {
                        void window.aether
                          .addToPlaylist(p.id, [menuTrack.id])
                          .then(() => {
                            toast.success(t('toast.added_to_playlist', { count: 1, name: p.name }))
                          })
                          .catch(() => {})
                        setMenuTrack(null)
                      }}
                    />
                  ))}
                </>
              ) : (
                <>
                  <SheetAction
                    icon={Play}
                    label={t('player.play')}
                    onClick={() => {
                      if (results && menuIndex >= 0) playTracks(results.tracks, menuIndex)
                      setMenuTrack(null)
                      close()
                    }}
                  />
                  <SheetAction
                    icon={ListStart}
                    label={t('player.play_next')}
                    onClick={() => {
                      playNext([menuTrack])
                      setMenuTrack(null)
                    }}
                  />
                  <SheetAction
                    icon={ListEnd}
                    label={t('player.add_to_queue')}
                    onClick={() => {
                      enqueue([menuTrack])
                      setMenuTrack(null)
                    }}
                  />
                  <SheetAction
                    icon={ListMusic}
                    label={t('playlists.title')}
                    onClick={() => setMenuPlaylists(true)}
                  />
                  <SheetAction
                    icon={Heart}
                    label={menuTrack.liked ? t('liked.remove') : t('liked.add')}
                    onClick={() => {
                      const next = !menuTrack.liked
                      void window.aether.setLiked(menuTrack.id, next).catch(() => {})
                      toast.success(next ? t('liked.added') : t('liked.removed'))
                      setMenuTrack(null)
                    }}
                  />
                </>
              )}
            </div>
          </BottomSheet>
        )}
      </>
    )
  }

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
        {body}
      </div>
    </div>
  )
}
