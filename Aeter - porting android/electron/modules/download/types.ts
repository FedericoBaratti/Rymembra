import type { AppSettings, DownloadItem, DownloadPreview } from '@shared/types'
import type { ParsedUrl } from './urlDetect'

export interface DownloadProgressPatch {
  progress?: number
  completed_tracks?: number
  total_tracks?: number
  current_file?: string | null
}

export interface DownloadOutcome {
  /** Absolute paths of finished files (when the tool reports them). */
  files: string[]
  /** Tracks that failed inside an otherwise-successful playlist run. */
  partialFailures: number
}

export interface DownloadContext {
  item: DownloadItem
  settings: AppSettings
  /** Aborted on user cancellation — handlers must kill their child process. */
  signal: AbortSignal
  onProgress(patch: DownloadProgressPatch): void
}

/** One implementation per source site. Adding a source = one file + one registry entry. */
export interface SourceHandler {
  readonly id: string
  detect(url: string): ParsedUrl | null
  preview(url: string, parsed: ParsedUrl): Promise<DownloadPreview>
  download(ctx: DownloadContext): Promise<DownloadOutcome>
}
