/**
 * Il dominio libreria.
 *
 * Per ora solo la parte decidibile della scansione: quel che si può stabilire
 * guardando due elenchi, senza toccare il disco. L'esecuzione — camminata,
 * lettura dei metadati, scrittura in transazione — arriva sopra questa, e le
 * resta esterna di proposito: è la parte che può cancellare righe, e va provata
 * senza costruire un albero di file veri.
 */

export {
  MIN_TRACK_BYTES,
  SUPPORTED_EXTENSIONS,
  TRASH_DIR_NAME,
  baseName,
  extensionOf,
  isInTrash,
  isSupportedAudioPath,
  isUnder,
  pathKey,
  type PathRules
} from './paths'

export {
  isNoOp,
  planScan,
  type DiscoveredFile,
  type KnownTrack,
  type PendingRemoval,
  type PendingUpdate,
  type RemoveReason,
  type ScanInput,
  type ScanPlan,
  type SkipReason,
  type SkippedFile
} from './scanPlan'
