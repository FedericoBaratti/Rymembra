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
 * Passano anche i tasti funzione, per la stessa ragione detta al contrario:
 * `F11` non è un carattere, e in nessuna lingua finisce dentro una parola.
 * Senza questa seconda eccezione, lo schermo intero sarebbe l'unico comando
 * della finestra che smette di funzionare mentre si cerca un disco.
 *
 * `Escape` è l'eccezione voluta, e **non è configurabile**: funziona anche
 * mentre si scrive, perché in un campo pieno il primo significato di Escape è
 * «lascia stare», e chi lo preme dentro la ricerca vuole uscirne. Lasciarlo
 * riassegnare vorrebbe dire poter chiudere a chiave una finestrella modale.
 */
import { useEffect, useRef } from "react";

import { posizioneAdesso } from "./riproduzione";
import { t, type Chiave } from "./lingue";

/** Di quanto si sposta il cursore con una freccia. */
const PASSO_MS = 5_000;

/** I comandi che si possono associare a un tasto. */
export type Comando =
  | "alterna"
  | "cerca"
  | "avanti"
  | "indietro"
  | "inRiproduzione"
  | "schermoIntero"
  | "ingrandisci"
  | "rimpicciolisci"
  | "zoomNormale"
  | "importazioni"
  | "incollaLink";

/**
 * Come si chiamano a schermo, e cosa fanno.
 *
 * Una funzione e non una costante: i titoli sono testo, e una tabella costruita
 * all'apertura del modulo resterebbe nella lingua di quel momento.
 *
 * L'ordine è quello con cui la scheda delle scorciatoie li elenca: i tre
 * dello zoom stanno accanto allo schermo intero perché sono l'altra metà
 * della stessa domanda — quanto della finestra si vede, e quanto grande — e
 * le due dell'importazione restano in fondo perché sono le due che si usano
 * meno.
 */
const CHIAVI: readonly Comando[] = [
  "alterna",
  "cerca",
  "avanti",
  "indietro",
  "inRiproduzione",
  "schermoIntero",
  "ingrandisci",
  "rimpicciolisci",
  "zoomNormale",
  "importazioni",
  "incollaLink",
];

export function comandi(): readonly {
  chiave: Comando;
  titolo: string;
  spiegazione: string;
}[] {
  const c = (chiave: Comando) => ({
    chiave,
    titolo: t(`cmd.${chiave}.title` as Chiave),
    spiegazione: t(`cmd.${chiave}.hint` as Chiave),
  });
  return CHIAVI.map(c);
}

/** Le associazioni di serie. */
export const DI_SERIE: Readonly<Record<Comando, readonly string[]>> = {
  alterna: ["Space"],
  cerca: ["/", "Ctrl+f"],
  avanti: ["ArrowRight"],
  indietro: ["ArrowLeft"],
  inRiproduzione: ["f"],
  // `F11` e non altro: è il tasto che ogni programma con una finestra usa per
  // questo, e chi lo preme qui dentro non sta imparando niente di nuovo. Non
  // collide con le lettere perché non è una lettera, e resta riassegnabile come
  // gli altri — la tabella qui sopra è l'unico posto dove guardarlo.
  schermoIntero: ["F11"],
  // **Tre accordi, e nessuno è di troppo.** `tastoDi` registra il *carattere*
  // arrivato, non il tasto premuto, e mette `Shift` fra le parti quando c'era:
  // quindi lo stesso gesto dà tre nomi diversi a seconda della tastiera.
  //
  // - Tastiera italiana (e tedesca): `+` sta accanto a `ì` e non vuole
  //   `Shift`, quindi arriva `Ctrl++`. Anche quello del tastierino numerico
  //   arriva così, ed è il motivo per cui non gli serve una riga sua.
  // - Tastiera americana (e francese): `+` è `Shift` sul tasto `=`. Chi non
  //   preme `Shift` manda `Ctrl+=`; chi lo preme manda `Ctrl+Shift++`, perché
  //   `e.key` è già diventato `+` **e** `shiftKey` è vero.
  //
  // Toglierne uno vuol dire una tastiera su cui la scorciatoia più famosa del
  // mondo non fa niente, e nessuno che possa capire perché. È lo stesso
  // motivo per cui `cerca` ne ha due.
  ingrandisci: ["Ctrl++", "Ctrl+=", "Ctrl+Shift++"],
  // Uno solo, e qui basta davvero: `-` non vuole `Shift` su nessuna delle
  // tastiere di sopra, e quello del tastierino dà lo stesso carattere.
  rimpicciolisci: ["Ctrl+-"],
  // La via di ritorno. Chi ingrandisce di un gradino di troppo, senza questa,
  // dovrebbe premere il tasto opposto contando i passi — e chi non li ha
  // contati non sa quante volte premere. È la terza scorciatoia di ogni
  // programma che ha le prime due, e costa una riga.
  zoomNormale: ["Ctrl+0"],
  // `Ctrl` e non `Meta`: `tastoDi` scarta il tasto Windows apposta, quindi qui
  // non esiste una variante per Mac da tenere allineata. Le associazioni assenti
  // in `ui.shortcuts` cadono su queste, quindi nessuna migrazione: chi ha già
  // personalizzato le sue si ritrova di serie tutte quelle arrivate dopo —
  // questi due, e i tre dello zoom qui sopra.
  importazioni: ["Ctrl+i"],
  incollaLink: ["Ctrl+l"],
};

