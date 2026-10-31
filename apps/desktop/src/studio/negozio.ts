/**
 * Lo stato dello Studio, in un posto solo.
 *
 * # Perché un riduttore
 *
 * Il componente dello Studio 2.x aveva una trentina di `useState` in un file
 * da quasi duemila righe, e metà dei difetti stavano fra l'uno e l'altro: la
 * selezione che restava su una parte sparita dal documento, lo stato forzato
 * che sopravviveva al cambio di parte, l'annullo che spostava due passi. Qui
 * lo stato è un valore, le azioni sono dati, e `riduci` è una funzione pura
 * che `node --test` prova senza un DOM (`test/negozio.test.ts`).
 *
 * # La sorgente di verità resta il testo
 *
 * Il documento sta nella storia come **testo** (vedi `patch.ts` e
 * `storia.ts`). Qui non c'è un modello della skin: c'è quel che l'editor sta
 * guardando e come lo sta guardando.
 */
import type { Validazione } from "../ipc";
import { senzaCaratteriDi } from "./risorse";
import { DOVE, SCENA_DI_SERIE, scenaPer, type Larghezza, type Scena } from "./scena";
import { annulla, ripeti, riscritta, scrivi, storiaDa, type Storia } from "./storia";

/** Quel che si sta modificando. */
export type Selezione =
  | { tipo: "parte"; nome: string }
  | { tipo: "token"; id: string }
  | { tipo: "colore"; nome: string }
  | { tipo: "motivo"; nome: string }
  | { tipo: "curva"; nome: string }
  | { tipo: "animazione"; nome: string }
  | { tipo: "risorsa"; nome: string }
  | { tipo: "carattere"; nome: string }
  | { tipo: "nodo"; via: readonly number[] }
  | { tipo: "documento" };

export type Zoom = "adatta" | 1 | 2;
export type SchedaIspettore = "stile" | "stati" | "movimento" | "codice";
export type SchedaDock = "timeline" | "problemi" | "codice";
export type SchedaSinistra = "livelli" | "variabili";
/** Quale dei due temi della skin si guarda sulla tela. */
export type Tema = "base" | "variante";

export interface StatoStudio {
  /** L'id della bozza aperta: cambia se il documento cambia `id`. */
  id: string;
  storia: Storia;
  /** Il testo com'era all'apertura. */
  originale: string;
  /** L'ultimo testo arrivato su disco come bozza. */
  salvato: string;
  /** Il documento è arrivato. */
  caricato: boolean;
  /** L'esito dell'ultima validazione, e il testo a cui si riferisce. */
  esito: Validazione | null;
  validato: string | null;
  /** L'ultimo esito senza errori: la tela resta lì mentre il testo è rotto. */
  valido: Validazione | null;
  selezione: Selezione | null;
  /**
   * Lo stato della parte che l'ispettore sta modificando: `null` è l'aspetto
   * di base, `hover` è l'aspetto al passaggio, e così via.
   */
  stato: string | null;
  /** Lo stato scelto si vede anche sulla tela, forzato. */
  forza: boolean;
  tema: Tema;
  scena: Scena;
  larghezza: Larghezza;
  zoom: Zoom;
  sinistra: SchedaSinistra;
  ispettore: SchedaIspettore;
  dock: SchedaDock | null;
  chat: boolean;
  /** La sonda: il puntatore sulla tela dice come si chiama quel che tocca. */
  sonda: boolean;
  /** Cresce per far ripartire le animazioni della tela. */
  rigioca: number;
  /** Il trasporto finto della tela: il disco che gira gira solo se suona. */
  inPausa: boolean;
  /**
   * Una riga del documento da mostrare nell'editor di codice, e un contatore
   * che cresce a ogni richiesta: chiedere due volte la stessa riga deve
   * riportarci il cursore anche se nel frattempo lo si è spostato.
   */
  riga: { riga: number; colonna: number; volta: number } | null;
  /** Quante istantanee ha preso lo Studio in questa sessione. */
  istantanee: number;
}

