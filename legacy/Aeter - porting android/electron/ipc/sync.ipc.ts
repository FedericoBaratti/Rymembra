import { handle } from './handle'
import type { SyncStatus, MissingFetchStatus } from '@shared/types'
import {
  connectDrive,
  disconnectDrive,
  syncNow,
  getSyncStatus
} from '../modules/sync/syncService'
import { getMissingFetchStatus, retryFailedFetches } from '../modules/sync/fetchMissing'

export function registerSyncIpc(): void {
  handle('driveConnect', (): Promise<SyncStatus> => connectDrive())
  handle('driveDisconnect', (): Promise<SyncStatus> => disconnectDrive())
  handle('driveSyncNow', (): Promise<SyncStatus> => syncNow())
  handle('driveSyncStatus', (): SyncStatus => getSyncStatus())
  handle('syncMissingStatus', (): MissingFetchStatus => getMissingFetchStatus())
  handle('syncRetryMissing', (): MissingFetchStatus => retryFailedFetches())
}
