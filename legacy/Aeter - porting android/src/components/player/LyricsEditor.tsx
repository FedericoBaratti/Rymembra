import { useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  X,
  Plus,
  Trash2,
  Timer,
  ChevronLeft,
  ChevronRight,
  RefreshCw,
  ClipboardPaste
} from 'lucide-react'
import type { Track } from '@shared/types'
import { formatLrcTime, serializeLrc } from '@shared/lrc'
import { useUiStore } from '@/store/useUiStore'
import { ipcErrorMessage } from '@/lib/ipcError'
import { usePlayerStore } from '@/store/usePlayerStore'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import { toast } from '@/store/useToastStore'
import { playerEngine } from '@/lib/player'
import ConfirmDialog from '@/components/ui/ConfirmDialog'
import { isMobile } from '@/lib/platform'
import FullScreenSheet from '@/components/ui/FullScreenSheet'

interface EditLine {
  time: number | null
  text: string
}

const NUDGE = 0.1

function linesFromLyrics(l: { synced: { time: number; text: string }[] | null; plain: string | null }): EditLine[] {
  if (l.synced) return l.synced.map((s) => ({ time: s.time, text: s.text }))
  if (l.plain) return l.plain.split(/\r?\n/).map((text) => ({ time: null, text }))
  return []
}

