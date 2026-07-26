import type { PhonePairingCode, PhoneSyncState, PhoneTrackPlanned } from '@shared/types'
import { handle } from './handle'
import {
  cancelPhonePairing,
  forgetPhone,
  getPhoneSyncState,
  startPhonePairing
} from '../modules/phoneSync/pairing'
import { cancelRepair, refreshPhoneTracks, startRepair } from '../modules/phoneSync/worker'

/** Desktop surface of the phone repair feature (src/pages/PhoneSync.tsx). */
export function registerPhoneSyncIpc(): void {
  handle('phonePairStart', (): Promise<PhonePairingCode> => startPhonePairing())
  handle('phonePairCancel', (): void => cancelPhonePairing())
  handle('phoneGetState', (): PhoneSyncState => getPhoneSyncState())
  handle('phoneListTracks', (): Promise<PhoneTrackPlanned[]> => refreshPhoneTracks())
  handle('phoneRepairStart', (_e, ids: number[] | 'all'): Promise<void> => startRepair(ids))
  handle('phoneRepairCancel', (): void => cancelRepair())
  handle('phoneForget', (): void => {
    cancelRepair()
    forgetPhone()
  })
}
