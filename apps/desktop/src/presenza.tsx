/**
 * Quel che entra, esce e vola: le transizioni di vista di una skin.
 *
 * # Perché React e non il motore a mano
 *
 * Un'**uscita** non si può scrivere in CSS: quando React toglie il menù dal
 * DOM, il menù non c'è più, e non c'è una regola che possa animare un elemento
 * che non esiste. `@starting-style` sa dire da dove si arriva, non dove si va.
 * `<ViewTransition>` di React 19.3 fa la fotografia **prima** di togliere, e la
 * lascia sparire con la regola che la skin ha scritto. Lo stesso per il volo
 * della copertina dalla griglia alla pagina dell'album: due elementi diversi,
 * in due schermate diverse, con lo stesso nome.
 *
 * # Il contratto col compilatore
 *
 * Per ogni parte che dichiara un innesco, `rymembra-skin` scrive una regola su
 * `::view-transition-new(.vt-<parte>-enter)`, `-old(.vt-<parte>-exit)` o
 * `-group(.vt-<parte>-share)`, e mette la parte in `CompiledSkin::presenze`.
 * Qui si legge quell'elenco e si passa la classe dell'innesco — o `"none"`,
 * che per React vuol dire «niente nome, niente fotografia»: senza una regola
 * che la raccolga, il motore farebbe la sua dissolvenza di serie, e sopra
 * l'`@starting-style` del foglio ne verrebbero due.
 *
 * # Una sola alla volta, la più esterna
 *
 * React anima l'entrata e l'uscita della sola `<ViewTransition>` più esterna
 * del pezzo che entra o esce: quelle annidate vengono dentro la sua
 * fotografia. Per questo `Presenza` **non si monta** quando la skin non
 * dichiara niente per la sua parte: il velo di una finestrella che non si
 * anima lascia il posto alla finestrella dentro, che così diventa la più
 * esterna e si anima lei. Se si animano tutti e due, vince il velo, e la
 * finestrella viaggia dentro di lui.
 *
 * # Le aperture passano da una transizione
 *
 * `<ViewTransition>` si accende solo su un aggiornamento dentro
 * `startTransition`. `useStatoPresente` è un `useState` che lo fa da sé quando
 * il valore passa da «c'è» a «non c'è» o viceversa, e lascia sincroni tutti gli
 * altri cambi: la barra della selezione si anima quando compare, non a ogni
 * riga in più che si sceglie.
 */
import {
  ViewTransition,
  createContext,
  startTransition,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type Dispatch,
  type ReactNode,
  type SetStateAction,
} from "react";

import type { Archivio } from "./archivio";
import type { Presenza as Inneschi } from "./ipc";
import { fermoRestando, ROTTA } from "./transizione";

/** Le presenze della skin indossata, per nome di parte. */
export type MappaPresenze = ReadonlyMap<string, Inneschi>;

/**
 * Le presenze della skin indossata.
 *
 * Vuota di serie: fuori da `App` — nell'anteprima dello Studio, dove le
 * transizioni di vista non esistono — ogni `Presenza` si fa da parte.
 */
export const Presenze = createContext<MappaPresenze>(new Map());

/** L'elenco che manda il nucleo, in una mappa per parte. */
export function mappaPresenze(elenco: readonly Inneschi[] | undefined): MappaPresenze {
  return new Map((elenco ?? []).map((voce) => [voce.parte, voce]));
}

/**
 * L'album la cui copertina sta volando verso la sua pagina, o `null`.
 *
 * Il nome di una transizione dev'essere unico nel documento: lo porta solo la
 * scheda che si è premuta, e la pagina dell'album. Tornando indietro la scheda
 * lo ritrova, e la copertina vola al contrario.
 */
export const Volo = createContext<string | null>(null);

/** Il nome della copertina in volo: uno, perché ne vola una alla volta. */
export const COPERTINA_IN_VOLO = "copertina-in-volo";

/** La classe di un innesco, o `"none"` se la skin non lo dichiara. */
function classe(parte: string, innesco: "enter" | "exit" | "share", dichiarato: boolean) {
  return dichiarato ? `vt-${parte}-${innesco}` : "none";
}

/**
 * Una parte che entra, esce o vola secondo la skin.
 *
 * Il figlio dev'essere **un** elemento del DOM: è quello che si fotografa.
 *
 * # Il volo è di due
 *
 * Una copertina vola solo se **tutte e due** le estremità hanno un nome e una
 * classe: la scheda che se ne va e la testata che arriva. Una skin però lo
 * dichiara su una parte sola — «la copertina della scheda vola», e basta — e
 * pretendere che lo scriva anche sull'altra vorrebbe dire un volo che non
 * parte per una dimenticanza. `compagna` è l'altra estremità: se la parte non
 * dichiara il volo e la compagna sì, si prende la classe della compagna. Il
 * gruppo del volo porta la classe di chi arriva, e la regola c'è comunque.
 */
