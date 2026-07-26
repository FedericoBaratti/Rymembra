import { useEffect, useMemo, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import {
  ArrowRight,
  Check,
  CheckCircle2,
  Link2,
  ListMusic,
  Loader2,
  Music2,
  X
} from 'lucide-react'
import type { DownloadItem, DownloadPreview } from '@shared/types'
import { splitYoutubeWatchUrl, type YoutubeUrlSplit } from '@shared/youtubeUrl'
import { remoteImageUrl } from '@/lib/format'
import { ipcErrorMessage, translateErrorCode } from '@/lib/ipcError'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import { useBackDismiss } from '@/hooks/useBackDismiss'
import { useDownloadsStore } from '@/store/useDownloadsStore'

const YT_RED = '#FF0000'

/** Inline YouTube glyph so the flow looks branded without an asset dependency. */
function YoutubeGlyph({ size = 22 }: { size?: number }): React.JSX.Element {
  return (
    <svg width={size} height={size} viewBox="0 0 24 24" fill={YT_RED} aria-hidden>
      <path d="M23.5 6.2a3.02 3.02 0 0 0-2.12-2.14C19.5 3.55 12 3.55 12 3.55s-7.5 0-9.38.51A3.02 3.02 0 0 0 .5 6.2C0 8.08 0 12 0 12s0 3.92.5 5.8a3.02 3.02 0 0 0 2.12 2.14c1.88.51 9.38.51 9.38.51s7.5 0 9.38-.51a3.02 3.02 0 0 0 2.12-2.14C24 15.92 24 12 24 12s0-3.92-.5-5.8zM9.55 15.57V8.43L15.82 12l-6.27 3.57z" />
    </svg>
  )
}

const ACTIVE = new Set<DownloadItem['status']>(['downloading', 'pending', 'paused'])

function remoteCover(url: string | null): string | null {
  return url ? remoteImageUrl(url) : null
}

export default function YoutubeDownloadFlow({ onClose }: { onClose: () => void }): React.JSX.Element {
  const { t } = useTranslation()
  const trapRef = useFocusTrap<HTMLDivElement>(true, onClose)
  // Mounted only while open: hardware back closes the flow overlay.
  useBackDismiss(true, onClose)
  const items = useDownloadsStore((s) => s.items)
  const refresh = useDownloadsStore((s) => s.refresh)
  const inputRef = useRef<HTMLInputElement>(null)

  // Resume the progress view if a YouTube download is already running.
  const existing = useMemo(
    () => items.find((i) => i.source_type.startsWith('youtube') && ACTIVE.has(i.status)),
    [items]
  )

  const [view, setView] = useState<'input' | 'preview' | 'progress'>(existing ? 'progress' : 'input')
  const [url, setUrl] = useState('')
  const [analyzing, setAnalyzing] = useState(false)
  const [choice, setChoice] = useState<YoutubeUrlSplit | null>(null)
  const [preview, setPreview] = useState<DownloadPreview | null>(null)
  const [activeId, setActiveId] = useState<number | null>(existing?.id ?? null)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (view === 'input') inputRef.current?.focus()
  }, [view])

  const analyzeUrl = async (target: string): Promise<void> => {
    setAnalyzing(true)
    setError(null)
    setChoice(null)
    setPreview(null)
    try {
      const p = await window.aether.previewDownload(target)
      if (!p.source_type.startsWith('youtube')) {
        setError(t('youtubeDownload.invalid_url'))
        return
      }
      setUrl(target)
      setPreview(p)
      setView('preview')
    } catch (err) {
      setError(ipcErrorMessage(err))
    } finally {
      setAnalyzing(false)
    }
  }

  const analyze = async (target?: string): Promise<void> => {
    const value = (target ?? url).trim()
    if (!value) return
    // A video link carrying a playlist/mix context: let the user pick first.
    const split = splitYoutubeWatchUrl(value)
    if (split) {
      setError(null)
      setPreview(null)
      setUrl(value)
      setChoice(split)
      return
    }
    await analyzeUrl(value)
  }

  const start = async (): Promise<void> => {
    if (!preview) return
    setError(null)
    try {
      const item = await window.aether.startDownload(preview)
      setActiveId(item.id)
      setPreview(null)
      setView('progress')
      void refresh()
    } catch (err) {
      setError(ipcErrorMessage(err))
    }
  }

  const reset = (): void => {
    setPreview(null)
    setChoice(null)
    setUrl('')
    setError(null)
    setActiveId(null)
    setView('input')
  }

  const active = activeId != null ? items.find((i) => i.id === activeId) ?? existing : existing
  const cover = remoteCover(preview?.cover_url ?? null)
  const aCover = remoteCover(active?.cover_url ?? null)
  const pct = active ? Math.round(active.progress * 100) : 0
  const finished =
    active?.status === 'completed' || active?.status === 'error' || active?.status === 'cancelled'

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
        aria-label={t('youtubeDownload.title')}
        className="glass-modal scale-in flex max-h-[88vh] w-[min(560px,calc(100vw-32px))] flex-col overflow-hidden rounded-2xl"
        style={{ boxShadow: '0 24px 80px rgba(0,0,0,0.7)' }}
        onClick={(e) => e.stopPropagation()}
      >
        {/* header with YouTube-red aura */}
        <div
          className="relative flex items-center justify-between px-5 py-4"
          style={{
            borderBottom: '1px solid var(--hairline)',
            background: `linear-gradient(120deg, ${YT_RED}22, transparent 70%)`
          }}
        >
          <h2 className="flex items-center gap-2.5 text-[15px] font-bold">
            <YoutubeGlyph />
            {t('youtubeDownload.title')}
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
                style={{ background: `${YT_RED}1a`, boxShadow: `0 0 40px ${YT_RED}33` }}
              >
                <YoutubeGlyph size={40} />
              </div>
              <div>
                <div className="text-[17px] font-bold">{t('youtubeDownload.hero_title')}</div>
                <p className="mx-auto mt-1.5 max-w-[360px] text-[12.5px] leading-relaxed text-text-2">
                  {t('youtubeDownload.hero_subtitle')}
                </p>
              </div>
              <div className="flex w-full max-w-[420px] flex-col gap-2.5">
                <div className="glass-chrome flex h-12 items-center gap-2.5 rounded-full px-4 transition focus-within:border-[rgba(255,0,0,0.6)]">
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
                    placeholder={t('youtubeDownload.placeholder')}
                    value={url}
                    onChange={(e) => setUrl(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && void analyze()}
                    onPaste={(e) => {
                      const pasted = e.clipboardData.getData('text')
                      if (pasted) setTimeout(() => void analyze(pasted), 30)
                    }}
                  />
                </div>

                {/* video-vs-playlist choice for links carrying both */}
                {choice && (
                  <div
                    className="fade-in flex flex-col gap-2 rounded-xl border p-3 text-left"
                    style={{ background: `${YT_RED}10`, borderColor: `${YT_RED}40` }}
                  >
                    <div className="text-[12.5px] text-text-2">
                      {t(choice.isMix ? 'downloader.link_has_mix' : 'downloader.link_has_both')}
                    </div>
                    <div className="flex gap-2">
                      <button
                        className="btn-ghost flex flex-1 items-center justify-center gap-1.5 rounded-full py-2 text-[12.5px] font-medium"
                        onClick={() => void analyzeUrl(choice.videoUrl)}
                      >
                        <Music2 size={14} /> {t('downloader.video_only')}
                      </button>
                      <button
                        className="flex flex-1 items-center justify-center gap-1.5 rounded-full py-2 text-[12.5px] font-semibold text-white"
                        style={{ background: YT_RED }}
                        onClick={() => void analyzeUrl(choice.playlistUrl)}
                      >
                        <ListMusic size={14} />
                        {t(choice.isMix ? 'downloader.whole_mix' : 'downloader.whole_playlist')}
                      </button>
                    </div>
                  </div>
                )}

                {!choice && (
                  <button
                    className="flex h-11 items-center justify-center gap-2 rounded-full text-[13.5px] font-semibold text-white transition active:scale-[0.98] disabled:opacity-50"
                    style={{ background: YT_RED }}
                    onClick={() => void analyze()}
                    disabled={analyzing || !url.trim()}
                  >
                    {analyzing ? <Loader2 size={16} className="animate-spin" /> : <ArrowRight size={16} />}
                    {t('youtubeDownload.analyze')}
                  </button>
                )}
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
                    style={{ background: `${YT_RED}22`, color: YT_RED }}
                  >
                    {preview.source_type === 'youtube-playlist'
                      ? t('downloader.whole_playlist')
                      : t('downloader.video_only')}
                  </span>
                  <div className="mt-1.5 truncate text-[17px] font-bold">{preview.title}</div>
                  {preview.artist && (
                    <div className="truncate text-[12.5px] text-text-2">{preview.artist}</div>
                  )}
                  <div className="mt-1 text-[12px] text-text-3">
                    {t('downloader.tracks', { count: preview.total_tracks })}
                    {preview.estimated_size_mb
                      ? ` · ${t('downloader.estimated_size', { size: preview.estimated_size_mb })}`
                      : ''}
                  </div>
                </div>
              </div>

              {error && <div className="error-banner text-[12.5px]">{error}</div>}

              <div className="flex gap-2.5">
                <button
                  className="btn-ghost flex-1 rounded-full py-2.5 text-[13px] font-medium text-text-2"
                  onClick={reset}
                >
                  {t('common.back')}
                </button>
                <button
                  className="flex flex-[2] items-center justify-center gap-2 rounded-full py-2.5 text-[13.5px] font-semibold text-white transition active:scale-[0.98]"
                  style={{ background: YT_RED }}
                  onClick={() => void start()}
                >
                  <ArrowRight size={16} /> {t('downloader.confirm_download')}
                </button>
              </div>
            </div>
          )}

          {/* ---------- STEP 3: progress ---------- */}
          {view === 'progress' && active && (
            <div className="fade-in flex flex-col gap-4">
              <div className="flex items-center gap-3.5">
                <div className="h-16 w-16 shrink-0 overflow-hidden rounded-xl bg-surface-3 shadow-lg">
                  {aCover ? (
                    <img src={aCover} alt="" className="h-full w-full object-cover" />
                  ) : (
                    <div className="flex h-full w-full items-center justify-center">
                      <Music2 size={20} className="text-text-3" />
                    </div>
                  )}
                </div>
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[15px] font-bold">{active.title}</div>
                  <div className="truncate text-[12px] text-text-3">
                    {finished
                      ? t(`downloader.status_${active.status}`)
                      : active.current_file
                        ? t('youtubeDownload.now_downloading', { title: active.current_file })
                        : t('downloader.status_pending')}
                  </div>
                </div>
                <div className="text-right">
                  <div className="tnum text-[20px] font-bold" style={{ color: YT_RED }}>
                    {pct}%
                  </div>
                  <div className="tnum text-[11px] text-text-3">
                    {active.completed_tracks}/{active.total_tracks}
                  </div>
                </div>
              </div>

              <div className="h-2 overflow-hidden rounded-full bg-white/10">
                <div
                  className="h-full rounded-full transition-[width] duration-300"
                  style={{
                    width: `${pct}%`,
                    background: YT_RED,
                    boxShadow: `0 0 10px ${YT_RED}88`
                  }}
                />
              </div>

              {active.status === 'completed' && (
                <div
                  className="flex items-center gap-2 rounded-lg px-3 py-2 text-[12px]"
                  style={{ background: `${YT_RED}14`, color: YT_RED }}
                >
                  <CheckCircle2 size={14} />{' '}
                  {t('youtubeDownload.summary', {
                    matched: active.completed_tracks,
                    total: active.total_tracks
                  })}
                </div>
              )}

              {active.status === 'error' && active.error_message && (
                <div className="error-banner text-[12.5px]">{translateErrorCode(active.error_message)}</div>
              )}

              <div className="flex gap-2.5">
                {!finished ? (
                  <button
                    className="btn-ghost flex-1 rounded-full py-2.5 text-[13px] font-medium text-text-2"
                    onClick={() => void window.aether.cancelDownload(active.id)}
                  >
                    {t('downloader.cancel')}
                  </button>
                ) : (
                  <>
                    <button
                      className="btn-ghost flex-1 rounded-full py-2.5 text-[13px] font-medium text-text-2"
                      onClick={reset}
                    >
                      {t('youtubeDownload.new_download')}
                    </button>
                    <button
                      className="flex flex-1 items-center justify-center gap-2 rounded-full py-2.5 text-[13.5px] font-semibold text-white"
                      style={{ background: YT_RED }}
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
