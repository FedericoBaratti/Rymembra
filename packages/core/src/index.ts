/**
 * Punto d'ingresso pubblico di @aether/core.
 *
 * Nella Fase 1 qui arrivano: Result, la tassonomia degli errori, la busta di
 * serializzazione, il contratto IPC tipizzato, il logger e il supervisor.
 * `AetherAPI` sarà DERIVATA dal contratto invece di essere scritta a mano come
 * nel legacy (shared/types.ts:651, ~200 righe più un cast `as unknown as`).
 */

/**
 * Superficie che il renderer vede come `window.aether`.
 *
 * Segnaposto volutamente aperto: serve solo a far compilare il ponteggio del
 * monorepo. La Fase 1 lo sostituisce con il tipo generato dal contratto, e da
 * quel momento un handler mancante diventa un errore di compilazione.
 */
export type AetherAPI = Record<string, (...args: never[]) => unknown>

export const CORE_PLACEHOLDER = true
