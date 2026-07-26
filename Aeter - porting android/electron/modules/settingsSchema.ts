import { z } from 'zod'
import type { AppSettings } from '@shared/types'

export const DEFAULTS: AppSettings = {
  watchFolders: [],
  downloadFolder: '',
  downloadQuality: 'mp3-320',
  downloadConcurrency: 5,
  autoFixYoutubeMetadata: true,
  crossfadeSeconds: 0,
  audioOffloadEnabled: false,
  replayGainEnabled: false,
  replayGainTargetDb: -18,
  eqEnabled: false,
  eqGains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  eqCustomPresets: [],
  theme: 'dark',
  skin: 'plain',
  language: 'it',
  volume: 0.8,
  muted: false,
  notificationsOnTrackChange: true,
  globalMediaKeys: true,
  hasSeenOnboarding: false,
  spotifyClientId: '',
  spotifyClientSecret: '',
  lastfmApiKey: '',
  lastfmApiSecret: '',
  lastfmSessionKey: '',
  lastfmUsername: '',
  scrobblingEnabled: false,
  acoustidApiKey: '',
  autoEnrichEnabled: true,
  enrichFingerprint: true,
  dedupeAutoRemove: true,
  dedupeKeep: 'higher',
  googleClientId: '',
  googleClientSecret: '',
  driveSyncEnabled: false,
  driveSyncLastAt: null,
  googleDriveEmail: '',
  syncDeviceId: '',
  driveFileId: null,
  driveLastLocalHash: null,
  driveLastRemoteMd5: null,
  googleRefreshToken: '',
  autoFetchMissing: true,
  autoFetchNetwork: 'wifi',
  transferServerEnabled: false
}

const eqPresetSchema = z.object({
  name: z.string(),
  gains: z.array(z.number().min(-12).max(12)).length(10)
})

// Each field falls back to its default alone, so one corrupt value never
// resets the rest of the user's settings.
const settingsSchema = z.object({
  watchFolders: z.array(z.string()).catch(DEFAULTS.watchFolders),
  downloadFolder: z.string().catch(DEFAULTS.downloadFolder),
  downloadQuality: z.enum(['mp3-320', 'flac', 'aac-256']).catch(DEFAULTS.downloadQuality),
  downloadConcurrency: z.number().int().min(1).max(10).catch(DEFAULTS.downloadConcurrency),
  autoFixYoutubeMetadata: z.boolean().catch(DEFAULTS.autoFixYoutubeMetadata),
  crossfadeSeconds: z.number().min(0).max(12).catch(DEFAULTS.crossfadeSeconds),
  audioOffloadEnabled: z.boolean().catch(DEFAULTS.audioOffloadEnabled),
  replayGainEnabled: z.boolean().catch(DEFAULTS.replayGainEnabled),
  replayGainTargetDb: z.number().min(-30).max(0).catch(DEFAULTS.replayGainTargetDb),
  eqEnabled: z.boolean().catch(DEFAULTS.eqEnabled),
  eqGains: z.array(z.number().min(-12).max(12)).length(10).catch(DEFAULTS.eqGains),
  eqCustomPresets: z.array(eqPresetSchema).catch(DEFAULTS.eqCustomPresets),
  theme: z.enum(['dark', 'light', 'system']).catch(DEFAULTS.theme),
  skin: z.enum(['plain', 'nothing', 'cyberpunk']).catch(DEFAULTS.skin),
  language: z.enum(['it', 'en']).catch(DEFAULTS.language),
  volume: z.number().min(0).max(1).catch(DEFAULTS.volume),
  muted: z.boolean().catch(DEFAULTS.muted),
  notificationsOnTrackChange: z.boolean().catch(DEFAULTS.notificationsOnTrackChange),
  globalMediaKeys: z.boolean().catch(DEFAULTS.globalMediaKeys),
  hasSeenOnboarding: z.boolean().catch(DEFAULTS.hasSeenOnboarding),
  spotifyClientId: z.string().catch(''),
  spotifyClientSecret: z.string().catch(''),
  lastfmApiKey: z.string().catch(''),
  lastfmApiSecret: z.string().catch(''),
  lastfmSessionKey: z.string().catch(''),
  lastfmUsername: z.string().catch(''),
  scrobblingEnabled: z.boolean().catch(DEFAULTS.scrobblingEnabled),
  acoustidApiKey: z.string().catch(''),
  autoEnrichEnabled: z.boolean().catch(DEFAULTS.autoEnrichEnabled),
  enrichFingerprint: z.boolean().catch(DEFAULTS.enrichFingerprint),
  dedupeAutoRemove: z.boolean().catch(DEFAULTS.dedupeAutoRemove),
  dedupeKeep: z.enum(['higher', 'lower']).catch(DEFAULTS.dedupeKeep),
  googleClientId: z.string().catch(''),
  googleClientSecret: z.string().catch(''),
  driveSyncEnabled: z.boolean().catch(DEFAULTS.driveSyncEnabled),
  driveSyncLastAt: z.number().nullable().catch(null),
  googleDriveEmail: z.string().catch(''),
  syncDeviceId: z.string().catch(''),
  driveFileId: z.string().nullable().catch(null),
  driveLastLocalHash: z.string().nullable().catch(null),
  driveLastRemoteMd5: z.string().nullable().catch(null),
  googleRefreshToken: z.string().catch(''),
  autoFetchMissing: z.boolean().catch(DEFAULTS.autoFetchMissing),
  autoFetchNetwork: z.enum(['wifi', 'any']).catch(DEFAULTS.autoFetchNetwork),
  transferServerEnabled: z.boolean().catch(DEFAULTS.transferServerEnabled)
}) satisfies z.ZodType<AppSettings>

/** Validates raw JSON into AppSettings; invalid fields fall back per-field. */
export function parseSettings(raw: unknown): AppSettings {
  if (typeof raw !== 'object' || raw === null) return { ...DEFAULTS }
  return settingsSchema.parse({ ...DEFAULTS, ...raw })
}
