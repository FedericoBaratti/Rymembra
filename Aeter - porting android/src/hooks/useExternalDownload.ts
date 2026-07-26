import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import type { ExternalRecoTrack } from '@shared/types'
import { useDownloadsStore } from '@/store/useDownloadsStore'
import { toast } from '@/store/useToastStore'
import { ipcErrorMessage } from '@/lib/ipcError'

export type ExternalDownloadState = 'idle' | 'busy' | 'done'

/**
 * One-tap "download from the web" shared by SearchOverlay's ExternalRow and
 * Carousels' ExternalCard.
 *
 * The button state is NOT fire-and-forget: after enqueueing it tracks the real
 * DownloadItem id and derives the icon from the live queue status pushed via
 * `download:updated` (see useAppBootstrap → useDownloadsStore). A download that
 * later fails flips the icon back to 'idle' so the user can simply tap again;
 * the terminal error toast comes from the global handler in useAppBootstrap.
 *
 * Error semantics of downloadExternalTrack: resolves null on a genuine
 * "no YouTube match", rejects with a DL_* code on real failures (403,
 * rate-limit, timeout, network) — translated here via ipcErrorMessage.
 */
export function useExternalDownload(track: ExternalRecoTrack): {
  state: ExternalDownloadState
  start: () => Promise<void>
} {
  const { t } = useTranslation()
  const [itemId, setItemId] = useState<number | null>(null)
  // 'busy' covers the resolve phase (before an item id exists); afterwards the
  // queue status below takes over.
  const [resolving, setResolving] = useState(false)
  const queueStatus = useDownloadsStore((s) =>
    itemId == null ? undefined : s.items.find((i) => i.id === itemId)?.status
  )

  let state: ExternalDownloadState = 'idle'
  if (resolving) state = 'busy'
  else if (queueStatus === 'completed') state = 'done'
  else if (
    queueStatus === 'pending' ||
    queueStatus === 'downloading' ||
    queueStatus === 'paused'
  ) {
    state = 'busy'
  }
  // error / cancelled / unknown → 'idle': the button becomes tappable again.

  const start = async (): Promise<void> => {
    if (state !== 'idle') return
    setResolving(true)
    try {
      const item = await window.aether.downloadExternalTrack({
        artist: track.artist,
        title: track.title,
        durationMs: track.durationMs ?? null,
        coverUrl: track.coverUrl ?? null
      })
      if (item) {
        // Seed the store immediately: the broadcast `download:updated` may not
        // have arrived yet, and an undefined status would briefly re-enable the
        // button (double-enqueue on a fast second tap).
        useDownloadsStore.getState().upsert(item)
        setItemId(item.id)
        toast.success(t('discover.download_started', { title: track.title }))
      } else {
        toast.error(t('discover.download_failed', { title: track.title }))
      }
    } catch (err) {
      toast.error(ipcErrorMessage(err), track.title)
    } finally {
      setResolving(false)
    }
  }

  return { state, start }
}
