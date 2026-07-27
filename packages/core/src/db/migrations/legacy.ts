/**
 * Le due storie legacy, congelate.
 *
 * NON si toccano. Non si rinumerano, non si "correggono", non si accorpano. Sono
 * la descrizione di cosa è già stato eseguito sui dischi degli utenti: l'unico
 * scopo è portare un file esistente fino al suo capolinea, dove le due storie
 * confluiscono nella baseline.
 *
 * Messe una accanto all'altra, la divergenza si legge:
 *
 *   v1..v6   identiche
 *   v7       desktop: sync Drive          android: migrazione Spotify (base)
 *   v8       desktop: library_fetch       android: metadati album Spotify
 *   v9..v11  identiche (copertine su file, album_key, identità persistente)
 *   v12      desktop: provenienza cover   android: strato scoperta
 *   v13      desktop: play_history        android: podcast
 *   v14      desktop: podcast             android: provenienza cover
 *   v15      desktop: migrazione Spotify  android: sync Drive
 *   v16      desktop: trackKey v2         android: library_fetch
 *   v17      desktop: phone_repair (fine) android: trackKey v2
 *   v18      —                            android: phone_repair (fine)
 *
 * Otto passi su diciotto sono a numeri diversi. È il motivo per cui `user_version`
 * da sola non basta a sapere che schema ha un file, ed è il problema che
 * `migrate.ts` risolve tenendo separate le due catene.
 */

import type { Migration } from '../migrate'
import {
  backfillAlbumKey,
  extractCoversToFiles,
  initialSchema,
  persistentAlbumIdentity,
  recomputeCoverProvenance,
  rekeyTrackKeysV2
} from './steps'
import {
  DISCOVERY,
  DOWNLOAD_RETRY_AND_API_CACHE,
  ENRICH_STATUS,
  LIBRARY_FETCH,
  PHONE_REPAIR,
  PLAY_HISTORY,
  PODCASTS,
  SCROBBLE_QUEUE,
  SMART_PLAYLISTS_AND_GENRE,
  SPOTIFY_MIGRATION_ALBUM_COLUMNS,
  SPOTIFY_MIGRATION_BASE,
  SPOTIFY_MIGRATION_FULL,
  SYNC_STATS_AND_TOMBSTONES,
  SYNC_WITH_LIKED,
  TRACK_SOURCE
} from './sql'

/** I sei passi con cui le due storie cominciano, identici. */
const COMMON_HEAD: readonly Migration[] = [
  { version: 1, name: 'schema-iniziale', up: initialSchema },
  { version: 2, name: 'playlist-intelligenti-e-genere', up: SMART_PLAYLISTS_AND_GENRE },
  { version: 3, name: 'retry-download-e-cache-api', up: DOWNLOAD_RETRY_AND_API_CACHE },
  { version: 4, name: 'stato-arricchimento', up: ENRICH_STATUS },
  { version: 5, name: 'coda-scrobble', up: SCROBBLE_QUEUE },
  { version: 6, name: 'provenienza-traccia', up: TRACK_SOURCE }
]

/** I tre passi calcolati che entrambe le storie eseguono a v9, v10 e v11. */
const COMMON_ALBUM_WORK: readonly Migration[] = [
  { version: 9, name: 'copertine-su-file', up: extractCoversToFiles },
  { version: 10, name: 'album-key', up: backfillAlbumKey },
  { version: 11, name: 'identita-album-persistente', up: persistentAlbumIdentity }
]

export const DESKTOP_HISTORY: readonly Migration[] = [
  ...COMMON_HEAD,
  { version: 7, name: 'sync-drive-con-liked', up: SYNC_WITH_LIKED },
  { version: 8, name: 'library-fetch', up: LIBRARY_FETCH },
  ...COMMON_ALBUM_WORK,
  { version: 12, name: 'provenienza-copertine', up: recomputeCoverProvenance },
  { version: 13, name: 'cronologia-ascolti', up: PLAY_HISTORY },
  { version: 14, name: 'podcast', up: PODCASTS },
  { version: 15, name: 'migrazione-spotify', up: SPOTIFY_MIGRATION_FULL },
  { version: 16, name: 'trackkey-v2', up: rekeyTrackKeysV2 },
  { version: 17, name: 'phone-repair', up: PHONE_REPAIR }
]

export const ANDROID_HISTORY: readonly Migration[] = [
  ...COMMON_HEAD,
  { version: 7, name: 'migrazione-spotify-base', up: SPOTIFY_MIGRATION_BASE },
  { version: 8, name: 'migrazione-spotify-metadati-album', up: SPOTIFY_MIGRATION_ALBUM_COLUMNS },
  ...COMMON_ALBUM_WORK,
  { version: 12, name: 'strato-scoperta', up: DISCOVERY },
  { version: 13, name: 'podcast', up: PODCASTS },
  { version: 14, name: 'provenienza-copertine', up: recomputeCoverProvenance },
  { version: 15, name: 'sync-drive', up: SYNC_STATS_AND_TOMBSTONES },
  { version: 16, name: 'library-fetch', up: LIBRARY_FETCH },
  { version: 17, name: 'trackkey-v2', up: rekeyTrackKeysV2 },
  { version: 18, name: 'phone-repair', up: PHONE_REPAIR }
]
