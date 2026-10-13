/**
 * Il backup su Drive, la sincronia e l'arricchimento, dal lato della finestra.
 *
 * Non decidono niente: i tre fili girano nel nucleo, mandano un evento a ogni
 * cambiamento, e questo modulo conserva l'ultima parola di ciascuno. Quel che
 * aggiunge sono le due funzioni in fondo — `conNuvola` e `conSincronia` — che
 * sono il modo in cui un comando si riflette sul pulsante che lo ha chiesto.
 *
 * # Perché fuori da `App`, e in un fornitore
 *
 * Perché questo grappolo ha **un solo ingresso e una sola uscita**. In ingresso
 * c'è `segnalaErrore` e basta: tre letture all'avvio e due comandi che, se
 * cadono, hanno solo quello da dire. In uscita ci sono le Impostazioni e
 * basta: i cinque stati e le due funzioni scendono lì e in nessun altro punto
 * dell'albero — niente pallino nella navigazione, niente riga nel lettore.
 * Un grappolo con un ingresso e un'uscita è una scatola già chiusa.
 *
 * Stava in `App`, chiamato come hook, e ogni passo di un arricchimento — un
 * evento per gruppo d'album — ridisegnava la finestra intera per una barra che
 * si guarda solo dalle Impostazioni. Adesso lo tiene `FornitoreNuvola`, montato
 * una volta fra i fornitori di `App`, e lo legge chi lo mostra
 * (`useNuvolaDellaFinestra`). Non dentro le Impostazioni: quelle si smontano
 * uscendo dalla pagina, e con loro se ne andrebbe l'esito dell'ultima passata
 * di questa sessione, che è proprio quel che si torna a guardare.
 *
 * Anche il tema e la selezione, che una volta erano rimasti in `App` perché
 * non avevano questa forma, adesso stanno fuori — in `aspettoFinestra.ts` e
 * in `selezioneVista.ts` — ma come hook di `App` e non come fornitori: i loro
 * ingressi sono stati di `App`, e lì è scritto perché.
 */
import {
  createContext,
  createElement,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useState,
  type Dispatch,
  type ReactNode,
  type SetStateAction,
} from "react";

import {
  ipc,
  type AvanzamentoArricchimento,
  type EsitoArricchimento,
  type StatoArricchimento,
  type StatoNuvola,
  type StatoSincronia,
} from "./ipc";
import { useAscolto } from "./pagine";

/** Quel che la finestra sa dei tre fili di sfondo, e come li comanda. */
export interface Nuvola {
  /** Lo stato del backup su Drive, o `null` finché non è stato chiesto. */
  nuvola: StatoNuvola | null;
  /** Lo stato della sincronia, o `null` finché non è stato chiesto. */
  sincronia: StatoSincronia | null;
  /**
   * Riscrive lo stato della sincronia senza passare da un comando.
   *
   * Serve agli aggiornamenti ottimistici che non hanno la forma di
   * `conSincronia` — l'accoppiamento e l'oblio di un dispositivo tornano
   * l'elenco dei dispositivi, non lo stato intero.
   */
  setSincronia: Dispatch<SetStateAction<StatoSincronia | null>>;
  /** Lo stato dell'arricchimento, o `null` finché non è stato chiesto. */
  arricchimento: StatoArricchimento | null;
  /**
   * Riscrive lo stato dell'arricchimento senza passare da un comando.
   *
   * Come sopra: l'interruttore e l'annullamento hanno ciascuno la propria
   * risposta, e nessuna delle due è quella di `conNuvola`.
   */
  setArricchimento: Dispatch<SetStateAction<StatoArricchimento | null>>;
  /** A che punto è la passata in corso, o `null` quando non ne gira nessuna. */
  avanzaArricchimento: AvanzamentoArricchimento | null;
  /** Cosa ha prodotto l'ultima passata di questa sessione. */
  esitoArricchimento: EsitoArricchimento | null;
  /** Un comando del backup: aggiorna lo stato, o mostra perché non ci riesce. */
  conNuvola: (azione: () => Promise<StatoNuvola>) => void;
  /** Un comando della sincronia: aggiorna lo stato, o mostra perché non ci riesce. */
  conSincronia: (azione: () => Promise<StatoSincronia>) => void;
}

/**
 * Segue backup, sincronia e arricchimento per tutta la vita della finestra.
 *
 * Va chiamato una volta sola, in `FornitoreNuvola`. Le tre letture d'avvio e i
 * cinque ascolti partono quando il fornitore si monta, cioè prima degli
 * effetti di `App` che lo contiene: stavano dopo l'avanzamento della scansione,
 * ma nessuno degli effetti di `App` fa scattare un evento di questi tre fili,
 * e le tre domande al nucleo sono indipendenti da quelle di `App`.
 */
