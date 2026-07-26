import { afterAll, beforeAll, describe, expect, it } from 'vitest'
import {
  createServer,
  get,
  type IncomingMessage,
  type Server,
  type ServerResponse
} from 'node:http'
import type { AddressInfo } from 'node:net'
import { proxyStream } from './proxyStream'

// Exercises the /stream proxy's upstream-idle timeout: a stalled origin
// (connected but never sending) must produce a 504 instead of hanging the
// client forever, and a mid-stream stall must tear the response down instead
// of leaving ExoPlayer waiting. Healthy pass-through is asserted too so the
// timeout can't regress the happy path.

function listen(handler: (req: IncomingMessage, res: ServerResponse) => void): Promise<Server> {
  return new Promise((resolve) => {
    const s = createServer(handler)
    s.listen(0, '127.0.0.1', () => resolve(s))
  })
}

const port = (s: Server): number => (s.address() as AddressInfo).port

interface RawResult {
  status: number
  body: string
  /** false when the connection died before the message completed (mid-stream stall). */
  complete: boolean
}

function fetchRaw(url: string): Promise<RawResult> {
  return new Promise((resolve, reject) => {
    const req = get(url, (res) => {
      const chunks: Buffer[] = []
      res.on('data', (c: Buffer) => chunks.push(c))
      const done = (): void =>
        resolve({
          status: res.statusCode ?? 0,
          body: Buffer.concat(chunks).toString('utf8'),
          complete: res.complete
        })
      res.on('end', done)
      // A destroyed response emits 'aborted'/'error' (no 'end'); still resolve
      // so the test can assert complete === false.
      res.on('aborted', done)
      res.on('error', done)
    })
    req.on('error', reject)
  })
}

let proxy: Server
let healthy: Server
let blackhole: Server
let staller: Server
let proxyUrl: (target: string) => string

beforeAll(async () => {
  process.env['AETHER_STREAM_TIMEOUT_MS'] = '300'

  healthy = await listen((_req, res) => {
    res.writeHead(200, { 'Content-Type': 'audio/mpeg' })
    res.end('AUDIOBYTES')
  })
  // Accepts the request and never answers → idle socket → proxy must 504.
  blackhole = await listen(() => {
    /* never respond */
  })
  // Sends headers + one chunk, then stalls forever → proxy must destroy the response.
  staller = await listen((_req, res) => {
    res.writeHead(200, { 'Content-Type': 'audio/mpeg' })
    res.write('PARTIAL')
    /* never end */
  })

  proxy = await listen((req, res) => {
    const target = new URL(req.url ?? '/', 'http://x').searchParams.get('url') ?? ''
    proxyStream(target, req, res)
  })
  proxyUrl = (target) =>
    `http://127.0.0.1:${port(proxy)}/stream?url=${encodeURIComponent(target)}`
})

afterAll(async () => {
  delete process.env['AETHER_STREAM_TIMEOUT_MS']
  for (const s of [proxy, healthy, blackhole, staller]) {
    // Stalled sockets keep close() pending forever; force-close them first
    // (Node 18.2+; this test suite runs on the dev machine's modern Node).
    ;(s as unknown as { closeAllConnections?: () => void }).closeAllConnections?.()
    await new Promise<void>((resolve) => s.close(() => resolve()))
  }
}, 10_000)

describe('proxyStream upstream timeout', () => {
  it('passes a healthy upstream through untouched', async () => {
    const r = await fetchRaw(proxyUrl(`http://127.0.0.1:${port(healthy)}/ep.mp3`))
    expect(r.status).toBe(200)
    expect(r.body).toBe('AUDIOBYTES')
    expect(r.complete).toBe(true)
  })

  it('answers 504 when the upstream never responds', async () => {
    const r = await fetchRaw(proxyUrl(`http://127.0.0.1:${port(blackhole)}/dead.mp3`))
    expect(r.status).toBe(504)
    expect(r.body).toBe('Gateway timeout')
  })

  it('tears down the response when the upstream stalls mid-stream', async () => {
    const r = await fetchRaw(proxyUrl(`http://127.0.0.1:${port(staller)}/stall.mp3`))
    expect(r.status).toBe(200) // headers were already relayed
    expect(r.complete).toBe(false) // …but the body was cut, not left hanging
  })
})
