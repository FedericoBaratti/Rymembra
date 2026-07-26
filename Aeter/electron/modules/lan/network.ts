import { networkInterfaces } from 'node:os'

/**
 * Non-internal IPv4 addresses of this machine, excluding link-local/APIPA
 * (169.254.x.x) — those indicate "no usable network" rather than a real LAN.
 * Used both for the pairing QR host and the hotspot-availability heuristic.
 */
export function getLocalIPv4Addresses(): string[] {
  const nets = networkInterfaces()
  const addrs: string[] = []
  for (const entries of Object.values(nets)) {
    for (const net of entries ?? []) {
      if (net.family === 'IPv4' && !net.internal && !net.address.startsWith('169.254.')) {
        addrs.push(net.address)
      }
    }
  }
  return addrs
}