export function useNuvola(segnalaErrore: (e: unknown) => void): Nuvola {
  const [nuvola, setNuvola] = useState<StatoNuvola | null>(null);
  const [sincronia, setSincronia] = useState<StatoSincronia | null>(null);
  const [arricchimento, setArricchimento] =
    useState<StatoArricchimento | null>(null);
  const [avanzaArricchimento, setAvanzaArricchimento] =
    useState<AvanzamentoArricchimento | null>(null);
  const [esitoArricchimento, setEsitoArricchimento] =
    useState<EsitoArricchimento | null>(null);

  /**
   * Lo stato del backup: una sola sorgente, come per la riproduzione.
   *
   * Si chiede una volta all'avvio e poi si **ascolta**: il filo di sottofondo
   * salva per conto suo, e una schermata che si aggiornasse solo quando la si
   * apre mostrerebbe l'ora dell'ultimo salvataggio di quando l'hai guardata,
   * non di adesso.
   *
   * Le due metà stanno separate perché hanno due vite diverse. La lettura
   * dipende da `segnalaErrore` — se cambia chi raccoglie i guasti, la si rifà —
   * mentre l'ascolto dura quanto la finestra e se lo prende `useAscolto`, che
   * apre un ascoltatore solo: il perché sta scritto per esteso accanto a lui,
   * in `pagine.ts`.
   */
  useEffect(() => {
    ipc.nuvolaStato().then(setNuvola).catch(segnalaErrore);
  }, [segnalaErrore]);

  useAscolto<StatoNuvola>("nuvola:stato", setNuvola);

  /**
   * Lo stato della sincronia, con la stessa disciplina del backup.
   *
   * Si chiede una volta e poi si **ascolta**, e qui conta più che altrove: la
   * sincronia scrive nella libreria da sola, e la schermata deve poter dire
   * cosa è arrivato mentre la si guardava. Lettura di qua, ascolto di là, per
   * la stessa ragione appena detta.
   */
  useEffect(() => {
    ipc.sincroniaStato().then(setSincronia).catch(segnalaErrore);
  }, [segnalaErrore]);

  useAscolto<StatoSincronia>("sincronia:stato", setSincronia);

  /**
   * Lo stato dell'arricchimento, con la stessa disciplina del backup.
   *
   * Si chiede una volta e poi si **ascolta**, per la stessa ragione: il filo
   * lavora per conto suo, e la sezione aperta mentre una passata gira deve
   * vedere i numeri salire invece di restare a quelli di quando l'hai aperta.
   *
   * Gli eventi sono tre e sono tre ascolti: uno per canale, ciascuno aperto una
   * volta sola, invece di una lista di promesse da sciogliere a mano.
   */
  useEffect(() => {
    ipc.arricchimentoStato().then(setArricchimento).catch(segnalaErrore);
  }, [segnalaErrore]);

  useAscolto<StatoArricchimento>("arricchimento:stato", (stato) => {
    setArricchimento(stato);
    // Lo stato è l'ultima parola su «sta girando»: una passata caduta a metà —
    // il database che non risponde, la finestra che si chiude — non manda
    // l'ultimo passo, e senza questa riga la barra resterebbe ferma a 3/12 per
    // sempre.
    if (!stato.inCorso) setAvanzaArricchimento(null);
  });

  useAscolto<AvanzamentoArricchimento>("arricchimento:avanzamento", (passo) =>
    // L'ultimo passo di una passata è `fatti === totale`, ed è anche il segnale
    // che è finita: tenerlo mostrato lascerebbe una barra piena sotto uno stato
    // che dice «ferma».
    setAvanzaArricchimento(passo.fatti >= passo.totale ? null : passo),
  );

  useAscolto<EsitoArricchimento>("arricchimento:esito", (esitoNuovo) => {
    setEsitoArricchimento(esitoNuovo);
    setAvanzaArricchimento(null);
  });

  /** Un comando del backup: aggiorna lo stato, o mostra perché non ci riesce. */
  const conNuvola = useCallback(
    (azione: () => Promise<StatoNuvola>) => {
      // Ottimistico su `inCorso`: `nuvolaCollega` apre un browser e può metterci
      // tre minuti, e senza questo il tasto resterebbe premibile per tutto quel
      // tempo — con il risultato che chi non vede succedere niente clicca due
      // volte e si prende un `sync.busy`.
      setNuvola((prima) => (prima ? { ...prima, inCorso: true } : prima));
      azione()
        .then(setNuvola)
        .catch((e: unknown) => {
          setNuvola((prima) => (prima ? { ...prima, inCorso: false } : prima));
          segnalaErrore(e);
        });
    },
    [segnalaErrore],
  );

  /** Un comando della sincronia: aggiorna lo stato, o mostra perché non ci riesce. */
  const conSincronia = useCallback(
    (azione: () => Promise<StatoSincronia>) => {
      setSincronia((prima) => (prima ? { ...prima, inCorso: true } : prima));
      azione()
        .then(setSincronia)
        .catch((e: unknown) => {
          setSincronia((prima) =>
            prima ? { ...prima, inCorso: false } : prima,
          );
          segnalaErrore(e);
        });
    },
    [segnalaErrore],
  );

  // La stessa identità finché niente cambia: è il valore di un contesto.
  return useMemo(
    () => ({
      nuvola,
      sincronia,
      setSincronia,
      arricchimento,
      setArricchimento,
      avanzaArricchimento,
      esitoArricchimento,
      conNuvola,
      conSincronia,
    }),
    [
      nuvola,
      sincronia,
      arricchimento,
      avanzaArricchimento,
      esitoArricchimento,
      conNuvola,
      conSincronia,
    ],
  );
}

/** Backup, sincronia e arricchimento della finestra: vedi `FornitoreNuvola`. */
const NuvolaDellaFinestra = createContext<Nuvola | null>(null);

/**
 * Tiene backup, sincronia e arricchimento per tutta la finestra, e li dà a chi
 * sta sotto: vedi «Perché fuori da `App`, e in un fornitore».
 */
export function FornitoreNuvola({
  segnalaErrore,
  children,
}: {
  segnalaErrore: (e: unknown) => void;
  children: ReactNode;
}) {
  return createElement(NuvolaDellaFinestra, { value: useNuvola(segnalaErrore) }, children);
}

/** Backup, sincronia e arricchimento, da sotto `FornitoreNuvola`. */
export function useNuvolaDellaFinestra(): Nuvola {
  const nuvola = useContext(NuvolaDellaFinestra);
  if (nuvola === null) throw new Error("useNuvolaDellaFinestra fuori da FornitoreNuvola");
  return nuvola;
}
