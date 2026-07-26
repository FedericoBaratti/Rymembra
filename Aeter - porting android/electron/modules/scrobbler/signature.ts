// Last.fm API signing + scrobble eligibility. Pure module, no electron imports.

import { createHash } from 'node:crypto'

/**
 * Last.fm api_sig: md5 of the params concatenated as key+value in key order
 * (format/callback excluded), followed by the shared secret. UTF-8.
 */
export function apiSig(params: Record<string, string>, secret: string): string {
  const concat = Object.keys(params)
    .filter((k) => k !== 'format' && k !== 'callback')
    .sort()
    .map((k) => k + params[k])
    .join('')
  return createHash('md5').update(concat + secret, 'utf8').digest('hex')
}

/**
 * Last.fm scrobble rule: track longer than 30s, listened for at least half
 * its duration or 4 minutes, whichever comes first.
 */
export function shouldScrobble(durationSec: number | null, playedSec: number): boolean {
  if (!durationSec || durationSec < 30) return false
  return playedSec >= Math.min(durationSec / 2, 240)
}
