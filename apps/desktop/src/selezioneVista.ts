/**
 * La selezione multipla della vista che si guarda: lo stato, e i gesti.
 *
 * La regola di ogni clic sta in `selezione.ts`, e si prova da sola; qui c'è
 * quel che la regola non può tenere: la mappa degli scelti, l'àncora, le
 * selezioni messe da parte dalle viste tenute montate, e le pagine da chiedere
 * quando un intervallo o «Seleziona tutto» passano da righe che la finestra non
 * ha in mano.
 *
 * # Perché un hook e non più il corpo di `App`
 *
 * `nuvola.ts` diceva che la selezione era rimasta in `App` perché esce da
 * quasi ogni schermata: le scorciatoie, il contesto dei widget, le azioni di
 * massa, ogni elenco. Resta vero — `App` la riceve e la passa — ma le sue
 * **entrate** sono poche e dette una volta: l'elenco che si guarda, la sua
 * descrizione per il nucleo, e la chiave che dice quando l'elenco cambia. Con
 * quelle, i sette pezzi che la fanno funzionare stanno insieme invece di essere
 * sparsi in mille righe.
 *
 * # L'ordine
 *
 * `App` chiama questo hook dopo aver ricavato l'elenco corrente, che è il punto
 * in cui il suo effetto di disposizione stava già. Lo stato, che prima era
 * dichiarato in cima al componente, nasce adesso qui: un ordine diverso fra
 * gli `useState` non cambia niente di quel che si vede.
 */
import { useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";

import type { Brano, VistaBrani } from "./ipc";
import { useStatoPresente } from "./presenza";
import type { ArchivioRado, Elenco } from "./rada";
import { gestoDelClic, nellOrdineDellaVista } from "./selezione";

/** Quel che la selezione legge dalla pagina. */
export interface IngressiSelezione {
  /** L'elenco che si sta guardando: vedi `elencoCorrente` in `App`. */
  elenco: Elenco<Brano>;
  /** La sua lista rada, quando lo è; `null` per un elenco intero. */
  rada: ArchivioRado<Brano> | null;
  /** Le righe dell'elenco intero (playlist, album); vuoto per uno rado. */
  pieni: Brano[];
  /** L'elenco detto al nucleo, per «Seleziona tutto»; `null` se arriva intero. */
  vistaBrani: VistaBrani | null;
  /** Di quale elenco è la selezione: vedi `chiaveDellaSelezione`. */
  chiave: string;
  /** La pagina è una vista tenuta montata, che ritrova la sua selezione. */
  tenuta: boolean;
  /** Gli identificativi di una vista intera, dal nucleo. */
  idDellaVista: (vista: VistaBrani) => Promise<number[]>;
  onErrore: (e: unknown) => void;
}

/** La selezione, e i gesti che la cambiano. */
export interface Selezione {
  /** Gli scelti, ciascuno con la sua posizione nella vista. */
  scelti: Map<number, number>;
  /** Gli scelti nell'ordine della vista: vedi `nellOrdineDellaVista`. */
  selezionati: number[];
  /** Clic su una riga, con i modificatori. */
  seleziona: (e: React.MouseEvent, indice: number) => void;
  /** Spazio sulla riga col fuoco. */
  alterna: (indice: number) => void;
  /** «Seleziona tutto», e Ctrl+A nell'elenco. */
  selezionaTutto: () => void;
  /** Chiude la selezione. */
  annulla: () => void;
  /** Toglie dei brani dalla selezione e da quelle messe da parte: se ne sono andati. */
  togli: (via: ReadonlySet<number>) => void;
}

/**
 * La selezione della vista che si guarda.
 *
 * Una per finestra, in `App`: è quel che la barra della selezione mostra, e
 * quel che le scorciatoie chiudono.
 */
export function useSelezione({
  elenco,
  rada,
  pieni,
  vistaBrani,
  chiave,
  tenuta,
  idDellaVista,
  onErrore,
}: IngressiSelezione): Selezione {
  /**
   * Gli identificativi selezionati, ciascuno con la sua posizione nella vista,
   * e da dove partire per un intervallo.
   *
   * Una mappa e non un insieme: la vista della libreria è rada (`rada.ts`), e
   * di una selezione fatta con «Seleziona tutto» la finestra ha in mano poche
   * righe su cinquantamila. L'ordine in cui i comandi della barra li mandano —
   * «Riproduci», «In coda» — è quello della vista, e senza la posizione
   * ricordata al momento della scelta non si saprebbe ricostruirlo.
   */
  // La barra della selezione compare con la prima riga scelta e se ne va con
  // l'ultima: solo quei due passaggi sono una presenza.
  const [scelti, setScelti] = useStatoPresente<Map<number, number>>(
    () => new Map(),
    (mappa) => mappa.size > 0,
  );
  const [ancora, setAncora] = useState<number | null>(null);
  /**
   * La selezione di ciascuna vista tenuta montata, mentre se ne guarda
   * un'altra: vedi `VISTE_TENUTE` in `App.tsx` e l'effetto che la mette da
   * parte, qui sotto.
   */
  const selezioniTenute = useRef(
    new Map<string, { scelti: Map<number, number>; ancora: number | null }>(),
  );

  /**
   * Quante volte la pagina è cambiata: una selezione a intervallo che aspetta
   * le sue pagine e arriva dopo un cambio di pagina si butta.
   */
  const giroSelezione = useRef(0);

  /**
   * La selezione multipla.
   *
   * Ctrl aggiunge o toglie una riga, Maiusc prende l'intervallo dall'ultima
   * toccata. Un clic normale **non** seleziona: su una lista di brani il clic
   * singolo non ha un significato ovvio — chi lo usa per «apri» vede partire
   * una canzone che non voleva — e riservarlo alla selezione renderebbe
   * impossibile cliccare senza conseguenze.
   *
   * L'intervallo, in una lista rada, può passare per righe che non sono in
   * mano: si chiedono le pagine che mancano (`ArchivioRado.intervallo`) e la
   * selezione arriva quando ci sono.
   *
   * Che cosa fa ogni clic, e dove resta l'àncora, lo decide `gestoDelClic` in
   * `selezione.ts`: l'àncora è una posizione nella vista, e si sposta anche col
   * clic semplice — prima no, e Maiusc+clic dopo un clic semplice partiva
   * dall'ultima riga toccata con Ctrl, magari tre schermate più su.
   */
  const seleziona = useCallback(
    (e: React.MouseEvent, indice: number) => {
      const brano = elenco.riga(indice);
      if (!brano) return;
      const { gesto, ancora: dopo } = gestoDelClic(
        { indice, maiuscolo: e.shiftKey, controllo: e.ctrlKey || e.metaKey },
        ancora,
      );
      setAncora(dopo);
      if (gesto.tipo === "intervallo") {
        e.preventDefault();
        const { da, a } = gesto;
        const mio = giroSelezione.current;
        const righe =
          rada !== null
            ? rada.intervallo(da, a)
            : Promise.resolve(
                Array.from({ length: a - da + 1 }, (_, k) => elenco.riga(da + k)),
              );
        void righe.then((arrivate) => {
          if (mio !== giroSelezione.current) return;
          const nuovi = new Map<number, number>();
          arrivate.forEach((b, k) => {
            if (b !== undefined && !nuovi.has(b.id)) nuovi.set(b.id, da + k);
          });
          setScelti(nuovi);
        });
        return;
      }
      if (gesto.tipo === "alterna") {
        e.preventDefault();
        setScelti((prima) => {
          const dopo = new Map(prima);
          if (!dopo.delete(brano.id)) dopo.set(brano.id, indice);
          return dopo;
        });
        return;
      }
      // Un clic senza modificatori con una selezione aperta la chiude: è il modo
      // di uscirne senza cercare un bottone.
      if (scelti.size > 0) setScelti(new Map());
    },
    [elenco, rada, ancora, scelti.size, setScelti],
  );

  /**
   * Spazio sulla riga col fuoco: aggiunge o toglie quella riga dalla selezione.
   *
   * È il Ctrl+clic detto da tastiera, e non una seconda regola: `seleziona` ha
   * bisogno di un evento del puntatore per leggere i modificatori, mentre da
   * tastiera il modificatore **è** il tasto. Senza questo non c'era modo di
   * costruire una selezione multipla senza mouse, cioè la barra di selezione e
   * tutti i suoi comandi erano inaccessibili da tastiera.
   */
  const alterna = useCallback(
    (indice: number) => {
      const brano = elenco.riga(indice);
      if (!brano) return;
      setScelti((prima) => {
        const dopo = new Map(prima);
        if (!dopo.delete(brano.id)) dopo.set(brano.id, indice);
        return dopo;
      });
      setAncora(indice);
    },
    [elenco, setScelti],
  );

  /*
   * La selezione non sopravvive al cambio di elenco: cinquanta brani scelti fra
   * i Preferiti non significano niente dentro un album, e tenerli farebbe agire
   * i comandi su righe che non si vedono più.
   *
   * Ma le viste tenute montate se la ritrovano tornando, come ritrovano lo
   * scorrimento: uscendo da una di loro la selezione si mette da parte
   * (`selezioniTenute`), rientrando si rimette. Le altre pagine la perdono come
   * prima. Quando l'elenco cambia lo dice la chiave: vedi
   * `chiaveDellaSelezione` in `selezione.ts`.
   *
   * In un effetto di disposizione: la barra della selezione non deve mostrare,
   * nemmeno per un fotogramma, i brani scelti nella vista di prima.
   */
  const selezioneAdesso = useRef({ scelti, ancora });
  selezioneAdesso.current = { scelti, ancora };
  const selezioneDiPrima = useRef({ chiave, tenuta });
  useLayoutEffect(() => {
    const prima = selezioneDiPrima.current;
    if (prima.chiave === chiave) return;
    selezioneDiPrima.current = { chiave, tenuta };
    giroSelezione.current += 1;
    if (prima.tenuta) selezioniTenute.current.set(prima.chiave, selezioneAdesso.current);
    const torna = tenuta ? selezioniTenute.current.get(chiave) : undefined;
    setScelti(torna?.scelti ?? new Map());
    setAncora(torna?.ancora ?? null);
  }, [chiave, tenuta, setScelti]);

  /** I selezionati, nell'ordine della vista: vedi `nellOrdineDellaVista`. */
  const selezionati = useMemo(
    () => nellOrdineDellaVista(scelti, rada === null ? pieni : null),
    [rada, pieni, scelti],
  );

  /**
   * «Seleziona tutto»: la vista intera, non le righe in mano.
   *
   * Da una lista rada le righe in mano sono qualche pagina, e la barra diceva
   * «200 brani» sopra una vista che ne aveva cinquantamila — e i comandi
   * agivano su quei duecento. Gli identificativi di tutta la vista li dà il
   * nucleo, con lo stesso ordinamento e la stessa ricerca delle pagine
   * (`viste.rs`), e la posizione di ciascuno è il suo posto in quell'elenco.
   * Una risposta arrivata dopo un cambio di pagina si butta.
   */
  const selezionaTutto = useCallback(() => {
    if (vistaBrani === null) {
      setScelti(new Map(pieni.map((b, i) => [b.id, i] as const)));
      return;
    }
    const mio = giroSelezione.current;
    idDellaVista(vistaBrani)
      .then((ids) => {
        if (mio !== giroSelezione.current) return;
        setScelti(new Map(ids.map((id, i) => [id, i] as const)));
      })
      .catch(onErrore);
  }, [vistaBrani, pieni, idDellaVista, onErrore, setScelti]);

  const annulla = useCallback(() => setScelti(new Map()), [setScelti]);

  /*
   * Dei brani se ne sono andati: via dalla selezione, e da quelle messe da
   * parte dalle viste tenute montate — tornandoci, la barra non deve contare
   * un brano che non c'è più.
   */
  const togli = useCallback(
    (via: ReadonlySet<number>) => {
      setScelti((prima) => {
        const dopo = new Map(prima);
        for (const id of via) dopo.delete(id);
        return dopo;
      });
      for (const messa of selezioniTenute.current.values()) {
        for (const id of via) messa.scelti.delete(id);
      }
    },
    [setScelti],
  );

  return { scelti, selezionati, seleziona, alterna, selezionaTutto, annulla, togli };
}
