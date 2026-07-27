/**
 * La decisione della scansione, separata dal suo lavoro.
 *
 * Una scansione fa due cose molto diverse: legge il disco (lento, asincrono, con
 * code e pressione termica) e **decide** cosa inserire, cosa rileggere, cosa
 * togliere. Nel vecchio albero erano un'unica funzione, e la conseguenza non è
 * estetica: la parte che decide è quella che può cancellare righe, ed era
 * provabile solo costruendo un albero di file veri e un database vero. Infatti
 * `library.scan.test.ts` sono 56 righe contro le 368 del modulo.
 *
 * Qui la decisione è una funzione pura da tre elenchi a un piano. Ogni caso che
 * costa dati — la cartella con il nome che comincia come quella sorvegliata, il
 * percorso che differisce per una barra, il file troncato che i lettori di
 * metadati accettano lo stesso — si prova come una chiamata di funzione.
 *
 * E il piano si può mostrare prima di eseguirlo. «Sto per togliere 340 brani
 * dalla libreria» è una frase che l'utente deve poter leggere quando ha staccato
 * il disco esterno per sbaglio, invece di scoprirlo dopo.
 */

import { MIN_TRACK_BYTES, isInTrash, isSupportedAudioPath, isUnder, pathKey } from './paths'
import type { PathRules } from './paths'

/** Un file trovato dalla camminata sul disco. */
export interface DiscoveredFile {
  readonly path: string
  readonly sizeBytes: number
  /**
   * Data di modifica in millisecondi, GIÀ troncata all'intero.
   *
   * Il troncamento sta a monte perché è la forma che il database persiste: se
   * il confronto avvenisse fra un valore troncato e uno con i decimali, ogni
   * scansione vedrebbe cambiato ogni file, e una riscansione da centomila brani
   * rileggerebbe tutti i metadati invece di nessuno.
   */
  readonly modifiedMs: number
}

/** Una riga già in libreria, come serve a decidere. */
export interface KnownTrack {
  readonly id: number
  readonly path: string
  readonly modifiedMs: number
}

export type SkipReason =
  /** Estensione fuori dall'elenco. */
  | 'nonAudio'
  /** Dentro la cartella dei doppioni scartati. */
  | 'inTrash'
  /** Sotto la soglia: avanzo troncato, non un brano. */
  | 'tooSmall'
  /** Un altro file trovato in questa stessa passata occupa già il suo posto. */
  | 'duplicate'

export type RemoveReason =
  /** Il file non c'è più, e la sua riga stava in una cartella scansionata. */
  | 'disappeared'
  /** Due righe per lo stesso file: su un filesystem che ignora le maiuscole capita. */
  | 'duplicateRow'

export interface SkippedFile {
  readonly file: DiscoveredFile
  readonly reason: SkipReason
}

export interface PendingUpdate {
  readonly file: DiscoveredFile
  readonly trackId: number
}

export interface PendingRemoval {
  readonly track: KnownTrack
  readonly reason: RemoveReason
}

export interface ScanPlan {
  /** Da leggere e inserire: non c'era. */
  readonly toInsert: readonly DiscoveredFile[]
  /** Da rileggere e aggiornare: la data di modifica è cambiata. */
  readonly toUpdate: readonly PendingUpdate[]
  /** Righe da togliere. È la parte del piano che va mostrata prima di eseguirla. */
  readonly toRemove: readonly PendingRemoval[]
  /** File visti e scartati, con il motivo. */
  readonly skipped: readonly SkippedFile[]
  /** Quante righe erano già a posto: il numero che rende veloce una riscansione. */
  readonly unchanged: number
  /**
   * Righe fuori dalle cartelle scansionate, lasciate stare.
   *
   * Non è un dettaglio contabile: una scansione della sola cartella dei download
   * non deve toccare i brani che stanno altrove. Contarle rende visibile che
   * sono state deliberatamente ignorate, invece che dimenticate.
   */
  readonly untouched: number
}

