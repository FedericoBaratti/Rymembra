import { describe, expect, it } from 'vitest'
import { AppError, isAppErrorPayload } from './appError'
import { CATALOG, ERROR_CODES, legacyCodeToErrorCode } from './catalog'
import { missingI18nKeys, requiredI18nKeys } from './i18nKeys'

describe('catalogo', () => {
  it('dichiara dominio e gravità per ogni codice', () => {
    for (const code of ERROR_CODES) {
      const meta = CATALOG[code]
      expect(meta.domain, code).toBeTruthy()
      expect(meta.severity, code).toBeTruthy()
      expect(['boolean', 'function'], code).toContain(typeof meta.retryable)
    }
  })

  it('usa il dominio come primo segmento del codice', () => {
    // Se le due cose divergono, il routing dei log e i filtri della UI mentono.
    for (const code of ERROR_CODES) {
      expect(code.split('.')[0], code).toBe(CATALOG[code].domain)
    }
  })

  it('non riusa lo stesso codice legacy per due codici nuovi', () => {
    const seen = new Map<string, string>()
    for (const code of ERROR_CODES) {
      const legacy = CATALOG[code].legacy
      if (legacy === undefined) continue
      expect(seen.has(legacy), `${legacy} duplicato: ${seen.get(legacy)} e ${code}`).toBe(false)
      seen.set(legacy, code)
    }
  })

  it('copre tutti i 33 codici stringa del legacy', () => {
    // L'elenco è preso da src/lib/ipcError.ts del legacy: 25 SIMPLE_CODES,
    // 7 PARAM_CODES e BINARY_MISSING. La riscrittura non deve perderne nessuno,
    // perché sono anche persistiti nelle righe di download in SQLite.
    const legacyCodes = [
      'DL_UNRECOGNIZED_URL', 'DL_AGE_RESTRICTED', 'DL_UNAVAILABLE', 'DL_RATE_LIMITED',
      'DL_RATE_LIMITED_RETRY', 'DL_FORBIDDEN', 'DL_NETWORK', 'EXT_SEARCH_FAILED',
      'DL_INVALID_URL', 'DL_PRIVATE', 'DL_FAILED', 'DL_NO_RESULTS', 'DL_INVALID_FILES',
      'DL_YTDLP_TIMEOUT', 'DL_YTDLP_BAD_RESPONSE', 'YTDLP_CORRUPTED', 'YTDLP_BUSY',
      'TRACK_NOT_FOUND', 'SMART_RULES_INVALID', 'LASTFM_NOT_CONFIGURED',
      'LASTFM_NO_PENDING_TOKEN', 'ENRICH_NO_MATCH', 'ENRICH_NEEDS_REVIEW',
      'ENRICH_MB_UNAVAILABLE', 'DL_SPOTDL_EXIT', 'DL_YT_ERROR', 'SPOTIFY_AUTH_FAILED',
      'TAG_VERIFY_FAILED', 'SMART_FIELD_INVALID', 'SMART_OP_INVALID', 'ENRICH_FOUND',
      'BINARY_MISSING'
    ]
    const unmapped = legacyCodes.filter((c) => legacyCodeToErrorCode(c) === undefined)
    expect(unmapped).toEqual([])
  })
})

describe('AppError.of', () => {
  it('legge dominio, gravità e ritentabilità dal catalogo', () => {
    const e = AppError.of('net.rateLimited', { service: 'lastfm', retryAfterMs: 5000 })
    expect(e.code).toBe('net.rateLimited')
    expect(e.domain).toBe('net')
    expect(e.retryable).toBe(true)
    expect(e.params).toEqual({ service: 'lastfm', retryAfterMs: 5000 })
  })

  it('valuta la ritentabilità dai parametri quando dipende da essi', () => {
    // È il caso HTTP: 429 e 5xx si ritentano, 404 no. Nel legacy questa
    // decisione era sparsa fra isRetryableError e classifyDownloadFailure, con
    // default divergenti fra desktop e mobile.
    expect(AppError.of('net.http', { status: 429 }).retryable).toBe(true)
    expect(AppError.of('net.http', { status: 503 }).retryable).toBe(true)
    expect(AppError.of('net.http', { status: 408 }).retryable).toBe(true)
    expect(AppError.of('net.http', { status: 404 }).retryable).toBe(false)
    expect(AppError.of('net.http', { status: 401 }).retryable).toBe(false)
  })

  it('ammette di omettere i parametri per i codici che non ne hanno', () => {
    const e = AppError.of('download.ageRestricted')
    expect(e.params).toEqual({})
    expect(e.message).toBe('[download.ageRestricted]')
  })

  it('produce un messaggio diagnostico leggibile', () => {
    const e = AppError.of('net.http', { status: 500, url: 'https://x.test/a' })
    expect(e.message).toContain('[net.http]')
    expect(e.message).toContain('status=500')
    expect(e.message).toContain('url=https://x.test/a')
  })

  it('resta un Error lanciabile con stack', () => {
    const e = AppError.of('db.locked')
    expect(e).toBeInstanceOf(Error)
    expect(e.stack).toBeTruthy()
    expect(() => { throw e }).toThrow()
  })

  it('assegna un traceId diverso a ogni errore', () => {
    const a = AppError.of('db.locked')
    const b = AppError.of('db.locked')
    expect(a.traceId).not.toBe(b.traceId)
  })
})

