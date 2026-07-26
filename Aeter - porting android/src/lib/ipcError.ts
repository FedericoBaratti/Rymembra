import i18n from '@/i18n'

const BINARY_URLS: Record<string, string> = {
  'yt-dlp': 'github.com/yt-dlp/yt-dlp',
  spotdl: 'github.com/spotDL/spotify-downloader',
  ffmpeg: 'ffmpeg.org/download.html',
  fpcalc: 'acoustid.org/chromaprint'
}

const SIMPLE_CODES: Record<string, string> = {
  DL_UNRECOGNIZED_URL: 'errors.dl_unrecognized_url',
  DL_AGE_RESTRICTED: 'errors.dl_age_restricted',
  DL_UNAVAILABLE: 'errors.dl_unavailable',
  DL_RATE_LIMITED: 'errors.dl_rate_limited',
  DL_RATE_LIMITED_RETRY: 'errors.dl_rate_limited_retry',
  DL_FORBIDDEN: 'errors.dl_forbidden',
  DL_NETWORK: 'errors.dl_network',
  EXT_SEARCH_FAILED: 'errors.ext_search_failed',
  DL_INVALID_URL: 'errors.dl_invalid_url',
  DL_PRIVATE: 'errors.dl_private',
  DL_FAILED: 'errors.dl_failed',
  DL_NO_RESULTS: 'errors.dl_no_results',
  DL_INVALID_FILES: 'errors.dl_invalid_files',
  DL_YTDLP_TIMEOUT: 'errors.dl_ytdlp_timeout',
  DL_YTDLP_BAD_RESPONSE: 'errors.dl_ytdlp_bad_response',
  YTDLP_CORRUPTED: 'errors.ytdlp_corrupted',
  YTDLP_BUSY: 'errors.ytdlp_busy',
  TRACK_NOT_FOUND: 'errors.track_not_found',
  SMART_RULES_INVALID: 'errors.smart_rules_invalid',
  LASTFM_NOT_CONFIGURED: 'errors.lastfm_not_configured',
  LASTFM_NO_PENDING_TOKEN: 'errors.lastfm_no_pending_token',
  ENRICH_NO_MATCH: 'errors.enrich_no_match',
  ENRICH_NEEDS_REVIEW: 'errors.enrich_needs_review',
  ENRICH_MB_UNAVAILABLE: 'errors.enrich_mb_unavailable'
}

// Codes shaped as PREFIX:<payload>; the payload becomes the interpolation value.
const PARAM_CODES: Record<string, { key: string; param: string }> = {
  DL_SPOTDL_EXIT: { key: 'errors.dl_spotdl_exit', param: 'code' },
  DL_YT_ERROR: { key: 'errors.dl_yt_error', param: 'detail' },
  SPOTIFY_AUTH_FAILED: { key: 'errors.spotify_auth_failed', param: 'status' },
  TAG_VERIFY_FAILED: { key: 'errors.tag_verify_failed', param: 'fields' },
  SMART_FIELD_INVALID: { key: 'errors.smart_field_invalid', param: 'value' },
  SMART_OP_INVALID: { key: 'errors.smart_op_invalid', param: 'value' },
  ENRICH_FOUND: { key: 'errors.enrich_found', param: 'what' }
}

/** Translates stable error codes coming from the main process; unknown
    strings (including legacy persisted messages) pass through verbatim. */
export function translateErrorCode(message: string): string {
  const binary = /^BINARY_MISSING:([^:]+):?(.*)$/.exec(message)
  if (binary) {
    const [, name, dir] = binary
    return i18n.t('errors.binary_missing', {
      name,
      dir: dir || 'resources/bin',
      url: BINARY_URLS[name] ?? ''
    })
  }
  const simple = SIMPLE_CODES[message]
  if (simple) return i18n.t(simple)
  const sep = message.indexOf(':')
  if (sep > 0) {
    const param = PARAM_CODES[message.slice(0, sep)]
    if (param) return i18n.t(param.key, { [param.param]: message.slice(sep + 1) })
  }
  return message
}

/** Extracts the human-readable message from an IPC invoke rejection,
    stripping Electron's "Error invoking remote method 'x': Error: " prefix. */
export function ipcErrorMessage(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err)
  return translateErrorCode(
    raw.replace(/^Error invoking remote method '[^']*':\s*(?:\w*Error:\s*)?/, '')
  )
}
