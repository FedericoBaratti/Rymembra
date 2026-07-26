import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import {
  AlertCircle,
  ArrowRight,
  Check,
  CheckCircle2,
  Clock,
  Link2,
  ListMusic,
  Loader2,
  Music2,
  RefreshCw,
  X
} from 'lucide-react'
import type { SpotifyMigrationPreview, SpotifyMigrationTrackStatus } from '@shared/types'
import { remoteImageUrl } from '@/lib/format'
import { ipcErrorMessage } from '@/lib/ipcError'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import { useBackDismiss } from '@/hooks/useBackDismiss'
import { useSpotifyMigrationStore } from '@/store/useSpotifyMigrationStore'

const SPOTIFY_GREEN = '#1DB954'

/** Inline Spotify glyph so the flow looks branded without an asset dependency. */
function SpotifyGlyph({ size = 22 }: { size?: number }): React.JSX.Element {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill={SPOTIFY_GREEN} aria-hidden>
      <path d="M12 0C5.4 0 0 5.4 0 12s5.4 12 12 12 12-5.4 12-12S18.66 0 12 0zm5.5 17.3a.75.75 0 0 1-1.03.25c-2.82-1.72-6.37-2.11-10.55-1.16a.75.75 0 1 1-.33-1.46c4.57-1.04 8.5-.59 11.66 1.34.36.22.47.69.25 1.03zm1.47-3.27a.94.94 0 0 1-1.29.31c-3.23-1.98-8.15-2.56-11.97-1.4a.94.94 0 1 1-.54-1.8c4.37-1.32 9.79-.67 13.5 1.6.44.27.58.85.3 1.29zm.13-3.4C15.78 8.26 8.9 8.03 5.1 9.18a1.12 1.12 0 1 1-.65-2.15c4.37-1.33 11.96-1.07 16.27 1.5a1.12 1.12 0 1 1-1.15 1.93z" />
    </svg>
  )
}

const STATUS_ICON: Record<SpotifyMigrationTrackStatus, React.JSX.Element> = {
  pending: <Clock size={13} className="text-text-3" />,
  downloading: <Loader2 size={13} className="animate-spin" style={{ color: SPOTIFY_GREEN }} />,
  done: <Check size={13} className="text-[var(--success)]" />,
  notfound: <AlertCircle size={13} className="text-amber-400" />,
  // Transient failure (network/timeout/403): being retried, not a real miss.
  failed: <RefreshCw size={13} className="text-rose-400" />
}

function kindLabel(kind: string, t: (k: string) => string): string {
  const map: Record<string, string> = {
    track: t('spotifyMigration.kind_track'),
    album: t('spotifyMigration.kind_album'),
    artist: t('spotifyMigration.kind_artist'),
    playlist: t('spotifyMigration.kind_playlist')
  }
  return map[kind] ?? kind
}

