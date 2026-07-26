import { statSync } from 'node:fs'
import { getDb } from '../db'
import { upsertTrackFromFile } from '../library'
import { writeTags } from '../tagIO'
import { logWarn } from '../logger'
import { cleanYoutubeMetadata } from './youtubeClean'

// Post-download correction for YouTube files: ingest them directly (so the
// track ids are known), mark provenance, and apply the heuristic metadata
// cleanup to both the DB row and the on-disk tags. The on-disk write keeps
// future rescans from resurrecting the dirty values.

/**
 * Force ONE consistent album + album_artist across a set of just-downloaded files
 * (a YouTube playlist/album whose per-entry yt-dlp tags diverge). Writes only the
 * file tags — a subsequent scan re-ingests and recomputes album_key — so the whole
 * batch resolves to a single release instead of scattering. Best-effort per file
 * (an unwritable file on Android scoped storage is logged and skipped).
 */
export async function unifyAlbumTags(
  files: string[],
  album: string,
  albumArtist: string | null
): Promise<void> {
  const albumName = album?.trim()
  if (!albumName || files.length < 2) return
  for (const file of files) {
    try {
      await writeTags(file, { album: albumName, album_artist: albumArtist ?? undefined }, null)
    } catch (err) {
      logWarn('download', `Unificazione album/album_artist fallita: ${file}`, err)
    }
  }
}

/** Returns the track ids of the ingested files (for enrichment enqueueing). */
export async function correctYoutubeDownloads(files: string[]): Promise<number[]> {
  const db = getDb()
  const ids: number[] = []

  for (const file of files) {
    const id = await upsertTrackFromFile(file)
    if (id == null) continue
    ids.push(id)
    db.prepare(`UPDATE tracks SET source = 'youtube' WHERE id = ?`).run(id)

    const row = db
      .prepare('SELECT title, artist, path FROM tracks WHERE id = ?')
      .get(id) as { title: string; artist: string; path: string } | undefined
    if (!row) continue

    const cleaned = cleanYoutubeMetadata(row.title, row.artist)
    if (!cleaned.changed) continue

    try {
      await writeTags(row.path, { title: cleaned.title, artist: cleaned.artist }, null)
    } catch (err) {
      // warn-only, mirroring the enrichment pipeline: the DB row still wins
      logWarn('download', `Tag non riscrivibili dopo la pulizia: ${row.path}`, err)
    }
    // store the post-write mtime so the scan fast-path doesn't re-parse the file
    let dateModified = Date.now()
    try {
      dateModified = Math.floor(statSync(row.path).mtimeMs)
    } catch {
      // file vanished mid-flight: keep the wall clock value
    }
    db.prepare('UPDATE tracks SET title = ?, artist = ?, date_modified = ? WHERE id = ?').run(
      cleaned.title,
      cleaned.artist,
      dateModified,
      id
    )
  }
  return ids
}
