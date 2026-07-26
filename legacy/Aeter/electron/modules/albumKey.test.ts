import { describe, expect, it } from 'vitest'
import {
  stripEditionSuffix,
  albumFolder,
  albumGroupKey,
  normalizeKeyText,
  buildAlbumGroups,
  pickCanonical,
  canonicalAlbumArtist,
  aggregateAlbums,
  pickAlbumCover,
  type AlbumAggInput
} from './albumKey'

const D = '/sd/Music/'

describe('albumKey.stripEditionSuffix', () => {
  it('strips edition/disc qualifiers in parentheses/brackets', () => {
    expect(stripEditionSuffix('Random Access Memories (Deluxe Edition)')).toBe(
      'Random Access Memories'
    )
    expect(stripEditionSuffix('Abbey Road [2019 Remaster]')).toBe('Abbey Road')
    expect(stripEditionSuffix('X (Bonus Track Version)')).toBe('X')
    expect(stripEditionSuffix('The Wall (CD1)')).toBe('The Wall')
    expect(stripEditionSuffix('The Wall (Disc 2)')).toBe('The Wall')
    expect(stripEditionSuffix('Album (Remastered) (Deluxe Edition)')).toBe('Album')
  })

  it('keeps parentheticals that are part of the real title (whole-word match)', () => {
    expect(stripEditionSuffix('Songs (For Drella)')).toBe('Songs (For Drella)')
    expect(stripEditionSuffix('Album (Deep Cuts)')).toBe('Album (Deep Cuts)') // "ep" inside "deep"
    expect(stripEditionSuffix('Live in Tokyo')).toBe('Live in Tokyo')
  })

  it('never empties the title', () => {
    expect(stripEditionSuffix('Version')).toBe('Version')
    expect(stripEditionSuffix('Greatest Hits')).toBe('Greatest Hits')
  })
})

describe('albumKey.albumFolder', () => {
  it('returns the full containing directory path', () => {
    expect(albumFolder(D + 'Queen/Greatest Hits/01.mp3')).toBe('sd/Music/Queen/Greatest Hits')
  })

  it('collapses a disc subfolder (CD1/Disc 2) to its parent', () => {
    expect(albumFolder(D + 'Pink Floyd/The Wall/CD1/01.mp3')).toBe(
      albumFolder(D + 'Pink Floyd/The Wall/CD2/01.mp3')
    )
  })

  it('handles Windows backslash paths', () => {
    expect(albumFolder('C:\\Music\\Artist\\AlbumX\\1.mp3')).toBe('C:/Music/Artist/AlbumX')
  })
})

describe('albumKey.albumGroupKey', () => {
  it('A: merges a release split by inconsistent album_artist in the same folder', () => {
    const k1 = albumGroupKey('Thriller', D + 'Michael Jackson/Thriller/01.mp3')
    const k2 = albumGroupKey('Thriller', D + 'Michael Jackson/Thriller/09.mp3')
    expect(k1).toBe(k2)
  })

  it('B: merges title variants (case/space/diacritics/edition) in the same folder', () => {
    const f = D + 'The Beatles/Abbey Road/x.mp3'
    const keys = new Set(
      [
        'Abbey Road',
        'abbey road ',
        'Abbey Road (Remastered)',
        'Abbey Road [2019 Remaster]',
        'Abbey Road (Deluxe Edition)'
      ].map((t) => albumGroupKey(t, f))
    )
    expect(keys.size).toBe(1)
    expect(albumGroupKey('Café Bleu', D + 'Style/Cafe/x.mp3')).toBe(
      albumGroupKey('Cafe Bleu', D + 'Style/Cafe/x.mp3')
    )
  })

  it('C: keeps same-titled releases in different artist folders separate', () => {
    expect(albumGroupKey('Greatest Hits', D + 'Queen/Greatest Hits/01.mp3')).not.toBe(
      albumGroupKey('Greatest Hits', D + 'ABBA/Greatest Hits/01.mp3')
    )
  })

  it('D: merges multi-disc albums (subfolders and title markers)', () => {
    expect(albumGroupKey('The Wall (CD1)', D + 'Pink Floyd/The Wall/CD1/01.mp3')).toBe(
      albumGroupKey('The Wall (CD2)', D + 'Pink Floyd/The Wall/CD2/01.mp3')
    )
    expect(albumGroupKey('The Wall (Disc 1)', D + 'Pink Floyd/The Wall/01.mp3')).toBe(
      albumGroupKey('The Wall (Disc 2)', D + 'Pink Floyd/The Wall/05.mp3')
    )
  })

  it('I: known limitation — different albums sharing title in ONE folder merge', () => {
    expect(albumGroupKey('Hits', D + 'Downloads/Hits/a.mp3')).toBe(
      albumGroupKey('Hits', D + 'Downloads/Hits/b.mp3')
    )
  })
})

