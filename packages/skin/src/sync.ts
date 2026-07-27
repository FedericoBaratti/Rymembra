/**
 * L'esecuzione di un piano di allineamento.
 *
 * `alignLibraries` decide *cosa* muovere; questo file lo muove. In mezzo c'è una
 * cosa che sembra un dettaglio e non lo è: **un piano è una fotografia.** Fra lo
 * scatto e l'esecuzione passano secondi in cui qualcuno può salvare una skin
 * dallo Studio, o l'altro dispositivo può riceverne una da un terzo. Eseguire un
 * piano vecchio senza accorgersene significa sovrascrivere lavoro che il piano
 * non ha mai visto — e siccome l'utente ha approvato *quel* piano, sarebbe un
 * danno fatto con la sua firma sopra.
 *
 * Per questo ogni scrittura è condizionata: prima di sovrascrivere si verifica
 * che la parte da sovrascrivere sia ancora quella su cui il piano ha deciso. Se
 * non lo è, la voce diventa `diverged` e non si tocca niente. È la stessa idea
 * di un aggiornamento condizionato, con l'impronta al posto dell'ETag; senza le
 * impronte nel piano il controllo non si potrebbe fare, ed è il motivo per cui
 * `AlignmentItem` le porta.
 *
 * Il secondo principio è che una voce che fallisce non ferma le altre. Dieci
 * skin da allineare e una con l'archivio rotto: le altre nove passano, e il
 * rapporto dice quale non è passata e perché. Stesso ragionamento di
 * `partition()` sulla scansione della libreria.
 *
 * La rete arriva iniettata come `SkinPeer`. Non è purismo: il trasporto reale
 * sono due server diversi (HTTP sul telefono, HTTP sul desktop) dietro due
 * autenticazioni diverse, e i casi che contano qui — il piano invecchiato, il
 * pacchetto che arriva diverso da quello annunciato, l'annullamento a metà — si
 * provano solo se il canale si può controllare.
 */

import { AppError } from '@aether/core'
import type { Result } from '@aether/core'
import type { AlignmentAction, AlignmentItem, AlignmentPlan } from './library'
import { skinFingerprint } from './library'
import { readSkinPackage } from './package'
import type { SkinLibrary } from './store'
import type { CommitDone, SkinListing, UploadAccepted } from './transfer'

/**
 * Il canale verso l'altro dispositivo.
 *
 * Rispecchia le rotte di `transfer.ts` una a una. `remove` non serve
 * all'allineamento — un piano non contiene mai una cancellazione — ma sta qui
 * perché è la stessa connessione: gestire la libreria del telefono dal PC è
 * l'altra metà della funzione.
 */
export interface SkinPeer {
  list(): Promise<Result<SkinListing, AppError>>
  download(id: string): Promise<Result<Uint8Array, AppError>>
  /** `fingerprint` è quella dichiarata: chi riceve la verifica sui byte arrivati. */
  upload(archive: Uint8Array, fingerprint: string): Promise<Result<UploadAccepted, AppError>>
  commit(uploadId: string, overwrite: boolean): Promise<Result<CommitDone, AppError>>
  remove(id: string): Promise<Result<true, AppError>>
}

export type AlignmentOutcome =
  /** Mandata e installata sull'altro dispositivo. */
  | 'sent'
  /** Ricevuta e installata qui. */
  | 'received'
  /** Le due parti si sono rivelate già identiche: niente da scrivere. */
  | 'alreadyInSync'
  /** Il piano non chiedeva niente per questa voce. */
  | 'skipped'
  /**
   * Il piano è invecchiato: la parte da sovrascrivere non è più quella su cui la
   * decisione è stata presa. Non si scrive, e si rifà il piano.
   */
  | 'diverged'
  | 'failed'
  /** L'allineamento è stato annullato prima di arrivare a questa voce. */
  | 'aborted'

export interface AlignmentResult {
  readonly id: string
  readonly action: AlignmentAction
  readonly outcome: AlignmentOutcome
  /** Cosa è successo, in una frase mostrabile. */
  readonly reason: string
  readonly error?: AppError
}

