/**
 * I servizi del cloud e i conti collegati, per tutta la vita della finestra.
 *
 * # Perché un fornitore, e non una lettura in chi li mostra
 *
 * Li guardano in due: la finestrella «Aggiungi una cartella» e la scheda
 * «Account cloud» delle Impostazioni. La prima deve essere **istantanea** —
 * le linguette dei servizi e il conto già scelto al primo fotogramma — e una
 * domanda al nucleo a ogni apertura sarebbe un fotogramma con la sola
 * «Questo PC» e poi le altre che compaiono. La seconda deve vedere i conti
 * cambiare mentre la si guarda: un token revocato, un elenco che finisce,
 * arrivano con l'evento `nuvola:conti`, e un ascolto aperto solo mentre la
 * scheda c'è perderebbe quel che succede mentre non c'è.
 *
 * È la stessa forma di `FornitoreNuvola` in `nuvola.ts`, e la stessa ragione
 * per non stare in `App`: un grappolo con un ingresso — il nucleo — e due
 * uscite, che ridisegnare la finestra intera a ogni cambio di un conto non
 * servirebbe a nessuno.
 *
 * # Perché i guasti non vanno nella fascia
 *
 * La lettura parte da sola all'avvio, senza che nessuno l'abbia chiesta. Se
 * fallisce — un nucleo che quel comando non lo conosce ancora, un portachiavi
 * che non risponde — una fascia rossa all'apertura del programma
 * racconterebbe un guasto di una cosa che chi ascolta non ha toccato. Il
 * guasto resta qui (`guasto`), e lo dice chi lo mostra, dentro la sua scheda o
 * la sua finestrella, con un «Riprova» accanto.
 *
 * # E le app dell'utente
 *
 * Dalla 3.2 OneDrive, Google Drive e Dropbox si collegano con un'app che
 * l'utente registra da sé (`appNuvola.ts`), e le guardano gli stessi posti più
 * il backup: la finestrella per sapere se una linguetta va prima configurata,
 * le Impostazioni per elencarle. Stanno qui per le stesse due ragioni dei
 * conti, e arrivano con loro. Un'app cambiata cambia anche i servizi — uno è
 * disponibile se la sua piattaforma ha un'app —, quindi `nuvola:app` li fa
 * rileggere.
 */
