// Minimal, dependency-free RSS/Atom podcast parser. Pure (no electron, no IO →
// unit-tested). Regex/string based so it runs on nodejs-mobile (Node 12) without
// an XML library; ICU-free (no \p{}), no .at()/.replaceAll()/.matchAll() spread.

export interface ParsedEpisode {
  guid: string
  title: string
  description: string | null
  audioUrl: string
  imageUrl: string | null
  /** Episode length in seconds, when the feed declares it. */
  duration: number | null
  /** Publish time in unix-ms, when parseable. */
  publishedAt: number | null
}

export interface ParsedFeed {
  title: string
  author: string | null
  description: string | null
  imageUrl: string | null
  episodes: ParsedEpisode[]
}

function decodeEntities(s: string): string {
  return s
    .replace(/<!\[CDATA\[([\s\S]*?)\]\]>/g, '$1')
    .replace(/&#x([0-9a-fA-F]+);/g, (_m, h) => String.fromCodePoint(parseInt(h, 16)))
    .replace(/&#(\d+);/g, (_m, d) => String.fromCodePoint(parseInt(d, 10)))
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&#39;/g, "'")
    .replace(/&lt;/g, '<')
    .replace(/&gt;/g, '>')
    .replace(/&amp;/g, '&')
    .trim()
}

/** Inner text of the first <name ...>…</name> in `block` (namespaced ok). */
function tagText(block: string, name: string): string | null {
  const re = new RegExp(`<${name}(?:\\s[^>]*)?>([\\s\\S]*?)</${name}>`, 'i')
  const m = re.exec(block)
  return m ? decodeEntities(m[1]) : null
}

/** Value of `attr` on the first <name …attr="…"…> tag (self-closing ok). */
function tagAttr(block: string, name: string, attr: string): string | null {
  const re = new RegExp(`<${name}\\b[^>]*?\\b${attr}\\s*=\\s*"([^"]*)"[^>]*>`, 'i')
  const m = re.exec(block)
  if (m) return decodeEntities(m[1])
  const re2 = new RegExp(`<${name}\\b[^>]*?\\b${attr}\\s*=\\s*'([^']*)'[^>]*>`, 'i')
  const m2 = re2.exec(block)
  return m2 ? decodeEntities(m2[1]) : null
}

/** "1:02:03" / "12:34" / "754" → seconds. */
function parseDuration(raw: string | null): number | null {
  if (!raw) return null
  const s = raw.trim()
  if (/^\d+$/.test(s)) return parseInt(s, 10)
  const parts = s.split(':').map((p) => parseInt(p, 10))
  if (parts.some((n) => Number.isNaN(n))) return null
  let sec = 0
  for (const p of parts) sec = sec * 60 + p
  return sec
}

function parseDate(raw: string | null): number | null {
  if (!raw) return null
  const t = Date.parse(raw.trim())
  return Number.isNaN(t) ? null : t
}

function episodeImage(item: string): string | null {
  return tagAttr(item, 'itunes:image', 'href') ?? tagAttr(item, 'media:thumbnail', 'url')
}

function audioFromItem(item: string): string | null {
  // Prefer an audio <enclosure>, else a media:content audio url.
  const enc = /<enclosure\b[^>]*>/i.exec(item)
  if (enc) {
    const type = /\btype\s*=\s*"([^"]*)"/i.exec(enc[0])
    const url = /\burl\s*=\s*"([^"]*)"/i.exec(enc[0])
    if (url && (!type || /audio|mpeg|mp4|mp3|ogg|aac|m4a/i.test(type[1]))) {
      return decodeEntities(url[1])
    }
    if (url) return decodeEntities(url[1])
  }
  const media = /<media:content\b[^>]*>/i.exec(item)
  if (media) {
    const url = /\burl\s*=\s*"([^"]*)"/i.exec(media[0])
    if (url) return decodeEntities(url[1])
  }
  return null
}

/** Parse a podcast RSS document. Throws only on a totally unusable document. */
export function parseFeed(xml: string): ParsedFeed {
  // Channel header = everything before the first <item> (or the whole doc).
  const firstItem = xml.search(/<item\b/i)
  const header = firstItem >= 0 ? xml.slice(0, firstItem) : xml

  const title =
    tagText(header, 'title') ?? tagText(header, 'itunes:title') ?? 'Podcast'
  const author = tagText(header, 'itunes:author') ?? tagText(header, 'managingEditor')
  const description = tagText(header, 'description') ?? tagText(header, 'itunes:summary')
  const imageUrl =
    tagAttr(header, 'itunes:image', 'href') ??
    (() => {
      const img = /<image\b[\s\S]*?<\/image>/i.exec(header)
      return img ? tagText(img[0], 'url') : null
    })()

  const episodes: ParsedEpisode[] = []
  const itemRe = /<item\b[\s\S]*?<\/item>/gi
  let m: RegExpExecArray | null
  while ((m = itemRe.exec(xml)) !== null) {
    const item = m[0]
    const audioUrl = audioFromItem(item)
    if (!audioUrl) continue // no playable audio → skip
    const epTitle = tagText(item, 'title') ?? '(untitled)'
    const guid = tagText(item, 'guid') ?? audioUrl
    episodes.push({
      guid,
      title: epTitle,
      description: tagText(item, 'description') ?? tagText(item, 'itunes:summary'),
      audioUrl,
      imageUrl: episodeImage(item),
      duration: parseDuration(tagText(item, 'itunes:duration')),
      publishedAt: parseDate(tagText(item, 'pubDate'))
    })
  }

  return { title, author, description, imageUrl, episodes }
}
