import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  AlertCircle,
  Check,
  Clock,
  Download,
  Link2,
  ListMusic,
  Loader2,
  Music2,
  Pause,
  Play,
  RotateCcw,
  Trash2,
  X
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type { DownloadItem, DownloadPreview, DownloadStatus } from '@shared/types'
import { splitYoutubeWatchUrl, type YoutubeUrlSplit } from '@shared/youtubeUrl'
import { ipcErrorMessage, translateErrorCode } from '@/lib/ipcError'
import PageHeader from '@/components/ui/PageHeader'
import EmptyState from '@/components/ui/EmptyState'
import { useDownloadsStore } from '@/store/useDownloadsStore'

const STATUS_PILL: Record<DownloadStatus, { cls: string; icon: LucideIcon }> = {
  pending: { cls: 'bg-white/[0.06] text-text-2', icon: Clock },
  downloading: { cls: 'bg-[var(--accent-soft)] text-[var(--accent)]', icon: Loader2 },
  paused: { cls: 'bg-[var(--warning-soft)] text-[var(--warning)]', icon: Pause },
  completed: { cls: 'bg-[var(--success-soft)] text-[var(--success)]', icon: Check },
  error: { cls: 'bg-[var(--danger-soft)] text-[var(--danger)]', icon: AlertCircle },
  cancelled: { cls: 'bg-white/[0.06] text-text-3', icon: X }
}

function StatusPill({ status }: { status: DownloadStatus }): React.JSX.Element {
  const { t } = useTranslation()
  const { cls, icon: Icon } = STATUS_PILL[status]
  return (
    // keyed by status so the pill remounts and "pops" on every transition
    <span
      key={status}
      data-status={status}
      className={`dl-pill scale-in inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[10.5px] font-semibold ${cls}`}
    >
      <Icon size={11} className={status === 'downloading' ? 'animate-spin' : ''} />
      {t(`downloader.status_${status}`)}
    </span>
  )
}

function sourceLabel(item: { source_type: string }, searchLabel: string): string {
  const map: Record<string, string> = {
    'youtube-video': 'YouTube',
    'youtube-playlist': 'YouTube Playlist',
    'spotify-track': 'Spotify Track',
    'spotify-album': 'Spotify Album',
    'spotify-artist': 'Spotify Artist',
    'spotify-playlist': 'Spotify Playlist',
    search: searchLabel
  }
  return map[item.source_type] ?? item.source_type
}

function remoteCover(url: string | null): string | null {
  if (!url) return null
  return `aether://remote/img?url=${encodeURIComponent(url)}`
}

function DownloadCard({ item }: { item: DownloadItem }): React.JSX.Element {
  const { t } = useTranslation()
  const cover = remoteCover(item.cover_url)
  const pct = Math.round(item.progress * 100)

  return (
    <div
      className="dl-row row-lift fade-in flex gap-3 border border-[var(--hairline)] bg-white/[0.03] p-3"
      data-status={item.status}
      style={{ borderRadius: 'var(--radius-card)' }}
    >
      <div className="h-12 w-12 shrink-0 overflow-hidden rounded-lg bg-surface-3">
        {cover ? (
          <img src={cover} alt="" className="h-full w-full object-cover" />
        ) : (
          <div className="flex h-full w-full items-center justify-center">
            <Music2 size={16} className="text-text-3" />
          </div>
        )}
      </div>
      <div className="min-w-0 flex-1">
        <div className="flex items-start justify-between gap-2">
          <div className="truncate text-[13.5px] font-semibold">
            {item.title}
            {item.artist ? <span className="font-normal text-text-2"> — {item.artist}</span> : null}
          </div>
          <div className="flex shrink-0 gap-1">
            {item.status === 'error' && (
              <button
                className="icon-btn h-6 w-6"
                onClick={() => void window.aether.retryDownload(item.id)}
                title={t('downloader.retry')}
                aria-label={t('downloader.retry')}
              >
                <RotateCcw size={13} />
              </button>
            )}
            {(item.status === 'pending' || item.status === 'downloading') && (
              <button
                className="icon-btn h-6 w-6"
                onClick={() => void window.aether.pauseDownload(item.id)}
                title={t('downloader.pause')}
                aria-label={t('downloader.pause')}
              >
                <Pause size={13} />
              </button>
            )}
            {item.status === 'paused' && (
              <button
                className="icon-btn h-6 w-6"
                onClick={() => void window.aether.resumeDownload(item.id)}
                title={t('downloader.resume')}
                aria-label={t('downloader.resume')}
              >
                <Play size={13} />
              </button>
            )}
            {(item.status === 'pending' || item.status === 'downloading' || item.status === 'paused') && (
              <button
                className="icon-btn h-6 w-6"
                onClick={() => void window.aether.cancelDownload(item.id)}
                title={t('downloader.cancel')}
                aria-label={t('downloader.cancel')}
              >
                <X size={14} />
              </button>
            )}
          </div>
        </div>
        <div className="mt-1 flex items-center gap-2 text-[11.5px] text-text-3">
          <span className="truncate">
            {sourceLabel(item, t('downloader.source_search'))} ·{' '}
            {t('downloader.tracks', { count: item.total_tracks })}
          </span>
          <StatusPill status={item.status} />
        </div>

        {(item.status === 'downloading' || item.status === 'paused') && (
          <>
            <div className="mt-2 flex items-center gap-2">
              <div className="dl-progress h-1.5 flex-1 overflow-hidden rounded-full bg-white/10">
                <div
                  className={`h-full rounded-full transition-[width] duration-300 ${
                    item.status === 'downloading' ? 'progress-sheen' : ''
                  }`}
                  style={{
                    width: `${pct}%`,
                    background: 'var(--accent)',
                    boxShadow: '0 0 8px var(--accent-glow)'
                  }}
                />
              </div>
              <span className="tnum text-[11px] text-text-2">
                {pct}% · {item.completed_tracks}/{item.total_tracks}
              </span>
            </div>
            {item.current_file && (
              <div className="mt-1 truncate text-[11px] text-text-3">
                {t('downloader.downloading_file', { file: item.current_file })}
              </div>
            )}
          </>
        )}

        {item.status === 'error' && item.error_message && (
          <div className="error-banner mt-1.5 px-2 py-1 text-[11.5px]">
            {translateErrorCode(item.error_message)}
          </div>
        )}
      </div>
    </div>
  )
}

