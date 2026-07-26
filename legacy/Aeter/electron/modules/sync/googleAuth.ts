/**
 * Google OAuth2 for the Drive sync — PKCE + loopback redirect, the one flow that
 * works on both desktop Electron and nodejs-mobile (the backend already runs a
 * local media server, and `shell.openExternal` is routed to a Custom Tab on
 * Android). No client secret ever leaves the device beyond the token request.
 *
 * `runLoopbackAuth` performs the interactive connect; `ensureAccessToken`
 * refreshes and caches a short-lived access token for the Drive client.
 */
import { shell } from 'electron'
import { createServer, type Server } from 'node:http'
import { createHash, randomBytes } from 'node:crypto'
import { getSettings } from '../settings'
import {
  GOOGLE_AUTH_ENDPOINT,
  GOOGLE_REVOKE_ENDPOINT,
  GOOGLE_SCOPES,
  GOOGLE_TOKEN_ENDPOINT,
  GOOGLE_USERINFO_ENDPOINT,
  getGoogleClientId,
  getGoogleClientSecret,
  isOAuthConfigured
} from './googleOAuthConfig'
import { withRetry } from '../net/retry'
import { NetworkError } from '../net/errors'

const AUTH_TIMEOUT_MS = 5 * 60_000

function base64url(buf: Buffer): string {
  return buf.toString('base64').replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
}

function resultPage(title: string, body: string): string {
  return `<!doctype html><html lang="it"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${title}</title><style>
body{font-family:system-ui,-apple-system,Segoe UI,Roboto,sans-serif;background:#0f0f12;
color:#eee;display:flex;min-height:100vh;align-items:center;justify-content:center;margin:0}
.card{max-width:26rem;padding:2rem 2.25rem;background:#1a1a1f;border-radius:14px;text-align:center}
h1{font-size:1.15rem;margin:0 0 .5rem}p{color:#aaa;margin:0;line-height:1.5}
</style></head><body><div class="card"><h1>${title}</h1><p>${body}</p></div></body></html>`
}

/**
 * Start an ephemeral loopback HTTP server and resolve with the OAuth code once
 * Google redirects back. Times out after 5 minutes.
 */
function startLoopbackServer(
  expectedState: string
): Promise<{ server: Server; port: number; code: Promise<string> }> {
  return new Promise((resolve, reject) => {
    let settle!: (code: string) => void
    let fail!: (err: Error) => void
    const code = new Promise<string>((res, rej) => {
      settle = res
      fail = rej
    })

    const server = createServer((req, httpRes) => {
      const url = new URL(req.url ?? '/', 'http://127.0.0.1')
      if (!url.pathname.startsWith('/callback')) {
        httpRes.statusCode = 404
        httpRes.end('Not found')
        return
      }
      const err = url.searchParams.get('error')
      const returnedCode = url.searchParams.get('code')
      const state = url.searchParams.get('state')
      httpRes.setHeader('Content-Type', 'text/html; charset=utf-8')
      if (err || !returnedCode || state !== expectedState) {
        httpRes.statusCode = 400
        httpRes.end(resultPage('Autenticazione non riuscita', 'Puoi chiudere questa scheda e riprovare da Aether.'))
        clearTimeout(timer)
        fail(new Error(err ? `DRIVE_AUTH_ERROR_${err}` : 'DRIVE_AUTH_BAD_CALLBACK'))
        return
      }
      httpRes.statusCode = 200
      httpRes.end(resultPage('Autenticazione completata', 'Torna su Aether: la sincronizzazione è pronta.'))
      clearTimeout(timer)
      settle(returnedCode)
    })

    const timer = setTimeout(() => {
      fail(new Error('DRIVE_AUTH_TIMEOUT'))
      server.close()
    }, AUTH_TIMEOUT_MS)
    timer.unref?.()

    server.on('error', reject)
    server.listen(0, '127.0.0.1', () => {
      const addr = server.address()
      const port = typeof addr === 'object' && addr ? addr.port : 0
      resolve({ server, port, code })
    })
  })
}

async function postForm(params: Record<string, string>): Promise<Record<string, unknown>> {
  // Retry transient DNS/timeout failures (nodejs-mobile getaddrinfo can be flaky,
  // and the raw request had no retry — a single ENOTFOUND aborted the whole
  // sign-in): a fetch throw becomes a retryable NetworkError, while a well-formed
  // HTTP response (even a 400 invalid_grant) is returned as-is and handled below,
  // never retried.
  const res = await withRetry(async () => {
    try {
      return await fetch(GOOGLE_TOKEN_ENDPOINT, {
        method: 'POST',
        headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
        body: new URLSearchParams(params).toString(),
        signal: AbortSignal.timeout(15_000)
      })
    } catch (err) {
      throw new NetworkError(`Google token endpoint irraggiungibile (${GOOGLE_TOKEN_ENDPOINT})`, err)
    }
  })
  const text = await res.text()
  let json: Record<string, unknown> = {}
  try {
    json = text ? (JSON.parse(text) as Record<string, unknown>) : {}
  } catch {
    // fall through to the status-based error below
  }
  if (!res.ok) {
    // A revoked/expired refresh token comes back as 400 invalid_grant.
    if (json.error === 'invalid_grant') throw new Error('DRIVE_REAUTH_REQUIRED')
    throw new Error(`DRIVE_TOKEN_HTTP_${res.status}: ${String(json.error ?? text).slice(0, 200)}`)
  }
  return json
}

