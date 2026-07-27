/**
 * Allineamento delle librerie skin fra due dispositivi.
 *
 * È la logica dietro «allinea tutto». Il trasporto — i due protocolli LAN che
 * esistono già — muove i byte; questo file decide **cosa** muovere, e la decisione
 * è la parte che si può sbagliare in silenzio.
 *
 * Il caso che rende il problema non banale: due dispositivi con la stessa skin
 * alla stessa versione ma con contenuto diverso. Succede appena qualcuno modifica
 * una skin nello Studio senza alzare la versione — cioè quasi sempre, durante il
 * lavoro. Confrontare solo le versioni direbbe «allineate», e la modifica
 * sparirebbe al primo allineamento. Per questo ogni voce porta un'impronta del
 * contenuto oltre alla versione.
 *
 * Sull'impronta: serve a rilevare una DIFFERENZA, non ad autenticare un
 * contenuto. Non è una firma e non protegge da un dispositivo malevolo — quello è
 * il lavoro dell'accoppiamento con token, che esiste già nel protocollo. Quindi un
 * hash non crittografico è la scelta giusta: è isomorfo (funziona identico nel
 * renderer, nel main e su Node 12), sincrono, e non richiede WebCrypto asincrono
 * né `node:crypto`.
 */

import type { SkinDocument } from './schema'

/**
 * JSON canonico: chiavi ordinate, a qualunque profondità.
 *
 * Senza questo, la stessa skin salvata da due editor diversi darebbe due impronte
 * diverse solo per l'ordine delle chiavi, e l'allineamento vedrebbe differenze
 * che non esistono. Gli array NON si ordinano: in questo formato l'ordine di un
 * array è significativo (i livelli di un'ombra, le fermate di un gradiente).
 */
export function canonicalJson(value: unknown): string {
  if (value === null || typeof value !== 'object') return JSON.stringify(value) ?? 'null'
  if (Array.isArray(value)) return `[${value.map(canonicalJson).join(',')}]`

  const entries = Object.keys(value as Record<string, unknown>)
    .sort()
    .filter((key) => (value as Record<string, unknown>)[key] !== undefined)
    .map((key) => `${JSON.stringify(key)}:${canonicalJson((value as Record<string, unknown>)[key])}`)
  return `{${entries.join(',')}}`
}

/**
 * FNV-1a a 32 bit, in esadecimale.
 *
 * Deterministico e identico su ogni runtime, che è l'unico requisito: due
 * dispositivi devono calcolare la stessa impronta sullo stesso contenuto. La
 * probabilità di collisione su 32 bit è irrilevante per una libreria di skin —
 * si parla di decine di voci, non di milioni.
 */
export function fingerprint(text: string): string {
  let hash = 0x811c9dc5
  for (let index = 0; index < text.length; index++) {
    hash ^= text.charCodeAt(index)
    // Moltiplicazione per 16777619 tenuta a 32 bit senza segno. Scritta come somma
    // di scorrimenti perché `hash * 16777619` supera il numero intero sicuro e
    // perderebbe bit.
    hash = (hash + (hash << 1) + (hash << 4) + (hash << 7) + (hash << 8) + (hash << 24)) >>> 0
  }
  return hash.toString(16).padStart(8, '0')
}

/** L'impronta di una skin, dalla sua forma sorgente. */
export function skinFingerprint(source: unknown): string {
  return fingerprint(canonicalJson(source))
}

export interface LibraryEntry {
  readonly id: string
  /** Versione dichiarata nei metadati, nella forma `1.2.3`. */
  readonly version: string
  readonly fingerprint: string
  /**
   * Le skin di serie non si sovrascrivono e non si mandano: esistono già su
   * entrambi i lati, e sono identiche per costruzione perché arrivano dal bundle.
   */
  readonly builtin: boolean
  readonly name?: string
}

/**
 * Confronto di versioni, campo per campo.
 *
 * Non alfabetico: `1.10.0` viene DOPO `1.9.0`, e confrontare le stringhe darebbe
 * il contrario. È l'errore classico, e su un allineamento significa sovrascrivere
 * la versione nuova con quella vecchia.
 */
export function compareVersions(a: string, b: string): -1 | 0 | 1 {
  const parse = (version: string): number[] =>
    version.split('.').map((part) => {
      const value = Number.parseInt(part, 10)
      return Number.isFinite(value) ? value : 0
    })

  const left = parse(a)
  const right = parse(b)
  for (let index = 0; index < 3; index++) {
    const l = left[index] ?? 0
    const r = right[index] ?? 0
    if (l > r) return 1
    if (l < r) return -1
  }
  return 0
}

export type AlignmentAction =
  /** Sta solo qui: va mandata all'altro dispositivo. */
  | 'send'
  /** Sta solo là: va ricevuta. */
  | 'receive'
  /** Qui è più recente: si manda. */
  | 'sendNewer'
  /** Là è più recente: si riceve. */
  | 'receiveNewer'
  /** Identiche: niente da fare. */
  | 'inSync'
  /**
   * Stessa versione, contenuto diverso. NON si risolve da soli: qualunque scelta
   * automatica butterebbe via il lavoro di qualcuno.
   */
  | 'conflict'
  /** Di serie: presente su entrambi per costruzione, fuori dall'allineamento. */
  | 'skipBuiltin'

