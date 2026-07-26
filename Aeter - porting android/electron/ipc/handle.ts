import { ipcMain, type IpcMainInvokeEvent } from 'electron'
import { logError } from '../modules/logger'

type Handler = (event: IpcMainInvokeEvent, ...args: never[]) => unknown

/** ipcMain.handle with centralized logging. Electron only serializes the
    error message across IPC, so rethrow a clean Error carrying just that. */
export function handle(channel: string, fn: Handler): void {
  ipcMain.handle(channel, async (event, ...args) => {
    try {
      return await fn(event, ...(args as never[]))
    } catch (err) {
      logError('ipc', channel, err)
      throw err instanceof Error ? new Error(err.message) : new Error(String(err))
    }
  })
}
