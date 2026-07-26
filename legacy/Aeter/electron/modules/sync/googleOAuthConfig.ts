/**
 * OAuth client configuration for the personal Google Drive library sync.
 *
 * The client id/secret are entered in the app UI (Settings → Library sync), the
 * same way the Spotify credentials are, and stored encrypted in the secrets
 * store. Create the client once in Google Cloud Console → APIs & Services →
 * Credentials → "Create OAuth client ID" → Application type **Desktop app**. For
 * an installed/desktop client the "secret" is NOT truly confidential (PKCE
 * protects the code exchange), but Google still requires it on the token request.
 *
 * One-time setup on the same Cloud project:
 *  - Enable the Google Drive API.
 *  - On the OAuth consent screen add the `drive.appdata` scope (non-sensitive,
 *    no verification) and either add yourself as a Test user OR, to avoid the
 *    7-day refresh-token expiry, set the publishing status to "In production".
 *
 * The constants below are an OPTIONAL fallback: leave them empty to configure
 * entirely from the UI, or hardcode a bundled client for a personal build.
 */
import { getSettings } from '../settings'

export const GOOGLE_CLIENT_ID =
  'CREDENZIALE-RIMOSSA-DALLA-STORIA.apps.googleusercontent.com'
export const GOOGLE_CLIENT_SECRET = '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ORIA'

/** appdata keeps the file private to this app + account and invisible to the user. */
export const GOOGLE_SCOPES = 'https://www.googleapis.com/auth/drive.appdata openid email'

export const GOOGLE_AUTH_ENDPOINT = 'https://accounts.google.com/o/oauth2/v2/auth'
export const GOOGLE_TOKEN_ENDPOINT = 'https://oauth2.googleapis.com/token'
export const GOOGLE_REVOKE_ENDPOINT = 'https://oauth2.googleapis.com/revoke'
export const GOOGLE_USERINFO_ENDPOINT = 'https://openidconnect.googleapis.com/v1/userinfo'

/** The active client id: UI-entered value first, then the bundled fallback. */
export function getGoogleClientId(): string {
  return getSettings().googleClientId || GOOGLE_CLIENT_ID
}

/** The active client secret: UI-entered value first, then the bundled fallback. */
export function getGoogleClientSecret(): string {
  return getSettings().googleClientSecret || GOOGLE_CLIENT_SECRET
}

/** True once a client id + secret are available (from the UI or the fallback). */
export function isOAuthConfigured(): boolean {
  return getGoogleClientId().length > 0 && getGoogleClientSecret().length > 0
}
