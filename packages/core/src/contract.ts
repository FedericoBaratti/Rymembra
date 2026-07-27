/**
 * Contratto IPC: una sola fonte per canali, tipi, validazione e preload.
 *
 * Nel legacy la stessa API era descritta in quattro posti da tenere allineati a
 * mano:
 *
 *   1. `shared/ipcMethods.ts`   — l'array INVOKE_METHODS con i nomi dei canali
 *   2. `shared/types.ts:651`    — l'interfaccia AetherAPI, ~200 righe di firme
 *   3. `electron/ipc/*.ipc.ts`  — 105 chiamate handle() con i nomi ripetuti
 *   4. `electron/preload.ts`    — il ciclo che espone i metodi, chiuso da un
 *                                 `as unknown as AetherAPI`
 *
 * Quel cast era il buco: niente garantiva che INVOKE_METHODS contenesse le
 * chiavi di AetherAPI, né che per ogni nome esistesse un handler registrato. Un
 * canale dimenticato non era un errore di compilazione, era un reject a runtime
 * — e su mobile un `unknown channel: X` indistinguibile da un guasto vero.
 *
 * Qui il contratto è il codice. Da esso si derivano il tipo `AetherAPI`, i nomi
 * dei canali, la validazione degli argomenti e la generazione del preload. Se
 * manca un handler, `defineHandlers` non compila.
 *
 * Sulla validazione degli argomenti: nel legacy la faceva UN SOLO handler su 105
 * (`thermal.ipc.ts`). Qui è di serie, perché il renderer non è un ambiente
 * fidato quanto sembra: una skin, una risposta LAN o un dato persistito
 * malformato arrivano fin qui.
 */

import { z } from 'zod'
import { AppError } from './errors'
import { err, ok, type Result } from './result'
import type { HandlerReturn } from './serialize'

/**
 * Definizione di un canale. `input` e `output` sono schemi zod: il primo valida
 * in ingresso sul backend, il secondo documenta il tipo di ritorno.
 */
export interface ChannelDef<TIn = unknown, TOut = unknown> {
  readonly input: z.ZodType<TIn>
  readonly output: z.ZodType<TOut>
  /**
   * Se validare anche il valore di ritorno. Spento per default: serializzare e
   * ri-validare 100k tracce a ogni chiamata costerebbe più della chiamata.
   * Da accendere sui canali dove il dato viene da fuori (rete, file, telefono).
   */
  readonly validateOutput?: boolean
}

/** Dichiara un canale. `input: z.void()` per i canali senza argomenti. */
export function channel<TIn, TOut>(
  input: z.ZodType<TIn>,
  output: z.ZodType<TOut>,
  options?: { validateOutput?: boolean }
): ChannelDef<TIn, TOut> {
  return {
    input,
    output,
    ...(options?.validateOutput !== undefined ? { validateOutput: options.validateOutput } : {})
  }
}

/**
 * Un contratto è una mappa da nome di canale a definizione.
 *
 * `any` nei parametri è necessario, non pigrizia: `z.ZodType<T>` usa T sia in
 * posizione di ingresso sia di uscita, quindi `ChannelDef<void, X>` non sarebbe
 * assegnabile a `ChannelDef<unknown, unknown>` né a `ChannelDef<never, never>`.
 * Serve un tipo che accetti qualunque istanziazione, e solo `any` lo fa. È lo
 * stesso motivo per cui zod e tRPC lo usano nei loro vincoli. I tipi concreti
 * restano recuperati da InputOf/OutputOf, quindi nulla si perde ai bordi.
 */
/* eslint-disable @typescript-eslint/no-explicit-any */
export type Contract = Record<string, ChannelDef<any, any>>

export type InputOf<C> = C extends ChannelDef<infer I, any> ? I : never
export type OutputOf<C> = C extends ChannelDef<any, infer O> ? O : never
/* eslint-enable @typescript-eslint/no-explicit-any */

/**
 * Il tipo che il renderer vede come `window.aether`, DERIVATO dal contratto.
 *
 * I canali con `input: z.void()` diventano funzioni senza argomenti, gli altri
 * prendono esattamente il loro tipo di ingresso. Sostituisce le ~200 righe
 * scritte a mano di `AetherAPI` e il cast che le teneva insieme.
 */
export type ApiFor<C extends Contract> = {
  [K in keyof C]: InputOf<C[K]> extends void
    ? () => Promise<OutputOf<C[K]>>
    : (input: InputOf<C[K]>) => Promise<OutputOf<C[K]>>
}

/** Contesto passato agli handler: chi chiama, e da dove. */
export interface HandlerContext {
  /**
   * Origine della chiamata. `renderer` è la finestra locale; `lan` è un
   * telefono accoppiato che parla col server LAN. La distinzione conta: un
   * client LAN non deve poter fare tutto (nel legacy l'elenco dei metodi NON
   * supportati in `lanClient.ts` era proprio questo confine, ma espresso
   * dall'altro lato e per omissione).
   */
  origin: 'renderer' | 'lan' | 'internal'
  /** Id del dispositivo accoppiato, quando origin è 'lan'. */
  deviceId?: string
}

/**
 * La mappa degli handler richiesta da un contratto.
 *
 * Il punto: è un mapped type su TUTTE le chiavi del contratto, quindi
 * dimenticare un canale è un errore di compilazione e non un reject a runtime.
 */