export interface ScanInput {
  /** Le cartelle che questa passata sta scansionando. */
  readonly roots: readonly string[]
  /** Quel che la camminata ha trovato. */
  readonly found: readonly DiscoveredFile[]
  /** Le righe locali già in libreria. */
  readonly known: readonly KnownTrack[]
}

/**
 * Confronta disco e database e produce il piano.
 *
 * Puro: nessuna lettura, nessuna scrittura, nessun orologio. Chi esegue prende
 * il piano e lo applica; chi mostra lo mostra.
 */
export function planScan(input: ScanInput, rules: PathRules): ScanPlan {
  const toInsert: DiscoveredFile[] = []
  const toUpdate: PendingUpdate[] = []
  const toRemove: PendingRemoval[] = []
  const skipped: SkippedFile[] = []
  let unchanged = 0
  let untouched = 0

  /*
   * Le righe conosciute, indicizzate per forma canonica.
   *
   * Se due righe cadono sulla stessa chiave sono due righe per lo stesso file —
   * possibile su un filesystem che ignora le maiuscole, dove `A.mp3` e `a.mp3`
   * passano il vincolo di unicità sul percorso ma nominano un file solo. La
   * seconda è residuo, e toglierla è la riparazione.
   */
  const knownByKey = new Map<string, KnownTrack>()
  for (const track of input.known) {
    const key = pathKey(track.path, rules)
    const existing = knownByKey.get(key)
    if (existing === undefined) {
      knownByKey.set(key, track)
      continue
    }
    // Si tiene la riga con l'id più basso: è la più vecchia, quindi quella a cui
    // playlist, preferiti e conteggi di ascolto puntano più probabilmente.
    const [keep, drop] = existing.id <= track.id ? [existing, track] : [track, existing]
    knownByKey.set(key, keep)
    toRemove.push({ track: drop, reason: 'duplicateRow' })
  }

  /** Le chiavi viste sul disco in questa passata, comprese quelle scartate. */
  const seen = new Set<string>()

  for (const file of input.found) {
    const key = pathKey(file.path, rules)

    if (seen.has(key)) {
      skipped.push({ file, reason: 'duplicate' })
      continue
    }
    seen.add(key)

    /*
     * L'ordine dei controlli è quello del costo crescente, ma soprattutto:
     * `inTrash` e `nonAudio` vengono prima di `tooSmall` perché un file nel
     * cestino o non audio va riportato per quel che è, non come «troppo
     * piccolo» — il motivo finisce nella diagnostica, e uno sbagliato manda a
     * cercare nel posto sbagliato.
     */
    if (isInTrash(file.path)) {
      skipped.push({ file, reason: 'inTrash' })
      continue
    }
    if (!isSupportedAudioPath(file.path)) {
      skipped.push({ file, reason: 'nonAudio' })
      continue
    }
    if (file.sizeBytes < MIN_TRACK_BYTES) {
      skipped.push({ file, reason: 'tooSmall' })
      continue
    }

    const existing = knownByKey.get(key)
    if (existing === undefined) {
      toInsert.push(file)
      continue
    }
    if (existing.modifiedMs === file.modifiedMs) {
      unchanged++
      continue
    }
    toUpdate.push({ file, trackId: existing.id })
  }

  for (const [key, track] of knownByKey) {
    if (seen.has(key)) continue

    /*
     * Sparita, ma solo se stava in una cartella che questa passata ha davvero
     * guardato. Una riga fuori dalle radici non è «non trovata»: è fuori
     * competenza, e toglierla svuoterebbe la libreria alla prima scansione della
     * sola cartella dei download.
     */
    const watched = input.roots.some((root) => isUnder(track.path, root, rules))
    if (watched) toRemove.push({ track, reason: 'disappeared' })
    else untouched++
  }

  return { toInsert, toUpdate, toRemove, skipped, unchanged, untouched }
}

/** Il piano non cambia niente? Utile per non aprire una transazione per nulla. */
export function isNoOp(plan: ScanPlan): boolean {
  return plan.toInsert.length === 0 && plan.toUpdate.length === 0 && plan.toRemove.length === 0
}
