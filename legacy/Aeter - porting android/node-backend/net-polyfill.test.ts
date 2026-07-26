import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import { createServer, type IncomingMessage, type Server, type ServerResponse } from 'http'
import { AddressInfo } from 'net'
import zlib from 'zlib'
import { polyFetch } from './net-polyfill'

// Exercises the nodejs-mobile fetch polyfill's transparent decompression.
// Node's core http never inflates Content-Encoding (only undici/WHATWG fetch
// does), so the enrichment/cover/Spotify code — which all run through this
// polyfill on the device — would otherwise see raw gzip/br/deflate bytes.
//
// polyFetch is tested directly: on this (modern) Node the real global.fetch
// exists, so installing the polyfill is a no-op and we must call it explicitly.

interface PolyResponse {
  ok: boolean
  status: number
  headers: { get(name: string): string | null }
  text(): Promise<string>
  json(): Promise<unknown>
  arrayBuffer(): Promise<ArrayBuffer>
}

const PAYLOAD = { x: 42, name: 'Björk — Jóga', list: [1, 2, 3] }
let lastAcceptEncoding: string | undefined

/** Serves the JSON payload encoded per the `?enc=` query (or identity). */
function handler(req: IncomingMessage, res: ServerResponse): void {
  lastAcceptEncoding = req.headers['accept-encoding'] as string | undefined
  const enc = new URL(req.url ?? '/', 'http://x').searchParams.get('enc') ?? 'identity'
  const json = Buffer.from(JSON.stringify(PAYLOAD), 'utf8')
  if (enc === 'gzip') {
    res.writeHead(200, { 'Content-Encoding': 'gzip', 'Content-Type': 'application/json' })
    res.end(zlib.gzipSync(json))
  } else if (enc === 'br') {
    res.writeHead(200, { 'Content-Encoding': 'br', 'Content-Type': 'application/json' })
    res.end(zlib.brotliCompressSync(json))
  } else if (enc === 'deflate') {
    res.writeHead(200, { 'Content-Encoding': 'deflate', 'Content-Type': 'application/json' })
    res.end(zlib.deflateSync(json))
  } else {
    res.writeHead(200, { 'Content-Type': 'application/json' })
    res.end(json)
  }
}

let server: Server
let base: string

beforeAll(async () => {
  server = createServer(handler)
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`
})

afterAll(async () => {
  await new Promise<void>((resolve) => server.close(() => resolve()))
})

describe('net-polyfill fetch decompression', () => {
  it('advertises gzip/deflate/br via Accept-Encoding', async () => {
    await polyFetch(`${base}/?enc=identity`)
    expect(lastAcceptEncoding ?? '').toContain('gzip')
    expect(lastAcceptEncoding ?? '').toContain('br')
  })

  it('decodes a gzip-encoded JSON body', async () => {
    const res = (await polyFetch(`${base}/?enc=gzip`)) as PolyResponse
    expect(res.ok).toBe(true)
    expect(await res.json()).toEqual(PAYLOAD)
  })

  it('decodes a brotli-encoded JSON body', async () => {
    const res = (await polyFetch(`${base}/?enc=br`)) as PolyResponse
    expect(await res.json()).toEqual(PAYLOAD)
  })

  it('decodes a deflate-encoded JSON body', async () => {
    const res = (await polyFetch(`${base}/?enc=deflate`)) as PolyResponse
    expect(await res.json()).toEqual(PAYLOAD)
  })

  it('leaves an identity body untouched (regression)', async () => {
    const res = (await polyFetch(`${base}/?enc=identity`)) as PolyResponse
    expect(await res.text()).toBe(JSON.stringify(PAYLOAD))
  })

  it('strips content-encoding from the decoded response headers', async () => {
    const res = (await polyFetch(`${base}/?enc=gzip`)) as PolyResponse
    expect(res.headers.get('content-encoding')).toBeNull()
  })

  it('exposes the decoded bytes via arrayBuffer()', async () => {
    const res = (await polyFetch(`${base}/?enc=br`)) as PolyResponse
    const buf = Buffer.from(await res.arrayBuffer())
    expect(JSON.parse(buf.toString('utf8'))).toEqual(PAYLOAD)
  })
})
