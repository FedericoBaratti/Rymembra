import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { X, Sparkles, Music2 } from 'lucide-react'
import type { Track, TrackMetadataUpdate } from '@shared/types'
import { useUiStore } from '@/store/useUiStore'
import { useLibraryStore } from '@/store/useLibraryStore'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import { toast } from '@/store/useToastStore'
import { coverUrl } from '@/lib/format'
import { ipcErrorMessage, translateErrorCode } from '@/lib/ipcError'

type Fields = {
  title: string
  artist: string
  album: string
  album_artist: string
  year: string
  track_number: string
  disc_number: string
  genre: string
  bpm: string
  comment: string
  lyrics: string
}

function fieldsFrom(track: Track): Fields {
  return {
    title: track.title,
    artist: track.artist,
    album: track.album,
    album_artist: track.album_artist ?? '',
    year: track.year?.toString() ?? '',
    track_number: track.track_number?.toString() ?? '',
    disc_number: track.disc_number?.toString() ?? '',
    genre: track.genre ?? '',
    bpm: track.bpm?.toString() ?? '',
    comment: track.comment ?? '',
    lyrics: track.lyrics ?? ''
  }
}

export default function MetadataEditor(): React.JSX.Element | null {
  const { t } = useTranslation()
  const trackId = useUiStore((s) => s.editTrackId)
  const setEditTrackId = useUiStore((s) => s.setEditTrackId)
  const refreshTracks = useLibraryStore((s) => s.refreshTracks)
  const updateTrackInPlayer = usePlayerStore((s) => s.updateTrack)

  const [track, setTrack] = useState<Track | null>(null)
  const [fields, setFields] = useState<Fields | null>(null)
  const [newCover, setNewCover] = useState<{ base64: string; mime: string; preview: string } | null>(null)
  const [busy, setBusy] = useState<'save' | 'enrich' | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [dragOver, setDragOver] = useState(false)

  useEffect(() => {
    setTrack(null)
    setFields(null)
    setNewCover(null)
    setError(null)
    if (trackId == null) return
    void window.aether.getTrackById(trackId).then((tr) => {
      if (tr) {
        setTrack(tr)
        setFields(fieldsFrom(tr))
      }
    })
  }, [trackId])

  const trapRef = useFocusTrap<HTMLDivElement>(trackId != null, () => setEditTrackId(null))

  if (trackId == null) return null

  const close = (): void => setEditTrackId(null)

  const set = (key: keyof Fields, value: string): void => {
    setFields((f) => (f ? { ...f, [key]: value } : f))
  }

  const onDropCover = async (e: React.DragEvent): Promise<void> => {
    e.preventDefault()
    setDragOver(false)
    const file = e.dataTransfer.files[0]
    if (!file || !file.type.startsWith('image/')) return
    const buf = await file.arrayBuffer()
    const bytes = new Uint8Array(buf)
    let binary = ''
    for (let i = 0; i < bytes.length; i += 0x8000) {
      binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
    }
    setNewCover({
      base64: btoa(binary),
      mime: file.type,
      preview: URL.createObjectURL(file)
    })
  }

  const save = async (): Promise<void> => {
    if (!fields || !track) return
    setBusy('save')
    setError(null)
    const num = (s: string): number | null => (s.trim() === '' ? null : Number(s) || null)
    const update: TrackMetadataUpdate = {
      title: fields.title.trim(),
      artist: fields.artist.trim(),
      album: fields.album.trim(),
      album_artist: fields.album_artist.trim(),
      year: num(fields.year),
      track_number: num(fields.track_number),
      disc_number: num(fields.disc_number),
      genre: fields.genre.trim(),
      bpm: num(fields.bpm),
      comment: fields.comment,
      lyrics: fields.lyrics,
      coverImageBase64: newCover?.base64 ?? null,
      coverImageMime: newCover?.mime ?? null
    }
    try {
      const updated = await window.aether.updateTrackMetadata(track.id, update)
      updateTrackInPlayer(updated)
      void refreshTracks()
      toast.success(t('toast.metadata_saved'), updated.title)
      close()
    } catch (err) {
      setError(ipcErrorMessage(err))
    } finally {
      setBusy(null)
    }
  }

  const enrich = async (): Promise<void> => {
    if (!track) return
    setBusy('enrich')
    setError(null)
    try {
      const result = await window.aether.enrichTrack(track.id)
      if (result.applied) {
        const fresh = await window.aether.getTrackById(track.id)
        if (fresh) {
          setTrack(fresh)
          setFields(fieldsFrom(fresh))
          updateTrackInPlayer(fresh)
          void refreshTracks()
        }
      } else {
        setError(translateErrorCode(result.message))
      }
    } catch (err) {
      setError(ipcErrorMessage(err))
    } finally {
      setBusy(null)
    }
  }

  const cover = newCover?.preview ?? coverUrl(track?.cover_art_hash)

  const input = (key: keyof Fields, label: string, span = 1): React.JSX.Element => (
    <label className={`flex flex-col gap-1 ${span === 2 ? 'col-span-2' : ''}`}>
      <span className="text-[11px] font-semibold uppercase tracking-wide text-text-3">{label}</span>
      <input
        className="field-input h-9"
        value={fields?.[key] ?? ''}
        onChange={(e) => set(key, e.target.value)}
      />
    </label>
  )

  return (
    <div className="overlay-in fixed inset-0 z-50 flex items-center justify-center bg-black/55 backdrop-blur-sm" onClick={close}>
      <div
        ref={trapRef}
        role="dialog"
        aria-modal="true"
        aria-label={t('metadata.edit')}
        className="glass-modal scale-in flex max-h-[85vh] w-[min(640px,calc(100vw-48px))] flex-col overflow-hidden rounded-2xl"
        style={{ boxShadow: '0 24px 80px rgba(0,0,0,0.7)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b px-5 py-3.5" style={{ borderColor: 'var(--hairline)' }}>
          <h2 className="text-[15px] font-bold">{t('metadata.edit')}</h2>
          <button className="icon-btn h-7 w-7" onClick={close} aria-label={t('common.close')}>
            <X size={15} />
          </button>
        </div>

        {!fields ? (
          <div className="grid grid-cols-2 gap-3 p-5">
            {Array.from({ length: 6 }).map((_, i) => (
              <div key={i} className="skeleton h-9" />
            ))}
          </div>
        ) : (
          <div className="min-h-0 flex-1 overflow-y-auto p-5">
            <div className="mb-4 flex gap-4">
              <div
                className={`relative flex h-32 w-32 shrink-0 items-center justify-center overflow-hidden rounded-xl border-2 border-dashed transition-colors ${
                  dragOver ? 'border-[var(--accent)]' : 'border-transparent'
                }`}
                style={{ background: 'var(--color-surface-3)' }}
                onDragOver={(e) => {
                  e.preventDefault()
                  setDragOver(true)
                }}
                onDragLeave={() => setDragOver(false)}
                onDrop={(e) => void onDropCover(e)}
                title={t('metadata.drop_cover')}
              >
                {cover ? (
                  <img src={cover} alt="" className="h-full w-full object-cover" />
                ) : (
                  <Music2 size={28} className="text-text-3" />
                )}
                {dragOver && (
                  <div className="absolute inset-0 flex items-center justify-center bg-black/60 p-2 text-center text-[11px] text-white">
                    {t('metadata.drop_cover')}
                  </div>
                )}
              </div>
              <div className="grid flex-1 grid-cols-2 content-start gap-3">
                {input('title', t('metadata.title'), 2)}
                {input('artist', t('metadata.artist'))}
                {input('album_artist', t('metadata.album_artist'))}
              </div>
            </div>

            <div className="grid grid-cols-2 gap-3">
              {input('album', t('metadata.album'))}
              {input('genre', t('metadata.genre'))}
              <div className="grid grid-cols-3 gap-2">
                {input('year', t('metadata.year'))}
                {input('track_number', t('metadata.track_number'))}
                {input('disc_number', t('metadata.disc_number'))}
              </div>
              {input('bpm', t('metadata.bpm'))}
              {input('comment', t('metadata.comment'), 2)}
              <label className="col-span-2 flex flex-col gap-1">
                <span className="text-[11px] font-semibold uppercase tracking-wide text-text-3">
                  {t('metadata.lyrics')}
                </span>
                <textarea
                  className="field-input h-28 resize-none py-2"
                  value={fields.lyrics}
                  onChange={(e) => set('lyrics', e.target.value)}
                />
              </label>
            </div>

            {error && <div className="error-banner mt-3 text-[12.5px]">{error}</div>}
          </div>
        )}

        <div className="flex items-center justify-between border-t px-5 py-3.5" style={{ borderColor: 'var(--hairline)' }}>
          <button
            className="flex items-center gap-2 rounded-lg px-3 py-2 text-[13px] font-medium text-[var(--accent)] transition-colors hover:bg-[var(--accent-soft)] disabled:opacity-50"
            onClick={() => void enrich()}
            disabled={busy != null || !fields}
          >
            <Sparkles size={15} />
            {busy === 'enrich' ? t('metadata.enriching') : t('metadata.auto_search')}
          </button>
          <div className="flex gap-2">
            <button
              className="btn-ghost rounded-lg px-4 py-2 text-[13px] font-medium text-text-2"
              onClick={close}
            >
              {t('metadata.cancel')}
            </button>
            <button
              className="btn-accent rounded-lg px-4 py-2 text-[13px]"
              onClick={() => void save()}
              disabled={busy != null || !fields}
            >
              {busy === 'save' ? t('metadata.saving') : t('metadata.save')}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