async function exchangeCode(
  code: string,
  verifier: string,
  redirectUri: string
): Promise<{ refreshToken: string; accessToken: string }> {
  const json = await postForm({
    client_id: getGoogleClientId(),
    client_secret: getGoogleClientSecret(),
    code,
    code_verifier: verifier,
    grant_type: 'authorization_code',
    redirect_uri: redirectUri
  })
  const refreshToken = typeof json.refresh_token === 'string' ? json.refresh_token : ''
  const accessToken = typeof json.access_token === 'string' ? json.access_token : ''
  if (!refreshToken) throw new Error('DRIVE_NO_REFRESH_TOKEN')
  return { refreshToken, accessToken }
}

async function fetchEmail(accessToken: string): Promise<string> {
  try {
    const res = await fetch(GOOGLE_USERINFO_ENDPOINT, {
      headers: { Authorization: `Bearer ${accessToken}` },
      signal: AbortSignal.timeout(15_000)
    })
    if (!res.ok) return ''
    const json = (await res.json()) as { email?: string }
    return json.email ?? ''
  } catch {
    return ''
  }
}

/** Interactive connect: opens the browser, captures the code, returns the
 *  refresh token + account email. The caller persists them. */
export async function runLoopbackAuth(): Promise<{ refreshToken: string; email: string }> {
  if (!isOAuthConfigured()) throw new Error('DRIVE_OAUTH_NOT_CONFIGURED')
  const verifier = base64url(randomBytes(32))
  const challenge = base64url(createHash('sha256').update(verifier).digest())
  const state = base64url(randomBytes(16))

  const { server, port, code } = await startLoopbackServer(state)
  try {
    const redirectUri = `http://127.0.0.1:${port}/callback`
    const authUrl =
      `${GOOGLE_AUTH_ENDPOINT}?` +
      new URLSearchParams({
        client_id: getGoogleClientId(),
        redirect_uri: redirectUri,
        response_type: 'code',
        scope: GOOGLE_SCOPES,
        code_challenge: challenge,
        code_challenge_method: 'S256',
        access_type: 'offline',
        prompt: 'consent',
        state
      }).toString()
    await shell.openExternal(authUrl)
    const authCode = await code
    const tokens = await exchangeCode(authCode, verifier, redirectUri)
    const email = await fetchEmail(tokens.accessToken)
    return { refreshToken: tokens.refreshToken, email }
  } finally {
    server.close()
  }
}

// ---- access-token cache -----------------------------------------------------

let accessToken: string | null = null
let accessTokenExpiresAt = 0

/** Return a valid access token, refreshing from the stored refresh token when
 *  the cached one is missing or within 60s of expiry. Throws
 *  'DRIVE_NOT_CONNECTED' when no refresh token is stored. */
export async function ensureAccessToken(): Promise<string> {
  if (accessToken && Date.now() < accessTokenExpiresAt - 60_000) return accessToken
  const refreshToken = getSettings().googleRefreshToken
  if (!refreshToken) throw new Error('DRIVE_NOT_CONNECTED')
  const json = await postForm({
    client_id: getGoogleClientId(),
    client_secret: getGoogleClientSecret(),
    refresh_token: refreshToken,
    grant_type: 'refresh_token'
  })
  const token = typeof json.access_token === 'string' ? json.access_token : ''
  const expiresIn = typeof json.expires_in === 'number' ? json.expires_in : 3600
  if (!token) throw new Error('DRIVE_REAUTH_REQUIRED')
  accessToken = token
  accessTokenExpiresAt = Date.now() + expiresIn * 1000
  return token
}

/** Drop the cached access token (e.g. after a 401 or on disconnect). */
export function clearAccessTokenCache(): void {
  accessToken = null
  accessTokenExpiresAt = 0
}

/** Best-effort revoke of the refresh token on disconnect. */
export async function revokeToken(token: string): Promise<void> {
  if (!token) return
  try {
    await fetch(`${GOOGLE_REVOKE_ENDPOINT}?token=${encodeURIComponent(token)}`, {
      method: 'POST',
      signal: AbortSignal.timeout(15_000)
    })
  } catch {
    // best effort — the token is cleared from secrets regardless
  }
}
