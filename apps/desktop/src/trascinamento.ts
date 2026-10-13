/**
 * Il trascinamento sulla finestra: l'ascolto.
 *
 * `dragDropEnabled` è acceso in `tauri.conf.json` da sempre e non aveva
 * nessun gestore: la finestra accettava i file e non ne faceva niente. Adesso
 * l'evento arriva qui, i percorsi vanno al nucleo che dice cosa sono
 * (`trascinati.rs`), e la regola di `trascinati.ts` dice cosa farne. Il gesto
 * lo fa chi chiama, che ha in mano le Impostazioni, la coda e il primo avvio.
 *
 * Un file a parte dalla regola perché questo tocca il nucleo e quella no: la
 * regola si prova con `node --test`, l'ascolto soltanto dal vivo.
 */
import { ipc } from "./ipc";
import { useAscolto } from "./pagine";
import {
  pianoDeiTrascinati,
  type PianoTrascinati,
  type SituazioneTrascinati,
} from "./trascinati";

/**
 * Ascolta i file lasciati cadere sulla finestra, e ne fa un piano.
 *
 * `situazione` e `esegui` sono quelli del disegno in cui i file sono caduti:
 * la situazione si legge quando il nucleo ha risposto, ma è quella di quel
 * momento, come quando il gestore stava scritto per intero in `App`.
 * `acceso` spegne l'ascolto: un file lasciato sullo Studio non va in coda né
 * fra le cartelle, perché sotto c'è un editor e quel che ci si lascia cadere è
 * per lui.
 */
export function useTrascinati(
  acceso: boolean,
  situazione: () => SituazioneTrascinati,
  esegui: (piano: PianoTrascinati) => void,
  onErrore: (e: unknown) => void,
): void {
  useAscolto<{ paths: string[] }>(
    "tauri://drag-drop",
    ({ paths: arrivati }) => {
      if (arrivati.length === 0) return;
      ipc
        .smistaTrascinati(arrivati)
        .then((smistati) => esegui(pianoDeiTrascinati(smistati, situazione())))
        .catch(onErrore);
    },
    acceso,
  );
}
