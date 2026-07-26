// Electron-bound Last.fm service: desktop auth flow, signed network calls
// (rate-limited, circuit-broken) and the singleton offline scrobble queue.

import { shell } from 'electron'
import { z } from 'zod'
import type { Track } from '@shared/types'
import { getDb } from '../db'
import { getSettings, setSettings } from '../settings'
import { logWarn } from '../logger'
import { fetchJson } from '../net/http'
import { HttpError, NetworkError, RateLimitError, CircuitOpenError } from '../net/errors'
import { CircuitBreaker } from '../net/circuitBreaker'
import { RateLimiter } from '../net/rateLimiter'
import { withRetry } from '../net/retry'
import { apiSig, shouldScrobble } from './signature'
import { createScrobbler, type Scrobbler, type SendResult } from './scrobbler'

const API = 'https://ws.audioscrobbler.com/2.0/'
const FLUSH_INTERVAL_MS = 5 * 60_000

const limiter = new RateLimiter({ name: 'lastfm-scrobble', minIntervalMs: 250 })
const breaker = new CircuitBreaker({ name: 'Last.fm scrobble' })

const TokenSchema = z.object({ token: z.string() })
const SessionSchema = z.object({ session: z.object({ name: z.string(), key: z.string() }) })

// ---- auth (desktop flow: get token -> browser authorize -> get session) ----

let pendingToken: string | null = null

function creds(): { apiKey: string; secret: string } {
  const { lastfmApiKey, lastfmApiSecret } = getSettings()
  if (!lastfmApiKey || !lastfmApiSecret) {
    throw new Error('LASTFM_NOT_CONFIGURED')
  }
  return { apiKey: lastfmApiKey, secret: lastfmApiSecret }
}

function signedUrl(params: Record<string, string>, secret: string): string {
  const sig = apiSig(params, secret)
  const qs = new URLSearchParams({ ...params, api_sig: sig, format: 'json' })
  return `${API}?${qs.toString()}`
}

export async function lastfmStartAuth(): Promise<void> {
  const { apiKey, secret } = creds()
  const data = await fetchJson(signedUrl({ method: 'auth.gettoken', api_key: apiKey }, secret), {
    schema: TokenSchema
  })
  pendingToken = data.token
  await shell.openExternal(
    `https://www.last.fm/api/auth/?api_key=${apiKey}&token=${data.token}`
  )
}

export async function lastfmCompleteAuth(): Promise<{ username: string }> {
  if (!pendingToken) throw new Error('LASTFM_NO_PENDING_TOKEN')
  const { apiKey, secret } = creds()
  const data = await fetchJson(
    signedUrl({ method: 'auth.getsession', api_key: apiKey, token: pendingToken }, secret),
    { schema: SessionSchema }
  )
  pendingToken = null
  setSettings({
    lastfmSessionKey: data.session.key,
    lastfmUsername: data.session.name,
    scrobblingEnabled: true
  })
  return { username: data.session.name }
}

export function lastfmDisconnect(): void {
  pendingToken = null
  setSettings({ lastfmSessionKey: '', lastfmUsername: '', scrobblingEnabled: false })
}

// ---- queue wiring ----

/** Last.fm error 9 = invalid session key (re-auth required). */
const INVALID_SESSION_RE = /"error"\s*:\s*9\b/

async function send(method: string, params: Record<string, string>): Promise<SendResult> {
  const settings = getSettings()
  if (!settings.lastfmApiKey || !settings.lastfmApiSecret || !settings.lastfmSessionKey) {
    return 'auth'
  }
  const signed: Record<string, string> = {
    ...params,
    method,
    api_key: settings.lastfmApiKey,
    sk: settings.lastfmSessionKey
  }
  signed.api_sig = apiSig(signed, settings.lastfmApiSecret)
  const body = new URLSearchParams({ ...signed, format: 'json' })

  try {
    await withRetry(
      () =>
        breaker.exec(() =>
          limiter.schedule(() =>
            fetchJson(API, {
              init: {
                method: 'POST',
                headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
                body: body.toString()
              }
            })
          )
        ),
      { retries: 1 }
    )
    return 'ok'
  } catch (err) {
    if (err instanceof HttpError && (err.status === 401 || err.status === 403)) {
      return INVALID_SESSION_RE.test(err.body ?? '') || err.status === 401 ? 'auth' : 'transient'
    }
    if (
      err instanceof RateLimitError ||
      err instanceof NetworkError ||
      err instanceof CircuitOpenError ||
      (err instanceof HttpError && err.status >= 500)
    ) {
      return 'transient'
    }
    logWarn('scrobble', `Chiamata Last.fm ${method} fallita`, err)
    return 'transient'
  }
}

let scrobbler: Scrobbler | null = null
let flushTimer: ReturnType<typeof setInterval> | null = null

export function getScrobbler(): Scrobbler {
  if (scrobbler) return scrobbler
  scrobbler = createScrobbler(getDb(), {
    send,
    onInvalidSession: () => {
      logWarn('scrobble', 'Sessione Last.fm non valida: riconnetti l’account nelle impostazioni')
      setSettings({ lastfmSessionKey: '' })
    }
  })
  flushTimer = setInterval(() => {
    if (scrobblingActive()) void scrobbler?.flush()
  }, FLUSH_INTERVAL_MS)
  flushTimer.unref?.()
  return scrobbler
}

/** Stop the periodic flush and drop the singleton (app shutdown / tests). */
export function stopScrobbler(): void {
  if (flushTimer) {
    clearInterval(flushTimer)
    flushTimer = null
  }
  scrobbler = null
}

function scrobblingActive(): boolean {
  const s = getSettings()
  return s.scrobblingEnabled && !!s.lastfmSessionKey && !!s.lastfmApiKey && !!s.lastfmApiSecret
}

function getTrack(id: number): Track | null {
  return (getDb().prepare('SELECT * FROM tracks WHERE id = ?').get(id) as Track | undefined) ?? null
}

/** Applies the 50%-or-4-minutes rule and enqueues + flushes when eligible. */
export async function submitScrobble(
  trackId: number,
  playedSec: number,
  startedAtSec: number
): Promise<void> {
  if (!scrobblingActive()) return
  const track = getTrack(trackId)
  // Last.fm rejects empty/unknown artists
  if (!track || !track.artist || track.artist === 'Artista sconosciuto') return
  if (!shouldScrobble(track.duration, playedSec)) return

  const q = getScrobbler()
  q.enqueue({
    artist: track.artist,
    title: track.title,
    album: track.album || null,
    duration: track.duration ? Math.round(track.duration) : null,
    playedAt: startedAtSec
  })
  void q.flush()
}

/** Fire-and-forget "now playing" update. */
export async function updateNowPlaying(trackId: number): Promise<void> {
  if (!scrobblingActive()) return
  const track = getTrack(trackId)
  if (!track || !track.artist || track.artist === 'Artista sconosciuto') return
  const params: Record<string, string> = { artist: track.artist, track: track.title }
  if (track.album) params.album = track.album
  if (track.duration) params.duration = String(Math.round(track.duration))
  void send('track.updateNowPlaying', params)
}

export function getScrobbleStatus(): { queued: number; connected: boolean; username: string } {
  const s = getSettings()
  return {
    queued: getScrobbler().queuedCount(),
    connected: !!s.lastfmSessionKey,
    username: s.lastfmUsername
  }
}

/** Drains any scrobbles left over from a previous session (call on app start). */
export function flushPendingScrobbles(): void {
  if (scrobblingActive()) void getScrobbler().flush()
}