export default function SpotifyMigrationFlow({ onClose }: { onClose: () => void }): React.JSX.Element {
  const { t } = useTranslation()
  const trapRef = useFocusTrap<HTMLDivElement>(true, onClose)
  // Mounted only while open: hardware back closes the flow overlay.
  useBackDismiss(true, onClose)
  const migration = useSpotifyMigrationStore((s) => s.state)
  const inputRef = useRef<HTMLInputElement>(null)

  const isActive = migration?.status === 'running' || migration?.status === 'resolving'
  const [view, setView] = useState<'input' | 'preview' | 'progress'>(isActive ? 'progress' : 'input')
  const [url, setUrl] = useState('')
  const [analyzing, setAnalyzing] = useState(false)
  const [preview, setPreview] = useState<SpotifyMigrationPreview | null>(null)
  const [recreate, setRecreate] = useState(true)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (view === 'input') inputRef.current?.focus()
  }, [view])

  const analyze = async (target?: string): Promise<void> => {
    const value = (target ?? url).trim()
    if (!value) return
    setAnalyzing(true)
    setError(null)
    try {
      const p = await window.aether.spotifyMigrationPreview(value)
      setPreview(p)
      setRecreate(p.kind !== 'track')
      setUrl(value)
      setView('preview')
    } catch (err) {
      setError(ipcErrorMessage(err))
    } finally {
      setAnalyzing(false)
    }
  }

  const start = async (): Promise<void> => {
    if (!preview) return
    setError(null)
    try {
      await window.aether.spotifyMigrationStart({ url, recreatePlaylist: recreate })
      setView('progress')
    } catch (err) {
      setError(ipcErrorMessage(err))
    }
  }

  const reset = (): void => {
    setPreview(null)
    setUrl('')
    setError(null)
    setView('input')
  }

  const cover = preview?.coverUrl ? remoteImageUrl(preview.coverUrl) : null
  const mCover = migration?.coverUrl ? remoteImageUrl(migration.coverUrl) : null
  const pct = migration && migration.total > 0 ? Math.round((migration.done / migration.total) * 100) : 0
  const finished =
    migration?.status === 'done' || migration?.status === 'error' || migration?.status === 'cancelled'

  return createPortal(
    <div
      className="overlay-in fixed inset-0 z-[60] flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={onClose}
      style={{ paddingTop: 'var(--sa-top, 0px)', paddingBottom: 'var(--sa-bottom, 0px)' }}
    >
      <div
        ref={trapRef}
        role="dialog"
        aria-modal="true"
        aria-label={t('spotifyMigration.title')}
        className="glass-modal scale-in flex max-h-[88vh] w-[min(560px,calc(100vw-32px))] flex-col overflow-hidden rounded-2xl"
        style={{ boxShadow: '0 24px 80px rgba(0,0,0,0.7)' }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* header with Spotify-green aura */}
        <div
          className="relative flex items-center justify-between px-5 py-4"
          style={{
            borderBottom: '1px solid var(--hairline)',
            background: `linear-gradient(120deg, ${SPOTIFY_GREEN}22, transparent 70%)`
          }}
        >
          <h2 className="flex items-center gap-2.5 text-[15px] font-bold">
            <SpotifyGlyph />
            {t('spotifyMigration.title')}
          </h2>
          <button className="icon-btn h-7 w-7" onClick={onClose} aria-label={t('common.close')}>
            <X size={15} />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-5">
          {/* ---------- STEP 1: paste link ---------- */}
          {view === 'input' && (
            <div className="fade-in flex flex-col items-center gap-5 py-4 text-center">
              <div
                className="flex h-20 w-20 items-center justify-center rounded-3xl"
                style={{ background: `${SPOTIFY_GREEN}1a`, boxShadow: `0 0 40px ${SPOTIFY_GREEN}33` }}
              >
                <SpotifyGlyph size={40} />
              </div>
              <div>
                <div className="text-[17px] font-bold">{t('spotifyMigration.hero_title')}</div>
                <p className="mx-auto mt-1.5 max-w-[360px] text-[12.5px] leading-relaxed text-text-2">
                  {t('spotifyMigration.hero_subtitle')}
                </p>
              </div>
              <div className="flex w-full max-w-[420px] flex-col gap-2.5">
                <div className="glass-chrome flex h-12 items-center gap-2.5 rounded-full px-4 transition focus-within:border-[rgba(29,185,84,0.6)]">
                  <Link2 size={16} className="shrink-0 text-text-3" />
                  <input
                    ref={inputRef}
                    type="url"
                    inputMode="url"
                    enterKeyHint="go"
                    autoCapitalize="off"
                    autoCorrect="off"
                    spellCheck={false}
                    className="h-full flex-1 bg-transparent text-[13.5px] outline-none placeholder:text-text-3"
                    placeholder={t('spotifyMigration.placeholder')}
                    value={url}
                    onChange={(e) => setUrl(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && void analyze()}
                    onPaste={(e) => {
                      const pasted = e.clipboardData.getData('text')
                      if (pasted) setTimeout(() => void analyze(pasted), 30)
                    }}
                  />
                </div>
                <button
                  className="flex h-11 items-center justify-center gap-2 rounded-full text-[13.5px] font-semibold text-black transition active:scale-[0.98] disabled:opacity-50"
                  style={{ background: SPOTIFY_GREEN }}
                  onClick={() => void analyze()}
                  disabled={analyzing || !url.trim()}
                >
                  {analyzing ? <Loader2 size={16} className="animate-spin" /> : <ArrowRight size={16} />}
                  {t('spotifyMigration.analyze')}
                </button>
                <p className="mt-1 text-[11px] text-text-3">{t('spotifyMigration.keyless_hint')}</p>
              </div>
              {error && <div className="error-banner w-full max-w-[420px] text-[12.5px]">{error}</div>}
            </div>
          )}

          {/* ---------- STEP 2: preview ---------- */}
          {view === 'preview' && preview && (
            <div className="fade-in flex flex-col gap-5">
              <div className="flex items-center gap-4">
                <div className="relative h-24 w-24 shrink-0 overflow-hidden rounded-xl bg-surface-3 shadow-lg">
                  {cover ? (
                    <img src={cover} alt="" className="h-full w-full object-cover" />
                  ) : (
                    <div className="flex h-full w-full items-center justify-center">
                      <Music2 size={26} className="text-text-3" />
                    </div>
                  )}
                </div>
                <div className="min-w-0 flex-1">
                  <span
                    className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[10.5px] font-semibold"
                    style={{ background: `${SPOTIFY_GREEN}22`, color: SPOTIFY_GREEN }}
                  >
                    {kindLabel(preview.kind, t)}
                  </span>
                  <div className="mt-1.5 truncate text-[17px] font-bold">{preview.title}</div>
                  {preview.artist && (
                    <div className="truncate text-[12.5px] text-text-2">{preview.artist}</div>
                  )}
                  <div className="mt-1 text-[12px] text-text-3">
                    {t('spotifyMigration.track_count', { count: preview.totalTracks })}
                  </div>
                </div>
              </div>

              {preview.kind !== 'track' && (
                <label
                  className="flex cursor-pointer items-center justify-between gap-4 rounded-xl border border-[var(--hairline)] bg-white/[0.03] p-3.5"
                  onClick={(e) => {
                    e.preventDefault()
                    setRecreate((v) => !v)
                  }}
                >
                  <div className="flex items-center gap-2.5">
                    <ListMusic size={16} style={{ color: SPOTIFY_GREEN }} />
                    <div>
                      <div className="text-[13px] font-semibold">{t('spotifyMigration.recreate_label')}</div>
                      <div className="text-[11.5px] text-text-3">{t('spotifyMigration.recreate_hint')}</div>
                    </div>
                  </div>
                  <span
                    className="relative h-6 w-11 shrink-0 rounded-full transition"
                    style={{ background: recreate ? SPOTIFY_GREEN : 'rgba(255,255,255,0.16)' }}
                  >
                    <span
                      className="absolute top-0.5 h-5 w-5 rounded-full bg-white transition-all"
                      style={{ left: recreate ? '22px' : '2px' }}
                    />
                  </span>
                </label>
              )}

              {error && <div className="error-banner text-[12.5px]">{error}</div>}

              <div className="flex gap-2.5">
                <button
                  className="btn-ghost flex-1 rounded-full py-2.5 text-[13px] font-medium text-text-2"
                  onClick={reset}
                >
                  {t('common.back')}
                </button>
                <button
                  className="flex flex-[2] items-center justify-center gap-2 rounded-full py-2.5 text-[13.5px] font-semibold text-black transition active:scale-[0.98]"
                  style={{ background: SPOTIFY_GREEN }}
                  onClick={() => void start()}
                >
                  <ArrowRight size={16} /> {t('spotifyMigration.start')}
                </button>
              </div>
            </div>
          )}

          {/* ---------- STEP 3: progress ---------- */}
          {view === 'progress' && migration && (
            <div className="fade-in flex flex-col gap-4">
              <div className="flex items-center gap-3.5">
                <div className="h-16 w-16 shrink-0 overflow-hidden rounded-xl bg-surface-3 shadow-lg">
                  {mCover ? (
                    <img src={mCover} alt="" className="h-full w-full object-cover" />
                  ) : (
                    <div className="flex h-full w-full items-center justify-center">
                      <Music2 size={20} className="text-text-3" />
                    </div>
                  )}
                </div>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[15px] font-bold">
                    {migration.title || t('spotifyMigration.resolving')}
                  </div>
                  <div className="truncate text-[12px] text-text-3">
                    {finished
                      ? t('spotifyMigration.summary', {
                          matched: migration.matched,
                          total: migration.total
                        })
                      : migration.currentTitle
                        ? t('spotifyMigration.now_downloading', { title: migration.currentTitle })
                        : t('spotifyMigration.resolving')}
                  </div>
                </div>
                <div className="text-right">
                  <div className="tnum text-[20px] font-bold" style={{ color: SPOTIFY_GREEN }}>
                    {pct}%
                  </div>
                  <div className="tnum text-[11px] text-text-3">
                    {migration.done}/{migration.total}
                  </div>
                </div>
              </div>

              <div className="h-2 overflow-hidden rounded-full bg-white/10">
                <div
                  className="h-full rounded-full transition-[width] duration-300"
                  style={{
                    width: `${pct}%`,
                    background: SPOTIFY_GREEN,
                    boxShadow: `0 0 10px ${SPOTIFY_GREEN}88`
                  }}
                />
              </div>

              {migration.recreatePlaylist && migration.playlistId != null && (
                <div
                  className="flex items-center gap-2 rounded-lg px-3 py-2 text-[12px]"
                  style={{ background: `${SPOTIFY_GREEN}14`, color: SPOTIFY_GREEN }}
                >
                  <CheckCircle2 size={14} /> {t('spotifyMigration.playlist_created')}
                </div>
              )}

              {migration.error && (
                <div className="error-banner text-[12.5px]">{migration.error}</div>
              )}

              <div className="flex max-h-[34vh] flex-col gap-0.5 overflow-y-auto rounded-xl border border-[var(--hairline)] bg-white/[0.02] p-1.5">
                {migration.tracks.map((tr, i) => (
                  <div
                    key={i}
                    className="flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-[12.5px]"
                  >
                    <span className="w-4 shrink-0">{STATUS_ICON[tr.status]}</span>
                    <span className="tnum w-6 shrink-0 text-right text-[11px] text-text-3">{i + 1}</span>
                    <span
                      className={`min-w-0 flex-1 truncate ${
                        tr.status === 'notfound'
                          ? 'text-text-3 line-through'
                          : tr.status === 'failed'
                            ? 'text-rose-300'
                            : ''
                      }`}
                    >
                      {tr.title}
                      {tr.artist ? <span className="text-text-3"> · {tr.artist}</span> : null}
                    </span>
                  </div>
                ))}
              </div>

              <div className="flex gap-2.5">
                {!finished ? (
                  <button
                    className="btn-ghost flex-1 rounded-full py-2.5 text-[13px] font-medium text-text-2"
                    onClick={() => void window.aether.spotifyMigrationCancel()}
                  >
                    {t('spotifyMigration.cancel')}
                  </button>
                ) : (
                  <>
                    <button
                      className="btn-ghost flex-1 rounded-full py-2.5 text-[13px] font-medium text-text-2"
                      onClick={reset}
                    >
                      {t('spotifyMigration.new_migration')}
                    </button>
                    <button
                      className="flex flex-1 items-center justify-center gap-2 rounded-full py-2.5 text-[13.5px] font-semibold text-black"
                      style={{ background: SPOTIFY_GREEN }}
                      onClick={onClose}
                    >
                      <Check size={16} /> {t('common.done')}
                    </button>
                  </>
                )}
              </div>
            </div>
          )}
        </div>
      </div>
    </div>,
    document.body
  )
}
