import { hostname } from 'node:os'
import { app } from 'electron'
import Bonjour from 'bonjour-service'

/** mDNS advertising of the LAN server as `<hostname>._aether._tcp.local`, so
    the Android thin client can auto-reconnect after the first QR pairing
    without the user re-entering an IP each session. */

let bonjour: InstanceType<typeof Bonjour> | null = null
let published: ReturnType<InstanceType<typeof Bonjour>['publish']> | null = null

export function startAdvertising(port: number): void {
  if (published) return
  if (!bonjour) bonjour = new Bonjour()
  published = bonjour.publish({
    name: hostname(),
    type: 'aether',
    protocol: 'tcp',
    port,
    txt: { deviceName: hostname(), version: app.getVersion() }
  })
}

export function stopAdvertising(): void {
  published?.stop()
  published = null
  // Destroy the Bonjour instance too: stop() only unpublishes, leaving the
  // underlying mDNS UDP sockets open until the process exits.
  bonjour?.destroy()
  bonjour = null
}

export function isAdvertising(): boolean {
  return published !== null
}
