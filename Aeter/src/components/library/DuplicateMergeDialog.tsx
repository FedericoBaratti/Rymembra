import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { X, Merge } from 'lucide-react'
import type { DuplicateGroup, Track } from '@shared/types'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import { toast } from '@/store/useToastStore'
import { formatBytes, formatDuration } from '@/lib/format'
import { ipcErrorMessage } from '@/lib/ipcError'

const LOSSLESS = ['flac', 'alac', 'wav', 'aiff', 'pcm']

// Mirrors pickBestQuality in electron/modules/mergeTracks.ts for the preselection.
function bestQualityId(tracks: Track[]): number {
  const rank = (t: Track): number =>
    LOSSLESS.some((c) => (t.codec ?? '').toLowerCase().includes(c)) ? 1 : 0
  return [...tracks].sort(
    (a, b) =>
      rank(b) - rank(a) ||
      (b.bitrate ?? 0) - (a.bitrate ?? 0) ||
      (b.sample_rate ?? 0) - (a.sample_rate ?? 0) ||
      b.file_size - a.file_size
  )[0].id
}

export default function DuplicateMergeDialog({
  group,
  onClose,
  onMerged
}: {
  group: DuplicateGroup
  onClose: () => void
  onMerged: () => void
}): React.JSX.Element {
  const { t } = useTranslation()
  const [survivorId, setSurvivorId] = useState(() => bestQualityId(group.tracks))
  const [trashFiles, setTrashFiles] = useState(true)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const trapRef = useFocusTrap<HTMLDivElement>(true, onClose)

  const merge = async (): Promise<void> => {
    setBusy(true)
    setError(null)
    const victims = group.tracks.filter((tr) => tr.id !== survivorId).map((tr) => tr.id)
    try {
      const outcome = await window.aether.mergeDuplicates(survivorId, victims, trashFiles)
      toast.success(t('merge.done', { count: outcome.merged }))
      onMerged()
      onClose()
    } catch (err) {
      setError(ipcErrorMessage(err))
      setBusy(false)
    }
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
        aria-label={t('merge.title')}
        className="glass-modal scale-in flex max-h-[80vh] w-[min(620px,calc(100vw-48px))] flex-col overflow-hidden rounded-2xl"
        style={{ boxShadow: '0 24px 80px rgba(0,0,0,0.7)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div
          className="flex items-center justify-between border-b px-5 py-3.5"
          style={{ borderColor: 'var(--hairline)' }}
        >
          <div>
            <h2 className="text-[15px] font-bold">{t('merge.title')}</h2>
            <div className="text-[12px] text-text-3">{t('merge.subtitle')}</div>
          </div>
          <button className="icon-btn h-7 w-7" onClick={onClose} aria-label={t('common.close')}>
            <X size={15} />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-5">
          <div className="flex flex-col gap-2">
            {group.tracks.map((tr) => (
              <label
                key={tr.id}
                className={`flex cursor-pointer items-start gap-3 rounded-xl border p-3 transition-colors ${
                  tr.id === survivorId
                    ? 'border-[var(--accent)] bg-[var(--accent-soft)]'
                    : 'border-[var(--hairline)] bg-white/[0.02] hover:bg-white/[0.05]'
                }`}
              >
                <input
                  type="radio"
                  name="survivor"
                  className="mt-1"
                  checked={tr.id === survivorId}
                  onChange={() => setSurvivorId(tr.id)}
                />
                <div className="min-w-0 flex-1">
                  <div className="truncate text-[13px] font-semibold">
                    {tr.artist} — {tr.title}
                  </div>
                  <div className="tnum mt-0.5 text-[11.5px] text-text-2">
                    {tr.codec ?? '?'} · {tr.bitrate ? `${Math.round(tr.bitrate / 1000)} kbps` : '?'} ·{' '}
                    {formatBytes(tr.file_size)} · {formatDuration(tr.duration)} ·{' '}
                    {t('merge.play_count', { count: tr.play_count })}
                  </div>
                  <div className="mt-0.5 truncate text-[11px] text-text-3" title={tr.path}>
                    {tr.path}
                  </div>
                </div>
                {tr.id === survivorId && (
                  <span className="shrink-0 rounded-full bg-[var(--accent)] px-2 py-0.5 text-[10.5px] font-semibold text-white">
                    {t('merge.keep')}
                  </span>
                )}
              </label>
            ))}
          </div>

          <label className="mt-4 flex items-center gap-2 text-[12.5px]">
            <input
              type="checkbox"
              checked={trashFiles}
              onChange={(e) => setTrashFiles(e.target.checked)}
            />
            {t('merge.trash_files')}
          </label>

          {error && <div className="error-banner mt-3 text-[12.5px]">{error}</div>}
        </div>

        <div
          className="flex items-center justify-end gap-2 border-t px-5 py-3.5"
          style={{ borderColor: 'var(--hairline)' }}
        >
          <button
            className="btn-ghost rounded-lg px-4 py-2 text-[13px] font-medium text-text-2"
            onClick={onClose}
          >
            {t('common.cancel')}
          </button>
          <button
            className="btn-accent flex items-center gap-2 rounded-lg px-4 py-2 text-[13px]"
            onClick={() => void merge()}
            disabled={busy}
          >
            <Merge size={14} />
            {busy ? t('merge.merging') : t('merge.confirm', { count: group.tracks.length - 1 })}
          </button>
        </div>
      </div>
    </div>
  )
}
