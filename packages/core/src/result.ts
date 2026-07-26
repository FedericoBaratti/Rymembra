/**
 * Result<T, E> — l'esito di un'operazione che può fallire, come valore.
 *
 * Perché non le eccezioni: fra il backend e il renderer c'è un confine di
 * serializzazione. Nel legacy `ipc/handle.ts` rilanciava `new Error(err.message)`
 * e il bridge mobile faceva `String(body)`: dieci classi di errore con campi
 * ricchi (status HTTP, retryAfterMs, classe di fallimento, causa) morivano al
 * primo salto, e il renderer ricostruiva l'identità dell'errore con una regex
 * sul messaggio. Un Result attraversa il confine come dato, senza perdite.
 *
 * Le eccezioni restano legittime per i bug veri (invarianti violate): quelle non
 * vanno gestite, vanno corrette. Il supervisor le raccoglie e le converte in un
 * AppError `internal.unexpected` come ultima rete.
 *
 * Vincolo: questo file gira anche sul backend nodejs-mobile (Node 12.19). Solo
 * API runtime ES2015 — esbuild abbassa la sintassi, non i metodi.
 */

export interface Ok<T> {
  readonly ok: true
  readonly value: T
}

export interface Err<E> {
  readonly ok: false
  readonly error: E
}

export type Result<T, E> = Ok<T> | Err<E>

export function ok<T>(value: T): Ok<T> {
  return { ok: true, value }
}

export function err<E>(error: E): Err<E> {
  return { ok: false, error }
}

export function isOk<T, E>(r: Result<T, E>): r is Ok<T> {
  return r.ok
}

export function isErr<T, E>(r: Result<T, E>): r is Err<E> {
  return !r.ok
}

/** Trasforma il valore di successo, lasciando passare l'errore intatto. */
export function map<T, U, E>(r: Result<T, E>, fn: (value: T) => U): Result<U, E> {
  return r.ok ? ok(fn(r.value)) : r
}

/** Trasforma l'errore, lasciando passare il successo intatto. */
export function mapErr<T, E, F>(r: Result<T, E>, fn: (error: E) => F): Result<T, F> {
  return r.ok ? r : err(fn(r.error))
}

/** Concatena un'operazione che può fallire a sua volta. */
export function andThen<T, U, E>(
  r: Result<T, E>,
  fn: (value: T) => Result<U, E>
): Result<U, E> {
  return r.ok ? fn(r.value) : r
}

/** Estrae il valore, o il fallback se l'operazione è fallita. */
export function unwrapOr<T, E>(r: Result<T, E>, fallback: T): T {
  return r.ok ? r.value : fallback
}

/**
 * Raccoglie una lista di Result in un Result di lista, fermandosi al primo
 * errore. Utile per validare N cose dove una sola invalida tutto (es. gli asset
 * di un pacchetto skin).
 */
export function all<T, E>(results: readonly Result<T, E>[]): Result<T[], E> {
  const values: T[] = []
  for (const r of results) {
    if (!r.ok) return r
    values.push(r.value)
  }
  return ok(values)
}

/**
 * Divide una lista di Result in successi e fallimenti, senza fermarsi.
 * È il comportamento giusto per le operazioni in blocco dove un elemento rotto
 * non deve annullare gli altri — la scansione della libreria su 100k file, per
 * dirne una: nel legacy quei fallimenti finivano in `logWarn` e sparivano.
 */
export function partition<T, E>(
  results: readonly Result<T, E>[]
): { values: T[]; errors: E[] } {
  const values: T[] = []
  const errors: E[] = []
  for (const r of results) {
    if (r.ok) values.push(r.value)
    else errors.push(r.error)
  }
  return { values, errors }
}
