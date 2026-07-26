import { app } from 'electron'
import { createServer, type IncomingMessage, type ServerResponse, type Server } from 'node:http'
import type { LanStatus } from '@shared/types'
import { getSettings } from '../settings'
import { logError } from '../logger'
import { handleMediaRoute, handleArtRoute } from './mediaRoutes'
import { handleApiRoute } from './routes'
import { authenticate } from './auth'
import { claimPairing } from './pairingStore'
import { sendJson, readJsonBody, CORS_HEADERS } from './httpJson'
import { getLocalIPv4Addresses } from './network'
import { attachLanWebSocket, detachLanWebSocket } from './ws'
import { startAdvertising, stopAdvertising, isAdvertising } from './mdns'
import { pairRateLimited } from './pairRateLimit'

let server: Server | null = null
let listening = false
let lastError: string | null = null

async function handleRequestAsync(req: IncomingMessage, res: ServerResponse): Promise<void> {
  const url = new URL(req.url ?? '/', 'http://localhost')
  const segments = url.pathname.split('/').filter(Boolean)
  const method = req.method ?? 'GET'

  if (method === 'OPTIONS') {
    res.writeHead(204, CORS_HEADERS).end()
    return
  }

  // HEAD gets the same headers as GET with no body (native players probe
  // /health and /media with HEAD before committing to a stream).
  if ((method === 'GET' || method === 'HEAD') && segments.length === 1 && segments[0] === 'health') {
    if (method === 'HEAD') {
      res.writeHead(200, { ...CORS_HEADERS, 'Content-Type': 'application/json' }).end()
      return
    }
    sendJson(res, 200, { ok: true, name: 'Aether', version: app.getVersion() })
    return
  }

  if (method === 'POST' && segments.length === 2 && segments[0] === 'api' && segments[1] === 'pair') {
    if (pairRateLimited(req.socket.remoteAddress ?? 'unknown')) {
      sendJson(res, 429, { error: 'too many pairing attempts, retry later' })
      return
    }
    const body = await readJsonBody<{ pairingToken?: string; deviceName?: string }>(req)
    const result = body?.pairingToken ? claimPairing(body.pairingToken, body.deviceName ?? '') : null
    if (!result) {
      sendJson(res, 401, { error: 'invalid or expired pairing code' })
      return
    }
    sendJson(res, 200, result)
    return
  }

  // Every other route requires a paired device's bearer token.
  const device = authenticate(req, url)
  if (!device) {
    sendJson(res, 401, { error: 'unauthorized' })
    return
  }

  if (segments.length === 2 && segments[0] === 'media') {
    handleMediaRoute(req, res, segments[1])
    return
  }
  if (segments.length === 2 && segments[0] === 'art') {
    handleArtRoute(req, res, segments[1], url.searchParams.get('thumb') === '1')
    return
  }
  if (segments[0] === 'api') {
    const handled = await handleApiRoute(req, res, url, segments.slice(1))
    if (!handled) sendJson(res, 404, { error: 'not found' })
    return
  }
  sendJson(res, 404, { error: 'not found' })
}

/** Exported for server.routes.test.ts, which mounts it on its own http server. */
export function handleRequest(req: IncomingMessage, res: ServerResponse): void {
  void handleRequestAsync(req, res).catch((err) => {
    logError('lan', 'request handler error', err)
    if (!res.headersSent) sendJson(res, 500, { error: 'internal error' })
    else res.destroy()
  })
}

export function startLanServer(): void {
  if (server) return
  const settings = getSettings()
  if (!settings.lanServerEnabled) return
  lastError = null
  const s = createServer(handleRequest)
  s.on('error', (err) => {
    logError('lan', 'server error', err)
    // A failure before 'listening' (e.g. EADDRINUSE) means the server never
    // came up: tear down so the UI shows the error, not a phantom "running".
    if (!listening) {
      lastError = err instanceof Error ? err.message : String(err)
      detachLanWebSocket()
      server = null
    }
  })
  s.on('listening', () => {
    listening = true
    startAdvertising(settings.lanServerPort)
  })
  attachLanWebSocket(s)
  s.listen(settings.lanServerPort, '0.0.0.0')
  server = s
}

export function stopLanServer(): void {
  if (!server) return
  stopAdvertising()
  detachLanWebSocket()
  // close() alone waits for keep-alive/long media streams; drop them so a
  // stop (or port change restart) is immediate.
  server.closeAllConnections()
  server.close()
  server = null
  listening = false
}

export function isLanServerRunning(): boolean {
  return server !== null && listening
}

/** Everything except `needsHotspot` (a hotspot.ts concern, composed by lan.ipc.ts). */
export function getLanServerState(): Omit<LanStatus, 'needsHotspot'> {
  const settings = getSettings()
  return {
    running: server !== null && listening,
    port: settings.lanServerPort,
    addresses: getLocalIPv4Addresses(),
    advertising: isAdvertising(),
    lastError
  }
}
