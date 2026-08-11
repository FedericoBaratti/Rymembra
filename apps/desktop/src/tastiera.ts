/**
 * Le scorciatoie, tutte qui — e adesso configurabili.
 *
 * # Perché in un file solo
 *
 * Una scorciatoia sparsa nel componente che la usa è una scorciatoia che
 * nessuno sa che esiste, e soprattutto è una scorciatoia che può collidere con
 * un'altra senza che niente lo dica: due `keydown` su `window` in due componenti
 * diversi si eseguono tutti e due. Qui il conflitto si vede leggendo, e la
 * documentazione della mappa è la mappa.
 *
 * # Perché una tabella e non più uno `switch`
 *
 * Finché i tasti erano scritti nei `case`, cambiarne uno voleva dire
 * ricompilare, e mostrarne l'elenco a schermo voleva dire riscriverlo a mano in
 * un altro file — cioè avere due elenchi che si scostano. Adesso c'è una
 * tabella: la stessa che l'ascoltatore consulta è quella che la scheda delle
 * impostazioni disegna, quindi non possono raccontare due cose diverse.
 *
 * Le associazioni scelte vivono in `ui.shortcuts`, nel database, come JSON
 * `{ comando: [tasti] }`. **Assente vuol dire «quella di serie», un elenco
 * vuoto vuol dire «nessuna»**: senza questa distinzione, togliere una
 * scorciatoia sarebbe indistinguibile dal non averla mai toccata, e al riavvio
 * successivo tornerebbe da sola.
 *
 * # La regola che vale per tutte: non mentre si scrive
 *
 * `Spazio` mette in pausa **tranne** quando il fuoco è in un campo di testo, e
 * `/` apre la ricerca tranne quando si sta già scrivendo. Senza questo controllo
 * chi cerca «space oddity» metterebbe in pausa a metà parola. Il controllo è sul
 * bersaglio dell'evento e non su uno stato dell'applicazione: l'unica fonte
 * attendibile di «dove sta il fuoco adesso» è il documento.
 *
 * L'eccezione è la combinazione con `Ctrl` o `Alt`, che funziona anche mentre
 * si scrive: era già così per `Ctrl+F`, e la regola generalizzata è quella
 * giusta — nessuno scrive `Ctrl+F` dentro una parola. `Shift` non basta a
 * concederlo, perché `Shift+a` è una lettera.
 *
 * `Escape` è l'eccezione voluta, e **non è configurabile**: funziona anche
 * mentre si scrive, perché in un campo pieno il primo significato di Escape è
 * «lascia stare», e chi lo preme dentro la ricerca vuole uscirne. Lasciarlo
 * riassegnare vorrebbe dire poter chiudere a chiave una finestrella modale.
 */
import { useEffect, useRef } from "react";

import { posizioneAdesso } from "./riproduzione";

/** Di quanto si sposta il cursore con una freccia. */
const PASSO_MS = 5_000;

/** I comandi che si possono associare a un tasto. */
export type Comando =
  | "alterna"
  | "cerca"
  | "avanti"
  | "indietro"
  | "inRiproduzione";

/** Come si chiamano a schermo, e cosa fanno. */
export const COMANDI: readonly {
  chiave: Comando;
  titolo: string;
  spiegazione: string;
}[] = [
  {
    chiave: "alterna",
    titolo: "Pausa e ripresa",
    spiegazione: "Ferma quel che suona, o lo riprende da dov'era.",
  },
  {
    chiave: "cerca",
    titolo: "Cerca",
    spiegazione: "Porta il cursore nel campo di ricerca, tornando in libreria se serve.",
  },
  {
    chiave: "avanti",
    titolo: "Avanti di cinque secondi",
    spiegazione: "Sposta la posizione nel brano, senza uscirne.",
  },
  {
    chiave: "indietro",
    titolo: "Indietro di cinque secondi",
    spiegazione: "Come sopra, dall'altra parte.",
  },
  {
    chiave: "inRiproduzione",
    titolo: "Apri «In riproduzione»",
    spiegazione: "Il brano a tutto schermo, e di nuovo per chiuderlo.",
  },
];

/** Le associazioni di serie. */
export const DI_SERIE: Readonly<Record<Comando, readonly string[]>> = {
  alterna: ["Space"],
  cerca: ["/", "Ctrl+f"],
  avanti: ["ArrowRight"],
  indietro: ["ArrowLeft"],
  inRiproduzione: ["f"],
};

/** Le associazioni in uso: per ogni comando, i tasti che lo eseguono. */
export type Associazioni = Record<Comando, string[]>;

/** I tasti che da soli non sono un tasto. */
const SOLO_MODIFICATORI = new Set(["Control", "Shift", "Alt", "Meta", "OS"]);