describe('AppError.from — accetta qualunque cosa senza lanciare', () => {
  it('restituisce lo stesso AppError se già lo è', () => {
    const original = AppError.of('net.offline', {})
    expect(AppError.from(original)).toBe(original)
  })

  it('mappa gli errno di Node sul dominio fs', () => {
    // È il guadagno concreto: nel legacy un ENOSPC in scrittura tag finiva in
    // logWarn e in UI diventava una stringa generica.
    const enoent = Object.assign(new Error('no such file'), {
      code: 'ENOENT',
      path: 'C:/musica/x.flac'
    })
    const mapped = AppError.from(enoent)
    expect(mapped.code).toBe('fs.notFound')
    expect(mapped.params).toEqual({ path: 'C:/musica/x.flac' })
    expect(mapped.causes[0]?.kind).toBe('ENOENT')

    expect(AppError.from(Object.assign(new Error('x'), { code: 'ENOSPC' })).code)
      .toBe('fs.diskFull')
    expect(AppError.from(Object.assign(new Error('x'), { code: 'EACCES' })).code)
      .toBe('fs.permissionDenied')
    expect(AppError.from(Object.assign(new Error('x'), { code: 'ECONNREFUSED' })).code)
      .toBe('net.offline')
    expect(AppError.from(Object.assign(new Error('x'), { code: 'ETIMEDOUT' })).code)
      .toBe('net.timeout')
  })

  it('riconosce i codici stringa del legacy, anche con parametri', () => {
    expect(AppError.from('DL_FORBIDDEN').code).toBe('download.forbidden')

    const binary = AppError.from('BINARY_MISSING:yt-dlp:resources/bin')
    expect(binary.code).toBe('download.binaryMissing')
    expect(binary.params).toEqual({ name: 'yt-dlp', dir: 'resources/bin' })

    const noDir = AppError.from('BINARY_MISSING:fpcalc')
    expect(noDir.params).toEqual({ name: 'fpcalc' })

    const yt = AppError.from('DL_YT_ERROR:Video unavailable')
    expect(yt.code).toBe('download.ytError')
    expect(yt.params).toEqual({ detail: 'Video unavailable' })
  })

  it('riconosce un codice legacy anche dentro il messaggio di un Error', () => {
    // Il vecchio confine IPC rilanciava `new Error(codice)`: quei valori
    // esistono ancora nei dati e nei log.
    const e = AppError.from(new Error('YTDLP_BUSY'))
    expect(e.code).toBe('download.ytdlpBusy')
  })

  it('degrada a internal.unexpected conservando il dettaglio', () => {
    const e = AppError.from(new Error('qualcosa di imprevisto'))
    expect(e.code).toBe('internal.unexpected')
    expect(e.params).toEqual({ detail: 'qualcosa di imprevisto' })
    expect(e.causes[0]?.message).toBe('qualcosa di imprevisto')
  })

  it.each([
    ['null', null],
    ['undefined', undefined],
    ['numero', 42],
    ['booleano', false],
    ['oggetto nudo', { a: 1 }],
    ['array', [1, 2]],
    ['simbolo-ish', Symbol.iterator.toString()]
  ])('non lancia mai su %s', (_label, value) => {
    const e = AppError.from(value)
    expect(e).toBeInstanceOf(AppError)
    expect(e.code).toBe('internal.unexpected')
    expect(typeof e.message).toBe('string')
  })

  it('sopravvive a una catena di cause circolare', () => {
    const a = new Error('a') as Error & { cause?: unknown }
    const b = new Error('b') as Error & { cause?: unknown }
    a.cause = b
    b.cause = a
    const e = AppError.from(a)
    expect(e.causes.length).toBeLessThanOrEqual(8)
  })

  it('non lancia su un oggetto con riferimenti circolari', () => {
    const circular: Record<string, unknown> = { name: 'x' }
    circular['self'] = circular
    const e = AppError.from(circular)
    expect(e.code).toBe('internal.unexpected')
    expect(String(e.params['detail'])).toContain('Circular')
  })
})

