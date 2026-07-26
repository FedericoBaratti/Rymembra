import { z } from 'zod'
import { getDb } from '../db'
import type { Podcast, PodcastEpisode, PodcastSearchResult } from '@shared/types'
import { fetchJson, DEFAULT_USER_AGENT } from '../net/http'
import { logWarn } from '../logger'
import { parseFeed } from './rss'

// Podcast subscriptions over public RSS — keyless, no account, no server. Feeds
// are fetched as text and parsed by ./rss; iTunes Search (also keyless) powers
// discovery. Episodes stream from audio_url via the player's stream_url override.

const ItunesPodcastSchema = z.object({
  results: z
    .array(
      z.object({
        collectionName: z.string().optional(),
        artistName: z.string().optional(),
        feedUrl: z.string().optional(),
        artworkUrl600: z.string().nullish(),
        artworkUrl100: z.string().nullish()
      })
    )
    .optional()
})

/** Fetch a URL as text (RSS/XML). Uses the polyfilled global fetch on Node 12. */
async function fetchText(url: string, timeoutMs = 20_000): Promise<string> {
  const res = await fetch(url, {
    headers: { 'User-Agent': DEFAULT_USER_AGENT, Accept: 'application/rss+xml, application/xml, text/xml, */*' },
    signal: AbortSignal.timeout(timeoutMs)
  })
  if (!res.ok) throw new Error(`HTTP ${res.status}`)
  return res.text()
}

/** iTunes podcast directory search (keyless). Soft-fails to []. */
export async function searchPodcasts(term: string): Promise<PodcastSearchResult[]> {
  const cleaned = term.trim()
  if (!cleaned) return []
  try {
    const url = `https://itunes.apple.com/search?term=${encodeURIComponent(cleaned)}&entity=podcast&limit=24`
    const data = await fetchJson(url, { schema: ItunesPodcastSchema })
    const out: PodcastSearchResult[] = []
    for (const r of data.results ?? []) {
      if (!r.feedUrl || !r.collectionName) continue
      out.push({
        title: r.collectionName,
        author: r.artistName ?? null,
        feedUrl: r.feedUrl,
        imageUrl: r.artworkUrl600 ?? r.artworkUrl100 ?? null
      })
    }
    return out
  } catch (err) {
    logWarn('podcasts', `iTunes search fallita (${cleaned})`, err)
    return []
  }
}

function podcastById(id: number): Podcast {
  const row = getDb()
    .prepare(
      `SELECT p.*, (SELECT COUNT(*) FROM podcast_episodes e WHERE e.podcast_id = p.id) AS episode_count
       FROM podcasts p WHERE p.id = ?`
    )
    .get(id) as Podcast | undefined
  if (!row) throw new Error('PODCAST_NOT_FOUND')
  return row
}

/** Upsert the parsed episodes for a podcast; returns how many were newly added. */
function upsertEpisodes(podcastId: number, feedXml: string): number {
  const db = getDb()
  const parsed = parseFeed(feedXml)
  // Count truly-new episodes by diffing guids (ON CONFLICT updates also report
  // changes=1, so res.changes can't distinguish insert from update).
  const existing = new Set(
    (db.prepare('SELECT guid FROM podcast_episodes WHERE podcast_id = ?').all(podcastId) as {
      guid: string
    }[]).map((r) => r.guid)
  )
  const ins = db.prepare(
    `INSERT INTO podcast_episodes
       (podcast_id, guid, title, description, audio_url, image_url, duration, published_at)
     VALUES (@podcast_id, @guid, @title, @description, @audio_url, @image_url, @duration, @published_at)
     ON CONFLICT(podcast_id, guid) DO UPDATE SET
       title = excluded.title, description = excluded.description, audio_url = excluded.audio_url,
       image_url = excluded.image_url, duration = excluded.duration, published_at = excluded.published_at`
  )
  let added = 0
  const tx = db.transaction(() => {
    for (const e of parsed.episodes) {
      if (!existing.has(e.guid)) added++
      ins.run({
        podcast_id: podcastId,
        guid: e.guid,
        title: e.title,
        description: e.description,
        audio_url: e.audioUrl,
        image_url: e.imageUrl,
        duration: e.duration,
        published_at: e.publishedAt
      })
    }
    db.prepare('UPDATE podcasts SET last_refreshed = ? WHERE id = ?').run(Date.now(), podcastId)
  })
  tx()
  return added
}

/** Subscribe to a feed by URL (fetch + parse + persist). Returns the podcast. */
export async function addPodcast(feedUrl: string): Promise<Podcast> {
  const url = feedUrl.trim()
  if (!url) throw new Error('PODCAST_EMPTY_URL')
  const xml = await fetchText(url)
  const parsed = parseFeed(xml)
  const db = getDb()
  const now = Date.now()
  db.prepare(
    `INSERT INTO podcasts (feed_url, title, author, description, image_url, added_at, last_refreshed)
     VALUES (?, ?, ?, ?, ?, ?, ?)
     ON CONFLICT(feed_url) DO UPDATE SET
       title = excluded.title, author = excluded.author, description = excluded.description,
       image_url = excluded.image_url`
  ).run(url, parsed.title, parsed.author, parsed.description, parsed.imageUrl, now, now)
  const id = Number(
    (db.prepare('SELECT id FROM podcasts WHERE feed_url = ?').get(url) as { id: number }).id
  )
  upsertEpisodes(id, xml)
  return podcastById(id)
}

export async function refreshPodcast(podcastId: number): Promise<{ added: number }> {
  const row = getDb().prepare('SELECT feed_url FROM podcasts WHERE id = ?').get(podcastId) as
    | { feed_url: string }
    | undefined
  if (!row) throw new Error('PODCAST_NOT_FOUND')
  const xml = await fetchText(row.feed_url)
  return { added: upsertEpisodes(podcastId, xml) }
}

export function removePodcast(podcastId: number): void {
  const db = getDb()
  const tx = db.transaction(() => {
    db.prepare('DELETE FROM podcast_episodes WHERE podcast_id = ?').run(podcastId)
    db.prepare('DELETE FROM podcasts WHERE id = ?').run(podcastId)
  })
  tx()
}

export function getPodcasts(): Podcast[] {
  return getDb()
    .prepare(
      `SELECT p.*, (SELECT COUNT(*) FROM podcast_episodes e WHERE e.podcast_id = p.id) AS episode_count
       FROM podcasts p ORDER BY p.title COLLATE NOCASE`
    )
    .all() as Podcast[]
}

export function getPodcastEpisodes(podcastId: number): PodcastEpisode[] {
  return getDb()
    .prepare(
      `SELECT * FROM podcast_episodes WHERE podcast_id = ?
       ORDER BY published_at DESC NULLS LAST, id DESC`
    )
    .all(podcastId) as PodcastEpisode[]
}

/** Newest episodes across all subscriptions (Home "latest episodes" rail). */
export function getLatestEpisodes(limit = 20): PodcastEpisode[] {
  return getDb()
    .prepare(
      `SELECT e.*, p.title AS podcast_title FROM podcast_episodes e
       JOIN podcasts p ON p.id = e.podcast_id
       ORDER BY e.published_at DESC NULLS LAST, e.id DESC LIMIT ?`
    )
    .all(limit) as PodcastEpisode[]
}

export function setEpisodeProgress(episodeId: number, progressSec: number, played: boolean): void {
  getDb()
    .prepare('UPDATE podcast_episodes SET progress_sec = ?, played = ? WHERE id = ?')
    .run(Math.max(0, Math.round(progressSec)), played ? 1 : 0, episodeId)
}
