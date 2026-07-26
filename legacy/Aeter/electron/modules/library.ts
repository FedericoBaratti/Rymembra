import { readdir, stat } from 'node:fs/promises'
import { join, extname } from 'node:path'
import PQueue from 'p-queue'
import { parseFile, type IAudioMetadata } from 'music-metadata'
import type { ScanProgress } from '@shared/types'
import { getDb } from './db'
import { storeCover } from './coverArt'
import { logWarn } from './logger'
import { thermalManager } from './adaptiveConcurrency'
import { albumGroupKey, buildAlbumGroups, type AlbumAggInput } from './albumKey'
import { MIN_BYTES } from './download/validate'

export const SUPPORTED_EXTS = new Set([
  '.mp3', '.flac', '.m4a', '.aac', '.ogg', '.wav', '.aiff', '.aif', '.opus', '.wma'
])

// Local trash folder for auto-dedupe victims (inside the download folder).
// Excluded from scans and watching so trashed files never re-enter the library.
export const TRASH_DIR_NAME = '.trash'

let scanning = false

export function isScanning(): boolean {
  return scanning
}

async function discoverFiles(folder: string): Promise<string[]> {
  try {
    const entries = await readdir(folder, { recursive: true, withFileTypes: true })
    return entries
      .filter((e) => e.isFile() && SUPPORTED_EXTS.has(extname(e.name).toLowerCase()))
      .map((e) => join(e.parentPath ?? (e as unknown as { path: string }).path, e.name))
      .filter((p) => !p.split(/[\\/]/).includes(TRASH_DIR_NAME)) // dedupe trash — never scanned
  } catch (err) {
    logWarn('scan', `Cartella non leggibile: ${folder}`, err)
    return []
  }
}

function firstComment(meta: IAudioMetadata): string | null {
  const c = meta.common.comment?.[0]
  if (!c) return null
  return typeof c === 'string' ? c : (c.text ?? null)
}

function firstLyrics(meta: IAudioMetadata): string | null {
  const l = meta.common.lyrics?.[0]
  if (!l) return null
  if (typeof l === 'string') return l
  return l.text ?? l.syncText?.map((s) => s.text).join('\n') ?? null
}

interface UpsertStatements {
  select: ReturnType<ReturnType<typeof getDb>['prepare']>
  update: ReturnType<ReturnType<typeof getDb>['prepare']>
  insert: ReturnType<ReturnType<typeof getDb>['prepare']>
}

let upsertStmts: UpsertStatements | null = null

function getUpsertStatements(): UpsertStatements {
  if (upsertStmts) return upsertStmts
  const db = getDb()
  upsertStmts = {
    select: db.prepare('SELECT id, date_modified FROM tracks WHERE path = ?'),
    update: db.prepare(
      `UPDATE tracks SET title=@title, artist=@artist, album=@album, album_artist=@album_artist,
        album_key=@album_key,
        mb_release_group_id=@mb_release_group_id, mb_release_id=@mb_release_id,
        year=@year, track_number=@track_number, disc_number=@disc_number, duration=@duration,
        bitrate=@bitrate, sample_rate=@sample_rate, codec=@codec, file_size=@file_size,
        date_modified=@date_modified, bpm=@bpm, key=@key, genre=COALESCE(@genre, genre),
        comment=@comment, lyrics=@lyrics,
        cover_art_hash=COALESCE(@cover_art_hash, cover_art_hash),
        replaygain_track_gain=@replaygain_track_gain, replaygain_album_gain=@replaygain_album_gain
       WHERE path=@path`
    ),
    insert: db.prepare(
      `INSERT INTO tracks (path, title, artist, album, album_artist, album_key,
        mb_release_group_id, mb_release_id, year, track_number, disc_number,
        duration, bitrate, sample_rate, codec, file_size, date_added, date_modified, bpm, key, genre,
        comment, lyrics, cover_art_hash, replaygain_track_gain, replaygain_album_gain)
       VALUES (@path, @title, @artist, @album, @album_artist, @album_key,
        @mb_release_group_id, @mb_release_id, @year, @track_number, @disc_number,
        @duration, @bitrate, @sample_rate, @codec, @file_size, @date_added, @date_modified, @bpm, @key, @genre,
        @comment, @lyrics, @cover_art_hash, @replaygain_track_gain, @replaygain_album_gain)`
    )
  }
  return upsertStmts
}