describe('round-trip attraverso il confine IPC', () => {
  it('conserva i campi ricchi — il caso che il legacy perdeva', () => {
    const original = AppError.of(
      'net.rateLimited',
      { service: 'musicbrainz', retryAfterMs: 12_000 },
      { context: { channel: 'enrichTrack', trackId: 91 }, cause: new Error('429 dal server') }
    )

    // Passaggio effettivo attraverso JSON: è ciò che fa l'IPC.
    const wire = JSON.parse(JSON.stringify(original.toPayload())) as unknown
    expect(isAppErrorPayload(wire)).toBe(true)

    const revived = AppError.fromPayload(wire as never)
    expect(revived.code).toBe('net.rateLimited')
    expect(revived.params['retryAfterMs']).toBe(12_000)
    expect(revived.params['service']).toBe('musicbrainz')
    expect(revived.retryable).toBe(true)
    expect(revived.severity).toBe('warning')
    expect(revived.domain).toBe('net')
    expect(revived.traceId).toBe(original.traceId)
    expect(revived.context?.['trackId']).toBe(91)
    expect(revived.causes[0]?.message).toBe('429 dal server')
    expect(revived.i18nKey).toBe('errors.net.rateLimited')
  })

  it('conserva la ritentabilità calcolata dallo status HTTP', () => {
    const notFound = AppError.of('net.http', { status: 404, url: 'https://x.test' })
    const revived = AppError.fromPayload(
      JSON.parse(JSON.stringify(notFound.toPayload())) as never
    )
    expect(revived.retryable).toBe(false)
    expect(revived.params['status']).toBe(404)
  })

  it('conserva lo stack del processo di origine', () => {
    const original = AppError.of('db.queryFailed', { detail: 'near "SELCT"' })
    const revived = AppError.fromPayload(
      JSON.parse(JSON.stringify(original.toPayload())) as never
    )
    expect(revived.stack).toBe(original.stack)
  })

  it('degrada un codice sconosciuto senza fallire', () => {
    // Scenario reale: un telefono accoppiato con una build più nuova manda un
    // codice che questa versione non conosce.
    const fromFuture = {
      __aetherError: true as const,
      code: 'quantum.entangled' as never,
      domain: 'net' as const,
      severity: 'warning' as const,
      retryable: true,
      params: { foo: 'bar' },
      i18nKey: 'errors.quantum.entangled',
      message: '[quantum.entangled] foo=bar',
      traceId: 'abc-1-000000'
    }
    const revived = AppError.fromPayload(fromFuture)
    expect(revived.code).toBe('internal.unexpected')
    expect(revived.context?.['unknownCode']).toBe('quantum.entangled')
    expect(revived.retryable).toBe(true)
  })

  it('withContext aggiunge dati senza perdere identità né catena', () => {
    const base = AppError.of('fs.writeFailed', { path: 'a.flac' }, { cause: new Error('EIO') })
    const enriched = base.withContext({ attempt: 2 })
    expect(enriched.code).toBe(base.code)
    expect(enriched.traceId).toBe(base.traceId)
    expect(enriched.params).toEqual({ path: 'a.flac' })
    expect(enriched.context).toEqual({ attempt: 2 })
    expect(enriched.causes).toEqual(base.causes)
  })
})

describe('chiavi i18n', () => {
  it('deriva una chiave per ogni codice, senza duplicati', () => {
    const keys = requiredI18nKeys()
    expect(keys.length).toBe(ERROR_CODES.length)
    expect(new Set(keys).size).toBe(keys.length)
    expect(keys.every((k) => k.startsWith('errors.'))).toBe(true)
  })

  it('riconosce le chiavi mancanti in un bundle, piatte o annidate', () => {
    expect(missingI18nKeys({})).toEqual([...ERROR_CODES].sort())

    const nested = { errors: { net: { http: 'Errore del server ({{status}})' } } }
    expect(missingI18nKeys(nested)).not.toContain('net.http')

    const flat = { 'errors.net.http': 'Errore del server' }
    expect(missingI18nKeys(flat)).not.toContain('net.http')

    // Una chiave presente ma vuota vale come mancante: in UI si vedrebbe nulla.
    expect(missingI18nKeys({ 'errors.net.http': '' })).toContain('net.http')
  })
})