/**
 * Il nome stabile di un tasto premuto, modificatori compresi.
 *
 * `Meta` non entra mai: il tasto Windows appartiene al sistema operativo, e una
 * scorciatoia che ci sopra non la vedremmo comunque arrivare per intera.
 * Restituisce `null` quando è stato premuto soltanto un modificatore — cioè
 * mentre si sta ancora componendo la combinazione, che è esattamente l'istante
 * in cui la cattura non deve concludere niente.
 */
export function tastoDi(e: {
  key: string;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  metaKey: boolean;
}): string | null {
  if (e.metaKey || SOLO_MODIFICATORI.has(e.key)) return null;
  const parti: string[] = [];
  if (e.ctrlKey) parti.push("Ctrl");
  if (e.shiftKey) parti.push("Shift");
  if (e.altKey) parti.push("Alt");
  // Una lettera si normalizza in minuscolo: con `Shift` premuto `e.key` è
  // maiuscola, e `Shift+F` e `Shift+f` sarebbero due associazioni diverse per
  // la stessa pressione.
  parti.push(e.key === " " ? "Space" : e.key.length === 1 ? e.key.toLowerCase() : e.key);
  return parti.join("+");
}

/** Come si legge un tasto a schermo. */
export function tastoScritto(tasto: string): string {
  const nomi: Record<string, string> = {
    Space: "Spazio",
    ArrowRight: "→",
    ArrowLeft: "←",
    ArrowUp: "↑",
    ArrowDown: "↓",
    Enter: "Invio",
    Escape: "Esc",
  };
  return tasto
    .split("+")
    .map((p) => nomi[p] ?? (p.length === 1 ? p.toUpperCase() : p))
    .join("+");
}

/** Le associazioni di serie, in una copia che si può modificare. */
export function serieMutabile(): Associazioni {
  return {
    alterna: [...DI_SERIE.alterna],
    cerca: [...DI_SERIE.cerca],
    avanti: [...DI_SERIE.avanti],
    indietro: [...DI_SERIE.indietro],
    inRiproduzione: [...DI_SERIE.inRiproduzione],
  };
}

/**
 * Legge le associazioni salvate, riempiendo con quelle di serie.
 *
 * Tollerante per scelta: il nucleo controlla che sia JSON e nient'altro — i
 * nomi dei comandi sono roba della finestra — quindi qui può arrivare
 * qualunque cosa, compreso un profilo scritto da una versione che aveva un
 * comando in più. Quel che non si riconosce si lascia cadere; quel che manca
 * torna di serie. In nessun caso si resta senza scorciatoie.
 */
export function leggiAssociazioni(json: string | null): Associazioni {
  const associazioni = serieMutabile();
  if (json === null || json.trim() === "") return associazioni;
  let letto: unknown;
  try {
    letto = JSON.parse(json);
  } catch {
    return associazioni;
  }
  if (typeof letto !== "object" || letto === null) return associazioni;
  for (const { chiave } of COMANDI) {
    const valore = (letto as Record<string, unknown>)[chiave];
    // Assente ⇒ quella di serie. Un elenco vuoto ⇒ nessuna, ed è una scelta
    // che va rispettata: vedi il preambolo.
    if (!Array.isArray(valore)) continue;
    associazioni[chiave] = valore.filter((t): t is string => typeof t === "string");
  }
  return associazioni;
}

/** Le associazioni come vanno scritte in `ui.shortcuts`. */
export function scriviAssociazioni(associazioni: Associazioni): string {
  return JSON.stringify(associazioni);
}

/**
 * I tasti assegnati a più di un comando.
 *
 * Restituisce, per ogni tasto conteso, i comandi che se lo dividono. Un
 * conflitto non è un errore da rifiutare — è una cosa da **dire**: chi
 * riassegna Spazio a «Cerca» senza toglierlo a «Pausa» otterrebbe un tasto che
 * fa una delle due cose e non capirebbe quale, mentre l'avviso lo manda a
 * togliere l'altra.
 */
export function conflitti(associazioni: Associazioni): Map<string, Comando[]> {
  const di = new Map<string, Comando[]>();
  for (const { chiave } of COMANDI) {
    for (const tasto of associazioni[chiave]) {
      const gia = di.get(tasto);
      if (gia) gia.push(chiave);
      else di.set(tasto, [chiave]);
    }
  }
  for (const [tasto, comandi] of di) if (comandi.length < 2) di.delete(tasto);
  return di;
}

/** Cosa succede a ogni comando. */
export type Azioni = {
  /** Pausa e ripresa. */
  alterna: () => void;
  /** Il campo di ricerca. */
  cerca: () => void;
  /** La posizione nel brano, in millisecondi assoluti. */
  vaiA: (ms: number) => void;
  /** `Escape`. Restituisce `true` se ha chiuso qualcosa. */
  chiudi: () => boolean;
  /** «In riproduzione» a tutto schermo. */
  inRiproduzione: () => void;
  /** La durata del brano, per non uscirne. */
  durataMs: number;
};

