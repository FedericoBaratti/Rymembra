/**
 * Le cartelle sorvegliate, dal lato dei gesti: aggiungerne una, toglierla,
 * scegliere dove finiscono i brani scaricati, e la domanda sul trasloco.
 *
 * Quali cartelle siano, e cosa voglia dire che due sono la stessa, lo decide
 * il nucleo (`radici::ordina_cartelle`). Qui c'è quel che la finestra fa con la
 * sua risposta: rileggere `avvio`, aprire la conferma del trasloco
 * (`ConfermaTrasloco.tsx`) quando arriva una gemella, dire in una riga quando
 * una cartella era già dentro un'altra. Li usano le Impostazioni, lo stato
 * vuoto «nessuna cartella», i file trascinati sulla finestra e la finestrella
 * «Aggiungi una cartella» (`AggiungiCartella.tsx`), che porta anche quelle del
 * cloud.
 *
 * Le funzioni si rifanno a ogni disegno, come quando stavano nel corpo di
 * `App`, ma l'elenco delle cartelle non lo prendono dal disegno: lo leggono
 * dall'archivio della libreria quando il gesto parte, e i gesti che lo
 * riscrivono aspettano in fila il loro turno. Il perché sta in
 * `elencoCartelle.ts`. Chi le passa sotto `memo` le avvolge in `useStabile`.
 */
import { open } from "@tauri-apps/plugin-dialog";
import { useState, type Dispatch, type SetStateAction } from "react";

import { nomeRadice, primaLocale } from "./cartelleNuvola";
import {
  conCartelle,
  creaFila,
  senzaCartella,
  traslocate,
} from "./elencoCartelle";
import { eErroreIpc, ipc, type CartellaGemella, type CartelleOrdinate } from "./ipc";
import type { Libreria } from "./libreria";
import { t } from "./lingue";
import { useStatoPresente } from "./presenza";

/** Quel che le cartelle leggono da `App`. */
export interface IngressiCartelle {
  /**
   * La libreria: `avvio` letto al momento del gesto, e riletto dopo.
   *
   * L'archivio e non il valore del disegno, ed è tutto il punto: vedi
   * `elencoCartelle.ts`.
   */
  libreria: Libreria;
  setNotizia: (testo: string) => void;
  segnalaErrore: (e: unknown) => void;
}

/** I gesti sulle cartelle sorvegliate, e la domanda aperta. */
export interface CartelleSorvegliate {
  /** La gemella che aspetta una risposta: vedi `ConfermaTrasloco.tsx`. */
  gemella: CartellaGemella | null;
  setGemella: Dispatch<SetStateAction<CartellaGemella | null>>;
  /**
   * Chiede una cartella col dialogo di sistema e la aggiunge alle sorvegliate.
   *
   * `true` se se n'è scelta una: «Questo PC» nella finestrella si chiude solo
   * allora, e chi ha annullato il dialogo di sistema resta dove era — magari
   * per scegliere una cartella del cloud.
   */
  scegliCartella: () => Promise<boolean>;
  /** Aggiunge dei percorsi alle sorvegliate: quelli lasciati sulla finestra. */
  aggiungiCartelle: (percorsi: readonly string[]) => Promise<void>;
  /**
   * Aggiunge una cartella del cloud alle sorvegliate, e torna i percorsi che
   * sono entrati davvero.
   *
   * **Lancia** invece di segnalare: il guasto lo dice la finestrella che l'ha
   * chiesto, che resta aperta con la sua scelta — chiuderla e dirlo nella
   * fascia vorrebbe dire rifare la strada nel drive per riprovare.
   */
  aggiungiNuvola: (conto: string, id: string, nomi: string[]) => Promise<string[]>;
  /**
   * Scollega un account del cloud; con `togliCartelle` le sue cartelle escono
   * anche dalle sorvegliate.
   */
  scollegaNuvola: (conto: string, togliCartelle: boolean) => Promise<void>;
  /** Quel che il nucleo ha deciso delle cartelle appena scritte, detto. */
  dopoLeCartelle: (esito: CartelleOrdinate) => void;
  /** Il «sì» della domanda sul trasloco. */
  trasloca: (quale: CartellaGemella) => Promise<void>;
  /** Chiede dove finiscono i brani scaricati. */
  scegliCartellaDownload: () => Promise<void>;
  /** Smette di sorvegliare una cartella. */
  togliCartella: (percorso: string) => Promise<void>;
  /**
   * Un gesto sull'elenco è in corso, o aspetta il suo turno.
   *
   * Lo leggono i tasti delle Impostazioni, che si spengono: la fila rende
   * innocua una seconda pressione, ma un tasto che non dice niente mentre il
   * nucleo lavora — una cartella di rete da risolvere può volerci qualche
   * secondo — viene premuto di nuovo.
   */
  occupate: boolean;
}

