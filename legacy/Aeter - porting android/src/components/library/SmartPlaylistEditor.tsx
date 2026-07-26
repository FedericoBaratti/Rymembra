import { useEffect, useState } from 'react'
import { createPortal } from 'react-dom'
import { useTranslation } from 'react-i18next'
import { X, Plus, Trash2, Sparkles } from 'lucide-react'
import type { Playlist, SmartField, SmartOp, SmartPlaylistRules, SmartRule, Track } from '@shared/types'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import { useBackDismiss } from '@/hooks/useBackDismiss'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'
import { isMobile } from '@/lib/platform'
import Select from '@/components/ui/Select'

const TEXT_FIELDS: SmartField[] = ['title', 'artist', 'album', 'genre']
const NUMBER_FIELDS: SmartField[] = ['year', 'rating', 'play_count']
const DATE_FIELDS: SmartField[] = ['last_played', 'date_added']
const ALL_FIELDS: SmartField[] = [...TEXT_FIELDS, ...NUMBER_FIELDS, ...DATE_FIELDS]

const SORT_FIELDS: (SmartField | 'random')[] = [...ALL_FIELDS, 'random']

function opsForField(field: SmartField): SmartOp[] {
  if (TEXT_FIELDS.includes(field)) return ['contains', 'not_contains', 'eq', 'neq']
  if (NUMBER_FIELDS.includes(field)) return ['eq', 'neq', 'gte', 'lte', 'gt', 'lt']
  return ['in_last_days']
}

function defaultRule(): SmartRule {
  return { field: 'genre', op: 'contains', value: '' }
}

function parseRules(playlist?: Playlist | null): SmartPlaylistRules {
  if (playlist?.rules) {
    try {
      return JSON.parse(playlist.rules) as SmartPlaylistRules
    } catch {
      // corrupt rules: start fresh
    }
  }
  return { combinator: 'and', rules: [defaultRule()] }
}

// Finger-friendly height on touch, compact on desktop.
const fieldCls = 'field-input h-10 text-[13px] sm:h-8 sm:text-[12.5px]'

