import { callNative } from './runtime'

/**
 * Android tag write-back seam (electron/modules/tagIO.ts setTagWriteBack).
 *
 * node-taglib-sharp writes audio tags in place via a Node fs path. On Android
 * shared storage that path is read-only (READ_MEDIA_AUDIO grants reads only),
 * so file.save() throws EACCES. tagIO instead edits a private temp copy and
 * calls this seam, which asks the FileAccess Capacitor plugin to copy the temp
 * back over the original through SAF (the persisted tree grant from pickFolder
 * carries write permission). Reverse-RPC, mirroring ytdlp-shim.ts.
 */
export async function safTagWriteBack(originalPath: string, tempPath: string): Promise<void> {
  await callNative('saveFileViaSaf', { originalPath, tempPath }, 30000)
}