export interface AlignmentReport {
  readonly results: readonly AlignmentResult[]
  readonly sent: number
  readonly received: number
  readonly diverged: number
  readonly failed: number
  readonly aborted: boolean
}

export interface RunAlignmentOptions {
  readonly local: SkinLibrary
  readonly peer: SkinPeer
  readonly signal?: AbortSignal
  /** Chiamata dopo ogni voce: è l'unica fonte di avanzamento per la UI. */
  readonly onProgress?: (result: AlignmentResult, done: number, total: number) => void
}

function outcome(
  item: AlignmentItem,
  result: AlignmentOutcome,
  reason: string,
  error?: AppError
): AlignmentResult {
  return {
    id: item.id,
    action: item.action,
    outcome: result,
    reason,
    ...(error === undefined ? {} : { error })
  }
}

/**
 * Esegue il piano, voce per voce, in ordine.
 *
 * In serie e non in parallelo, di proposito: il collo di bottiglia è il Wi-Fi di
 * un telefono, dove quattro trasferimenti insieme sono più lenti di quattro di
 * fila, e un avanzamento che procede una skin alla volta è l'unico che si possa
 * mostrare onestamente.
 */
export async function runAlignment(
  plan: AlignmentPlan,
  options: RunAlignmentOptions
): Promise<AlignmentReport> {
  const { local, peer, signal, onProgress } = options
  const results: AlignmentResult[] = []
  const total = plan.items.length

  for (const item of plan.items) {
    const result = signal?.aborted === true
      ? outcome(item, 'aborted', 'allineamento annullato')
      : await runOne(item, local, peer, signal)

    results.push(result)
    onProgress?.(result, results.length, total)
  }

  const count = (value: AlignmentOutcome): number =>
    results.filter((result) => result.outcome === value).length

  return {
    results,
    sent: count('sent'),
    received: count('received'),
    diverged: count('diverged'),
    failed: count('failed'),
    aborted: count('aborted') > 0
  }
}

async function runOne(
  item: AlignmentItem,
  local: SkinLibrary,
  peer: SkinPeer,
  signal: AbortSignal | undefined
): Promise<AlignmentResult> {
  try {
    switch (item.action) {
      case 'inSync':
        return outcome(item, 'skipped', 'già identiche')
      case 'skipBuiltin':
        return outcome(item, 'skipped', 'skin di serie: sta su entrambi per costruzione')
      case 'conflict':
        // Qualunque scelta automatica butterebbe via il lavoro di uno dei due
        // lati. L'unica cosa corretta da fare qui è non fare niente.
        return outcome(item, 'skipped', item.reason)
      case 'send':
      case 'sendNewer':
        return await send(item, local, peer, signal)
      case 'receive':
      case 'receiveNewer':
        return await receive(item, local, peer, signal)
    }
  } catch (cause) {
    // Il canale è iniettato: può lanciare. Una voce che lancia non deve portarsi
    // dietro le altre nove.
    return outcome(item, 'failed', 'errore imprevisto', AppError.from(cause))
  }
}