import {
  createContext,
  createElement,
  useCallback,
  useContext,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { conDisponibili } from "./appNuvola";
import { ipc, type AppNuvolaIpc, type ContoNuvola, type ServizioOfferto } from "./ipc";
import { useAscolto } from "./pagine";

/** Quel che la finestra sa dei servizi, dei conti e delle app. */
export interface ContiNuvola {
  /** I servizi di questa build, nell'ordine in cui mostrarli; `null` finché non arrivano. */
  servizi: ServizioOfferto[] | null;
  /** I conti collegati; `null` finché non arrivano. */
  conti: ContoNuvola[] | null;
  /** Le piattaforme e le app dell'utente; `null` finché non arrivano. */
  app: AppNuvolaIpc | null;
  /** Perché l'ultima lettura non è riuscita, o `null`. */
  guasto: unknown;
  /** Rilegge servizi e conti: il «Riprova» di chi mostra il guasto. */
  ricarica: () => void;
  /**
   * Un conto appena collegato, scritto subito.
   *
   * `nuvolaContoCollega` lo restituisce, e l'evento arriverà; ma fra i due ci
   * sarebbe un istante in cui la finestrella che ha appena collegato non trova
   * il conto che ha in mano, e mostrerebbe di nuovo «Collega».
   */
  accogli: (conto: ContoNuvola) => void;
  /**
   * Le app appena salvate o tolte, scritte subito: la risposta di
   * `nuvolaAppSalva` e `nuvolaAppTogli`.
   *
   * Per la stessa ragione di `accogli`, e con i servizi aggiornati insieme:
   * senza, la linguetta appena configurata mostrerebbe ancora il modulo
   * dell'app fino alla rilettura.
   */
  accogliApp: (app: AppNuvolaIpc) => void;
}

/** Il conto al suo posto nell'elenco: sostituito se c'era, in fondo se è nuovo. */
function conIlConto(conti: ContoNuvola[] | null, conto: ContoNuvola): ContoNuvola[] {
  const elenco = conti ?? [];
  return elenco.some((c) => c.conto === conto.conto)
    ? elenco.map((c) => (c.conto === conto.conto ? conto : c))
    : [...elenco, conto];
}

/** Vedi il commento in testa. Va chiamato una volta, in `FornitoreContiNuvola`. */
function useContiNuvola(): ContiNuvola {
  const [servizi, setServizi] = useState<ServizioOfferto[] | null>(null);
  const [conti, setConti] = useState<ContoNuvola[] | null>(null);
  const [app, setApp] = useState<AppNuvolaIpc | null>(null);
  const [guasto, setGuasto] = useState<unknown>(null);
  /** Una lettura in corso: un secondo «Riprova» non ne apre un'altra. */
  const inLettura = useRef(false);

  const ricarica = useCallback(() => {
    if (inLettura.current) return;
    inLettura.current = true;
    setGuasto(null);
    Promise.all([ipc.nuvolaServizi(), ipc.nuvolaConti(), ipc.nuvolaApp()])
      .then(([offerti, collegati, registrate]) => {
        setServizi(offerti);
        setConti(collegati);
        setApp(registrate);
      })
      .catch((e: unknown) => setGuasto(e))
      .finally(() => {
        inLettura.current = false;
      });
  }, []);

  /**
   * Le app nuove, e subito i servizi che ne discendono; poi la parola del
   * nucleo sui servizi, che è quella che vale.
   *
   * La rilettura non tocca `guasto`: se fallisce, i servizi restano quelli
   * appena dedotti — che dicono la stessa cosa — e una fascia per una lettura
   * che nessuno ha chiesto sarebbe di troppo (vedi il commento in testa).
   */
  const accogliApp = useCallback((registrate: AppNuvolaIpc) => {
    setApp(registrate);
    setServizi((prima) => (prima === null ? prima : conDisponibili(prima, registrate.app)));
    ipc.nuvolaServizi().then(setServizi, () => undefined);
  }, []);

  /** Le app di adesso, rilette: la fotografia dopo che il loro ascolto è aperto. */
  const rileggiApp = useCallback(() => {
    ipc.nuvolaApp().then(accogliApp, () => undefined);
  }, [accogliApp]);

  // La fotografia **dopo** che l'ascolto è aperto: il `poi` di `useAscolto`
  // esiste per questo — un cambiamento arrivato fra le due cose andrebbe perso.
  useAscolto<ContoNuvola[]>("nuvola:conti", setConti, true, ricarica);
  // Due ascolti e due fotografie: si aprono ognuno per conto suo, e un'app
  // salvata fra l'apertura del primo e quella del secondo sfuggirebbe a una
  // fotografia sola.
  useAscolto<AppNuvolaIpc>("nuvola:app", accogliApp, true, rileggiApp);

  const accogli = useCallback((conto: ContoNuvola) => {
    setConti((prima) => conIlConto(prima, conto));
  }, []);

  return useMemo(
    () => ({ servizi, conti, app, guasto, ricarica, accogli, accogliApp }),
    [servizi, conti, app, guasto, ricarica, accogli, accogliApp],
  );
}

const ContiDellaFinestra = createContext<ContiNuvola | null>(null);

/** Tiene servizi, conti e app del cloud per tutta la finestra: vedi il commento in testa. */
export function FornitoreContiNuvola({ children }: { children: ReactNode }) {
  return createElement(ContiDellaFinestra, { value: useContiNuvola() }, children);
}

/** Servizi, conti e app del cloud, da sotto `FornitoreContiNuvola`. */
export function useContiNuvolaDellaFinestra(): ContiNuvola {
  const conti = useContext(ContiDellaFinestra);
  if (conti === null) {
    throw new Error("useContiNuvolaDellaFinestra fuori da FornitoreContiNuvola");
  }
  return conti;
}