export default function SmartPlaylistEditor({
  playlist,
  onSaved,
  onClose
}: {
  /** When provided, edits this smart playlist's rules; otherwise creates a new one. */
  playlist?: Playlist | null
  onSaved: () => void
  onClose: () => void
}): React.JSX.Element {
  const { t } = useTranslation()
  const trapRef = useFocusTrap<HTMLDivElement>(true, onClose)
  // Mounted only while open: hardware back closes the editor.
  useBackDismiss(true, onClose)
  const [name, setName] = useState(playlist?.name ?? '')
  const [spec, setSpec] = useState<SmartPlaylistRules>(() => parseRules(playlist))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [preview, setPreview] = useState<Track[] | null>(null)

  // Live preview: debounce rule changes, then query matching tracks (no save).
  useEffect(() => {
    const handle = setTimeout(() => {
      void window.aether
        .previewSmartPlaylist(spec)
        .then(setPreview)
        .catch(() => setPreview(null))
    }, 300)
    return () => clearTimeout(handle)
  }, [spec])

  const setRule = (i: number, patch: Partial<SmartRule>): void => {
    setSpec((s) => {
      const rules = [...s.rules]
      const next = { ...rules[i], ...patch }
      if (patch.field) {
        const ops = opsForField(patch.field)
        if (!ops.includes(next.op)) next.op = ops[0]
        next.value = ''
      }
      rules[i] = next
      return { ...s, rules }
    })
  }

  const save = async (): Promise<void> => {
    if (!name.trim() || spec.rules.length === 0) return
    setBusy(true)
    setError(null)
    try {
      if (playlist) {
        await window.aether.setSmartPlaylistRules(playlist.id, name.trim(), spec)
      } else {
        await window.aether.createSmartPlaylist(name.trim(), spec)
        toast.success(t('toast.playlist_created'), name.trim())
      }
      onSaved()
      onClose()
    } catch (err) {
      setError(ipcErrorMessage(err))
    } finally {
      setBusy(false)
    }
  }

  const isNumericValue = (rule: SmartRule): boolean =>
    NUMBER_FIELDS.includes(rule.field) || DATE_FIELDS.includes(rule.field)

  // Portal to <body> so the overlay escapes the page's `relative z-10` stacking
  // context (App.tsx); otherwise the z-30 PlayerBar/BottomNav, which are its
  // siblings, paint over the dialog footer. Same pattern as SpotifyMigrationFlow.
  return createPortal(
    <div className="overlay-in fixed inset-0 z-50 flex items-end justify-center bg-black/55 backdrop-blur-sm sm:items-center" onClick={onClose}>
      <div
        ref={trapRef}
        role="dialog"
        aria-modal="true"
        aria-label={t('smart.title')}
        className="glass-modal scale-in flex max-h-[90vh] w-full flex-col overflow-hidden rounded-t-2xl sm:max-h-[85vh] sm:w-[min(560px,calc(100vw-48px))] sm:rounded-2xl"
        style={{
          boxShadow: '0 24px 80px rgba(0,0,0,0.7)',
          paddingBottom: isMobile ? 'var(--sa-bottom, env(safe-area-inset-bottom, 0px))' : undefined
        }}
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b px-5 py-3.5" style={{ borderColor: 'var(--hairline)' }}>
          <h2 className="flex items-center gap-2 text-[15px] font-bold">
            <Sparkles size={15} className="text-[var(--accent)]" />
            {playlist ? t('smart.edit_rules') : t('smart.new_smart')}
          </h2>
          <button className="icon-btn h-7 w-7" onClick={onClose} aria-label={t('common.close')}>
            <X size={15} />
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-5">
          <label className="mb-4 flex flex-col gap-1">
            <span className="text-[11px] font-semibold uppercase tracking-wide text-text-3">
              {t('playlists.name_placeholder')}
            </span>
            <input
              autoFocus
              className="field-input h-9"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </label>

          <div className="mb-3 flex items-center gap-2">
            <span className="text-[12.5px] text-text-2">{t('smart.match')}</span>
            <div className="flex overflow-hidden rounded-lg border" style={{ borderColor: 'var(--hairline)' }}>
              {(['and', 'or'] as const).map((c) => (
                <button
                  key={c}
                  className={`px-3 py-1.5 text-[12px] font-medium transition-colors ${
                    spec.combinator === c ? 'bg-[var(--accent)] text-white' : 'bg-surface-3 text-text-2 hover:bg-white/[0.08]'
                  }`}
                  onClick={() => setSpec((s) => ({ ...s, combinator: c }))}
                >
                  {c === 'and' ? t('smart.match_all') : t('smart.match_any')}
                </button>
              ))}
            </div>
          </div>

          <div className="flex flex-col gap-3 sm:gap-2">
            {/* On touch the controls stack (2-col selects + value/trash row) so a
                rule never overflows a narrow sheet; desktop keeps a single row. */}
            {spec.rules.map((rule, i) => (
              <div key={i} className="grid grid-cols-2 gap-2 sm:flex sm:items-center">
                <Select<SmartField>
                  fill
                  triggerClassName={fieldCls}
                  value={rule.field}
                  options={ALL_FIELDS.map((f) => ({ value: f, label: t(`smart.field_${f}`) }))}
                  onChange={(field) => setRule(i, { field })}
                />
                <Select<SmartOp>
                  fill
                  triggerClassName={fieldCls}
                  value={rule.op}
                  options={opsForField(rule.field).map((op) => ({ value: op, label: t(`smart.op_${op}`) }))}
                  onChange={(op) => setRule(i, { op })}
                />
                <div className="col-span-2 flex items-center gap-2 sm:contents">
                  <input
                    className={`${fieldCls} min-w-0 flex-1`}
                    style={{ borderColor: 'var(--hairline)' }}
                    type={isNumericValue(rule) ? 'number' : 'text'}
                    value={String(rule.value)}
                    onChange={(e) =>
                      setRule(i, {
                        value: isNumericValue(rule) ? Number(e.target.value) || 0 : e.target.value
                      })
                    }
                  />
                  <button
                    className="icon-btn h-10 w-10 shrink-0 sm:h-7 sm:w-7"
                    disabled={spec.rules.length <= 1}
                    onClick={() => setSpec((s) => ({ ...s, rules: s.rules.filter((_, j) => j !== i) }))}
                    aria-label={t('common.delete')}
                  >
                    <Trash2 size={14} />
                  </button>
                </div>
              </div>
            ))}
          </div>

          <button
            className="mt-2 flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-[12.5px] font-medium text-[var(--accent)] transition-colors hover:bg-[var(--accent-soft)]"
            onClick={() => setSpec((s) => ({ ...s, rules: [...s.rules, defaultRule()] }))}
          >
            <Plus size={13} /> {t('smart.add_rule')}
          </button>

          <div className="mt-4 grid grid-cols-2 gap-3 sm:grid-cols-3">
            <label className="flex flex-col gap-1">
              <span className="text-[11px] font-semibold uppercase tracking-wide text-text-3">
                {t('smart.limit')}
              </span>
              <input
                className={fieldCls}
                style={{ borderColor: 'var(--hairline)' }}
                type="number"
                min={1}
                max={10000}
                value={spec.limit ?? ''}
                onChange={(e) =>
                  setSpec((s) => ({ ...s, limit: e.target.value ? Number(e.target.value) : undefined }))
                }
              />
            </label>
            <label className="flex flex-col gap-1">
              <span className="text-[11px] font-semibold uppercase tracking-wide text-text-3">
                {t('smart.sort_by')}
              </span>
              <Select<string>
                fill
                triggerClassName={fieldCls}
                title={t('smart.sort_by')}
                ariaLabel={t('smart.sort_by')}
                value={spec.sortBy ?? ''}
                options={[
                  { value: '', label: '—' },
                  ...SORT_FIELDS.map((f) => ({
                    value: f as string,
                    label: f === 'random' ? t('smart.random') : t(`smart.field_${f}`)
                  }))
                ]}
                onChange={(v) =>
                  setSpec((s) => ({
                    ...s,
                    sortBy: (v || undefined) as SmartPlaylistRules['sortBy']
                  }))
                }
              />
            </label>
            {spec.sortBy && spec.sortBy !== 'random' && (
              <label className="flex flex-col gap-1">
                <span className="text-[11px] font-semibold uppercase tracking-wide text-text-3">
                  {t('smart.direction')}
                </span>
                <Select<'asc' | 'desc'>
                  fill
                  triggerClassName={fieldCls}
                  title={t('smart.direction')}
                  ariaLabel={t('smart.direction')}
                  value={spec.sortDir ?? 'asc'}
                  options={[
                    { value: 'asc', label: t('smart.asc') },
                    { value: 'desc', label: t('smart.desc') }
                  ]}
                  onChange={(sortDir) => setSpec((s) => ({ ...s, sortDir }))}
                />
              </label>
            )}
          </div>

          {/* Live preview of matching tracks */}
          <div className="mt-4 rounded-xl border" style={{ borderColor: 'var(--hairline)' }}>
            <div className="flex items-center justify-between px-3 py-2 text-[12px] font-semibold text-text-2">
              <span>{t('smart.preview')}</span>
              <span className="tnum text-[var(--accent)]">
                {preview == null ? '…' : t('playlists.tracks_count', { count: preview.length })}
              </span>
            </div>
            {preview && preview.length > 0 && (
              <div className="max-h-40 overflow-y-auto border-t px-1 py-1" style={{ borderColor: 'var(--hairline)' }}>
                {preview.slice(0, 50).map((tr) => (
                  <div key={tr.id} className="truncate rounded px-2 py-1 text-[12px] text-text-2">
                    <span className="text-text-1">{tr.title}</span>
                    <span className="text-text-3"> — {tr.artist}</span>
                  </div>
                ))}
                {preview.length > 50 && (
                  <div className="px-2 py-1 text-[11px] text-text-3">
                    {t('smart.preview_more', { count: preview.length - 50 })}
                  </div>
                )}
              </div>
            )}
          </div>

          {error && <div className="error-banner mt-3 text-[12.5px]">{error}</div>}
        </div>

        <div className="flex items-center justify-end gap-2 border-t px-5 py-3.5" style={{ borderColor: 'var(--hairline)' }}>
          <button
            className="btn-ghost rounded-lg px-4 py-2 text-[13px] font-medium text-text-2"
            onClick={onClose}
          >
            {t('common.cancel')}
          </button>
          <button
            className="btn-accent rounded-lg px-4 py-2 text-[13px]"
            onClick={() => void save()}
            disabled={busy || !name.trim() || spec.rules.length === 0}
          >
            {busy ? t('metadata.saving') : t('metadata.save')}
          </button>
        </div>
      </div>
    </div>,
    document.body
  )
}