describe('albumKey.normalizeKeyText (hardened folding)', () => {
  it('folds case + diacritics and collapses surrounding/internal whitespace', () => {
    expect(normalizeKeyText('  Café   BLEU  ')).toBe('cafe bleu')
  })

  it('unifies curly quotes, dash variants and ellipsis so they never split a release', () => {
    const f = D + 'Artist/Album/x.mp3'
    // straight vs curly apostrophe
    expect(albumGroupKey("Don't Stop", f)).toBe(albumGroupKey('Don’t Stop', f))
    // double space vs single
    expect(albumGroupKey('A  B', f)).toBe(albumGroupKey('A B', f))
    // en/em-dash vs hyphen
    expect(albumGroupKey('Rock – Roll', f)).toBe(albumGroupKey('Rock - Roll', f))
    expect(albumGroupKey('Rock — Roll', f)).toBe(albumGroupKey('Rock - Roll', f))
    // ellipsis char vs three dots
    expect(albumGroupKey('Etc…', f)).toBe(albumGroupKey('Etc...', f))
  })
})

describe('albumKey.buildAlbumGroups (Persistent ID merge — merge, never split)', () => {
  const t = (over: Partial<AlbumAggInput>): AlbumAggInput => ({
    album_key: 'k',
    album: 'Album',
    album_artist: null,
    artist: 'Artist',
    year: null,
    cover_art_hash: null,
    ...over
  })

  it('merges two different folders that share a MusicBrainz release-group id', () => {
    const { albums, remap } = buildAlbumGroups([
      t({ album_key: 'base-a', mb_release_group_id: 'RG1' }),
      t({ album_key: 'base-b', mb_release_group_id: 'RG1' })
    ])
    expect(albums).toHaveLength(1)
    expect(albums[0].album_key).toBe('mbrg:RG1')
    expect(albums[0].total_tracks).toBe(2)
    expect(remap.get('base-a')).toBe('mbrg:RG1')
    expect(remap.get('base-b')).toBe('mbrg:RG1')
  })

  it('does NOT split when only some tracks of a base group carry the id (partial tagging)', () => {
    const { albums } = buildAlbumGroups([
      t({ album_key: 'base-a', mb_release_group_id: 'RG1' }),
      t({ album_key: 'base-a', mb_release_group_id: null }),
      t({ album_key: 'base-a', mb_release_group_id: null })
    ])
    expect(albums).toHaveLength(1)
    expect(albums[0].total_tracks).toBe(3)
    expect(albums[0].album_key).toBe('mbrg:RG1')
  })

  it('keeps different releases apart (distinct ids never merge)', () => {
    const { albums } = buildAlbumGroups([
      t({ album_key: 'base-a', mb_release_group_id: 'RG1' }),
      t({ album_key: 'base-b', mb_release_group_id: 'RG2' })
    ])
    expect(albums).toHaveLength(2)
  })

  it('merges via release id and via Spotify album id, populating album columns', () => {
    const byRelease = buildAlbumGroups([
      t({ album_key: 'a', mb_release_id: 'REL1' }),
      t({ album_key: 'b', mb_release_id: 'REL1' })
    ])
    expect(byRelease.albums).toHaveLength(1)
    expect(byRelease.albums[0].album_key).toBe('mbr:REL1')
    expect(byRelease.albums[0].mb_album_id).toBe('REL1')

    const bySpotify = buildAlbumGroups([
      t({ album_key: 'a', spotify_album_id: 'SP1' }),
      t({ album_key: 'b', spotify_album_id: 'SP1' })
    ])
    expect(bySpotify.albums).toHaveLength(1)
    expect(bySpotify.albums[0].album_key).toBe('sp:SP1')
    expect(bySpotify.albums[0].spotify_id).toBe('SP1')
  })

  it('release-group id wins over release id (merges editions of one album)', () => {
    const { albums } = buildAlbumGroups([
      t({ album_key: 'a', mb_release_group_id: 'RG1', mb_release_id: 'REL1' }),
      t({ album_key: 'b', mb_release_group_id: 'RG1', mb_release_id: 'REL2' })
    ])
    expect(albums).toHaveLength(1)
    expect(albums[0].album_key).toBe('mbrg:RG1')
  })

  it('falls back to the lexicographically smallest base key when no id is present', () => {
    const { albums, remap } = buildAlbumGroups([t({ album_key: 'zzz' }), t({ album_key: 'aaa' })])
    expect(albums.map((a) => a.album_key).sort()).toEqual(['aaa', 'zzz'])
    expect(remap.get('aaa')).toBe('aaa')
    expect(remap.get('zzz')).toBe('zzz')
  })
})