export interface AlignmentItem {
  readonly id: string
  readonly action: AlignmentAction
  readonly name: string
  readonly localVersion: string | null
  readonly remoteVersion: string | null
  /** Perché questa decisione, in una frase mostrabile. */
  readonly reason: string
}

export interface AlignmentPlan {
  readonly items: readonly AlignmentItem[]
  /** Le voci che «allinea tutto» eseguirebbe senza chiedere. */
  readonly automatic: readonly AlignmentItem[]
  /** Le voci che richiedono una scelta dell'utente. */
  readonly conflicts: readonly AlignmentItem[]
}

const AUTOMATIC: readonly AlignmentAction[] = ['send', 'receive', 'sendNewer', 'receiveNewer']

/**
 * Confronta due librerie e produce il piano.
 *
 * Puro e senza rete: il piano si può mostrare all'utente prima di muovere un byte,
 * che è l'unico modo di rendere «allinea tutto» un pulsante di cui fidarsi.
 */
export function alignLibraries(
  local: readonly LibraryEntry[],
  remote: readonly LibraryEntry[]
): AlignmentPlan {
  const byId = new Map<string, { local?: LibraryEntry; remote?: LibraryEntry }>()

  for (const entry of local) {
    byId.set(entry.id, { ...byId.get(entry.id), local: entry })
  }
  for (const entry of remote) {
    byId.set(entry.id, { ...byId.get(entry.id), remote: entry })
  }

  const items: AlignmentItem[] = []

  // Ordine alfabetico per id: un piano che cambia ordine fra due letture è un
  // piano che l'utente non può ricontrollare.
  for (const id of [...byId.keys()].sort()) {
    const pair = byId.get(id)
    if (pair === undefined) continue
    const { local: here, remote: there } = pair
    const name = here?.name ?? there?.name ?? id

    if (here?.builtin === true || there?.builtin === true) {
      items.push({
        id,
        action: 'skipBuiltin',
        name,
        localVersion: here?.version ?? null,
        remoteVersion: there?.version ?? null,
        reason: 'skin di serie: presente su entrambi i dispositivi'
      })
      continue
    }

    if (here !== undefined && there === undefined) {
      items.push({
        id,
        action: 'send',
        name,
        localVersion: here.version,
        remoteVersion: null,
        reason: 'manca sull\'altro dispositivo'
      })
      continue
    }

    if (here === undefined && there !== undefined) {
      items.push({
        id,
        action: 'receive',
        name,
        localVersion: null,
        remoteVersion: there.version,
        reason: 'presente solo sull\'altro dispositivo'
      })
      continue
    }

    if (here === undefined || there === undefined) continue

    if (here.fingerprint === there.fingerprint) {
      items.push({
        id,
        action: 'inSync',
        name,
        localVersion: here.version,
        remoteVersion: there.version,
        reason: 'identiche'
      })
      continue
    }

    const order = compareVersions(here.version, there.version)
    if (order > 0) {
      items.push({
        id,
        action: 'sendNewer',
        name,
        localVersion: here.version,
        remoteVersion: there.version,
        reason: `qui è ${here.version}, là è ${there.version}`
      })
      continue
    }
    if (order < 0) {
      items.push({
        id,
        action: 'receiveNewer',
        name,
        localVersion: here.version,
        remoteVersion: there.version,
        reason: `là è ${there.version}, qui è ${here.version}`
      })
      continue
    }

    /*
     * Stessa versione, contenuto diverso: succede appena qualcuno modifica una
     * skin senza alzare la versione, cioè quasi sempre durante il lavoro.
     * Qualunque scelta automatica butterebbe via il lavoro di uno dei due lati,
     * quindi si chiede.
     */
    items.push({
      id,
      action: 'conflict',
      name,
      localVersion: here.version,
      remoteVersion: there.version,
      reason: `entrambe a ${here.version} ma con contenuto diverso: serve una scelta`
    })
  }

  return {
    items,
    automatic: items.filter((item) => AUTOMATIC.includes(item.action)),
    conflicts: items.filter((item) => item.action === 'conflict')
  }
}

/**
 * La voce di libreria di una skin.
 *
 * `source` e non `document`: l'impronta va calcolata su ciò che viene trasferito,
 * altrimenti due dispositivi che validano la stessa sorgente con versioni diverse
 * del formato potrebbero calcolare impronte diverse su un contenuto identico.
 */
export function libraryEntryFor(
  document: SkinDocument,
  source: unknown,
  builtin: boolean
): LibraryEntry {
  return {
    id: document.id,
    version: document.meta.version,
    fingerprint: skinFingerprint(source),
    builtin,
    name: document.meta.name
  }
}