export function Presenza({
  parte,
  nome,
  compagna,
  children,
}: {
  parte: string;
  /** Il nome condiviso, per il volo fra due schermate. */
  nome?: string | undefined;
  /** L'altra estremità del volo, per chi non lo dichiara. */
  compagna?: string | undefined;
  children: ReactNode;
}) {
  const presenze = useContext(Presenze);
  const inneschi = presenze.get(parte);
  const altra = compagna === undefined ? undefined : presenze.get(compagna);
  const volo = inneschi?.share
    ? classe(parte, "share", true)
    : altra?.share && compagna !== undefined
      ? classe(compagna, "share", true)
      : "none";
  if (inneschi === undefined && volo === "none") return children;
  return (
    <ViewTransition
      name={nome}
      default="none"
      enter={classe(parte, "enter", inneschi?.enter ?? false)}
      exit={classe(parte, "exit", inneschi?.exit ?? false)}
      share={volo}
    >
      {children}
    </ViewTransition>
  );
}

/**
 * La pagina, che non entra e non esce: **cambia**.
 *
 * Il contenitore della pagina resta montato da una vista all'altra — è lo
 * scorrevole, e gli elenchi virtuali ci tengono sopra i loro riferimenti —
 * quindi per React il cambio di vista è un aggiornamento, non un'entrata.
 * Le due classi vanno insieme sul gruppo: `::view-transition-old` prende la
 * fotografia di prima con l'uscita, `::view-transition-new` quella di adesso
 * con l'entrata. Solo sulle transizioni di tipo `rotta`: un filtro che cambia
 * l'elenco non è un cambio di pagina.
 */
export function PresenzaPagina({ children }: { children: ReactNode }) {
  const inneschi = useContext(Presenze).get("page-content");
  if (inneschi === undefined) return children;
  const classi = [
    inneschi.exit ? "vt-page-content-exit" : "",
    inneschi.enter ? "vt-page-content-enter" : "",
  ]
    .filter((c) => c !== "")
    .join(" ");
  return (
    <ViewTransition
      default="none"
      enter="none"
      exit="none"
      share="none"
      update={{ [ROTTA]: classi, default: "none" }}
    >
      {children}
    </ViewTransition>
  );
}

/**
 * Un cambio che fa entrare o uscire qualcosa.
 *
 * Dentro `startTransition`, così le presenze si accendono; fuori, quando il
 * movimento è spento, perché una fotografia che non si anima è lavoro fatto
 * per niente.
 */
export function conPresenza(cambia: () => void): void {
  if (fermoRestando()) cambia();
  else startTransition(cambia);
}

/**
 * Un `useState` per quel che compare e scompare.
 *
 * Il cambio passa da `conPresenza` solo quando `presente` cambia risposta —
 * per un booleano o un «qualcosa o `null`» è già quella di serie. Un
 * aggiornamento funzionale si calcola una volta in più, qui, sul valore
 * dell'ultimo disegno, solo per sapere da che parte va: React poi lo calcola
 * da sé come sempre.
 */
export function useStatoPresente<T>(
  iniziale: T | (() => T),
  presente: (valore: T) => boolean = Boolean,
): [T, Dispatch<SetStateAction<T>>] {
  const [valore, setValore] = useState(iniziale);
  const ultimo = useRef(valore);
  ultimo.current = valore;
  const presenteRef = useRef(presente);
  presenteRef.current = presente;

  const imposta = useCallback((nuovo: SetStateAction<T>) => {
    const prima = ultimo.current;
    const dopo =
      typeof nuovo === "function" ? (nuovo as (prima: T) => T)(prima) : nuovo;
    const ce = presenteRef.current;
    if (ce(prima) === ce(dopo)) setValore(nuovo);
    else conPresenza(() => setValore(nuovo));
  }, []);

  return [valore, imposta];
}

/**
 * Un archivio (`archivio.ts`) seguito da uno stato che sa della presenza.
 *
 * `useArchivio` passa da `useSyncExternalStore`, e un aggiornamento che arriva
 * di lì è sempre sincrono: non entra in `startTransition`, e le
 * `<ViewTransition>` non si accendono. Per un valore che fa comparire e
 * scomparire qualcosa — la barra della scansione nella pila dei toast — sarebbe
 * un'entrata senza animazione dove prima c'era. Qui il valore scelto si copia
 * in un `useStatoPresente`, che passa dalla presenza come sempre quando
 * `presente` cambia risposta e resta sincrono negli altri casi.
 *
 * L'iscrizione sta in un effetto, e appena iscritta rilegge: un cambio
 * arrivato fra il disegno e l'effetto non si perde. Chi scrive l'archivio
 * dentro un gestore sveglia tutte le copie nello stesso giro, e React le
 * disegna insieme — nella stessa transizione quando c'è.
 */
export function useSpecchioPresente<T, S>(
  archivio: Archivio<T>,
  scegli: (valore: T) => S,
  presente: (valore: S) => boolean = Boolean,
): S {
  const [valore, imposta] = useStatoPresente<S>(
    () => scegli(archivio.leggi()),
    presente,
  );
  const scelta = useRef(scegli);
  scelta.current = scegli;
  useEffect(() => {
    const segui = () => imposta(scelta.current(archivio.leggi()));
    segui();
    return archivio.iscrivi(segui);
  }, [archivio, imposta]);
  return valore;
}
