import { useEffect, useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import {
  AlertCircle,
  Check,
  Loader2,
  MonitorSmartphone,
  QrCode,
  RefreshCw,
  Smartphone,
  Sparkles,
  Trash2,
  Wrench,
  X
} from 'lucide-react'
import type { LucideIcon } from 'lucide-react'
import type {
  PhonePairingCode,
  PhonePlanBadge,
  PhoneRepairStatus,
  PhoneTrackPlanned
} from '@shared/types'
import PageHeader from '@/components/ui/PageHeader'
import EmptyState from '@/components/ui/EmptyState'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'
import { usePhoneSyncStore } from '@/store/usePhoneSyncStore'

const BADGE_STYLE: Record<PhonePlanBadge, string> = {
  ok: 'bg-[var(--success-soft)] text-[var(--success)]',
  'needs-codec': 'bg-[var(--warning-soft)] text-[var(--warning)]',
  'needs-enrich': 'bg-[var(--accent-soft)] text-[var(--accent)]',
  both: 'bg-[var(--danger-soft)] text-[var(--danger)]'
}

const ACTIVE_STATUSES: PhoneRepairStatus[] = [
  'pulling',
  'validating',
  'transcoding',
  'enriching',
  'pushing',
  'committing'
]

const STATUS_STYLE: Record<PhoneRepairStatus, { cls: string; icon: LucideIcon; spin?: boolean }> = {
  pending: { cls: 'bg-white/[0.06] text-text-2', icon: Loader2 },
  pulling: { cls: 'bg-[var(--accent-soft)] text-[var(--accent)]', icon: Loader2, spin: true },
  validating: { cls: 'bg-[var(--accent-soft)] text-[var(--accent)]', icon: Loader2, spin: true },
  transcoding: { cls: 'bg-[var(--accent-soft)] text-[var(--accent)]', icon: Loader2, spin: true },
  enriching: { cls: 'bg-[var(--accent-soft)] text-[var(--accent)]', icon: Loader2, spin: true },
  pushing: { cls: 'bg-[var(--accent-soft)] text-[var(--accent)]', icon: Loader2, spin: true },
  committing: { cls: 'bg-[var(--accent-soft)] text-[var(--accent)]', icon: Loader2, spin: true },
  done: { cls: 'bg-[var(--success-soft)] text-[var(--success)]', icon: Check },
  skipped: { cls: 'bg-white/[0.06] text-text-3', icon: Check },
  failed: { cls: 'bg-[var(--danger-soft)] text-[var(--danger)]', icon: AlertCircle }
}

function PlanBadge({ badge }: { badge: PhonePlanBadge }): React.JSX.Element {
  const { t } = useTranslation()
  return (
    <span
      className={`inline-flex shrink-0 items-center rounded-full px-2 py-0.5 text-[10.5px] font-semibold ${BADGE_STYLE[badge]}`}
    >
      {t(`phone_sync.badge_${badge.replace('-', '_')}`)}
    </span>
  )
}

function RepairPill({ status }: { status: PhoneRepairStatus }): React.JSX.Element {
  const { t } = useTranslation()
  const { cls, icon: Icon, spin } = STATUS_STYLE[status]
  return (
    <span
      key={status}
      className={`scale-in inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[10.5px] font-semibold ${cls}`}
    >
      <Icon size={11} className={spin ? 'animate-spin' : ''} />
      {t(`phone_sync.status_${status}`)}
    </span>
  )
}

function PairModal({ onClose }: { onClose: () => void }): React.JSX.Element {
  const { t } = useTranslation()
  const [code, setCode] = useState<PhonePairingCode | null>(null)
  const paired = usePhoneSyncStore((s) => s.state?.paired ?? false)

  useEffect(() => {
    let mounted = true
    window.aether
      .phonePairStart()
      .then((c) => {
        if (!mounted) return
        setCode(c)
        if (c.error) toast.error(t('remote_access.no_lan_address'))
      })
      .catch((err) => toast.error(ipcErrorMessage(err)))
    return () => {
      mounted = false
      void window.aether.phonePairCancel()
    }
  }, [t])

  // The phone completed the handshake → 'phone:state' flips paired → close.
  useEffect(() => {
    if (paired) {
      toast.success(t('phone_sync.paired_toast'))
      onClose()
    }
  }, [paired, onClose, t])

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60 backdrop-blur-sm"
      onClick={onClose}
    >
      <div
        className="slide-up-in flex w-[340px] flex-col items-center gap-4 rounded-2xl border border-[var(--hairline)] bg-surface-1 p-6 text-center"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="text-[15px] font-bold">{t('phone_sync.pair_title')}</div>
        <p className="text-[12px] leading-relaxed text-text-3">{t('phone_sync.pair_hint')}</p>
        {code?.qrDataUrl ? (
          <img src={code.qrDataUrl} alt="QR" className="h-56 w-56 rounded-xl bg-white p-2" />
        ) : (
          <div className="flex h-56 w-56 items-center justify-center">
            <Loader2 size={22} className="animate-spin text-text-3" />
          </div>
        )}
        <button className="btn-ghost rounded-lg px-4 py-2 text-[12.5px] font-medium" onClick={onClose}>
          {t('common.cancel')}
        </button>
      </div>
    </div>
  )
}

