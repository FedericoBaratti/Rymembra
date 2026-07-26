import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { X, Music2, ImagePlus } from 'lucide-react'
import type { Track, TrackMetadataUpdate } from '@shared/types'
import { useUiStore } from '@/store/useUiStore'
import { useLibraryStore } from '@/store/useLibraryStore'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'
import { isMobile } from '@/lib/platform'
import FullScreenSheet from '@/components/ui/FullScreenSheet'

type BatchField = 'artist' | 'album_artist' | 'album' | 'genre' | 'year' | 'disc_number' | 'comment'

const FIELDS: BatchField[] = ['artist', 'album_artist', 'album', 'genre', 'year', 'disc_number', 'comment']
const NUMERIC: Set<BatchField> = new Set(['year', 'disc_number'])

function commonValue(tracks: Track[], field: BatchField): string {
  const values = new Set(tracks.map((tr) => String(tr[field] ?? '')))
  return values.size === 1 ? [...values][0] : ''
}

function hasMixedValues(tracks: Track[], field: BatchField): boolean {
  return new Set(tracks.map((tr) => String(tr[field] ?? ''))).size > 1
}

export default function BatchMetadataEditor(): React.JSX.Element | null {
  const { t } = useTranslation()
  const trackIds = useUiStore((s) => s.batchEditTrackIds)
  const setBatchEditTrackIds = useUiStore((s) => s.setBatchEditTrackIds)
  const refreshTracks = useLibraryStore((s) => s.refreshTracks)

  const [tracks, setTracks] = useState<Track[] | null>(null)
  const [fields, setFields] = useState<Record<BatchField, string>>({} as Record<BatchField, string>)
  const [dirty, setDirty] = useState<Set<BatchField>>(new Set())
  const [newCover, setNewCover] = useState<{ base64: string; preview: string } | null>(null)
  const [dragOver, setDragOver] = useState(false)
  const [busy, setBusy] = useState(false)
  const [progress, setProgress] = useState<{ done: number; total: number; errors: number } | null>(null)
  const [error, setError] = useState<string | null>(null)
  const fileInputRef = useRef<HTMLInputElement>(null)

  const open = trackIds != null && trackIds.length > 0
  const trapRef = useFocusTrap<HTMLDivElement>(open && !isMobile, () => !busy && setBatchEditTrackIds(null))

  useEffect(() => {
    setTracks(null)
    setDirty(new Set())
    setNewCover((prev) => {
      // Free the previous preview blob: object URLs are never GC'd on their own.
      if (prev) URL.revokeObjectURL(prev.preview)
      return null
    })
    setError(null)
    setProgress(null)
    if (!trackIds || trackIds.length === 0) return
    void window.aether
      .getTracksByIds(trackIds)
      .then((loaded) => {
        setTracks(loaded)
        const initial = {} as Record<BatchField, string>
        for (const f of FIELDS) initial[f] = commonValue(loaded, f)
        setFields(initial)
      })
      .catch((err) => setError(ipcErrorMessage(err)))
  }, [trackIds])

  useEffect(() => {
    if (!open) return
    return window.aether.on('batch-metadata:progress', (p) => setProgress(p))
  }, [open])

  if (!open) return null

  const close = (): void => {
    if (!busy) setBatchEditTrackIds(null)
  }

  const set = (key: BatchField, value: string): void => {
    setFields((f) => ({ ...f, [key]: value }))
    setDirty((d) => new Set(d).add(key))
  }

  // Shared by drag-and-drop (desktop) and the tap-to-pick file input (touch).
  const applyCoverFile = async (file: File): Promise<void> => {
    if (!file.type.startsWith('image/')) return
    const buf = await file.arrayBuffer()
    const bytes = new Uint8Array(buf)
    let binary = ''
    for (let i = 0; i < bytes.length; i += 0x8000) {
      binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
    }
    setNewCover((prev) => {
      if (prev) URL.revokeObjectURL(prev.preview)
      return { base64: btoa(binary), preview: URL.createObjectURL(file) }
    })
  }

  const onDropCover = async (e: React.DragEvent): Promise<void> => {
    e.preventDefault()
    setDragOver(false)
    const file = e.dataTransfer.files[0]
    if (file) await applyCoverFile(file)
  }

  const save = async (): Promise<void> => {
    if (!tracks || !trackIds) return
    const update: TrackMetadataUpdate = {}
    for (const f of dirty) {
      if (NUMERIC.has(f)) {
        const v = fields[f].trim()
        ;(update as Record<string, unknown>)[f] = v === '' ? null : Number(v) || null
      } else {
        ;(update as Record<string, unknown>)[f] = fields[f].trim()
      }
    }
    if (newCover) update.coverImageBase64 = newCover.base64
    if (Object.keys(update).length === 0) {
      close()
      return
    }
    setBusy(true)
    setError(null)
    try {
      const res = await window.aether.updateTracksMetadata(trackIds, update)
      void refreshTracks()
      if (res.errors > 0) {
        setError(t('metadata.batch_errors', { count: res.errors }))
      } else {
        toast.success(t('toast.metadata_saved_n', { count: trackIds.length }))
        setBatchEditTrackIds(null)
      }
    } catch (err) {
      setError(ipcErrorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  const input = (key: BatchField, label: string, span = 1): React.JSX.Element => (
    <label className={`flex flex-col gap-1 ${span === 2 ? 'col-span-2' : ''}`}>
      <span className="text-[11px] font-semibold uppercase tracking-wide text-text-3">{label}</span>
      <input
        className="field-input h-9"
        inputMode={NUMERIC.has(key) ? 'numeric' : undefined}
        value={fields[key] ?? ''}
        placeholder={tracks && hasMixedValues(tracks, key) && !dirty.has(key) ? t('metadata.mixed_values') : ''}
        onChange={(e) => set(key, e.target.value)}
      />
    </label>
  )

  const formBody = !tracks ? (
    <div className="grid grid-cols-2 gap-3 p-5">
      {Array.from({ length: 6 }).map((_, i) => (
        <div key={i} className="skeleton h-9" />
      ))}
    </div>
  ) : (
    <div className={isMobile ? 'p-4' : 'min-h-0 flex-1 overflow-y-auto p-5'}>
      <div className={`mb-4 flex gap-4 ${isMobile ? 'flex-col' : ''}`}>
              <div
                className={`relative flex h-28 w-28 shrink-0 cursor-pointer items-center justify-center overflow-hidden rounded-xl border-2 border-dashed transition-colors ${
                  dragOver ? 'border-[var(--accent)]' : 'border-transparent'
                }`}
                style={{ background: 'var(--color-surface-3)' }}
                role="button"
                tabIndex={0}
                onClick={() => fileInputRef.current?.click()}
                onKeyDown={(e) => {
                  if (e.key === 'Enter' || e.key === ' ') fileInputRef.current?.click()
                }}
                onDragOver={(e) => {
                  e.preventDefault()
                  setDragOver(true)
                }}
                onDragLeave={() => setDragOver(false)}
                onDrop={(e) => void onDropCover(e)}
                title={t('metadata.drop_cover')}
                aria-label={t('metadata.pick_cover')}
              >
                {newCover ? (
                  <img src={newCover.preview} alt="" className="h-full w-full object-cover" draggable={false} />
                ) : (
                  <Music2 size={24} className="text-text-3" />
                )}
                <span className="absolute bottom-1.5 right-1.5 flex h-7 w-7 items-center justify-center rounded-full bg-black/60 text-white">
                  <ImagePlus size={14} />
                </span>
                {dragOver && (
                  <div className="absolute inset-0 flex items-center justify-center bg-black/60 p-2 text-center text-[11px] text-white">
                    {t('metadata.drop_cover')}
                  </div>
                )}
                <input
                  ref={fileInputRef}
                  type="file"
                  accept="image/*"
                  className="hidden"
                  onChange={(e) => {
                    const f = e.target.files?.[0]
                    if (f) void applyCoverFile(f)
                    e.target.value = ''
                  }}
                />
              </div>
              <div className="grid flex-1 grid-cols-2 content-start gap-3">
                {input('artist', t('metadata.artist'))}
                {input('album_artist', t('metadata.album_artist'))}
                {input('album', t('metadata.album'), 2)}
              </div>
            </div>

            <div className="grid grid-cols-2 gap-3">
              {input('genre', t('metadata.genre'))}
              <div className="grid grid-cols-2 gap-2">
                {input('year', t('metadata.year'))}
                {input('disc_number', t('metadata.disc_number'))}
              </div>
              {input('comment', t('metadata.comment'), 2)}
            </div>

            {busy && progress && (
              <div className="mt-4">
                <div className="mb-1 text-[12px] text-text-2">
                  {t('metadata.applying', { done: progress.done, total: progress.total })}
                </div>
                <div className="h-1.5 overflow-hidden rounded-full bg-surface-3">
                  <div
                    className="h-full rounded-full transition-all"
                    style={{
                      width: `${(progress.done / Math.max(1, progress.total)) * 100}%`,
                      background: 'var(--accent)'
                    }}
                  />
                </div>
              </div>
            )}

      {error && <div className="error-banner mt-3 text-[12.5px]">{error}</div>}
    </div>
  )

  const saveButton = (
    <button
      className="btn-accent rounded-lg px-4 py-2 text-[13px]"
      onClick={() => void save()}
      disabled={busy || !tracks || (dirty.size === 0 && !newCover)}
    >
      {busy ? t('metadata.saving') : t('metadata.save')}
    </button>
  )

  if (isMobile) {
    return (
      <FullScreenSheet
        open
        onClose={close}
        title={t('metadata.edit_n', { count: trackIds.length })}
        actions={saveButton}
      >
        {formBody}
      </FullScreenSheet>
    )
  }

  return (
    <div className="overlay-in fixed inset-0 z-50 flex items-center justify-center bg-black/55 backdrop-blur-sm" onClick={close}>
      <div
        ref={trapRef}
        role="dialog"
        aria-modal="true"
        aria-label={t('metadata.edit_n', { count: trackIds.length })}
        className="glass-modal scale-in flex max-h-[85vh] w-[min(560px,calc(100vw-48px))] flex-col overflow-hidden rounded-2xl"
        style={{ boxShadow: '0 24px 80px rgba(0,0,0,0.7)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b px-5 py-3.5" style={{ borderColor: 'var(--hairline)' }}>
          <h2 className="text-[15px] font-bold">{t('metadata.edit_n', { count: trackIds.length })}</h2>
          <button className="icon-btn h-7 w-7" onClick={close} aria-label={t('common.close')} disabled={busy}>
            <X size={15} />
          </button>
        </div>

        {formBody}

        <div className="flex items-center justify-end gap-2 border-t px-5 py-3.5" style={{ borderColor: 'var(--hairline)' }}>
          <button
            className="btn-ghost rounded-lg px-4 py-2 text-[13px] font-medium text-text-2"
            onClick={close}
            disabled={busy}
          >
            {t('metadata.cancel')}
          </button>
          {saveButton}
        </div>
      </div>
    </div>
  )
}
