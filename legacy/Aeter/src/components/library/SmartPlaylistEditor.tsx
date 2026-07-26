import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { X, Plus, Trash2, Sparkles } from 'lucide-react'
import type { Playlist, SmartField, SmartOp, SmartPlaylistRules, SmartRule } from '@shared/types'
import { useFocusTrap } from '@/hooks/useFocusTrap'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'

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

const fieldCls = 'field-input h-8 text-[12.5px]'

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
  const [name, setName] = useState(playlist?.name ?? '')
  const [spec, setSpec] = useState<SmartPlaylistRules>(() => parseRules(playlist))
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)

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

  return (
    <div className="overlay-in fixed inset-0 z-50 flex items-center justify-center bg-black/55 backdrop-blur-sm" onClick={onClose}>
      <div
        ref={trapRef}
        role="dialog"
        aria-modal="true"
        aria-label={t('smart.title')}
        className="glass-modal scale-in flex max-h-[85vh] w-[min(560px,calc(100vw-48px))] flex-col overflow-hidden rounded-2xl"
        style={{ boxShadow: '0 24px 80px rgba(0,0,0,0.7)' }}
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

          <div className="flex flex-col gap-2">
            {spec.rules.map((rule, i) => (
              <div key={i} className="flex items-center gap-2">
                <select
                  className={fieldCls}
                  style={{ borderColor: 'var(--hairline)' }}
                  value={rule.field}
                  onChange={(e) => setRule(i, { field: e.target.value as SmartField })}
                >
                  {ALL_FIELDS.map((f) => (
                    <option key={f} value={f}>
                      {t(`smart.field_${f}`)}
                    </option>
                  ))}
                </select>
                <select
                  className={fieldCls}
                  style={{ borderColor: 'var(--hairline)' }}
                  value={rule.op}
                  onChange={(e) => setRule(i, { op: e.target.value as SmartOp })}
                >
                  {opsForField(rule.field).map((op) => (
                    <option key={op} value={op}>
                      {t(`smart.op_${op}`)}
                    </option>
                  ))}
                </select>
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
                  className="icon-btn h-7 w-7 shrink-0"
                  disabled={spec.rules.length <= 1}
                  onClick={() => setSpec((s) => ({ ...s, rules: s.rules.filter((_, j) => j !== i) }))}
                  aria-label={t('common.delete')}
                >
                  <Trash2 size={13} />
                </button>
              </div>
            ))}
          </div>

          <button
            className="mt-2 flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-[12.5px] font-medium text-[var(--accent)] transition-colors hover:bg-[var(--accent-soft)]"
            onClick={() => setSpec((s) => ({ ...s, rules: [...s.rules, defaultRule()] }))}
          >
            <Plus size={13} /> {t('smart.add_rule')}
          </button>

          <div className="mt-4 grid grid-cols-3 gap-3">
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
              <select
                className={fieldCls}
                style={{ borderColor: 'var(--hairline)' }}
                value={spec.sortBy ?? ''}
                onChange={(e) =>
                  setSpec((s) => ({
                    ...s,
                    sortBy: (e.target.value || undefined) as SmartPlaylistRules['sortBy']
                  }))
                }
              >
                <option value="">—</option>
                {SORT_FIELDS.map((f) => (
                  <option key={f} value={f}>
                    {f === 'random' ? t('smart.random') : t(`smart.field_${f}`)}
                  </option>
                ))}
              </select>
            </label>
            {spec.sortBy && spec.sortBy !== 'random' && (
              <label className="flex flex-col gap-1">
                <span className="text-[11px] font-semibold uppercase tracking-wide text-text-3">
                  {t('smart.direction')}
                </span>
                <select
                  className={fieldCls}
                  style={{ borderColor: 'var(--hairline)' }}
                  value={spec.sortDir ?? 'asc'}
                  onChange={(e) => setSpec((s) => ({ ...s, sortDir: e.target.value as 'asc' | 'desc' }))}
                >
                  <option value="asc">{t('smart.asc')}</option>
                  <option value="desc">{t('smart.desc')}</option>
                </select>
              </label>
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
    </div>
  )
}
