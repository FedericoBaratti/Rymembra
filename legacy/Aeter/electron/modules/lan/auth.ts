import type { IncomingMessage } from 'node:http'
import { verifyDeviceToken, type StoredDevice } from './pairingStore'

/**
 * Bearer token via the Authorization header (used by native clients that can
 * set custom headers, e.g. ExoPlayer) or a `?token=` query param (needed for
 * plain `<audio>`/`<img>` tags, which can't set headers).
 */
function extractToken(req: IncomingMessage, url: URL): string | null {
  const header = req.headers['authorization']
  if (typeof header === 'string' && header.startsWith('Bearer ')) return header.slice(7)
  return url.searchParams.get('token')
}

export function authenticate(req: IncomingMessage, url: URL): StoredDevice | null {
  return verifyDeviceToken(extractToken(req, url))
}