/** Le associazioni in uso: per ogni comando, i tasti che lo eseguono. */
export type Associazioni = Record<Comando, string[]>;

/** I tasti che da soli non sono un tasto. */
const SOLO_MODIFICATORI = new Set(["Control", "Shift", "Alt", "Meta", "OS"]);

/** I tasti che non sono un carattere: `F1`…`F12`. */
const FUNZIONE = /^F\d{1,2}$/;

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
  // Le frecce non si traducono: sono glifi, e in ogni lingua dicono la stessa
  // cosa meglio di qualunque parola.
  const nomi: Record<string, string> = {
    Space: t("keys.space"),
    ArrowRight: "→",
    ArrowLeft: "←",
    ArrowUp: "↑",
    ArrowDown: "↓",
    Enter: t("keys.enter"),
    Escape: t("keys.esc"),
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
    schermoIntero: [...DI_SERIE.schermoIntero],
    ingrandisci: [...DI_SERIE.ingrandisci],
    rimpicciolisci: [...DI_SERIE.rimpicciolisci],
    zoomNormale: [...DI_SERIE.zoomNormale],
    importazioni: [...DI_SERIE.importazioni],
    incollaLink: [...DI_SERIE.incollaLink],
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
  for (const chiave of CHIAVI) {
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
  for (const chiave of CHIAVI) {
    for (const tasto of associazioni[chiave]) {
      const gia = di.get(tasto);
      if (gia) gia.push(chiave);
      else di.set(tasto, [chiave]);
    }
  }
  for (const [tasto, quali] of di) if (quali.length < 2) di.delete(tasto);
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
  /** La finestra a schermo intero, e di nuovo per tornare com'era. */
  schermoIntero: () => void;
  /**
   * L'interfaccia di un gradino più grande, o più piccola.
   *
   * Una sola azione con un verso e non due: la scala sta nel nucleo, e la
   * finestra non ha nessun motivo di conoscerla — chiedere «uno in più» è
   * tutto quel che una pressione sa dire.
   */
  zoom: (su: boolean) => void;
  /** L'interfaccia alla misura di serie. */
  zoomNormale: () => void;
  /** La pagina delle importazioni. */
  importazioni: () => void;
  /** La finestrella che legge un link, da qualunque pagina. */
  incollaLink: () => void;
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
    case "schermoIntero":
      azioni.schermoIntero();
      break;
    case "ingrandisci":
      azioni.zoom(true);
      break;
    case "rimpicciolisci":
      azioni.zoom(false);
      break;
    case "zoomNormale":
      azioni.zoomNormale();
      break;
    case "importazioni":
      azioni.importazioni();
      break;
    case "incollaLink":
      azioni.incollaLink();
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
  // il primo dichiarato in `CHIAVI`, che è l'ordine che la scheda mostra.
  mappa.current = (() => {
    const di = new Map<string, Comando>();
    for (const chiave of CHIAVI) {
      for (const tasto of associazioni[chiave])
        if (!di.has(tasto)) di.set(tasto, chiave);
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

      // Un accordo con `Ctrl` o `Alt` passa anche mentre si scrive, e con esso
      // i tasti funzione, che nessuna parola contiene; un tasto nudo no. Vedi il
      // preambolo.
      if (!e.ctrlKey && !e.altKey && !FUNZIONE.test(e.key) && siStaScrivendo(e.target))
        return;

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