export type HandlerMap<C extends Contract> = {
  [K in keyof C]: (
    input: InputOf<C[K]>,
    ctx: HandlerContext
  ) => HandlerReturn<OutputOf<C[K]>>
}

/** Handler pronto per il trasporto: valida, esegue, e non lancia mai. */
export type BoundHandler = (
  rawInput: unknown,
  ctx: HandlerContext
) => Promise<Result<unknown, AppError>>

/**
 * Compone contratto e handler in una mappa di funzioni pronte al trasporto.
 *
 * Cosa fa ogni handler risultante, nell'ordine: valida l'ingresso con lo schema
 * del canale, esegue, normalizza l'uscita in un Result, e converte qualunque
 * cosa sollevata in un AppError. Restituisce un Result, non lancia: il
 * trasporto lo impacchetta nella busta di serialize.ts.
 */
export function defineHandlers<C extends Contract>(
  contract: C,
  handlers: HandlerMap<C>
): Record<keyof C & string, BoundHandler> {
  const bound = {} as Record<keyof C & string, BoundHandler>

  for (const name of Object.keys(contract) as (keyof C & string)[]) {
    const def = contract[name] as ChannelDef<never, never>
    const handler = handlers[name]

    bound[name] = async (rawInput, ctx) => {
      const parsedInput = def.input.safeParse(rawInput)
      if (!parsedInput.success) {
        return err(
          AppError.of('ipc.payloadInvalid', {
            channel: name,
            detail: formatZodIssues(parsedInput.error)
          })
        )
      }

      let raw: unknown
      try {
        raw = await handler(parsedInput.data as never, ctx)
      } catch (cause) {
        return err(AppError.from(cause).withContext({ channel: name, origin: ctx.origin }))
      }

      // L'handler può restituire un valore nudo o un Result: entrambi vanno bene.
      const asResult = isResultLike(raw)
        ? (raw as Result<unknown, unknown>)
        : ok(raw)

      if (!asResult.ok) {
        return err(
          AppError.from(asResult.error).withContext({ channel: name, origin: ctx.origin })
        )
      }

      if (def.validateOutput === true) {
        const parsedOutput = def.output.safeParse(asResult.value)
        if (!parsedOutput.success) {
          // Un'uscita che non rispetta il contratto è un bug nostro, non un
          // input cattivo: va segnalata come tale invece di far esplodere il
          // renderer con un dato della forma sbagliata.
          return err(
            AppError.of('internal.invariantViolated', {
              what: `uscita del canale ${name} fuori contratto: ${formatZodIssues(parsedOutput.error)}`
            })
          )
        }
        return ok(parsedOutput.data)
      }

      return ok(asResult.value)
    }
  }

  return bound
}

function isResultLike(value: unknown): boolean {
  return (
    typeof value === 'object' &&
    value !== null &&
    typeof (value as { ok?: unknown }).ok === 'boolean' &&
    ('value' in value || 'error' in value)
  )
}

/** Messaggio compatto e leggibile per i problemi di validazione. */
function formatZodIssues(error: z.ZodError): string {
  return error.issues
    .slice(0, 5)
    .map((issue) => {
      const path = issue.path.length > 0 ? issue.path.join('.') : '(radice)'
      return `${path}: ${issue.message}`
    })
    .join('; ')
}

/**
 * I nomi dei canali, derivati dal contratto.
 * Sostituisce l'array INVOKE_METHODS mantenuto a mano.
 */
export function channelNames<C extends Contract>(contract: C): (keyof C & string)[] {
  return Object.keys(contract) as (keyof C & string)[]
}

/**
 * Verifica che gli handler coprano il contratto anche a RUNTIME.
 *
 * I tipi già lo garantiscono a compile time, ma un contratto composto da più
 * moduli di dominio può essere assemblato dinamicamente: questo controllo va
 * chiamato all'avvio e trasforma un buco in un errore immediato e nominato,
 * invece che in un reject quando l'utente clicca.
 */
export function assertHandlersComplete<C extends Contract>(
  contract: C,
  bound: Record<string, BoundHandler>
): Result<true, AppError> {
  const missing = channelNames(contract).filter((name) => typeof bound[name] !== 'function')
  if (missing.length > 0) {
    return err(AppError.of('ipc.handlerMissing', { channel: missing.join(', ') }))
  }
  return ok(true)
}

/**
 * Definizione degli eventi backend→renderer.
 *
 * Nel legacy erano un unico canale `aether:event` con un union di nomi in
 * `shared/types.ts`, e i due alberi avevano già divergito: il desktop aveva
 * `phone:state` e `phoneRepair:updated`, il mobile `backend:fatal` e
 * `transfer:state`. Qui gli eventi stanno nello stesso posto dei canali, così
 * non possono divergere per omissione.
 */
export type EventContract = Record<string, z.ZodType<unknown>>

export type EventPayload<E extends EventContract, K extends keyof E> = z.infer<E[K]>

export type EventEmitter<E extends EventContract> = <K extends keyof E & string>(
  name: K,
  payload: EventPayload<E, K>
) => void

export type EventListener<E extends EventContract> = <K extends keyof E & string>(
  name: K,
  handler: (payload: EventPayload<E, K>) => void
) => () => void