/** Insert or update a single audio file in the DB. Returns the track id, or null on failure. */
export async function upsertTrackFromFile(path: string): Promise<number | null> {
  const stmts = getUpsertStatements()
  let st
  try {
    st = await stat(path)
  } catch (err) {
    logWarn('scan', `stat fallita: ${path}`, err)
    return null
  }

  // No real song is under 32 KB: such files are truncated leftovers (killed
  // ffmpeg conversion, interrupted download) and would enter the library as
  // unplayable tracks — parseFile often still succeeds on them.
  if (st.size < MIN_BYTES) {
    logWarn('scan', `File troppo piccolo (${st.size}B), probabile download troncato, saltato: ${path}`)
    return null
  }

  const existing = stmts.select.get(path) as { id: number; date_modified: number } | undefined
  if (existing && existing.date_modified === Math.floor(st.mtimeMs)) {
    return existing.id
  }

  let meta: IAudioMetadata
  try {
    meta = await parseFile(path, { duration: false })
  } catch (err) {
    logWarn('scan', `Metadati non leggibili, file saltato: ${path}`, err)
    return null
  }

  const c = meta.common
  let coverHash: string | null = null
  // Prefer the FrontCover picture: files can embed several (back, booklet,
  // artist) and picture[0] is whatever the tagger wrote first.
  const pics = c.picture ?? []
  const pic = pics.find((p) => /front/i.test(p.type ?? '')) ?? pics[0]
  if (pic) {
    // A corrupt embedded image makes sharp throw; that must NOT abort the whole
    // scan (queue.addAll rejects on the first throw) — skip the cover instead.
    try {
      coverHash = await storeCover(Buffer.from(pic.data), { source: 'tag' })
    } catch (err) {
      logWarn('scan', `Errore cover art: ${path}`, err)
    }
  }

  const fileName = path.split(/[\\/]/).pop() ?? path
  const album = c.album?.trim() || 'Album sconosciuto'
  const row = {
    path,
    title: c.title?.trim() || fileName.replace(/\.[^.]+$/, ''),
    artist: c.artist?.trim() || 'Artista sconosciuto',
    album,
    album_artist: c.albumartist?.trim() || null,
    album_key: albumGroupKey(album, path),
    // Authoritative album identifiers from file tags (e.g. MusicBrainz Picard).
    // Drive the cross-folder/edition album merge in rebuildAggregates.
    // spotify_album_id is intentionally NOT touched here: it is stamped by the
    // downloader and must survive a rescan (omitted from INSERT → NULL default;
    // omitted from UPDATE → kept).
    mb_release_group_id: c.musicbrainz_releasegroupid?.trim() || null,
    mb_release_id: c.musicbrainz_albumid?.trim() || null,
    year: c.year ?? null,
    track_number: c.track?.no ?? null,
    disc_number: c.disk?.no ?? null,
    duration: meta.format.duration ?? 0,
    bitrate: meta.format.bitrate ? Math.round(meta.format.bitrate) : null,
    sample_rate: meta.format.sampleRate ?? null,
    codec: meta.format.codec ?? meta.format.container ?? null,
    file_size: st.size,
    date_modified: Math.floor(st.mtimeMs),
    bpm: c.bpm ?? null,
    key: c.key ?? null,
    genre: c.genre?.[0]?.trim() || null,
    comment: firstComment(meta),
    lyrics: firstLyrics(meta),
    cover_art_hash: coverHash,
    replaygain_track_gain: c.replaygain_track_gain?.dB ?? null,
    replaygain_album_gain: c.replaygain_album_gain?.dB ?? null
  }

  // A single row's DB error must not abort the whole scan (queue.addAll rejects
  // on the first throw) — log and skip the file instead.
  try {
    if (existing) {
      stmts.update.run(row)
      return existing.id
    }
    const res = stmts.insert.run({ ...row, date_added: Date.now() })
    return Number(res.lastInsertRowid)
  } catch (err) {
    logWarn('scan', `Errore db durante upsert: ${path}`, err)
    return null
  }
}