export default function Downloader(): React.JSX.Element {
  const { t } = useTranslation()
  const items = useDownloadsStore((s) => s.items)
  const refresh = useDownloadsStore((s) => s.refresh)
  const inputRef = useRef<HTMLInputElement>(null)
  const [url, setUrl] = useState('')
  const [preview, setPreview] = useState<DownloadPreview | null>(null)
  const [choice, setChoice] = useState<YoutubeUrlSplit | null>(null)
  const [analyzing, setAnalyzing] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [missingBinary, setMissingBinary] = useState<string | null>(null)

  useEffect(() => {
    window.aether
      .getBinaryStatus()
      .then((status) => {
        if (!status['yt-dlp'].found) {
          setMissingBinary(
            t('errors.binary_missing', {
              name: 'yt-dlp',
              dir: status['yt-dlp'].dir,
              url: 'github.com/yt-dlp/yt-dlp'
            })
          )
        }
      })
      .catch(() => {})
  }, [t])

  const analyzeUrl = async (target: string): Promise<void> => {
    setAnalyzing(true)
    setError(null)
    setPreview(null)
    setChoice(null)
    try {
      setPreview(await window.aether.previewDownload(target))
    } catch (err) {
      setError(ipcErrorMessage(err))
    } finally {
      setAnalyzing(false)
    }
  }

  const analyze = async (): Promise<void> => {
    const trimmed = url.trim()
    if (!trimmed) return
    // video link carrying a playlist/mix context: let the user pick first
    const split = splitYoutubeWatchUrl(trimmed)
    if (split) {
      setError(null)
      setPreview(null)
      setChoice(split)
      return
    }
    await analyzeUrl(trimmed)
  }

  const confirm = async (): Promise<void> => {
    if (!preview) return
    try {
      await window.aether.startDownload(preview)
      setPreview(null)
      setUrl('')
      void refresh()
    } catch (err) {
      setError(ipcErrorMessage(err))
    }
  }

  const hasFinished = items.some(
    (i) => i.status === 'completed' || i.status === 'error' || i.status === 'cancelled'
  )

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader
        title={t('downloader.title')}
        actions={
          hasFinished ? (
            <button
              className="btn-ghost flex items-center gap-2 rounded-lg px-3 py-2 text-[12.5px] font-medium"
              onClick={() => void window.aether.clearFinishedDownloads().then(refresh)}
            >
              <Trash2 size={13} /> {t('downloader.clear_finished')}
            </button>
          ) : undefined
        }
      />

      <div className="px-[var(--content-x)] pb-4">
        <div className="mx-auto flex max-w-[880px] gap-2" data-tour="download-input">
          <div
            className="glass-chrome flex h-12 flex-1 items-center gap-2.5 rounded-full px-4 transition-[border-color,box-shadow] duration-150 focus-within:border-[rgba(var(--accent-rgb)/0.5)] focus-within:shadow-[0_0_0_3px_rgba(var(--accent-rgb)/0.15)]"
          >
            <Link2 size={16} className="shrink-0 text-text-3" />
            <input
              ref={inputRef}
              className="h-full flex-1 bg-transparent text-[13.5px] outline-none placeholder:text-text-3"
              placeholder={t('downloader.placeholder')}
              value={url}
              onChange={(e) => setUrl(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && void analyze()}
              onPaste={() => setTimeout(() => void analyze(), 50)}
            />
          </div>
          <button
            className="btn-accent flex items-center gap-2 rounded-full px-5 text-[13px]"
            onClick={() => void analyze()}
            disabled={analyzing || !url.trim()}
          >
            {analyzing ? <Loader2 size={15} className="animate-spin" /> : <Download size={15} />}
            {t('downloader.analyze')}
          </button>
        </div>

        {missingBinary && (
          <div className="fade-in mx-auto mt-2 flex max-w-[880px] items-start gap-2 rounded-xl border border-[var(--warning-soft)] bg-[var(--warning-soft)] px-3 py-2 text-[12.5px] text-[var(--warning)]">
            <AlertCircle size={15} className="mt-0.5 shrink-0" />
            <div className="min-w-0 flex-1">{missingBinary}</div>
            <button
              className="icon-btn h-6 w-6 shrink-0 text-[var(--warning)]"
              onClick={() => setMissingBinary(null)}
              aria-label={t('downloader.cancel')}
            >
              <X size={13} />
            </button>
          </div>
        )}

        {error && <div className="error-banner mt-2 text-[12.5px]">{error}</div>}

        {analyzing && !preview && (
          <div
            className="fade-in mt-3 flex items-center gap-4 rounded-xl border border-[var(--hairline)] bg-white/[0.02] p-4"
          >
            <div className="skeleton h-16 w-16 shrink-0 rounded-lg" />
            <div className="flex min-w-0 flex-1 flex-col gap-2">
              <div className="skeleton h-4 w-2/5" />
              <div className="skeleton h-3 w-1/4" />
            </div>
          </div>
        )}

        {choice && (
          <div
            className="fade-in mt-3 flex flex-wrap items-center gap-3 rounded-xl border p-4"
            style={{ background: 'var(--accent-soft)', borderColor: 'var(--accent-glow)' }}
          >
            <div className="min-w-0 flex-1 text-[13px] text-text-2">
              {t(choice.isMix ? 'downloader.link_has_mix' : 'downloader.link_has_both')}
            </div>
            <div className="flex shrink-0 items-center gap-2">
              <button
                className="btn-ghost rounded-lg px-3 py-2 text-[12.5px] font-medium text-text-2"
                onClick={() => setChoice(null)}
              >
                {t('downloader.cancel')}
              </button>
              <button
                className="btn-ghost flex items-center gap-2 rounded-lg px-4 py-2 text-[13px] font-semibold"
                onClick={() => void analyzeUrl(choice.playlistUrl)}
              >
                <ListMusic size={14} />
                {t(choice.isMix ? 'downloader.whole_mix' : 'downloader.whole_playlist')}
              </button>
              <button
                className="btn-accent flex items-center gap-2 rounded-lg px-4 py-2 text-[13px]"
                onClick={() => void analyzeUrl(choice.videoUrl)}
              >
                <Music2 size={14} /> {t('downloader.video_only')}
              </button>
            </div>
          </div>
        )}

        {preview && (
          <div
            className="slide-up-in mt-3 flex items-center gap-4 rounded-xl border p-4"
            style={{ background: 'var(--accent-soft)', borderColor: 'var(--accent-glow)' }}
          >
            <div className="h-16 w-16 shrink-0 overflow-hidden rounded-lg bg-surface-3 shadow-lg">
              {remoteCover(preview.cover_url) ? (
                <img src={remoteCover(preview.cover_url)!} alt="" className="h-full w-full object-cover" />
              ) : (
                <div className="flex h-full w-full items-center justify-center">
                  <Music2 size={20} className="text-text-3" />
                </div>
              )}
            </div>
            <div className="min-w-0 flex-1">
              <div className="truncate text-[14.5px] font-bold">{preview.title}</div>
              <div className="truncate text-[12.5px] text-text-2">
                {preview.artist ?? ''}
                {preview.album && preview.album !== preview.title ? ` · ${preview.album}` : ''}
              </div>
              <div className="mt-0.5 text-[11.5px] text-text-3">
                {sourceLabel(preview, t('downloader.source_search'))} ·{' '}
                {t('downloader.tracks', { count: preview.total_tracks })}
                {preview.estimated_size_mb
                  ? ` · ${t('downloader.estimated_size', { size: preview.estimated_size_mb })}`
                  : ''}
              </div>
            </div>
            <button
              className="btn-ghost rounded-lg px-3 py-2 text-[12.5px] font-medium text-text-2"
              onClick={() => setPreview(null)}
            >
              {t('downloader.cancel')}
            </button>
            <button
              className="btn-accent flex items-center gap-2 rounded-lg px-4 py-2 text-[13px]"
              onClick={() => void confirm()}
            >
              <Download size={14} /> {t('downloader.confirm_download')}
            </button>
          </div>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-[var(--content-x)] pb-[var(--player-clearance)]">
        {items.length === 0 ? (
          <EmptyState
            icon={Download}
            title={t('downloader.empty')}
            action={
              <button
                className="btn-accent flex items-center gap-2 rounded-full px-4 py-2 text-[13px]"
                onClick={() => inputRef.current?.focus()}
              >
                <Link2 size={14} /> {t('downloader.empty_cta')}
              </button>
            }
          />
        ) : (
          <div className="mx-auto flex max-w-[880px] flex-col gap-2">
            {items.map((item) => (
              <DownloadCard key={item.id} item={item} />
            ))}
          </div>
        )}
      </div>
    </div>
  )
}
