/**
 * Lo stato della riproduzione, dal lato della finestra.
 *
 * Non decide niente: il nucleo manda `riproduzione:stato` a ogni cambiamento e
 * questo modulo lo conserva. L'unica cosa che aggiunge è il tempo fra un colpo
 * e l'altro, per il motivo scritto sotto.
 */
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  useCallback,
  useEffect,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";

import {
  ipc,
  type ErroreIpc,
  type StatoEq,
  type StatoRiproduzione,
  type Tempo,
} from "./ipc";

/**
 * Ogni quanto la posizione interpolata arriva a React.
 *
 * Il nucleo manda la posizione quattro volte al secondo — `PASSO_TEMPO` in
 * `riproduzione.rs` — e lascia alla finestra il compito di riempire i buchi:
 * sessanta eventi al secondo attraverso l'IPC sarebbero sessanta
 * serializzazioni per spostare un pixel.
 *
 * Riempirli a ogni fotogramma però sarebbe lo stesso spreco spostato di un
 * lato: sessanta render al secondo per una barra che su un brano di quattro
 * minuti avanza di due pixel al secondo. A 50 ms il passo è un decimo di pixel,
 * sotto la soglia di ciò che si vede e sopra quella di ciò che costa.
 */
const PASSO_INTERPOLAZIONE = 50;

/** Nessun brano, nessuna coda: com'è prima che il nucleo risponda. */
const FERMO: StatoRiproduzione = {
  brano: null,
  inPausa: true,
  posizioneMs: 0,
  durataMs: 0,
  shuffle: false,
  ripeti: "off",
  volume: 1,
  muto: false,
  coda: [],
  posizioneCoda: null,
  eqAttivo: false,
  eqGuadagni: [],
  // Acceso, come il motore: il valore di ripiego finché il nucleo non risponde
  // deve dire quel che sta succedendo davvero, non la posizione più prudente.
  replaygain: true,
};

// ── La posizione, fuori da React ────────────────────────────────────────────
//
// # Perché non è uno `useState`
//
// Lo era, e stava in `App`. Venti volte al secondo — `PASSO_INTERPOLAZIONE` —
// quello stato cambiava, e con lui si rifaceva `App` **per intero**: `testa()`,
// `corpo()` che costruisce le duecento righe dell'elenco, l'oggetto `slot`, e
// `contesto`, il cui `useMemo` aveva `posizioneMs` fra le dipendenze e quindi
// non serviva a niente. React evitava di riscrivere il DOM, ma riconciliava
// duecento righe per otto elementi venti volte al secondo per muovere una barra
// di avanzamento — mentre `disegno-ux.md §9` dichiara che «nessun componente
// deve chiedere niente a quel ritmo».
//
// Con un archivio esterno si ridisegna **solo** chi la legge, che è una foglia
// sola: `Scrubber`. Tutto il resto della catena — `Lettore`, `Colonna`,
// `InRiproduzione`, `Impaginazione`, `App` — non la vede più passare.
//
// Vive nel modulo e non in un contesto perché di riproduzioni ce n'è una per
// finestra: `useRiproduzione` si chiama una volta sola, in `App`, ed è quella
// chiamata che alimenta questo archivio.

let posizioneCorrente = 0;
const ascoltatori = new Set<() => void>();

function iscrivi(avvisa: () => void): () => void {
  ascoltatori.add(avvisa);
  return () => {
    ascoltatori.delete(avvisa);
  };
}

function leggi(): number {
  return posizioneCorrente;
}

function pubblica(ms: number): void {
  if (ms === posizioneCorrente) return;
  posizioneCorrente = ms;
  for (const avvisa of ascoltatori) avvisa();
}

/**
 * La posizione **adesso**, senza iscriversi.
 *
 * Per chi la legge in risposta a un gesto invece che per disegnarla: le frecce
 * della tastiera vogliono sapere da dove saltare, non essere svegliate ogni
 * cinquanta millisecondi.
 */
export function posizioneAdesso(): number {
  return posizioneCorrente;
}

/**
 * Segue la posizione interpolata.
 *
 * La chiama **solo** chi la disegna. Chiamarla in un componente che la passa
 * più in basso rimetterebbe esattamente il difetto che questo archivio esiste
 * per togliere.
 */
export function usePosizioneMs(): number {
  return useSyncExternalStore(iscrivi, leggi, leggi);
}

