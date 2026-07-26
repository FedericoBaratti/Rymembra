import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { AlertTriangle, Image, Loader2, RotateCcw, Sparkles } from 'lucide-react'
import type { EnrichmentBucket, EnrichmentStats, Track } from '@shared/types'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage, translateErrorCode } from '@/lib/ipcError'

type Phase = 'enrich' | 'covers'

function StatPill({
  label,
  value,
  tone
}: {
  label: string
  value: number
  tone?: 'ok' | 'warn' | 'err'
}): React.JSX.Element {
  const cls =
    tone === 'ok'
      ? 'bg-[var(--success-soft)] text-[var(--success)]'
      : tone === 'err'
        ? 'bg-[var(--danger-soft)] text-[var(--danger)]'
        : tone === 'warn'
          ? 'bg-[var(--warning-soft)] text-[var(--warning)]'
          : 'bg-white/[0.06] text-text-2'
  return (
    <span className={`inline-flex items-center gap-1.5 rounded-full px-2.5 py-1 text-[11.5px] font-semibold ${cls}`}>
      <span className="tnum">{value}</span> {label}
    </span>
  )
}

export default function EnrichmentDashboard(): React.JSX.Element {
  const { t } = useTranslation()
  const [stats, setStats] = useState<EnrichmentStats | null>(null)
  const [progress, setProgress] = useState<{ phase: Phase; done: number; total: number } | null>(null)
  const [busy, setBusy] = useState<'retry' | 'covers' | null>(null)
  const [bucket, setBucket] = useState<EnrichmentBucket | null>(null)
  const [bucketTracks, setBucketTracks] = useState<Track[]>([])
  const [enriching, setEnriching] = useState<Set<number>>(new Set())
  const [fpcalcMissing, setFpcalcMissing] = useState(false)

  const refresh = useCallback(() => {
    void window.aether.getEnrichmentStats().then(setStats)
  }, [])

  useEffect(() => {
    window.aether
      .getBinaryStatus()
      .then((status) => setFpcalcMissing(!status.fpcalc.found))
      .catch(() => {})
  }, [])

  useEffect(() => {
    refresh()
    const offUpdated = window.aether.on('enrichment:updated', refresh)
    const offProgress = window.aether.on('enrichment:progress', (p) => {
      setProgress(p.done >= p.total ? null : p)
      if (p.done >= p.total) refresh()
    })
    return () => {
      offUpdated()
      offProgress()
    }
  }, [refresh])

  const openBucket = async (b: EnrichmentBucket): Promise<void> => {
    if (bucket === b) {
      setBucket(null)
      return
    }
    setBucket(b)
    setBucketTracks(await window.aether.getEnrichmentTracks(b, 0, 100))
  }

  const retryFailed = async (): Promise<void> => {
    setBusy('retry')
    try {
      const { reset } = await window.aether.retryFailedEnrichment()
      toast.success(t('enrich_dash.retry_started', { count: reset }))
      setBucket(null)
      refresh()
    } finally {
      setBusy(null)
    }
  }

  const runBackfill = async (): Promise<void> => {
    setBusy('covers')
    try {
      const { updated, total } = await window.aether.backfillCovers()
      toast.success(t('enrich_dash.covers_done', { updated, total }))
      refresh()
    } finally {
      setBusy(null)
      setProgress(null)
    }
  }

  const enrichOne = async (trackId: number): Promise<void> => {
    setEnriching((s) => new Set(s).add(trackId))
    try {
      const result = await window.aether.enrichTrack(trackId)
      if (result.applied) {
        setBucketTracks((rows) => rows.filter((tr) => tr.id !== trackId))
        toast.success(translateErrorCode(result.message))
      } else {
        toast.error(translateErrorCode(result.message))
      }
      refresh()
    } catch (err) {
      toast.error(ipcErrorMessage(err))
    } finally {
      setEnriching((s) => {
        const next = new Set(s)
        next.delete(trackId)
        return next
      })
    }
  }

  const failedCount = (stats?.noMatch ?? 0) + (stats?.error ?? 0)

  return (
    <div className="flex flex-col gap-3">
      {fpcalcMissing && (
        <div className="flex items-start gap-2 rounded-lg border border-[var(--warning-soft)] bg-[var(--warning-soft)] px-3 py-2 text-[12px] text-[var(--warning)]">
          <AlertTriangle size={14} className="mt-0.5 shrink-0" />
          <span>{t('enrich_dash.fpcalc_missing')}</span>
        </div>
      )}
      {!stats ? (
        <div className="skeleton h-8" />
      ) : (
        <div className="flex flex-wrap items-center gap-1.5">
          <StatPill label={t('enrich_dash.ok')} value={stats.ok} tone="ok" />
          <button onClick={() => void openBucket('pending')} className={bucket === 'pending' ? 'rounded-full ring-1 ring-[var(--accent)]' : ''}>
            <StatPill label={t('enrich_dash.pending')} value={stats.pending} />
          </button>
          <button onClick={() => void openBucket('no-match')} className={bucket === 'no-match' ? 'rounded-full ring-1 ring-[var(--accent)]' : ''}>
            <StatPill label={t('enrich_dash.no_match')} value={stats.noMatch} tone="warn" />
          </button>
          <button onClick={() => void openBucket('error')} className={bucket === 'error' ? 'rounded-full ring-1 ring-[var(--accent)]' : ''}>
            <StatPill label={t('enrich_dash.error')} value={stats.error} tone="err" />
          </button>
          <StatPill label={t('enrich_dash.missing_covers')} value={stats.missingCovers} />
        </div>
      )}

      {progress && (
        <div className="flex items-center gap-2">
          <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-white/10">
            <div
              className="progress-sheen h-full rounded-full transition-[width] duration-300"
              style={{
                width: `${Math.round((progress.done / Math.max(1, progress.total)) * 100)}%`,
                background: 'var(--accent)'
              }}
            />
          </div>
          <span className="tnum text-[11px] text-text-2">
            {t(progress.phase === 'covers' ? 'enrich_dash.progress_covers' : 'enrich_dash.progress_enrich', {
              done: progress.done,
              total: progress.total
            })}
          </span>
        </div>
      )}

      <div className="flex flex-wrap gap-2">
        <button
          className="btn-ghost flex items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px] font-medium disabled:opacity-50"
          onClick={() => void retryFailed()}
          disabled={busy != null || failedCount === 0}
        >
          {busy === 'retry' ? <Loader2 size={13} className="animate-spin" /> : <RotateCcw size={13} />}
          {t('enrich_dash.retry_failed', { count: failedCount })}
        </button>
        <button
          className="btn-ghost flex items-center gap-1.5 rounded-lg px-3 py-2 text-[12.5px] font-medium disabled:opacity-50"
          onClick={() => void runBackfill()}
          disabled={busy != null || (stats != null && stats.missingCovers === 0)}
        >
          {busy === 'covers' ? <Loader2 size={13} className="animate-spin" /> : <Image size={13} />}
          {t('enrich_dash.backfill_covers')}
        </button>
      </div>

      {bucket && (
        <div className="max-h-64 overflow-y-auto rounded-lg bg-white/[0.03] p-2">
          {bucketTracks.length === 0 ? (
            <div className="px-2 py-3 text-[12px] text-text-3">—</div>
          ) : (
            bucketTracks.map((tr) => (
              <div key={tr.id} className="flex items-center justify-between gap-3 rounded px-2 py-1 text-[12px] hover:bg-white/[0.04]">
                <span className="truncate">
                  {tr.artist} — {tr.title}
                </span>
                <button
                  className="flex shrink-0 items-center gap-1 rounded-lg px-2 py-1 text-[11.5px] font-medium text-[var(--accent)] transition-colors hover:bg-[var(--accent-soft)] disabled:opacity-50"
                  onClick={() => void enrichOne(tr.id)}
                  disabled={enriching.has(tr.id)}
                >
                  {enriching.has(tr.id) ? (
                    <Loader2 size={11} className="animate-spin" />
                  ) : (
                    <Sparkles size={11} />
                  )}
                  {t('enrich_dash.enrich_now')}
                </button>
              </div>
            ))
          )}
        </div>
      )}
    </div>
  )
}