/** Il fuoco è dentro qualcosa in cui si scrive. */
function siStaScrivendo(bersaglio: EventTarget | null): boolean {
  if (!(bersaglio instanceof HTMLElement)) return false;
  if (bersaglio.isContentEditable) return true;
  const nome = bersaglio.tagName;
  if (nome === "TEXTAREA" || nome === "SELECT") return true;
  if (nome !== "INPUT") return false;
  // Un cursore a scorrimento è un `input` ma non ci si scrive dentro, e le
  // frecce ce le vuole lui: senza questa distinzione, spostare il volume con la
  // tastiera sposterebbe anche la posizione nel brano.
  const tipo = (bersaglio as HTMLInputElement).type;
  return tipo !== "range" && tipo !== "checkbox" && tipo !== "radio";
}

/** Il campo di ricerca dell'intestazione, se la pagina ne ha uno. */
export function campoRicerca(): HTMLInputElement | null {
  return document.querySelector<HTMLInputElement>("[data-cerca]");
}

/** Esegue un comando. La posizione si legge adesso, non si fa passare. */
function esegui(comando: Comando, azioni: Azioni): void {
  switch (comando) {
    case "alterna":
      azioni.alterna();
      break;
    case "cerca":
      azioni.cerca();
      break;
    // La posizione si legge **adesso**, dall'archivio: è l'unico posto che la
    // vuole in risposta a un gesto invece che per disegnarla, e farsela passare
    // come prop la rimetterebbe fra le cose che ridisegnano `App` venti volte
    // al secondo.
    case "avanti":
      azioni.vaiA(Math.min(posizioneAdesso() + PASSO_MS, azioni.durataMs));
      break;
    case "indietro":
      azioni.vaiA(Math.max(posizioneAdesso() - PASSO_MS, 0));
      break;
    case "inRiproduzione":
      azioni.inRiproduzione();
      break;
  }
}

export function useScorciatoie(azioni: Azioni, associazioni: Associazioni): void {
  // In un ref, e l'effetto senza dipendenze: `azioni` arriva come oggetto
  // scritto sul posto, quindi la sua identità cambia a ogni disegno di `App`.
  // Tenerlo fra le dipendenze voleva dire togliere e rimettere l'ascoltatore
  // della tastiera a ogni disegno — cioè venti volte al secondo mentre suona,
  // per una mappa di tasti che non cambia mai.
  const ultime = useRef(azioni);
  ultime.current = azioni;
  const mappa = useRef<Map<string, Comando>>(new Map());
  // La mappa si ricostruisce quando cambiano le associazioni, non a ogni
  // pressione: cinque comandi sono pochi, ma la ricerca per tasto deve essere
  // una lettura sola anche quando saranno cinquanta. In caso di conflitto vince
  // il primo dichiarato in `COMANDI`, che è l'ordine che la scheda mostra.
  mappa.current = (() => {
    const di = new Map<string, Comando>();
    for (const { chiave } of COMANDI) {
      for (const tasto of associazioni[chiave]) if (!di.has(tasto)) di.set(tasto, chiave);
    }
    return di;
  })();

  useEffect(() => {
    const ascolta = (e: KeyboardEvent) => {
      const azioni = ultime.current;

      if (e.key === "Escape") {
        // Prima si esce dal campo, poi si chiude quel che c'è aperto: due
        // pressioni per due significati, invece di uno che ne annulla un altro.
        if (siStaScrivendo(e.target) && e.target instanceof HTMLElement) {
          e.target.blur();
          e.preventDefault();
          return;
        }
        if (azioni.chiudi()) e.preventDefault();
        return;
      }

      const tasto = tastoDi(e);
      if (tasto === null) return;
      const comando = mappa.current.get(tasto);
      if (comando === undefined) return;

      // Un accordo con `Ctrl` o `Alt` passa anche mentre si scrive; un tasto
      // nudo no. Vedi il preambolo.
      if (!e.ctrlKey && !e.altKey && siStaScrivendo(e.target)) return;

      // `preventDefault` sempre, e non solo per lo Spazio: un tasto che
      // abbiamo preso non deve fare anche il suo mestiere di serie — lo Spazio
      // scorrerebbe la pagina di uno schermo, e mettere in pausa sposterebbe
      // l'elenco.
      e.preventDefault();
      esegui(comando, azioni);
    };

    window.addEventListener("keydown", ascolta);
    return () => window.removeEventListener("keydown", ascolta);
  }, []);
}
