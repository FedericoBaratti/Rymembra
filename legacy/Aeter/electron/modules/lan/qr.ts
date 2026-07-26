import QRCode from 'qrcode'

/** Renders arbitrary pairing payload as a data-URL PNG for the Settings UI. */
export async function generateQrDataUrl(payload: unknown): Promise<string> {
  return QRCode.toDataURL(JSON.stringify(payload), { margin: 1, scale: 6 })
}