function TrackRow({
  item,
  selected,
  onToggle
}: {
  item: PhoneTrackPlanned
  selected: boolean
  onToggle: () => void
}): React.JSX.Element {
  const { t } = useTranslation()
  const repairable = item.badge !== 'ok'
  return (
    <div
      className="row-lift flex items-center gap-3 border border-[var(--hairline)] bg-white/[0.03] px-3 py-2"
      style={{ borderRadius: 'var(--radius-card)' }}
    >
      <input
        type="checkbox"
        className="accent-[var(--accent)]"
        checked={selected}
        disabled={!repairable && !item.repair}
        onChange={onToggle}
        aria-label={item.info.title}
      />
      <div className="min-w-0 flex-1">
        <div className="truncate text-[13px] font-medium">
          {item.info.title}
          <span className="font-normal text-text-2"> — {item.info.artist}</span>
        </div>
        <div className="mt-0.5 flex items-center gap-2 text-[11px] text-text-3">
          <span className="truncate">
            {item.info.ext.replace('.', '').toUpperCase()}
            {item.info.bitrate ? ` · ${Math.round(item.info.bitrate / 1000)}kbps` : ''}
            {item.info.album ? ` · ${item.info.album}` : ''}
          </span>
        </div>
        {item.repair?.status === 'failed' && item.repair.error && (
          <div className="error-banner mt-1 px-2 py-0.5 text-[11px]">
            {item.repair.error.startsWith('SRC_CORRUPT')
              ? t('phone_sync.err_src_corrupt')
              : item.repair.error}
          </div>
        )}
      </div>
      <PlanBadge badge={item.badge} />
      {item.repair && <RepairPill status={item.repair.status} />}
    </div>
  )
}

