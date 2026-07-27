/**
 * Il preload, generato dal contratto.
 *
 * Nel legacy questo file era un ciclo su `INVOKE_METHODS` chiuso da
 * `as unknown as AetherAPI`. Quel cast era il buco dell'architettura: niente
 * garantiva che l'array dei nomi contenesse le chiavi dell'interfaccia, né che per
 * ogni nome esistesse un handler. Un canale dimenticato era un reject a runtime.
 *
 * Qui i nomi vengono da `channelNames(CONTRACT)`, cioè dalla stessa fonte da cui
 * il main registra gli handler e da cui il renderer prende i tipi. Non possono
 * divergere: sono lo stesso oggetto.
 *
 * E ogni chiamata scarta la busta: il renderer riceve il valore, oppure un
 * AppError ricostruito con tutti i suoi campi — codice, dominio, parametri,
 * catena delle cause. Nel legacy arrivava una stringa, e il renderer ne
 * ricostruiva l'identità con una regex sul messaggio.
 */

import { contextBridge, ipcRenderer } from 'electron'
import { channelNames, unwrapEnvelope } from '@aether/core'
import { CONTRACT } from './contract'

const api: Record<string, (input?: unknown) => Promise<unknown>> = {}

for (const name of channelNames(CONTRACT)) {
  api[name] = async (input?: unknown) => unwrapEnvelope(await ipcRenderer.invoke(name, input))
}

/** Gli eventi backend→renderer. Uno solo per ora: il guasto fatale. */
const events = {
  onFatal: (handler: (payload: unknown) => void): (() => void) => {
    const listener = (_event: unknown, payload: unknown): void => handler(payload)
    ipcRenderer.on('aether:fatal', listener)
    return () => ipcRenderer.removeListener('aether:fatal', listener)
  }
}

contextBridge.exposeInMainWorld('aether', { ...api, ...events })