/** I campi dell'interfaccia che si impostano così come sono. */
type CampiUi = Pick<
  StatoStudio,
  | "tema"
  | "larghezza"
  | "zoom"
  | "sinistra"
  | "ispettore"
  | "dock"
  | "chat"
  | "sonda"
  | "forza"
  | "inPausa"
>;

export type Azione =
  /** Il documento è arrivato dal disco. */
  | { tipo: "apri"; testo: string }
  /** Una modifica: da un controllo, dall'editor di codice, dalla chat. */
  | { tipo: "scrivi"; testo: string; etichetta: string; gruppo: string; adesso: number }
  | { tipo: "annulla" }
  | { tipo: "ripeti" }
  /** È tornata una validazione: vale solo se è del testo di adesso. */
  | { tipo: "validato"; testo: string; esito: Validazione }
  | { tipo: "salvato"; testo: string }
  | { tipo: "scegli"; selezione: Selezione | null }
  | { tipo: "stato"; stato: string | null }
  | { tipo: "scena"; scena: Scena }
  /** Porta sulla tela la scena in cui si vede una parte. */
  | { tipo: "mostra"; parte: string }
  | { tipo: "rinominato"; id: string }
  | { tipo: "rigioca" }
  /** Apre il codice del documento a una riga: dai Problemi, da «vai». */
  | { tipo: "vaiRiga"; riga: number; colonna: number }
  /** L'editor ha mostrato la riga chiesta con quel contatore: si dimentica. */
  | { tipo: "rigaMostrata"; volta: number }
  /**
   * Un file del pacchetto è stato cancellato dal disco: le dichiarazioni dei
   * caratteri che lo usavano si tolgono da tutta la storia, senza un passo.
   */
  | { tipo: "risorsaTolta"; nome: string }
  /** È stata presa un'istantanea: Ctrl+S, «Salva e usa», «Esporta», la chat. */
  | { tipo: "fotografata" }
  | { tipo: "imposta"; campi: Partial<CampiUi> };

export function iniziale(id: string): StatoStudio {
  return {
    id,
    storia: storiaDa(""),
    originale: "",
    salvato: "",
    caricato: false,
    esito: null,
    validato: null,
    valido: null,
    selezione: null,
    stato: null,
    forza: true,
    tema: "base",
    scena: SCENA_DI_SERIE,
    larghezza: "tre",
    zoom: "adatta",
    sinistra: "livelli",
    ispettore: "stile",
    dock: null,
    chat: false,
    sonda: true,
    rigioca: 0,
    inPausa: false,
    riga: null,
    istantanee: 0,
  };
}

/** Due selezioni sono la stessa cosa. */
export function stessaSelezione(a: Selezione | null, b: Selezione | null): boolean {
  if (a === null || b === null) return a === b;
  return JSON.stringify(a) === JSON.stringify(b);
}

