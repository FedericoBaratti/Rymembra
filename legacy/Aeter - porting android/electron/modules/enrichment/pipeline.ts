import type { EnrichmentResult, Track } from '@shared/types'
import { getDb } from '../db'
import { albumGroupKey } from '../albumKey'
import { broadcast } from '../events'
import { logWarn } from '../logger'
import { storeCover } from '../coverArt'
import { writeTags, verifyTags } from '../tagIO'
import { CircuitOpenError, HttpError, NetworkError } from '../net/errors'
import { resolveMetadata } from './resolve'
import { recognizeTrack } from './shazam/recognizeTrack'
import { computeFingerprint, acoustidLookup, acoustidConfigured } from './services/acoustid'
import { COVER_CONFIDENCE } from './decision'
import { resolveCover } from './services/coverart'
import { fetchLastfmGenre } from './services/lastfm'

// Stable codes translated in the renderer (src/lib/ipcError.ts). Never
// persisted: enrich_status stores its own enum, these only cross the IPC.
export const NO_MATCH_MESSAGE = 'ENRICH_NO_MATCH'
export const NEEDS_REVIEW_MESSAGE = 'ENRICH_NEEDS_REVIEW'
export const MB_UNAVAILABLE_MESSAGE = 'ENRICH_MB_UNAVAILABLE'

// Any cover write (first application and replacement alike) is gated on a
// strong, corroborated match. The constant lives in decision.ts.
export const COVER_REPLACE_CONFIDENCE = COVER_CONFIDENCE

export interface EnrichOptions {
  /** Allow replacing an existing cover when the new match is high-confidence. */
  replaceCover?: boolean
}

/** The subset of a track the file-level pipeline needs — a DB row is not required. */
export interface FileSnapshot {
  path: string
  title: string
  artist: string
  album: string | null
  duration: number | null
  year: number | null
  mbRecordingId: string | null
  hasCover: boolean
}

export interface FileEnrichOptions extends EnrichOptions {
  /**
   * Store a fetched cover in the app's sidecar cache (cover_art table + WebP
   * files). True for library tracks; false for staged foreign files (e.g. the
   * phone-repair pipeline), whose covers must not pollute this device's cache
   * — the cover still gets embedded in the audio file itself.
   */
  storeCoverSidecar?: boolean
  /** Invoked as soon as the fingerprint is computed (before any network I/O). */
  onFingerprint?: (fingerprint: string) => void
}

export interface FileEnrichOutcome {
  status: 'applied' | 'no-match' | 'needs-review' | 'unavailable'
  fields: Partial<Track>
  genre: string | null
  /** Cover fetched this run (already embedded in the file), or null. */
  coverBuffer: Buffer | null
  /** Sidecar hash — only set when storeCoverSidecar was enabled. */
  coverHash: string | null
  fingerprint: string | null
  message: string
}

function getTrack(id: number): Track | null {
  return (getDb().prepare('SELECT * FROM tracks WHERE id = ?').get(id) as Track | undefined) ?? null
}

function emitTrack(id: number): void {
  const track = getTrack(id)
  if (track) broadcast('track:updated', track)
}

/**
 * Path-based enrichment core: fingerprint → provider resolve → cover → genre →
 * tags written to the file on disk. Touches NO database row — enrichTrack()
 * wraps it for library tracks; the phone-repair worker calls it directly on
 * staged files.
 */
