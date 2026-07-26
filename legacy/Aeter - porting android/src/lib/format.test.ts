import { describe, it, expect } from 'vitest'
import { formatDuration, formatLongDuration, formatBytes, coverUrl, mediaUrl } from './format'
import { ipcErrorMessage } from './ipcError'

describe('formatDuration', () => {
  it('formats minutes and seconds', () => {
    expect(formatDuration(0)).toBe('0:00')
    expect(formatDuration(59)).toBe('0:59')
    expect(formatDuration(61)).toBe('1:01')
    expect(formatDuration(600)).toBe('10:00')
  })

  it('includes hours past 3600s', () => {
    expect(formatDuration(3600)).toBe('1:00:00')
    expect(formatDuration(3725)).toBe('1:02:05')
  })

  it('handles invalid input', () => {
    expect(formatDuration(-5)).toBe('0:00')
    expect(formatDuration(NaN)).toBe('0:00')
    expect(formatDuration(Infinity)).toBe('0:00')
  })
})

describe('formatLongDuration', () => {
  it('formats minutes-only below an hour', () => {
    expect(formatLongDuration(120)).toBe('2 min')
  })

  it('formats hours and minutes', () => {
    expect(formatLongDuration(3660)).toBe('1 h 1 min')
  })
})

describe('formatBytes', () => {
  it('keeps small values in bytes', () => {
    expect(formatBytes(0)).toBe('0 B')
    expect(formatBytes(1023)).toBe('1023 B')
  })

  it('scales units', () => {
    expect(formatBytes(1024)).toBe('1.0 KB')
    expect(formatBytes(1024 * 1024)).toBe('1.0 MB')
    expect(formatBytes(5.5 * 1024 * 1024 * 1024)).toBe('5.5 GB')
  })
})

describe('coverUrl / mediaUrl', () => {
  it('builds aether:// URLs', () => {
    expect(coverUrl('abc')).toBe('aether://art/abc')
    expect(coverUrl('abc', true)).toBe('aether://art/abc?thumb=1')
    expect(coverUrl(null)).toBeNull()
    expect(mediaUrl(7)).toBe('aether://media/7')
  })
})

describe('ipcErrorMessage', () => {
  it('strips the Electron invoke prefix', () => {
    expect(
      ipcErrorMessage(new Error("Error invoking remote method 'previewDownload': Error: URL non valido"))
    ).toBe('URL non valido')
  })

  it('strips typed error prefixes', () => {
    expect(
      ipcErrorMessage(new Error("Error invoking remote method 'startDownload': DownloadError: rete assente"))
    ).toBe('rete assente')
  })

  it('passes plain messages through', () => {
    expect(ipcErrorMessage(new Error('semplice'))).toBe('semplice')
    expect(ipcErrorMessage('stringa')).toBe('stringa')
  })

  it('translates simple stable codes (default lang: it)', () => {
    expect(ipcErrorMessage(new Error('DL_PRIVATE'))).toBe('Video privato.')
    expect(ipcErrorMessage(new Error('ENRICH_NO_MATCH'))).toBe(
      'Nessuna corrispondenza trovata su MusicBrainz.'
    )
    expect(ipcErrorMessage(new Error('LASTFM_NO_PENDING_TOKEN'))).toBe(
      'Avvia prima la connessione a Last.fm.'
    )
  })

  it('translates parameterized codes', () => {
    expect(ipcErrorMessage(new Error('DL_SPOTDL_EXIT:2'))).toBe(
      'spotdl terminato con errore (codice 2).'
    )
    expect(ipcErrorMessage(new Error('TAG_VERIFY_FAILED:title, artist'))).toBe(
      'Scrittura dei tag non verificata: title, artist'
    )
    expect(ipcErrorMessage(new Error('ENRICH_FOUND:Aphex Twin — Xtal'))).toBe(
      'Trovato: Aphex Twin — Xtal'
    )
  })

  it('translates codes behind the Electron invoke prefix', () => {
    expect(
      ipcErrorMessage(new Error("Error invoking remote method 'previewDownload': Error: DL_UNRECOGNIZED_URL"))
    ).toBe('URL non riconosciuto. Incolla un link Spotify o YouTube valido.')
  })

  it('leaves legacy persisted Italian messages verbatim', () => {
    expect(ipcErrorMessage(new Error('Video non disponibile o rimosso.'))).toBe(
      'Video non disponibile o rimosso.'
    )
  })
})