export function removeTrackByPath(path: string): void {
  getDb().prepare('DELETE FROM tracks WHERE path = ?').run(path)
}

/** Rebuild albums/artists aggregate tables from the tracks table. */
export function rebuildAggregates(): void {
  const db = getDb()
  try {
    const run = db.transaction(() => {
      // 1. Aggregate albums by album_key (normalized folder+title key, see
      //    albumKey.ts) instead of exact (album, album_artist) text — so an
      //    album never splits on tag inconsistencies and two same-titled releases
      //    in different folders stay separate. Same logic as the v10 migration.
      const aggInput = db
        .prepare(
          `SELECT t.album_key, t.album, t.album_artist, t.artist, t.year, t.cover_art_hash,
                  t.mb_release_group_id, t.mb_release_id, t.spotify_album_id,
                  c.source AS cover_source, c.width AS cover_w, c.height AS cover_h
           FROM tracks t LEFT JOIN cover_art c ON c.hash = t.cover_art_hash`
        )
        .all() as AlbumAggInput[]

      // Resolve the album identity (folder+title base key, then MERGE base groups
      // that share an authoritative id). Rewrite each track's album_key to the
      // canonical value so the tracks⋈albums join and the artist album-count stay
      // consistent. Ids only ever merge, never split.
      const { albums, remap } = buildAlbumGroups(aggInput)
      const reKey = db.prepare('UPDATE tracks SET album_key = ? WHERE album_key = ?')
      for (const [base, canonical] of remap) {
        if (base !== canonical) reKey.run(canonical, base)
      }

      const upsert = db.prepare(
        `INSERT INTO albums (album_key, title, artist, year, total_tracks, cover_art_hash, mb_album_id, spotify_id)
         VALUES (@album_key, @title, @artist, @year, @total_tracks, @cover_art_hash, @mb_album_id, @spotify_id)
         ON CONFLICT(album_key) DO UPDATE SET
           title = excluded.title,
           artist = excluded.artist,
           year = excluded.year,
           total_tracks = excluded.total_tracks,
           cover_art_hash = excluded.cover_art_hash,
           mb_album_id = COALESCE(excluded.mb_album_id, albums.mb_album_id),
           spotify_id = COALESCE(excluded.spotify_id, albums.spotify_id)`
      )
      for (const a of albums) upsert.run(a)

      // 2. Drop orphan albums (album_key no longer present among tracks). The
      //    IS NOT NULL filter avoids a NULL album_key turning the whole NOT IN
      //    into "unknown" (→ no rows deleted).
      db.exec(
        'DELETE FROM albums WHERE album_key NOT IN (SELECT album_key FROM tracks WHERE album_key IS NOT NULL)'
      )

      // 3. Populate unique artists from tracks.
      db.exec(`
        INSERT OR IGNORE INTO artists (name)
        SELECT DISTINCT COALESCE(album_artist, artist) FROM tracks;

        INSERT OR IGNORE INTO artists (name)
        SELECT DISTINCT artist FROM tracks;
      `)

      // 4. Remove orphan artists.
      db.exec(`
        DELETE FROM artists WHERE NOT EXISTS (
          SELECT 1 FROM tracks t
          WHERE t.artist = artists.name OR t.album_artist = artists.name
        );
      `)
    })
    run()
  } catch (err) {
    logWarn('scan', 'Errore durante la ricostruzione degli aggregati', err)
  }
}

