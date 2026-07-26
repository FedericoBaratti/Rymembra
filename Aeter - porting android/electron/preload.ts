import { contextBridge, ipcRenderer } from 'electron'
import type { AetherAPI, AetherEventName } from '@shared/types'
import { INVOKE_METHODS } from '@shared/ipcMethods'

const api: Record<string, unknown> = {}
for (const method of INVOKE_METHODS) {
  api[method] = (...args: unknown[]) => ipcRenderer.invoke(method, ...args)
}

api['on'] = (event: AetherEventName, cb: (payload: unknown) => void) => {
  const listener = (_e: Electron.IpcRendererEvent, name: string, payload: unknown): void => {
    if (name === event) cb(payload)
  }
  ipcRenderer.on('aether:event', listener)
  return () => ipcRenderer.removeListener('aether:event', listener)
}

contextBridge.exposeInMainWorld('aether', api as unknown as AetherAPI)