export async function enrichFile(
  snap: FileSnapshot,
  opts: FileEnrichOptions = {}
): Promise<FileEnrichOutcome> {
  // stage: fingerprint — keyless Shazam identification of the audio itself.
  // Never throws and never blocks: any failure (decoder missing, endpoint
  // down, schema drift) yields null and the textual path proceeds alone.
  const shazam = snap.path ? await recognizeTrack(snap.path, snap.duration ?? 0) : null

  // stage: acoustid — fpcalc/Chromaprint fingerprint against the AcoustID DB
  // (gold-standard, 30M+ prints). Desktop only: fpcalc is absent on Android, so
  // computeFingerprint returns null and this is a no-op there. Skipped when
  // Shazam already corroborated (avoids a redundant fpcalc subprocess) and when
  // no AcoustID key is configured. Best-effort: any failure yields null and the
  // textual/Shazam path proceeds alone.
  let acoustidId: string | null = null
  let fingerprint: string | null = null
  if (snap.path && !(shazam && shazam.corroborated) && acoustidConfigured()) {
    try {
      const fp = await computeFingerprint(snap.path)
      if (fp) {
        // truncated fingerprint doubles as the duplicate-detection key
        fingerprint = fp.fingerprint.slice(0, 256)
        opts.onFingerprint?.(fingerprint)
        acoustidId = await acoustidLookup(fp)
      }
    } catch (err) {
      logWarn('enrich', `AcoustID non disponibile per ${snap.path}`, err)
    }
  }

  let resolved
  try {
    resolved = await resolveMetadata(
      {
        title: snap.title,
        artist: snap.artist,
        // durationScore treats 0 as "unknown" → neutral, matching a null snap
        duration: snap.duration ?? 0,
        album: snap.album
      },
      shazam,
      acoustidId
    )
  } catch (err) {
    if (err instanceof NetworkError || err instanceof CircuitOpenError || err instanceof HttpError) {
      logWarn('enrich', `Provider metadati non raggiungibili per ${snap.path}`, err)
      return {
        status: 'unavailable',
        fields: {},
        genre: null,
        coverBuffer: null,
        coverHash: null,
        fingerprint,
        message: MB_UNAVAILABLE_MESSAGE
      }
    }
    throw err
  }

  if (!resolved) {
    return {
      status: 'no-match',
      fields: {},
      genre: null,
      coverBuffer: null,
      coverHash: null,
      fingerprint,
      message: NO_MATCH_MESSAGE
    }
  }

  if (resolved.verdict !== 'apply') {
    // Abstention: a plausible candidate exists but the evidence doesn't prove
    // it. Write NOTHING (no tags, no cover, no fields).
    return {
      status: 'needs-review',
      fields: {},
      genre: null,
      coverBuffer: null,
      coverHash: null,
      fingerprint,
      message: NEEDS_REVIEW_MESSAGE
    }
  }

  const { best, confidence } = resolved
  const artist = best.artist || snap.artist

  const fields: Partial<Track> = {
    title: best.title,
    artist,
    album: best.album ?? snap.album ?? undefined,
    year: best.year ?? snap.year,
    mb_recording_id: best.mbRecordingId ?? snap.mbRecordingId
  }

  // stage: cover — fetch when missing, or replace when explicitly requested.
  // Both share the same confidence bar (or a corroborated fingerprint): a
  // fresh track must not get a cover a re-enrich would refuse to apply. The
  // fallback chain (provider URL → CAA release-group → CAA per-release)
  // returns only validated images.
  const coverProven =
    confidence >= COVER_CONFIDENCE || resolved.evidence.fingerprint === 'corroborated'
  const wantCover = (!snap.hasCover || opts.replaceCover === true) && coverProven
  let coverBuffer: Buffer | null = null
  let coverHash: string | null = null
  if (wantCover) {
    const resolvedCover = await resolveCover({
      coverUrl: best.coverUrl,
      mbReleaseGroupId: best.mbReleaseGroupId,
      mbReleaseIds: best.mbReleaseIds
    })
    if (resolvedCover) {
      if (opts.storeCoverSidecar === false) {
        coverBuffer = resolvedCover.buffer
      } else {
        const newHash = await storeCover(resolvedCover.buffer, { source: resolvedCover.origin })
        if (newHash) {
          coverHash = newHash
          coverBuffer = resolvedCover.buffer
        }
      }
    }
  }

  // stage: genre — Last.fm top tag when a key is set, else the provider's genre
  // (iTunes primaryGenreName). Keyless-friendly.
  const lastfmGenre = await fetchLastfmGenre(artist, best.title)
  const genre = lastfmGenre ?? best.genre ?? null

  // stage: apply tags on disk
  const tagUpdate = {
    title: fields.title,
    artist: fields.artist,
    album: fields.album,
    year: fields.year ?? null,
    genre: genre ?? undefined
  }
  try {
    await writeTags(snap.path, tagUpdate, coverBuffer)
    const mismatches = verifyTags(snap.path, tagUpdate)
    if (mismatches.length > 0) {
      // warn-only: auto jobs must not hard-fail on a locked/read-only file
      // (Android shared storage EACCES); the caller's state is still correct
      logWarn('enrich', `Tag non verificati su ${snap.path}: ${mismatches.join(', ')}`)
    }
  } catch (err) {
    // best-effort: never abort because the file couldn't be written (Android
    // shared storage EACCES / SAF failure, locked file). The DB UPDATE still runs.
    logWarn('enrich', `Scrittura tag fallita su ${snap.path} (procedo)`, err)
  }

  return {
    status: 'applied',
    fields,
    genre,
    coverBuffer,
    coverHash,
    fingerprint,
    message: `ENRICH_FOUND:${artist} — ${best.title}`
  }
}