export default function LyricsEditor(): React.JSX.Element | null {
  const { t } = useTranslation()
  const trackId = useUiStore((s) => s.lyricsEditTrackId)
  const setLyricsEditTrackId = useUiStore((s) => s.setLyricsEditTrackId)
  const updateTrackInPlayer = usePlayerStore((s) => s.updateTrack)
  const playingTrack = usePlayerStore((s) => s.currentTrack)

  const [track, setTrack] = useState<Track | null>(null)
  const [lines, setLines] = useState<EditLine[] | null>(null)
  const [dirty, setDirty] = useState(false)
  const [busy, setBusy] = useState<'save' | 'refetch' | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [syncIdx, setSyncIdx] = useState(0)
  const [activeLine, setActiveLine] = useState(-1)
  const [pasteMode, setPasteMode] = useState(false)
  const [pasteText, setPasteText] = useState('')
  // in-app confirmation for destructive actions while dirty (no window.confirm:
  // native WebView dialogs clash with the app chrome, see ConfirmDialog)
  const [confirmAction, setConfirmAction] = useState<'discard' | 'refetch' | null>(null)
  const listRef = useRef<HTMLDivElement>(null)

  // the edited track is pinned: playback can move on, the editor stays put
  const isCurrent = playingTrack?.id === trackId

  useEffect(() => {
    setTrack(null)
    setLines(null)
    setDirty(false)
    setError(null)
    setSyncIdx(0)
    setPasteMode(false)
    if (trackId == null) return
    void window.aether.getTrackById(trackId).then((tr) => tr && setTrack(tr))
    void window.aether
      .getLyrics(trackId)
      .then((l) => setLines(linesFromLyrics(l)))
      .catch(() => setLines([]))
  }, [trackId])

  // highlight the line matching the playback position (same scan as LyricsPanel)
  useEffect(() => {
    if (!isCurrent || !lines) return
    const iv = window.setInterval(() => {
      const pos = playerEngine.position()
      let idx = -1
      for (let i = 0; i < lines.length; i++) {
        const tm = lines[i].time
        if (tm != null && tm <= pos) idx = i
        else if (tm != null) break
      }
      setActiveLine(idx)
    }, 200)
    return () => window.clearInterval(iv)
  }, [isCurrent, lines])

  const close = (): void => {
    if (dirty) {
      setConfirmAction('discard')
      return
    }
    setLyricsEditTrackId(null)
  }

  const trapRef = useFocusTrap<HTMLDivElement>(trackId != null && !isMobile, close)

  if (trackId == null) return null

  const mutate = (fn: (prev: EditLine[]) => EditLine[]): void => {
    setLines((prev) => (prev ? fn(prev) : prev))
    setDirty(true)
  }

  const stamp = (i: number): void => {
    const pos = playerEngine.position()
    mutate((prev) => prev.map((l, idx) => (idx === i ? { ...l, time: pos } : l)))
    setSyncIdx(i + 1)
    listRef.current?.children[Math.min(i + 1, (lines?.length ?? 1) - 1)]?.scrollIntoView({
      block: 'center',
      behavior: 'smooth'
    })
  }

  const nudge = (i: number, delta: number): void => {
    mutate((prev) =>
      prev.map((l, idx) =>
        idx === i && l.time != null ? { ...l, time: Math.max(0, l.time + delta) } : l
      )
    )
  }

  const shiftAll = (delta: number): void => {
    mutate((prev) =>
      prev.map((l) => (l.time != null ? { ...l, time: Math.max(0, l.time + delta) } : l))
    )
  }

  const setText = (i: number, text: string): void => {
    mutate((prev) => prev.map((l, idx) => (idx === i ? { ...l, text } : l)))
  }

  const removeLine = (i: number): void => {
    mutate((prev) => prev.filter((_, idx) => idx !== i))
    if (syncIdx > i) setSyncIdx(syncIdx - 1)
  }

  const addLine = (): void => {
    mutate((prev) => [...prev, { time: null, text: '' }])
  }

  const playFrom = (i: number): void => {
    const tm = lines?.[i]?.time
    if (tm != null && isCurrent) playerEngine.seek(tm)
  }

  const applyPaste = (): void => {
    const pasted = pasteText
      .split(/\r?\n/)
      .map((text) => ({ time: null, text: text.trim() }))
      .filter((l) => l.text)
    if (pasted.length === 0) return
    setLines(pasted)
    setDirty(true)
    setSyncIdx(0)
    setPasteMode(false)
    setPasteText('')
  }

  const refetch = (): void => {
    if (dirty) {
      setConfirmAction('refetch')
      return
    }
    void doRefetch()
  }

  const doRefetch = async (): Promise<void> => {
    setBusy('refetch')
    setError(null)
    try {
      const l = await window.aether.refetchLyrics(trackId)
      const next = linesFromLyrics(l)
      if (next.length === 0) {
        setError(t('lyrics_editor.no_remote'))
      } else {
        setLines(next)
        setDirty(false)
        setSyncIdx(0)
      }
    } catch (err) {
      setError(ipcErrorMessage(err))
    } finally {
      setBusy(null)
    }
  }

  const timedCount = lines?.filter((l) => l.time != null).length ?? 0
  const savesAsPlain = timedCount < 3

  const save = async (): Promise<void> => {
    if (!lines) return
    setBusy('save')
    setError(null)
    const timed = lines.filter((l): l is { time: number; text: string } => l.time != null)
    const lyrics = savesAsPlain
      ? lines.map((l) => l.text).join('\n')
      : serializeLrc(timed)
    try {
      const updated = await window.aether.saveLyrics(trackId, lyrics)
      updateTrackInPlayer(updated)
      toast.success(t('lyrics_editor.saved'), updated.title)
      setDirty(false)
      setLyricsEditTrackId(null)
    } catch (err) {
      setError(ipcErrorMessage(err))
    } finally {
      setBusy(null)
    }
  }

  const saveButton = (
    <button
      className="btn-accent rounded-lg px-4 py-2 text-[13px]"
      onClick={() => void save()}
      disabled={busy != null || !dirty}
    >
      {busy === 'save' ? t('metadata.saving') : t('metadata.save')}
    </button>
  )
  const footerInfo = lines && (
    <span className="text-[11.5px] text-text-3">
      {savesAsPlain && lines.length > 0
        ? t('lyrics_editor.plain_warning')
        : t('lyrics_editor.timed_count', { count: timedCount })}
    </span>
  )
  const confirmDialog = (
    <ConfirmDialog
      open={confirmAction != null}
      title={t(
        confirmAction === 'refetch'
          ? 'lyrics_editor.refetch_confirm'
          : 'lyrics_editor.discard_confirm'
      )}
      danger
      onConfirm={() => {
        if (confirmAction === 'refetch') void doRefetch()
        else setLyricsEditTrackId(null)
      }}
      onClose={() => setConfirmAction(null)}
    />
  )

  const editorBody = !lines ? (
          <div className="flex flex-col gap-2 p-5">
            {Array.from({ length: 6 }).map((_, i) => (
              <div key={i} className="skeleton h-8" />
            ))}
          </div>
        ) : pasteMode ? (
          <div className="flex min-h-0 flex-1 flex-col gap-3 p-5">
            <textarea
              className="field-input min-h-[260px] flex-1 resize-none py-2 font-sans text-[13px]"
              placeholder={t('lyrics_editor.paste_placeholder')}
              value={pasteText}
              onChange={(e) => setPasteText(e.target.value)}
              autoFocus
            />
            <div className="flex justify-end gap-2">
              <button
                className="btn-ghost rounded-lg px-3 py-2 text-[12.5px] font-medium text-text-2"
                onClick={() => setPasteMode(false)}
              >
                {t('common.cancel')}
              </button>
              <button
                className="btn-accent rounded-lg px-4 py-2 text-[13px]"
                onClick={applyPaste}
                disabled={!pasteText.trim()}
              >
                {t('lyrics_editor.paste_apply')}
              </button>
            </div>
          </div>
        ) : (
          <>
            <div
              className="flex flex-wrap items-center gap-2 border-b px-5 py-2.5"
              style={{ borderColor: 'var(--hairline)' }}
            >
              <button
                className="btn-accent flex items-center gap-1.5 rounded-lg px-3 py-1.5 text-[12.5px] disabled:opacity-50"
                onClick={() => stamp(Math.min(syncIdx, lines.length - 1))}
                disabled={!isCurrent || lines.length === 0}
                title={t('lyrics_editor.tap_hint')}
              >
                <Timer size={13} /> {t('lyrics_editor.tap')}
              </button>
              <span className="text-[11.5px] text-text-3">
                {!isCurrent
                  ? t('lyrics_editor.not_playing')
                  : lines[Math.min(syncIdx, lines.length - 1)]
                    ? t('lyrics_editor.next_line', {
                        text: lines[Math.min(syncIdx, lines.length - 1)].text || '♪'
                      })
                    : ''}
              </span>
              <div className="ml-auto flex items-center gap-1.5">
                <span className="text-[11px] uppercase tracking-wide text-text-3">
                  {t('lyrics_editor.shift_all')}
                </span>
                <button className="icon-btn h-6 w-6" onClick={() => shiftAll(-NUDGE)} title={`-${NUDGE}s`}>
                  <ChevronLeft size={13} />
                </button>
                <button className="icon-btn h-6 w-6" onClick={() => shiftAll(NUDGE)} title={`+${NUDGE}s`}>
                  <ChevronRight size={13} />
                </button>
                <button
                  className="icon-btn h-6 w-6"
                  onClick={() => setPasteMode(true)}
                  title={t('lyrics_editor.paste')}
                >
                  <ClipboardPaste size={13} />
                </button>
                <button
                  className="icon-btn h-6 w-6"
                  onClick={() => void refetch()}
                  disabled={busy != null}
                  title={t('lyrics_editor.refetch')}
                >
                  <RefreshCw size={13} className={busy === 'refetch' ? 'animate-spin' : ''} />
                </button>
              </div>
            </div>

            <div ref={listRef} className="min-h-0 flex-1 overflow-y-auto px-3 py-2">
              {lines.length === 0 && (
                <div className="px-2 py-8 text-center text-[13px] text-text-3">
                  {t('lyrics_editor.empty')}
                </div>
              )}
              {lines.map((line, i) => (
                <div
                  key={i}
                  className={`flex items-center gap-1.5 rounded-lg px-2 py-1 ${
                    i === activeLine && isCurrent ? 'bg-[var(--accent-soft)]' : ''
                  } ${i === syncIdx ? 'ring-1 ring-[var(--accent-glow)]' : ''}`}
                >
                  <button
                    className="icon-btn h-6 w-6 shrink-0"
                    onClick={() => stamp(i)}
                    disabled={!isCurrent}
                    title={t('lyrics_editor.stamp')}
                  >
                    <Timer size={12} />
                  </button>
                  <button
                    className="tnum w-[64px] shrink-0 rounded px-1 text-left text-[11.5px] text-text-2 transition-colors hover:text-[var(--accent)] disabled:opacity-40"
                    onClick={() => playFrom(i)}
                    disabled={line.time == null || !isCurrent}
                    title={t('lyrics_editor.play_from')}
                  >
                    {line.time != null ? formatLrcTime(line.time) : '--:--.--'}
                  </button>
                  <button
                    className="icon-btn h-5 w-5 shrink-0"
                    onClick={() => nudge(i, -NUDGE)}
                    disabled={line.time == null}
                    title={`-${NUDGE}s`}
                  >
                    <ChevronLeft size={11} />
                  </button>
                  <button
                    className="icon-btn h-5 w-5 shrink-0"
                    onClick={() => nudge(i, NUDGE)}
                    disabled={line.time == null}
                    title={`+${NUDGE}s`}
                  >
                    <ChevronRight size={11} />
                  </button>
                  <input
                    className={`${isMobile ? 'h-10' : 'h-7'} min-w-0 flex-1 rounded bg-transparent px-1.5 text-[13px] outline-none transition-colors focus:bg-white/[0.05]`}
                    value={line.text}
                    onChange={(e) => setText(i, e.target.value)}
                  />
                  <button
                    className="icon-btn h-5 w-5 shrink-0 opacity-40 hover:opacity-100"
                    onClick={() => removeLine(i)}
                    title={t('lyrics_editor.delete_line')}
                  >
                    <Trash2 size={11} />
                  </button>
                </div>
              ))}
              {lines.length > 0 && (
                <button
                  className="mt-1 flex items-center gap-1.5 rounded-lg px-2 py-1.5 text-[12.5px] text-text-3 transition-colors hover:text-text-1"
                  onClick={addLine}
                >
                  <Plus size={13} /> {t('lyrics_editor.add_line')}
                </button>
              )}
              {lines.length === 0 && (
                <div className="flex justify-center gap-2 pb-4">
                  <button
                    className="btn-ghost flex items-center gap-2 rounded-lg px-3 py-2 text-[12.5px]"
                    onClick={() => setPasteMode(true)}
                  >
                    <ClipboardPaste size={13} /> {t('lyrics_editor.paste')}
                  </button>
                  <button
                    className="btn-ghost flex items-center gap-2 rounded-lg px-3 py-2 text-[12.5px]"
                    onClick={() => void refetch()}
                    disabled={busy != null}
                  >
                    <RefreshCw size={13} /> {t('lyrics_editor.refetch')}
                  </button>
                </div>
              )}
            </div>

            {error && <div className="error-banner mx-5 mb-2 text-[12.5px]">{error}</div>}
          </>
        )

  // Mobile: full-screen sheet — Salva nell'app bar, toolbar fissa, lista con
  // scroll interno (scrollBody={false}), info-riga in coda.
  if (isMobile) {
    return (
      <>
        <FullScreenSheet
          open
          onClose={close}
          title={t('lyrics_editor.title')}
          actions={!pasteMode ? saveButton : undefined}
          scrollBody={false}
        >
          {track && (
            <div className="shrink-0 truncate px-4 pb-1 pt-2 text-[12px] text-text-3">
              {track.title} — {track.artist}
            </div>
          )}
          {editorBody}
          {lines && !pasteMode && (
            <div className="shrink-0 border-t px-4 py-2" style={{ borderColor: 'var(--hairline)' }}>
              {footerInfo}
            </div>
          )}
        </FullScreenSheet>
        {confirmDialog}
      </>
    )
  }

  return (
    <>
    <div
      className="overlay-in fixed inset-0 z-50 flex items-center justify-center bg-black/55 backdrop-blur-sm"
      onClick={close}
    >
      <div
        ref={trapRef}
        role="dialog"
        aria-modal="true"
        aria-label={t('lyrics_editor.title')}
        className="glass-modal scale-in flex max-h-[85vh] w-[min(720px,calc(100vw-48px))] flex-col overflow-hidden rounded-2xl"
        style={{ boxShadow: '0 24px 80px rgba(0,0,0,0.7)' }}
        onClick={(e) => e.stopPropagation()}
      >
        <div
          className="flex items-center justify-between border-b px-5 py-3.5"
          style={{ borderColor: 'var(--hairline)' }}
        >
          <div className="min-w-0">
            <h2 className="text-[15px] font-bold">{t('lyrics_editor.title')}</h2>
            {track && (
              <div className="truncate text-[12px] text-text-3">
                {track.title} — {track.artist}
              </div>
            )}
          </div>
          <button className="icon-btn h-7 w-7" onClick={close} aria-label={t('common.close')}>
            <X size={15} />
          </button>
        </div>

        {editorBody}

        {lines && !pasteMode && (
          <div
            className="flex items-center justify-between border-t px-5 py-3.5"
            style={{ borderColor: 'var(--hairline)' }}
          >
            {footerInfo}
            <div className="flex gap-2">
              <button
                className="btn-ghost rounded-lg px-4 py-2 text-[13px] font-medium text-text-2"
                onClick={close}
              >
                {t('common.cancel')}
              </button>
              {saveButton}
            </div>
          </div>
        )}
      </div>
    </div>
    {confirmDialog}
    </>
  )
}
