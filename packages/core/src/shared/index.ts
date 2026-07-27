/**
 * Strato isomorfo: logica pura, zero I/O, zero DOM, zero dipendenze.
 *
 * È l'unica parte di @aether/core che il renderer può importare liberamente —
 * tutto il resto del core presuppone un ambiente Node con un adapter di
 * piattaforma. Il renderer importa da qui con `@aether/core/shared`.
 *
 * Vincolo: questi file girano anche sul backend nodejs-mobile (Node 12.19), che
 * non ha le regex con proprietà unicode. `foldText` filtra i codepoint a mano
 * proprio per questo — non "semplificarlo" in `/\p{M}/gu`.
 */
export { foldText } from './text'
export {
  aggregateAlbums,
  albumFolder,
  albumGroupKey,
  buildAlbumGroups,
  canonicalAlbumArtist,
  normalizeKeyText,
  pickAlbumCover,
  pickCanonical,
  stripEditionSuffix,
  type AlbumAgg,
  type AlbumAggInput,
  type AlbumBuildResult,
  type ArtistRow
} from './albumKey'
export { formatLrcTime, serializeLrc, type TimedLine } from './lrc'
export { trackKey, upgradeLegacyTrackKey, normalizeKey, playlistKey } from './trackKey'
export { splitYoutubeWatchUrl } from './youtubeUrl'
