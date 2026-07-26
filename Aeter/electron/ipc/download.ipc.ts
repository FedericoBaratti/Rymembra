import { handle } from './handle'
import type { DownloadPreview } from '@shared/types'
import {
  previewDownload,
  startDownload,
  cancelDownload,
  pauseDownload,
  resumeDownload,
  retryDownload,
  clearFinished,
  getDownloads,
  recoverStaleDownloads
} from '../modules/downloader'
import { getBinaryStatus, updateYtDlp } from '../modules/binaries'

export function registerDownloadIpc(): void {
  recoverStaleDownloads()

  handle('previewDownload', (_e, url: string) => previewDownload(url))
  handle('startDownload', (_e, preview: DownloadPreview) => startDownload(preview))
  handle('cancelDownload', (_e, id: number) => cancelDownload(id))
  handle('pauseDownload', (_e, id: number) => pauseDownload(id))
  handle('resumeDownload', (_e, id: number) => resumeDownload(id))
  handle('retryDownload', (_e, id: number) => retryDownload(id))
  handle('clearFinishedDownloads', () => clearFinished())
  handle('getDownloads', () => getDownloads())
  handle('updateYtDlp', () => updateYtDlp())
  handle('getBinaryStatus', () => getBinaryStatus())
}
