import { shell } from 'electron'
import { getLocalIPv4Addresses } from './network'

/**
 * Windows Mobile Hotspot fallback for when the phone isn't on the same LAN.
 * Windows exposes no plain cmdlet for this — only the WinRT
 * NetworkOperatorTetheringManager API, which requires elevation. Rather than
 * have the app register a persistent elevated Scheduled Task (a standing
 * privilege-escalation mechanism), this deep-links the user to the native
 * Mobile Hotspot settings page for a single manual click — no elevation, no
 * background task, nothing the app can trigger unattended.
 */

/** True when no usable (non-link-local) LAN IPv4 address exists — the
    heuristic that promotes the hotspot button to the primary action in the
    "Connetti telefono" UI instead of a secondary/collapsed one. */
export function needsHotspot(): boolean {
  return getLocalIPv4Addresses().length === 0
}

/** Opens the native Mobile Hotspot settings page so the user can flip the
    toggle by hand, then return to "Connetti telefono" to regenerate the QR
    against the hotspot adapter's IP. */
export async function openHotspotSettings(): Promise<void> {
  await shell.openExternal('ms-settings:network-mobilehotspot')
}