export function riduci(stato: StatoStudio, azione: Azione): StatoStudio {
  switch (azione.tipo) {
    case "apri":
      return {
        ...stato,
        storia: storiaDa(azione.testo),
        originale: azione.testo,
        salvato: azione.testo,
        caricato: true,
      };
    case "scrivi":
      return {
        ...stato,
        storia: scrivi(stato.storia, {
          testo: azione.testo,
          etichetta: azione.etichetta,
          gruppo: azione.gruppo,
          adesso: azione.adesso,
        }),
      };
    case "annulla":
      return { ...stato, storia: annulla(stato.storia) };
    case "ripeti":
      return { ...stato, storia: ripeti(stato.storia) };
    case "validato": {
      // Una risposta per un testo che non c'è più non tocca niente: arriva
      // tardi, e mostrarla direbbe un errore che è già stato corretto. Il
      // mittente (`mittente.ts`) ne tiene in volo una sola, ma l'annulla può
      // cambiare il testo mentre quella è in viaggio.
      if (azione.testo !== stato.storia.presente.testo) return stato;
      return {
        ...stato,
        esito: azione.esito,
        validato: azione.testo,
        valido: azione.esito.errori.length === 0 ? azione.esito : stato.valido,
      };
    }
    case "salvato":
      return { ...stato, salvato: azione.testo };
    case "scegli": {
      if (stessaSelezione(stato.selezione, azione.selezione)) return stato;
      // Lo stato scelto è di una parte: passando a un'altra si torna alla base,
      // altrimenti si modificherebbe il `hover` di una parte che non ne ha
      // chiesto uno.
      return { ...stato, selezione: azione.selezione, stato: null };
    }
    case "stato":
      return { ...stato, stato: azione.stato };
    case "scena":
      return { ...stato, scena: azione.scena };
    case "mostra":
      return {
        ...stato,
        scena: scenaPer(azione.parte, stato.scena),
        larghezza: DOVE[azione.parte]?.larghezza ?? stato.larghezza,
      };
    case "rinominato":
      return { ...stato, id: azione.id };
    case "rigioca":
      return { ...stato, rigioca: stato.rigioca + 1 };
    case "vaiRiga":
      return {
        ...stato,
        dock: "codice",
        riga: {
          riga: azione.riga,
          colonna: azione.colonna,
          volta: (stato.riga?.volta ?? 0) + 1,
        },
      };
    case "rigaMostrata":
      // Solo se è ancora quella: una richiesta nuova arrivata nel frattempo
      // deve ancora essere mostrata.
      return stato.riga?.volta === azione.volta ? { ...stato, riga: null } : stato;
    case "imposta":
      return { ...stato, ...azione.campi };
    case "fotografata":
      // Il pannello delle istantanee rilegge l'elenco quando questo cambia:
      // fino alla 3.2 lo faceva solo coi suoi tasti, e un Ctrl+S — che
      // l'ispettore vuoto presenta come «salva e prende un'istantanea» —
      // sembrava non aver fatto niente.
      return { ...stato, istantanee: stato.istantanee + 1 };
    case "risorsaTolta":
      // Non un passo: il file non torna con Ctrl+Z, e le dichiarazioni dei
      // caratteri che lo usavano spariscono da ogni versione della storia.
      // Vedi `risorse.ts`.
      return {
        ...stato,
        storia: riscritta(stato.storia, (t) => senzaCaratteriDi(t, azione.nome)),
      };
  }
}

// ── Letture ────────────────────────────────────────────────────────────────

/** Il testo di adesso. */
export function testo(stato: StatoStudio): string {
  return stato.storia.presente.testo;
}

/** Ci sono modifiche che il disco non ha ancora. */
export function daSalvare(stato: StatoStudio): boolean {
  return stato.caricato && testo(stato) !== stato.salvato;
}

/** La validazione è indietro rispetto al testo. */
export function inValidazione(stato: StatoStudio): boolean {
  return stato.caricato && stato.validato !== testo(stato);
}

/**
 * Lo scafale si può riscrivere dall'albero che si vede.
 *
 * L'albero viene da `valido`, che si aggiorna solo quando torna la
 * validazione: con un testo che ha errori è quello dell'ultima versione
 * buona, e mentre la validazione è in volo è quello del testo di prima. In
 * tutti e due i casi una scrittura riscriverebbe `layout.shell` intero da una
 * versione vecchia — due Canc rapidi, e il secondo rimetteva il nodo tolto
 * dal primo. La nota «correggi gli errori» guarda solo gli errori: questa è
 * la guardia muta delle scritture (`AlberoScafale.tsx`).
 */
export function scafaleScrivibile(stato: StatoStudio): boolean {
  if (stato.esito !== null && stato.esito.errori.length > 0) return false;
  return !inValidazione(stato);
}

/** Il nome della parte selezionata, se è una parte. */
export function parteScelta(stato: StatoStudio): string | null {
  return stato.selezione?.tipo === "parte" ? stato.selezione.nome : null;
}

/**
 * Com'è la bozza, in una parola: quel che si legge accanto al nome.
 *
 * «Rotta» vince su «da salvare» solo quando l'esito è del testo di adesso: un
 * errore di un testo già corretto non è più vero, e dirlo mentre la
 * validazione è in volo farebbe lampeggiare la bandierina a ogni battuta.
 */
export function statoBozza(stato: StatoStudio): "salvata" | "daSalvare" | "rotta" {
  if (stato.esito !== null && stato.esito.errori.length > 0 && stato.validato === testo(stato))
    return "rotta";
  return daSalvare(stato) ? "daSalvare" : "salvata";
}