/** Vedi il commento in testa. */
export function useCartelleSorvegliate({
  libreria,
  setNotizia,
  segnalaErrore,
}: IngressiCartelle): CartelleSorvegliate {
  /**
   * Una cartella appena scelta che il nucleo ha riconosciuto come una già
   * sorvegliata scritta in un altro modo: vedi `ConfermaTrasloco.tsx`.
   */
  const [gemella, setGemella] = useStatoPresente<CartellaGemella | null>(null);
  /** Quanti gesti sono in fila: vedi `occupate`. */
  const [inFila, setInFila] = useState(0);
  const [fila] = useState(() => creaFila(setInFila));

  /**
   * Scrive l'elenco calcolato da quello vero di adesso, e aggiorna l'archivio
   * con quel che il nucleo ha scritto davvero.
   *
   * Si chiama solo da dentro la fila: fuori, due chiamate leggerebbero lo
   * stesso elenco. L'archivio si aggiorna con la risposta **prima** della
   * rilettura: se la rilettura fallisse, il gesto dopo partirebbe comunque da
   * quel che il nucleo ha detto di avere, e non dall'elenco di prima.
   *
   * `null` quando `avvio` non c'è ancora: non c'è un elenco da cui partire.
   */
  const scriviCartelle = async (
    calcola: (cartelle: readonly string[]) => string[],
  ): Promise<CartelleOrdinate | null> => {
    const adesso = libreria.avvio.leggi();
    if (adesso === null) return null;
    const esito = await ipc.impostaCartelle(calcola(adesso.cartelle));
    libreria.avvio.scrivi((prima) =>
      prima === null ? prima : { ...prima, cartelle: esito.cartelle },
    );
    return esito;
  };

  const aggiungiCartelle = async (percorsi: readonly string[]) => {
    try {
      const esito = await fila(() => scriviCartelle((c) => conCartelle(c, percorsi)));
      // La rilettura fuori dalla fila: l'archivio ha già le cartelle che il
      // nucleo ha scritto, e il gesto dopo parte da quelle. Dentro, i tasti
      // resterebbero spenti anche per tutta la rilettura dell'avvio — che
      // aspetta la libreria, e durante una scansione può volerci un po'.
      await libreria.ricarica();
      if (esito !== null) dopoLeCartelle(esito);
    } catch (e) {
      segnalaErrore(e);
    }
  };

  /**
   * Il dialogo **fuori** dalla fila: può restare aperto un minuto, e un
   * trascinamento nel frattempo non deve aspettare chi sta ancora scegliendo.
   */
  const scegliCartella = async () => {
    const scelta = await open({ directory: true, multiple: false }).catch(
      (e: unknown) => {
        segnalaErrore(e);
        return null;
      },
    );
    if (typeof scelta !== "string") return false;
    await aggiungiCartelle([scelta]);
    return true;
  };

  /**
   * Una cartella del cloud, nella **stessa fila** delle altre.
   *
   * Il nucleo la aggiunge all'elenco che ha lui, ma la finestra scrive l'elenco
   * intero quando toglie o quando arriva un trascinamento: se uno di quei gesti
   * partisse mentre questo è in volo, calcolerebbe dall'elenco di prima — senza
   * la cartella del cloud — e la riscriverebbe via. In fila, e con l'archivio
   * aggiornato dalla risposta prima di lasciare il turno, il gesto dopo parte
   * dall'elenco che la contiene. È la regola di `elencoCartelle.ts`.
   */
  const aggiungiNuvola = async (conto: string, id: string, nomi: string[]) => {
    const { esito, nuove } = await fila(async () => {
      const prima = libreria.avvio.leggi()?.cartelle ?? [];
      const esito = await ipc.nuvolaAggiungi(conto, id, nomi);
      libreria.avvio.scrivi((p) => (p === null ? p : { ...p, cartelle: esito.cartelle }));
      return { esito, nuove: esito.cartelle.filter((c) => !prima.includes(c)) };
    });
    // La rilettura fuori dalla fila, come in `aggiungiCartelle`; e un suo guasto
    // non è un guasto dell'aggiunta, che è già scritta.
    await libreria.ricarica().catch(segnalaErrore);
    if (esito.gemelle.length > 0 || esito.assorbite.length > 0) {
      dopoLeCartelle(esito);
    } else {
      const prima = nuove[0];
      // I brani arrivano a ondate — prima le righe dall'elenco, poi i tag —
      // e chi chiude la finestrella deve sapere che una libreria che si
      // riempie piano è il comportamento giusto, non una scansione bloccata.
      setNotizia(
        prima === undefined
          ? t("folders.add.already")
          : t("folders.add.arriving", { nome: nomeRadice(prima) }),
      );
    }
    return nuove;
  };

  /**
   * Scollegare un conto, nella stessa fila: con le cartelle tolte, il nucleo
   * riscrive l'elenco, e un gesto partito in mezzo lo riscriverebbe con
   * dentro quelle appena tolte. Come per il trasloco, il comando non risponde
   * con l'elenco, e la rilettura sta dentro la fila: è lei che porta
   * nell'archivio l'elenco da cui deve partire il gesto dopo.
   */
  const scollegaNuvola = async (conto: string, togliCartelle: boolean) => {
    try {
      await fila(async () => {
        await ipc.nuvolaContoScollega(conto, togliCartelle);
        await libreria.ricarica();
      });
    } catch (e) {
      segnalaErrore(e);
    }
  };

  /**
   * Quel che il nucleo ha deciso delle cartelle appena scritte, detto a chi le
   * ha scelte.
   *
   * Una gemella — la stessa cartella con un altro nome — apre la domanda sul
   * trasloco; una cartella assorbita — già dentro un'altra — si dice in una
   * riga. Una alla volta: sceglierne due insieme che siano tutte e due così è
   * il caso raro, e la seconda si ritrova riprovando.
   */
  const dopoLeCartelle = (esito: CartelleOrdinate) => {
    const prima = esito.gemelle[0];
    if (prima) {
      setGemella(prima);
      return;
    }
    const assorbita = esito.assorbite[0];
    if (assorbita) {
      setNotizia(
        t("folders.absorbed", {
          cartella: nomeRadice(assorbita.cartella),
          dentro: nomeRadice(assorbita.dentro),
        }),
      );
    }
  };

  /**
   * Il «sì» della domanda sul trasloco.
   *
   * Prima si riscrivono i percorsi dei brani e la cartella sorvegliata; poi,
   * se la cartella scelta **conteneva** quella già sorvegliata, la si aggiunge:
   * adesso le due hanno la stessa grafia, e la madre raccoglie la figlia senza
   * che nessun brano cambi nome una seconda volta.
   */
  const trasloca = async (quale: CartellaGemella) => {
    try {
      const fatto = await fila(async () => {
        const fatto = await ipc.cartelleTrasloca(quale.esistente, quale.a);
        if (quale.a !== quale.nuova) {
          await scriviCartelle((c) => traslocate(c, quale));
        }
        // Questa sì, dentro: il trasloco riscrive le cartelle nel nucleo senza
        // rispondere con l'elenco, e solo la rilettura porta nell'archivio la
        // grafia nuova da cui deve partire il gesto dopo.
        await libreria.ricarica();
        return fatto;
      });
      setGemella(null);
      setNotizia(t("folders.moved", { n: fatto.percorsi }));
    } catch (e) {
      // Con una scansione in corso la domanda resta aperta: la risposta è
      // ancora quella, e chiuderla vorrebbe dire scegliere di nuovo la
      // cartella solo per rivederla. Ogni altro guasto la chiude.
      if (!(eErroreIpc(e) && e.code === "library.scanBusy")) setGemella(null);
      segnalaErrore(e);
    }
  };

  /**
   * Sceglie dove finiscono i brani scaricati.
   *
   * Il dialogo si apre sulla cartella già scelta, o sulla prima sorvegliata:
   * chi la cambia quasi sempre la sposta lì accanto, e ripartire dalla radice
   * del disco ogni volta è la differenza fra due clic e otto.
   */
  const scegliCartellaDownload = async () => {
    try {
      const avvio = libreria.avvio.leggi();
      // La prima **del disco**: una cartella del cloud il dialogo di sistema
      // non la sa aprire, e i download lì non ci finiscono.
      const partenza = avvio?.cartellaDownload ?? primaLocale(avvio?.cartelle ?? []);
      const scelta = await open({
        directory: true,
        multiple: false,
        ...(partenza === undefined ? {} : { defaultPath: partenza }),
      });
      if (typeof scelta !== "string") return;
      await ipc.impostaCartellaDownload(scelta);
      await libreria.ricarica();
    } catch (e) {
      segnalaErrore(e);
    }
  };

  const togliCartella = async (percorso: string) => {
    try {
      await fila(() => scriviCartelle((c) => senzaCartella(c, percorso)));
      // Fuori dalla fila, come in `aggiungiCartelle`.
      await libreria.ricarica();
    } catch (e) {
      segnalaErrore(e);
    }
  };

  return {
    gemella,
    setGemella,
    scegliCartella,
    aggiungiCartelle,
    aggiungiNuvola,
    scollegaNuvola,
    dopoLeCartelle,
    trasloca,
    scegliCartellaDownload,
    togliCartella,
    occupate: inFila > 0,
  };
}