/**
 * True when `p` is the folder itself or a file/dir strictly inside it. A plain
 * `startsWith` would wrongly match siblings like 'C:\Musica' against a watched
 * 'C:\Music', deleting their rows; require a separator boundary.
 */
export function isUnder(p: string, folder: string): boolean {
  const a = p.toLowerCase()
  const b = folder.toLowerCase().replace(/[\\/]+$/, '')
  return a === b || a.startsWith(b + '/') || a.startsWith(b + '\\')
}

/**
 * Canonical form for path SET MEMBERSHIP only (never for I/O): separators
 * unified, case folded on win32 (case-insensitive fs). Guards the stale-row
 * sweep in scanFolders against separator/case drift between the path stored
 * in the DB and the one the walk just produced — an exact-string mismatch
 * there would delete rows for files that still exist.
 */
function pathKey(p: string): string {
  const norm = p.replace(/\\/g, '/')
  return process.platform === 'win32' ? norm.toLowerCase() : norm
}

// A scan requested while one is running is not dropped: it is coalesced into
// ONE follow-up pass (with the latest arguments), so a download that lands
// mid-scan is never left un-ingested.
let rescanPending: { folders: string[]; onProgress: (p: ScanProgress) => void } | null = null

/**
 * Full scan of the given folders. Removes DB rows whose file disappeared,
 * adds/updates the rest. Reports progress via callback.
 */
export async function scanFolders(
  folders: string[],
  onProgress: (p: ScanProgress) => void
): Promise<void> {
  if (scanning) {
    rescanPending = { folders, onProgress }
    return
  }
  scanning = true
  try {
    onProgress({ phase: 'discovering', current: 0, total: 0, file: null })
    const found = new Set<string>()
    for (const folder of folders) {
      for (const f of await discoverFiles(folder)) found.add(f)
    }

    const db = getDb()
    const known = db.prepare('SELECT path FROM tracks WHERE is_local = 1').all() as {
      path: string
    }[]
    const foundKeys = new Set<string>()
    for (const f of found) foundKeys.add(pathKey(f))
    for (const { path } of known) {
      const inWatched = folders.some((f) => isUnder(path, f))
      if (inWatched && !foundKeys.has(pathKey(path))) removeTrackByPath(path)
    }

    const files = [...found]
    const total = files.length
    let done = 0
    // Thermal-aware workers: full speed when cool, halved/serialized when the
    // OS reports thermal pressure — retuned live while the scan is running.
    // (Only Android ever reports; on desktop this stays at the base value.)
    const SCAN_CONCURRENCY = 8
    const queue = new PQueue({ concurrency: thermalManager.getConcurrency(SCAN_CONCURRENCY) })
    const offThermal = thermalManager.onChange(() => {
      queue.concurrency = thermalManager.getConcurrency(SCAN_CONCURRENCY)
    })
    try {
      await queue.addAll(
        files.map((file) => async () => {
          try {
            await upsertTrackFromFile(file)
          } catch (err) {
            // One unreadable/corrupt file must not abort the whole scan
            // (queue.addAll rejects on the first throw).
            logWarn('scan', `Errore imprevisto durante la scansione: ${file}`, err)
          }
          done++
          if (done % 25 === 0 || done === total) {
            onProgress({ phase: 'scanning', current: done, total, file })
          }
        })
      )
    } catch (err) {
      logWarn('scan', 'Errore globale durante la scansione', err)
    } finally {
      offThermal()
    }

    rebuildAggregates()
    onProgress({ phase: 'done', current: total, total, file: null })
  } finally {
    scanning = false
  }
  if (rescanPending) {
    const next = rescanPending
    rescanPending = null
    await scanFolders(next.folders, next.onProgress)
  }
}