describe('albumKey.pickCanonical / canonicalAlbumArtist', () => {
  it('F: drops the feat. guest, keeping the dominant base artist', () => {
    expect(
      pickCanonical([
        { artist: 'Daft Punk' },
        { artist: 'Daft Punk, Pharrell Williams' },
        { artist: 'Daft Punk' }
      ])
    ).toBe('Daft Punk')
  })

  it('E: honours album_artist (Various Artists) for compilations', () => {
    expect(
      canonicalAlbumArtist([
        { album_artist: 'Various Artists', artist: 'A' },
        { album_artist: 'Various Artists', artist: 'B' },
        { album_artist: null, artist: 'C' }
      ])
    ).toBe('Various Artists')
  })

  it('breaks frequency ties by the shorter value then alphabetically', () => {
    expect(pickCanonical([{ artist: 'Daft Punk, Pharrell Williams' }, { artist: 'Daft Punk' }])).toBe(
      'Daft Punk'
    )
  })

  it('ignores empty/blank artists', () => {
    expect(pickCanonical([{ artist: '' }, { artist: 'Radiohead' }, { artist: null }])).toBe(
      'Radiohead'
    )
  })
})

describe('albumKey.aggregateAlbums', () => {
  const t = (over: Partial<AlbumAggInput>): AlbumAggInput => ({
    album_key: 'k',
    album: 'Album',
    album_artist: null,
    artist: 'Artist',
    year: null,
    cover_art_hash: null,
    ...over
  })

  it('produces one album per key with canonical title/artist and aggregate fields', () => {
    const albums = aggregateAlbums([
      t({ album_key: 'a', album: 'Thriller', album_artist: 'Michael Jackson', year: 1982, cover_art_hash: 'h1' }),
      t({ album_key: 'a', album: 'Thriller', album_artist: 'Michael Jackson feat. X', year: 1983 }),
      t({ album_key: 'b', album: 'Bad', album_artist: 'Michael Jackson', year: 1987 })
    ])
    expect(albums).toHaveLength(2)
    const a = albums.find((x) => x.album_key === 'a')!
    expect(a.title).toBe('Thriller')
    expect(a.artist).toBe('Michael Jackson') // dominant, drops feat.
    expect(a.total_tracks).toBe(2)
    expect(a.year).toBe(1983) // max
    expect(a.cover_art_hash).toBe('h1') // only hash present
  })
})

describe('albumKey.pickAlbumCover', () => {
  const m = (over: Partial<AlbumAggInput>): AlbumAggInput => ({
    album_key: 'k',
    album: 'Album',
    album_artist: null,
    artist: 'Artist',
    year: null,
    cover_art_hash: null,
    ...over
  })

  it('returns null when no member has a cover', () => {
    expect(pickAlbumCover([m({}), m({})])).toBeNull()
  })

  it('prefers tag provenance over provider and caa', () => {
    const hash = pickAlbumCover([
      m({ cover_art_hash: 'caa1', cover_source: 'caa' }),
      m({ cover_art_hash: 'prov1', cover_source: 'provider' }),
      m({ cover_art_hash: 'tag1', cover_source: 'tag' })
    ])
    expect(hash).toBe('tag1')
  })

  it('breaks provenance ties by member majority', () => {
    const hash = pickAlbumCover([
      m({ cover_art_hash: 'a', cover_source: 'tag' }),
      m({ cover_art_hash: 'b', cover_source: 'tag' }),
      m({ cover_art_hash: 'b', cover_source: 'tag' })
    ])
    expect(hash).toBe('b')
  })

  it('breaks majority ties by pixel area, then hash (deterministic)', () => {
    const byPixels = pickAlbumCover([
      m({ cover_art_hash: 'small', cover_source: 'tag', cover_w: 300, cover_h: 300 }),
      m({ cover_art_hash: 'big', cover_source: 'tag', cover_w: 1000, cover_h: 1000 })
    ])
    expect(byPixels).toBe('big')
    const byHash = pickAlbumCover([
      m({ cover_art_hash: 'zz', cover_source: 'tag', cover_w: 500, cover_h: 500 }),
      m({ cover_art_hash: 'aa', cover_source: 'tag', cover_w: 500, cover_h: 500 })
    ])
    expect(byHash).toBe('aa')
  })

  it('an unknown-provenance majority still loses to a single tagged cover', () => {
    const hash = pickAlbumCover([
      m({ cover_art_hash: 'u', cover_source: 'unknown' }),
      m({ cover_art_hash: 'u', cover_source: 'unknown' }),
      m({ cover_art_hash: 'u' }),
      m({ cover_art_hash: 't', cover_source: 'tag' })
    ])
    expect(hash).toBe('t')
  })

  it('order of members never changes the outcome', () => {
    const members = [
      m({ cover_art_hash: 'x', cover_source: 'caa', cover_w: 800, cover_h: 800 }),
      m({ cover_art_hash: 'y', cover_source: 'provider', cover_w: 400, cover_h: 400 }),
      m({ cover_art_hash: 'x', cover_source: 'caa' })
    ]
    const forward = pickAlbumCover(members)
    const backward = pickAlbumCover(members.slice().reverse())
    expect(forward).toBe(backward)
    expect(forward).toBe('y') // provider outranks caa despite fewer members
  })
})
