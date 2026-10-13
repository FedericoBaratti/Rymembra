/**
 * Quel che la finestra sa della libreria: `avvio`, le playlist, la scansione
 * e il suo esito.
 *
 * # Perché fuori da `App`
 *
 * Stavano in quattro `useState` di `App`, e ciascuno ridisegnava la finestra
 * intera quando cambiava. Due di loro cambiano spesso: l'avanzamento della
 * scansione arriva quattro volte al secondo per tutta la durata di una
 * passata, e ogni arrivo rifaceva elenchi, griglie, colonna e Impostazioni per
 * spostare una barra in un angolo. Gli altri due cambiano di rado, ma li legge
 * chi non ha niente a che fare con il resto: la navigazione vuole i conteggi,
 * la finestrella «Aggiungi a playlist» le playlist, le Impostazioni l'esito.
 *
 * # Quattro archivi, e non uno con le scelte
 *
 * Uno solo con dentro tutto, letto con una scelta per ciascuno, sarebbe stato
 * corretto: chi sceglie i conteggi non si ridisegna per una playlist nuova. Ma
 * ogni scrittura sveglierebbe comunque **tutti** gli iscritti per fargli
 * rifare la loro scelta — quattro volte al secondo durante una scansione — e
 * le quattro cose non hanno niente in comune se non il nome: le scrivono
 * gesti diversi, in momenti diversi, e nessuna lettura ne vuole due insieme.
 * Separati, una scrittura sveglia solo chi legge quella cosa. La scelta resta
 * dove serve davvero: i conteggi della navigazione si ricavano da `avvio`
 * (`conteggiDi`), e un `avvio` riletto con gli stessi numeri non li cambia.
 *
 * # Vince l'ultima
 *
 * `ricarica` e `ricaricaPlaylist` portano qui la regola di `ultima.ts`: si
 * rilegge in risposta agli eventi e dopo ogni gesto, due richieste
 * ravvicinate possono tornare nell'ordine sbagliato, e la risposta — o
 * l'errore — di una richiesta superata si butta. `test/libreria.test.ts` lo
 * prova senza nucleo.
 *
 * # La scansione e le transizioni
 *
 * L'avanzamento è in un archivio come il resto, ma chi lo disegna non lo
 * legge con `useArchivio`: la barra nella pila dei toast entra ed esce con una
 * transizione di vista, e un aggiornamento da `useSyncExternalStore` non ne
 * accende. Lo legge con `useSpecchioPresente` (`presenza.tsx`).
 *
 * Senza niente del nucleo a runtime: le domande arrivano da fuori (`Domande`),
 * e le prove le fingono.
 */
import { createContext, useContext } from "react";

import { creaArchivio, type Archivio } from "./archivio";
import type { Avanzamento, Avvio, EsitoScansione, Playlist } from "./ipc";
import type { Vista } from "./parti/Navigazione";
import { vinceLUltima } from "./ultima";

/** Le due domande al nucleo che la libreria rifà. */
export interface Domande {
  avvio: () => Promise<Avvio>;
  playlistElenco: () => Promise<Playlist[]>;
}

/** Quel che la libreria fa sapere a chi la usa. */
export interface Avvisi {
  /** Una lettura è fallita, ed era ancora quella buona. */
  onErrore: (e: unknown) => void;
  /**
   * `avvio` si è letto: il guasto mostrato, se c'era, non vale più. È quel
   * che `ricarica` faceva in `App` con `setErrore(null)`.
   */
  onLetta: () => void;
}

/** La libreria della finestra. */
export interface Libreria {
  /** Conteggi, cartelle, preferenze. `null` finché il nucleo non risponde. */
  avvio: Archivio<Avvio | null>;
  /** Le playlist, nell'ordine del nucleo. */
  playlist: Archivio<Playlist[]>;
  /** La scansione in corso, o `null`. Vedi «La scansione e le transizioni». */
  scansione: Archivio<Avanzamento | null>;
  /** Com'è andata l'ultima scansione chiesta da questa finestra. */
  esito: Archivio<EsitoScansione | null>;
  /** Rilegge `avvio`. Vince l'ultima. */
  ricarica: () => Promise<void>;
  /** Rilegge le playlist. Vince l'ultima. */
  ricaricaPlaylist: () => Promise<void>;
}

/** Una libreria nuova: vuota, finché qualcuno non chiede. */
export function creaLibreria(domande: Domande, avvisi: Avvisi): Libreria {
  const avvio = creaArchivio<Avvio | null>(null);
  const playlist = creaArchivio<Playlist[]>([]);
  const turnoAvvio = vinceLUltima();
  const turnoPlaylist = vinceLUltima();
  return {
    avvio,
    playlist,
    scansione: creaArchivio<Avanzamento | null>(null),
    esito: creaArchivio<EsitoScansione | null>(null),
    /*
     * Vince l'ultima (`ultima.ts`): si rilegge in risposta agli eventi — una
     * scansione finita, dei brani scaricati — e a mano dopo ogni gesto sulle
     * cartelle, e due richieste ravvicinate possono tornare nell'ordine
     * sbagliato. La risposta, o l'errore, di una richiesta superata si butta:
     * scriverla rimetterebbe a schermo la libreria di un istante prima.
     */
    ricarica: async () => {
      const mio = turnoAvvio();
      try {
        const arrivato = await domande.avvio();
        if (!mio()) return;
        avvio.scrivi(arrivato);
        avvisi.onLetta();
      } catch (e) {
        if (mio()) avvisi.onErrore(e);
      }
    },
    /*
     * Vince l'ultima anche qui: la chiamano gli eventi della libreria e i
     * gesti sulle playlist, e due risposte possono incrociarsi — una playlist
     * appena creata che sparisce perché la lettura di prima è arrivata dopo.
     */
    ricaricaPlaylist: async () => {
      const mio = turnoPlaylist();
      try {
        const elenco = await domande.playlistElenco();
        if (mio()) playlist.scrivi(elenco);
      } catch (e) {
        if (mio()) avvisi.onErrore(e);
      }
    },
  };
}

/**
 * I numeri accanto alle voci della navigazione.
 *
 * Un oggetto vuoto finché `avvio` non c'è: la navigazione allora non scrive
 * numeri, invece di scrivere zeri che non sono veri.
 */
export function conteggiDi(avvio: Avvio | null): Partial<Record<Vista, number>> {
  if (avvio === null) return {};
  const numeri = avvio.numeri;
  return {
    album: numeri.albums,
    artisti: numeri.artists,
    brani: numeri.tracks,
    // Le radici, non i brani: l'albero si costruisce alla prima
    // apertura della vista, quindi finché nessuno l'ha aperta un
    // conteggio di cartelle non esiste — e quello che esiste è pure
    // quello giusto, perché dice quante cartelle sorvegliate ci sono.
    cartelle: avvio.cartelle.length,
    preferiti: numeri.liked,
  };
}

/**
 * La libreria della finestra, per chi la legge più in basso di `App`.
 *
 * `null` di serie, e chi la chiede fuori da `App` lo scopre subito: è un
 * errore di montaggio, non uno stato da disegnare.
 */
export const LibreriaDellaFinestra = createContext<Libreria | null>(null);

/** La libreria della finestra: vedi `LibreriaDellaFinestra`. */
export function useLibreria(): Libreria {
  const libreria = useContext(LibreriaDellaFinestra);
  if (libreria === null) {
    throw new Error("useLibreria fuori da LibreriaDellaFinestra");
  }
  return libreria;
}
