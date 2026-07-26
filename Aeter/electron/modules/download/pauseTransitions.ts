import type { DownloadStatus } from '@shared/types'

// Pure state-transition rules for download pause/resume, kept free of
// electron/DB imports so they stay unit-testable like progress.ts/errors.ts.

export function canPause(status: DownloadStatus): boolean {
  return status === 'pending' || status === 'downloading'
}

export function canResume(status: DownloadStatus): boolean {
  return status === 'paused'
}

/**
 * Status to write when a running download aborts: a user-requested pause
 * keeps the row resumable, anything else is a cancellation.
 */
export function statusOnAbort(pauseIntended: boolean): DownloadStatus {
  return pauseIntended ? 'paused' : 'cancelled'
}
