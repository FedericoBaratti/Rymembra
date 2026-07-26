import type { DownloadQuality, PhonePlanBadge, PhoneTrackInfo } from '@shared/types'
import { decideTranscode, type TranscodePlan } from '../audio/transcodePlan'

// Pure planning half of the phone repair (no electron/fs imports → unit-tested):
// given a PhoneTrackInfo from the transfer server, decide what the desktop
// pipeline would do. Corruption is NOT plannable from metadata — it only shows
// up after the pull, in validateAudioFile.

export interface TrackPlan {
  badge: PhonePlanBadge
  transcode: TranscodePlan
  enrich: boolean
}

/**
 * A track wants enrichment when its metadata story is incomplete: never
 * enriched successfully, no embedded/known cover, or missing basic tags the
 * providers reliably fill (genre, year).
 */
export function needsEnrich(info: PhoneTrackInfo): boolean {
  return info.enrichStatus !== 'ok' || !info.hasCover || !info.genre || !info.year
}

export function planTrack(info: PhoneTrackInfo, quality: DownloadQuality): TrackPlan {
  const transcode = decideTranscode(info.codec, info.ext, quality)
  const enrich = needsEnrich(info)
  const badge: PhonePlanBadge =
    transcode.needed && enrich
      ? 'both'
      : transcode.needed
        ? 'needs-codec'
        : enrich
          ? 'needs-enrich'
          : 'ok'
  return { badge, transcode, enrich }
}