export default function PhoneSync(): React.JSX.Element {
  const { t } = useTranslation()
  const state = usePhoneSyncStore((s) => s.state)
  const tracks = usePhoneSyncStore((s) => s.tracks)
  const loading = usePhoneSyncStore((s) => s.loadingTracks)
  const tracksError = usePhoneSyncStore((s) => s.tracksError)
  const refreshState = usePhoneSyncStore((s) => s.refreshState)
  const refreshTracks = usePhoneSyncStore((s) => s.refreshTracks)
  const [pairOpen, setPairOpen] = useState(false)
  const [selected, setSelected] = useState<Set<number>>(new Set())
  const [starting, setStarting] = useState(false)

  useEffect(() => {
    void refreshState()
  }, [refreshState])

  useEffect(() => {
    if (state?.paired) void refreshTracks()
  }, [state?.paired, refreshTracks])

  const running = useMemo(
    () => tracks.some((x) => x.repair && ACTIVE_STATUSES.includes(x.repair.status)),
    [tracks]
  )
  const summary = useMemo(() => {
    let done = 0
    let failed = 0
    let skipped = 0
    let active = 0
    for (const x of tracks) {
      if (!x.repair) continue
      if (x.repair.status === 'done') done++
      else if (x.repair.status === 'failed') failed++
      else if (x.repair.status === 'skipped') skipped++
      else if (ACTIVE_STATUSES.includes(x.repair.status) || x.repair.status === 'pending') active++
    }
    return { done, failed, skipped, active }
  }, [tracks])
  const repairable = useMemo(() => tracks.filter((x) => x.badge !== 'ok'), [tracks])

  const toggle = (id: number): void => {
    setSelected((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })
  }

  const start = async (ids: number[] | 'all'): Promise<void> => {
    setStarting(true)
    try {
      await window.aether.phoneRepairStart(ids)
      setSelected(new Set())
      toast.success(t('phone_sync.run_started'))
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err)
      if (msg.includes('PHONE_OFFLINE')) toast.error(t('phone_sync.offline'))
      else if (msg.includes('PHONE_BUSY')) toast.error(t('phone_sync.busy'))
      else toast.error(ipcErrorMessage(err))
    } finally {
      setStarting(false)
    }
  }

  const forget = async (): Promise<void> => {
    await window.aether.phoneForget()
    toast.success(t('phone_sync.forgotten_toast'))
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <PageHeader
        title={t('phone_sync.title')}
        actions={
          state?.paired ? (
            <button
              className="btn-ghost flex items-center gap-2 rounded-lg px-3 py-2 text-[12.5px] font-medium"
              onClick={() => void refreshTracks()}
              disabled={loading}
            >
              <RefreshCw size={13} className={loading ? 'animate-spin' : ''} />
              {t('phone_sync.refresh')}
            </button>
          ) : undefined
        }
      />

      <div className="px-[var(--content-x)] pb-4">
        <div
          className="mx-auto flex max-w-[880px] flex-wrap items-center gap-4 rounded-xl border border-[var(--hairline)] bg-white/[0.03] p-4"
        >
          <div
            className="flex h-12 w-12 shrink-0 items-center justify-center rounded-xl"
            style={{ background: 'var(--accent-soft)' }}
          >
            <Smartphone size={20} style={{ color: 'var(--accent)' }} />
          </div>
          <div className="min-w-0 flex-1">
            {state?.paired && state.peer ? (
              <>
                <div className="text-[14px] font-bold">{state.peer.deviceName}</div>
                <div className="mt-0.5 flex items-center gap-2 text-[11.5px] text-text-3">
                  <span
                    className="inline-block h-2 w-2 rounded-full"
                    style={{
                      background: state.online ? 'var(--success)' : 'var(--danger)'
                    }}
                  />
                  {state.online ? t('phone_sync.online') : t('phone_sync.offline_short')}
                  <span>· {state.peer.lastHost}</span>
                </div>
              </>
            ) : (
              <>
                <div className="text-[14px] font-bold">{t('phone_sync.not_paired')}</div>
                <div className="mt-0.5 text-[11.5px] text-text-3">
                  {t('phone_sync.not_paired_hint')}
                </div>
              </>
            )}
          </div>
          {state?.paired ? (
            <button
              className="btn-ghost flex items-center gap-2 rounded-lg px-3 py-2 text-[12.5px] font-medium text-text-2"
              onClick={() => void forget()}
            >
              <Trash2 size={13} /> {t('phone_sync.forget')}
            </button>
          ) : (
            <button
              className="btn-accent flex items-center gap-2 rounded-lg px-4 py-2 text-[13px]"
              onClick={() => setPairOpen(true)}
            >
              <QrCode size={14} /> {t('phone_sync.pair_button')}
            </button>
          )}
        </div>

        {state?.paired && tracks.length > 0 && (
          <div className="mx-auto mt-3 flex max-w-[880px] flex-wrap items-center gap-2">
            <button
              className="btn-accent flex items-center gap-2 rounded-lg px-4 py-2 text-[13px] disabled:opacity-50"
              onClick={() => void start('all')}
              disabled={starting || running || repairable.length === 0}
            >
              {starting ? <Loader2 size={14} className="animate-spin" /> : <Sparkles size={14} />}
              {t('phone_sync.repair_all', { count: repairable.length })}
            </button>
            <button
              className="btn-ghost flex items-center gap-2 rounded-lg px-4 py-2 text-[12.5px] font-medium disabled:opacity-50"
              onClick={() => void start([...selected])}
              disabled={starting || running || selected.size === 0}
            >
              <Wrench size={13} /> {t('phone_sync.repair_selected', { count: selected.size })}
            </button>
            {running && (
              <button
                className="btn-ghost flex items-center gap-2 rounded-lg px-4 py-2 text-[12.5px] font-medium text-[var(--danger)]"
                onClick={() => void window.aether.phoneRepairCancel()}
              >
                <X size={13} /> {t('phone_sync.cancel_run')}
              </button>
            )}
            <div className="ml-auto text-[11.5px] text-text-3">
              {t('phone_sync.summary', {
                done: summary.done,
                skipped: summary.skipped,
                failed: summary.failed,
                active: summary.active
              })}
            </div>
          </div>
        )}

        {tracksError && (
          <div className="error-banner mx-auto mt-2 max-w-[880px] text-[12.5px]">
            {tracksError.includes('PHONE_OFFLINE') || tracksError.includes('PHONE_UNREACHABLE')
              ? t('phone_sync.offline')
              : tracksError}
          </div>
        )}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto px-[var(--content-x)] pb-[var(--player-clearance)]">
        {!state?.paired ? (
          <EmptyState
            icon={MonitorSmartphone}
            title={t('phone_sync.empty_unpaired')}
            action={
              <button
                className="btn-accent flex items-center gap-2 rounded-full px-4 py-2 text-[13px]"
                onClick={() => setPairOpen(true)}
              >
                <QrCode size={14} /> {t('phone_sync.pair_button')}
              </button>
            }
          />
        ) : loading && tracks.length === 0 ? (
          <div className="mx-auto flex max-w-[880px] flex-col gap-2">
            {Array.from({ length: 6 }).map((_, i) => (
              <div key={i} className="skeleton h-14" style={{ borderRadius: 'var(--radius-card)' }} />
            ))}
          </div>
        ) : tracks.length === 0 ? (
          <EmptyState icon={Smartphone} title={t('phone_sync.empty_tracks')} />
        ) : (
          <div className="mx-auto flex max-w-[880px] flex-col gap-1.5">
            {tracks.map((item) => (
              <TrackRow
                key={item.info.id}
                item={item}
                selected={selected.has(item.info.id)}
                onToggle={() => toggle(item.info.id)}
              />
            ))}
          </div>
        )}
      </div>

      {pairOpen && <PairModal onClose={() => setPairOpen(false)} />}
    </div>
  )
}