/** Quel che la finestra sa della riproduzione. */
export interface Riproduzione {
  /** L'ultimo stato mandato dal nucleo. */
  stato: StatoRiproduzione;
  /**
   * Il dispositivo audio si è aperto.
   *
   * Quando è `false` non c'è niente da comandare: `StatoLettore::avvia`
   * conserva il guasto invece di far cadere l'avvio, e l'applicazione resta un
   * catalogo consultabile. La barra si toglie di mezzo e il motivo si legge
   * nell'errore.
   */
  disponibile: boolean;
  /**
   * L'ultimo guasto della riproduzione.
   *
   * `unknown` e non `ErroreIpc`: dal canale degli eventi arriva sempre un
   * record del nucleo, ma dal rifiuto di `riproduzioneStato` può arrivare
   * qualunque cosa, e `testoErrore` sa già leggere entrambi. Dichiararlo
   * `ErroreIpc` sarebbe una promessa che questo modulo non può mantenere.
   */
  errore: unknown;
  /** Scarta l'errore mostrato. */
  scartaErrore: () => void;
}

/** Segue la riproduzione per tutta la vita della finestra. */
export function useRiproduzione(): Riproduzione {
  const [stato, setStato] = useState<StatoRiproduzione>(FERMO);
  const [disponibile, setDisponibile] = useState(true);
  const [errore, setErrore] = useState<unknown>(null);

  // Da dove contare e da quando. In un ref e non nello stato: lo legge il ciclo
  // dei fotogrammi, che non deve far ridisegnare niente per sapere che ora è.
  const ancora = useRef({ ms: 0, quando: 0, inPausa: true, durataMs: 0 });

  const ancoraggio = useCallback((tempo: Tempo) => {
    ancora.current = {
      ms: tempo.posizioneMs,
      quando: performance.now(),
      inPausa: tempo.inPausa,
      durataMs: tempo.durataMs,
    };
    pubblica(tempo.posizioneMs);
  }, []);

  // Lo stato all'apertura. La coda di ieri è già stata ripresa dal nucleo in
  // `riprendi_coda`, ma la finestra non c'era: senza questa chiamata la barra
  // resterebbe vuota fino al primo comando.
  useEffect(() => {
    ipc
      .riproduzioneStato()
      .then((iniziale) => {
        setStato(iniziale);
        ancoraggio(iniziale);
      })
      .catch((e: unknown) => {
        setDisponibile(false);
        setErrore(e);
      });
  }, [ancoraggio]);

  useEffect(() => {
    const iscrizioni: Promise<UnlistenFn>[] = [
      listen<StatoRiproduzione>("riproduzione:stato", (evento) => {
        setStato(evento.payload);
        ancoraggio(evento.payload);
      }),
      listen<Tempo>("riproduzione:tempo", (evento) => ancoraggio(evento.payload)),
      // La curva da sola. Non tocca l'ancora del tempo: il nucleo la manda
      // proprio per non dover comporre lo stato intero a ogni cursore mosso, e
      // riancorare qui rifarebbe il lavoro dall'altro lato.
      listen<StatoEq>("riproduzione:eq", (evento) =>
        setStato((prima) => ({
          ...prima,
          eqAttivo: evento.payload.attivo,
          eqGuadagni: evento.payload.guadagni,
        })),
      ),
      listen<ErroreIpc>("riproduzione:errore", (evento) =>
        setErrore(evento.payload),
      ),
    ];
    return () => {
      for (const iscrizione of iscrizioni) {
        void iscrizione.then((stop) => stop());
      }
    };
  }, [ancoraggio]);

  useEffect(() => {
    let fotogramma = 0;
    let ultima = -1;
    const passo = () => {
      fotogramma = requestAnimationFrame(passo);
      const { ms, quando, inPausa, durataMs } = ancora.current;
      // Fermo: la posizione è quella dell'ancora, e l'ha già scritta
      // `ancoraggio`. Continuare a calcolarla la farebbe avanzare in pausa.
      if (inPausa) return;
      const stimata = ms + (performance.now() - quando);
      const limitata = durataMs > 0 ? Math.min(stimata, durataMs) : stimata;
      const quantizzata =
        Math.round(limitata / PASSO_INTERPOLAZIONE) * PASSO_INTERPOLAZIONE;
      if (quantizzata !== ultima) {
        ultima = quantizzata;
        pubblica(quantizzata);
      }
    };
    fotogramma = requestAnimationFrame(passo);
    return () => cancelAnimationFrame(fotogramma);
  }, []);

  const scartaErrore = useCallback(() => setErrore(null), []);

  return { stato, disponibile, errore, scartaErrore };
}
