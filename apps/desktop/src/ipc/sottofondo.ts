/**
 * Il sottofondo del computer: l'avvio con Windows e la proposta che lo accende.
 *
 * I tipi di `src-tauri/src/avvio_con_windows.rs`, com'è fatta ogni forma sul
 * filo. Un file suo e non in `servizi.ts`, che è lungo e ha già troppi
 * padroni: questi due tipi li usano una scheda delle Impostazioni e una
 * proposta, e nessun altro.
 */

/**
 * Perché «Avvia con Windows» non si può toccare.
 *
 * - `sistema`: l'utente l'ha spento da Windows (Gestione attività, Impostazioni
 *   › App › Avvio), e si rimette da lì;
 * - `criterio`: lo decide un criterio di questo computer;
 * - `prova`: è una copia di prova, con la sua cartella dati — la voce `Run` si
 *   chiama come quella della copia vera, e non si tocca;
 * - `nonDisponibile`: qui l'avvio con Windows non esiste.
 */
export type ImpedimentoAvvio = "sistema" | "criterio" | "prova" | "nonDisponibile";

/** Com'è l'avvio con Windows, **secondo il sistema** (non secondo una preferenza nostra). */
export interface AvvioConWindows {
  acceso: boolean;
  /** Se l'interruttore non si tocca, e perché. */
  impedito: ImpedimentoAvvio | null;
}

/** Com'è rimasto tutto dopo la risposta alla proposta del sottofondo. */
export interface RispostaProposta {
  avvio: AvvioConWindows;
  secondoPiano: boolean;
}
