import type { IncomingMessage, Server } from 'node:http'
import type { Socket } from 'node:net'
import { WebSocketServer, type WebSocket } from 'ws'
import type { AetherEventName } from '@shared/types'
import { onBroadcast } from '../events'
import { authenticate } from './auth'

/** Desktop-only concerns (sync status, downloads, media keys, ...) are
    deliberately not forwarded — the phone only needs to know about the
    library/playback state relevant to browsing/searching/playing. */
const FORWARDED_EVENTS = new Set<AetherEventName>([
  'library:changed',
  'track:updated',
  'enrichment:updated',
  'scan:progress',
  'duplicates:removed'
])

/** Idle NAT/Wi-Fi paths silently drop connections; ping on this cadence and
    terminate clients that miss a full interval without ponging. */
const HEARTBEAT_INTERVAL_MS = 30_000

interface TrackedSocket extends WebSocket {
  deviceId?: string
  isAlive?: boolean
}

let wss: WebSocketServer | null = null
let unsubscribeBroadcast: (() => void) | null = null
let heartbeatTimer: NodeJS.Timeout | null = null

/** Wires a `/ws` upgrade handler onto an already-listening LAN HTTP server and
    starts forwarding the allow-listed broadcast() events to connected clients. */
export function attachLanWebSocket(server: Server): void {
  const socketServer = new WebSocketServer({ noServer: true })
  wss = socketServer

  server.on('upgrade', (req: IncomingMessage, socket: Socket, head: Buffer) => {
    const url = new URL(req.url ?? '/', 'http://localhost')
    if (url.pathname !== '/ws') {
      socket.destroy()
      return
    }
    const device = authenticate(req, url)
    if (!device) {
      socket.write('HTTP/1.1 401 Unauthorized\r\n\r\n')
      socket.destroy()
      return
    }
    socketServer.handleUpgrade(req, socket, head, (ws) => {
      const tracked = ws as TrackedSocket
      tracked.deviceId = device.deviceId
      tracked.isAlive = true
      tracked.on('pong', () => {
        tracked.isAlive = true
      })
      socketServer.emit('connection', ws, req)
    })
  })

  heartbeatTimer = setInterval(() => {
    for (const client of socketServer.clients) {
      const tracked = client as TrackedSocket
      if (tracked.isAlive === false) {
        tracked.terminate()
        continue
      }
      tracked.isAlive = false
      tracked.ping()
    }
  }, HEARTBEAT_INTERVAL_MS)

  unsubscribeBroadcast = onBroadcast((event, payload) => {
    if (!FORWARDED_EVENTS.has(event)) return
    const message = JSON.stringify({ event, payload })
    for (const client of socketServer.clients) {
      if (client.readyState === client.OPEN) client.send(message)
    }
  })
}

/** Drops the live connections of a just-revoked device — without this it would
    keep receiving events until its next reconnect finally 401s. */
export function closeLanSocketsForDevice(deviceId: string): void {
  if (!wss) return
  for (const client of wss.clients) {
    if ((client as TrackedSocket).deviceId === deviceId) client.terminate()
  }
}

export function detachLanWebSocket(): void {
  unsubscribeBroadcast?.()
  unsubscribeBroadcast = null
  if (heartbeatTimer) {
    clearInterval(heartbeatTimer)
    heartbeatTimer = null
  }
  if (wss) {
    for (const client of wss.clients) client.close()
    wss.close()
    wss = null
  }
}
