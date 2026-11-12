/**
 * L'apertura della libreria e dell'uscita audio, viste dalla finestra.
 *
 * # Perché la finestra aspetta
 *
 * Fino alla 3.2.0 la libreria si apriva nel `setup`, sul filo principale,
 * prima che esistesse una finestra: una migrazione lunga — la copia di prima e
 * gli schemi nuovi su un database da un gigabyte — era un processo invisibile,
 * e un secondo doppio clic finiva all'istanza unica, ferma sullo stesso filo
 * (R06b-5). Lo stesso per il dispositivo audio, che un driver piantato teneva
 * fermo fino a venti secondi (R23-9). Adesso tutte e due si aprono su fili
 * loro, e la finestra compare subito.
 *
 * Il resto dell'avvio — i fili di sottofondo, il lettore, il vassoio — parte
 * quando la libreria è aperta, e solo allora la finestra vera si disegna: è la
 * regola che c'era, «quando il `setup` è finito, gli stati ci sono», con
 * un'attesa che adesso si vede. Le funzioni qui sotto sono le decisioni pure,
 * provate in `test/apertura.test.ts`; il componente è `AttesaDellaLibreria.tsx`.
 */
import type { FaseLibreria } from "./ipc";

/** L'avvio è finito: la libreria si è aperta, o non si aprirà. */
export function finita(fase: FaseLibreria | null): boolean {
  return fase !== null && (fase.fase === "pronta" || fase.fase === "guasto");
}

/**
 * La fase da tenere fra quella che c'è e una appena arrivata.
 *
 * La risposta di `libreriaFase` e l'evento `libreria:stato` possono incrociarsi:
 * una domanda partita durante l'apertura può tornare dopo l'evento che l'ha
 * chiusa. Una fine non torna indietro; fra le fasi d'attesa vince l'ultima.
 */
export function prossima(ora: FaseLibreria | null, arrivata: FaseLibreria): FaseLibreria {
  if (ora !== null && finita(ora)) return ora;
  return arrivata;
}

/**
 * Il nome della copia che un aggiornamento fa prima di migrare: lo stesso di
 * `db::nome_istantanea`, `rymembra.db.schema-038.bak`.
 */
export function copiaDi(da: number): string {
  return `rymembra.db.schema-${String(da).padStart(3, "0")}.bak`;
}

/**
 * L'uscita audio si sta ancora aprendo (`playback.engineOpening`): non è un
 * guasto da mostrare, e la barra del lettore non va spenta. Lo stato arriva
 * da sé, con `riproduzione:stato`, appena l'uscita è aperta.
 */
export function apreAncora(errore: unknown): boolean {
  // Il controllo di `eErroreIpc` ridotto al campo che serve: questo modulo
  // non importa niente a tempo d'esecuzione, così le prove lo leggono senza
  // tirarsi dietro le lingue.
  return (
    typeof errore === "object" &&
    errore !== null &&
    "code" in errore &&
    errore.code === "playback.engineOpening"
  );
}
