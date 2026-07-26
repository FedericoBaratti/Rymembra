/**
 * Supervisor: la rete di sicurezza a livello di processo.
 *
 * Stato di partenza, verificato: il desktop non aveva NIENTE. Zero
 * `uncaughtException`, zero `unhandledRejection`, nessun handler per
 * `render-process-gone` / `child-process-gone` / `unresponsive`, nessun
 * crashReporter. Un throw non gestito nel main process uccideva l'app e con essa
 * tutte le scritture in sospeso: impostazioni, coda di riproduzione, segreti,
 * store di accoppiamento — tutti su debounce.
 *
 * E peggio: in `main.ts:210` il `will-quit` concatenava quattro flush NON
 * guardati (settings → secrets → queueState → pairingStore). Il primo che
 * lanciava saltava gli altri tre. Anche una chiusura pulita poteva quindi
 * perdere dati.
 *
 * Il mobile aveva già la soluzione giusta in `node-backend/fatal.ts`: iniezione
 * delle dipendenze, guardia di rientranza, ogni flush protetto per conto suo, e
 * processo tenuto in vita perché degradato è meglio che morto. Questo modulo
 * generalizza quel design e aggiunge la cosa che gli mancava: i flush hanno un
 * NOME, quindi "le impostazioni non si sono salvate" si vede nei log invece di
 * essere ingoiato da un `catch {}`.
 *
 * Resta a dipendenze iniettate e senza import da electron, così è testabile
 * senza avviare nulla.
 */

import { AppError } from './errors'
import { flushLogs, logger, recentLogs, type LogRecord } from './logger'

const log = logger('supervisor')

/** Da cosa nasce l'intervento del supervisor. */
export type SupervisorTrigger =
  | 'uncaughtException'
  | 'unhandledRejection'
  | 'renderProcessGone'
  | 'childProcessGone'
  | 'unresponsive'
  | 'quit'
  | 'manual'

/**
 * Una scrittura da mettere in salvo. Sincrona di proposito: in un handler di
 * crash il ciclo di eventi può non girare più, quindi una flush asincrona non
 * ha garanzia di completare.
 */
export interface FlushStep {
  /** Identifica il passo nei log. Senza, un fallimento è anonimo. */
  name: string
  run: () => void
}

export interface FlushOutcome {
  name: string
  ok: boolean
  error?: AppError
}

export interface SupervisorEvent {
  trigger: SupervisorTrigger
  error: AppError
  /** Esito di ogni flush, così si sa cosa è stato salvato e cosa no. */
  flushes: FlushOutcome[]
  /** Le ultime righe di log prima del guasto: il contesto che serve davvero. */
  recent: LogRecord[]
}

export interface SupervisorDeps {
  /** Scritture da mettere in salvo, nell'ordine dato. */
  flushes: FlushStep[]
  /**
   * Avvisa il renderer, così mostra un banner invece di girare a vuoto sugli
   * scheletri. Protetta: se il renderer è già morto non deve importare.
   */
  notify?: (event: SupervisorEvent) => void
  /**
   * Se terminare il processo dopo un guasto fatale.
   *
   * Default false, e non è pigrizia: sul mobile il motore nodejs-mobile NON si
   * può riavviare in-process (un secondo start aborta con
   * `Check failed: !platform_`), quindi uscire significa app morta. Sul desktop
   * i dati sono già stati messi in salvo e un'app degradata con un banner
   * visibile è più utile di una chiusa di colpo.
   */
  exitOnFatal?: boolean
  /** Iniettabile per i test. */
  exit?: (code: number) => void
}

export interface Supervisor {
  /** Gestisce un guasto: log, flush, avviso. Non lancia mai. */
  handleFatal(trigger: SupervisorTrigger, cause: unknown): SupervisorEvent
  /** Esegue i flush, ognuno protetto. Usata anche dalla chiusura pulita. */
  runFlushes(trigger: SupervisorTrigger): FlushOutcome[]
  /** Collega gli handler di processo. Restituisce la funzione per scollegarli. */
  installProcessHandlers(): () => void
}

export function createSupervisor(deps: SupervisorDeps): Supervisor {
  // Guardia di rientranza: se il gestore stesso lancia, non deve richiamarsi.
  let handling = false

  function runFlushes(trigger: SupervisorTrigger): FlushOutcome[] {
    const outcomes: FlushOutcome[] = []
    for (const step of deps.flushes) {
      try {
        step.run()
        outcomes.push({ name: step.name, ok: true })
      } catch (cause) {
        const error = AppError.from(cause).withContext({ flush: step.name, trigger })
        outcomes.push({ name: step.name, ok: false, error })
        // Il punto della riscrittura: un flush fallito è VISIBILE e ha un nome,
        // e i successivi girano comunque.
        try {
          log.error(`flush "${step.name}" fallito`, error, { trigger })
        } catch {
          // Anche il logger può essere rotto in questo stato. Si continua.
        }
      }
    }
    return outcomes
  }

  function handleFatal(trigger: SupervisorTrigger, cause: unknown): SupervisorEvent {
    const error = AppError.from(cause).withContext({ trigger })

    if (handling) {
      // Un secondo guasto mentre si gestisce il primo: si ingoia, altrimenti si
      // ricorre all'infinito e si perde anche il primo.
      return { trigger, error, flushes: [], recent: [] }
    }
    handling = true

    try {
      // Lo snapshot si prende PRIMA di loggare il guasto, così le ultime righe
      // sono quelle che lo precedono e non il guasto stesso.
      let recent: LogRecord[] = []
      try {
        recent = recentLogs()
      } catch {
        /* la diagnostica non deve poter aggravare un crash */
      }

      try {
        log.fatal(`guasto di processo: ${trigger}`, error)
      } catch {
        /* il logger può essere rotto proprio adesso */
      }

      const flushes = runFlushes(trigger)
      const event: SupervisorEvent = { trigger, error, flushes, recent }

      try {
        deps.notify?.(event)
      } catch {
        // Renderer già morto: la riga di log qui sopra è l'unica traccia, ed è
        // esattamente il motivo per cui viene scritta prima.
      }

      // I log si svuotano per ultimi e senza attendere: se il processo sta
      // morendo, una promise pendente non aiuta nessuno.
      void flushLogs()

      if (deps.exitOnFatal === true) {
        const exit = deps.exit ?? ((code: number) => process.exit(code))
        try {
          exit(1)
        } catch {
          /* niente da fare oltre questo punto */
        }
      }

      return event
    } finally {
      handling = false
    }
  }

  function installProcessHandlers(): () => void {
    const onUncaught = (error: unknown): void => {
      handleFatal('uncaughtException', error)
    }
    const onRejection = (reason: unknown): void => {
      handleFatal('unhandledRejection', reason)
    }

    process.on('uncaughtException', onUncaught)
    process.on('unhandledRejection', onRejection)

    return () => {
      process.off('uncaughtException', onUncaught)
      process.off('unhandledRejection', onRejection)
    }
  }

  return { handleFatal, runFlushes, installProcessHandlers }
}