/** Manda una skin all'altro dispositivo, se là non c'è nulla da perdere. */
async function send(
  item: AlignmentItem,
  local: SkinLibrary,
  peer: SkinPeer,
  signal: AbortSignal | undefined
): Promise<AlignmentResult> {
  const here = local.get(item.id)
  if (!here.ok) return outcome(item, 'failed', 'la skin non si legge da qui', here.error)

  const archive = local.export(item.id)
  if (!archive.ok) return outcome(item, 'failed', 'il pacchetto non si legge', archive.error)

  const accepted = await peer.upload(archive.value, here.value.entry.fingerprint)
  if (!accepted.ok) return outcome(item, 'failed', 'invio non riuscito', accepted.error)
  if (signal?.aborted === true) return outcome(item, 'aborted', 'annullato dopo l\'invio')

  const installed = accepted.value.installed

  if (accepted.value.identical) {
    // Succede quando la stessa skin è arrivata sui due lati fra il piano e ora.
    // Il commit sarebbe una scrittura che non cambia niente.
    return outcome(item, 'alreadyInSync', 'l\'altro dispositivo ha già lo stesso contenuto')
  }

  /*
   * Il controllo che rende sicuro il pulsante «allinea tutto».
   *
   * `send` significa che il piano ha visto il posto vuoto: se ora c'è qualcosa,
   * è arrivata dopo lo scatto e sovrascriverla distruggerebbe un lavoro che
   * l'utente non ha mai visto nella lista che ha approvato.
   */
  if (item.action === 'send' && installed !== null) {
    return outcome(item, 'diverged', 'sull\'altro dispositivo è comparsa una skin con questo id')
  }

  /*
   * `sendNewer` significa che il piano ha visto una versione precisa dall'altra
   * parte. Se non è più quella, non è più la versione che l'utente ha accettato
   * di sostituire. Un pacchetto diventato illeggibile è l'eccezione: lì non c'è
   * lavoro da proteggere, e sovrascriverlo È la riparazione.
   */
  if (
    item.action === 'sendNewer' &&
    installed !== null &&
    !installed.unreadable &&
    installed.fingerprint !== item.remoteFingerprint
  ) {
    return outcome(item, 'diverged', 'la copia sull\'altro dispositivo è cambiata dopo il piano')
  }

  const done = await peer.commit(accepted.value.uploadId, installed !== null)
  if (!done.ok) return outcome(item, 'failed', 'installazione sull\'altro dispositivo fallita', done.error)
  return outcome(item, 'sent', `mandata la versione ${done.value.version}`)
}

/** Riceve una skin dall'altro dispositivo, se qui non c'è nulla da perdere. */
async function receive(
  item: AlignmentItem,
  local: SkinLibrary,
  peer: SkinPeer,
  signal: AbortSignal | undefined
): Promise<AlignmentResult> {
  const downloaded = await peer.download(item.id)
  if (!downloaded.ok) return outcome(item, 'failed', 'scaricamento non riuscito', downloaded.error)
  if (signal?.aborted === true) return outcome(item, 'aborted', 'annullato dopo lo scaricamento')

  /*
   * Si legge il pacchetto PRIMA di installarlo, per confrontarne l'impronta con
   * quella del piano. Costa una decompressione in più e la vale: l'utente ha
   * approvato un contenuto preciso, e installarne un altro perché nel frattempo
   * è cambiato sarebbe fare una cosa diversa da quella accettata.
   */
  const parsed = readSkinPackage(downloaded.value)
  if (!parsed.ok) return outcome(item, 'failed', 'il pacchetto arrivato non si legge', parsed.error)

  const arrived = skinFingerprint(parsed.value.source)
  if (arrived !== item.remoteFingerprint) {
    return outcome(item, 'diverged', 'quel che è arrivato non è quel che il piano aveva visto')
  }

  const here = local.get(item.id)
  const unreadable = !here.ok && here.error.code !== 'skin.notFound'

  if (item.action === 'receive' && here.ok) {
    return outcome(item, 'diverged', 'qui è comparsa una skin con questo id')
  }

  if (
    item.action === 'receiveNewer' &&
    here.ok &&
    here.value.entry.fingerprint !== item.localFingerprint
  ) {
    return outcome(item, 'diverged', 'la copia qui è cambiata dopo il piano')
  }

  // Si sovrascrive solo dove il piano prevedeva di trovare qualcosa, o dove
  // quel qualcosa è un pacchetto illeggibile — che è il caso in cui installare
  // sopra è la riparazione.
  const installed = local.install(downloaded.value, { overwrite: here.ok || unreadable })
  if (!installed.ok) return outcome(item, 'failed', 'installazione qui fallita', installed.error)
  return outcome(item, 'received', `ricevuta la versione ${installed.value.document.meta.version}`)
}