export async function enrichTrack(
  trackId: number,
  opts: EnrichOptions = {}
): Promise<EnrichmentResult> {
  const track = getTrack(trackId)
  if (!track) throw new Error('TRACK_NOT_FOUND')

  const outcome = await enrichFile(
    {
      path: track.path,
      title: track.title,
      artist: track.artist,
      album: track.album,
      duration: track.duration,
      year: track.year,
      mbRecordingId: track.mb_recording_id,
      hasCover: !!track.cover_art_hash
    },
    {
      ...opts,
      storeCoverSidecar: true,
      // persisted as soon as it's computed so a later hard failure (e.g. a
      // provider throwing mid-resolve) doesn't lose the fingerprint
      onFingerprint: (fp) => {
        getDb().prepare('UPDATE tracks SET acoustid_fingerprint = ? WHERE id = ?').run(fp, track.id)
      }
    }
  )

  if (outcome.status === 'unavailable') {
    // MB_UNAVAILABLE leaves enrich_status untouched: transient, retried later.
    return { trackId, applied: false, fields: {}, message: outcome.message }
  }

  if (outcome.status === 'no-match') {
    // no-match is a definitive verdict; never downgrade a track that already
    // matched.
    getDb()
      .prepare(
        `UPDATE tracks SET enrich_status = 'no-match', enrich_attempted_at = ?
         WHERE id = ? AND mb_recording_id IS NULL`
      )
      .run(Date.now(), trackId)
    return { trackId, applied: false, fields: {}, message: outcome.message }
  }

  if (outcome.status === 'needs-review') {
    // mark the track for review; the skip-cache TTL retries it later.
    getDb()
      .prepare(
        `UPDATE tracks SET enrich_status = 'needs-review', enrich_attempted_at = ?
         WHERE id = ? AND mb_recording_id IS NULL`
      )
      .run(Date.now(), trackId)
    return { trackId, applied: false, fields: {}, message: outcome.message }
  }

  const { fields, genre } = outcome
  getDb()
    .prepare(
      `UPDATE tracks SET title = ?, artist = ?, album = ?, album_key = ?, year = ?,
        genre = COALESCE(?, genre),
        mb_recording_id = ?, cover_art_hash = ?, date_modified = ?,
        enrich_status = 'ok', enrich_attempted_at = ? WHERE id = ?`
    )
    .run(
      fields.title,
      fields.artist,
      fields.album,
      // album identity is the derived album_key — recompute it whenever the
      // enriched album differs, or the track stays under its pre-enrich group.
      albumGroupKey(fields.album ?? track.album, track.path),
      fields.year,
      genre,
      fields.mb_recording_id ?? null,
      outcome.coverHash ?? track.cover_art_hash,
      Date.now(),
      Date.now(),
      trackId
    )

  emitTrack(trackId)
  return { trackId, applied: true, fields, message: outcome.message }
}
