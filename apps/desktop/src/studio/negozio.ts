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
import type { OperazioneIa, Validazione } from "../ipc";
import { applicaOperazioni } from "./patch";
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
  /**
   * Le scorciatoie di una lettera sola — «i», «f», «1»–«4» — sono accese.
   *
   * Si possono spegnere (D-69, WCAG 2.1.4): chi usa il riconoscimento vocale,
   * o batte una lettera col fuoco su un tasto, accendeva la sonda o faceva
   * ripartire le animazioni senza volerlo. La scelta si ricorda nelle
   * impostazioni (`studio_lettere`), e arriva dopo l'apertura.
   */
  lettere: boolean;
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
  /**
   * Cresce quando la conversazione salvata è cambiata sul disco fuori dalla
   * chat: «Butta la bozza» la mette da parte, un ripristino la può rimettere.
   * La chat la rilegge — se tenesse quella in memoria, alla battuta dopo
   * riscriverebbe la conversazione appena messa da parte.
   */
  chatSulDisco: number;
  /**
   * Il codice di una parte scritto e non applicato, di un editor che non c'è
   * più: per `tema/parte`.
   *
   * L'editor del codice di una parte applica il suo testo solo quando è JSON,
   * e fino alla 3.2 un testo a metà viveva nel solo componente: un clic su
   * un'altra parte, un cambio di scheda o di tema, e spariva — non era mai
   * arrivato al documento, quindi nemmeno Ctrl+Z lo ritrovava (R14-3). Qui
   * resta finché l'editor della stessa parte non torna a prenderlo.
   */
  codiceInSospeso: Readonly<Record<string, string>>;
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
  | "lettere"
  | "forza"
  | "inPausa"
>;

export type Azione =
  /** Il documento è arrivato dal disco. */
  | { tipo: "apri"; testo: string }
  /** Una modifica: da un controllo, dall'editor di codice, dalla chat. */
  | { tipo: "scrivi"; testo: string; etichetta: string; gruppo: string; adesso: number }
  /**
   * Una modifica di un controllo, come operazioni sul documento: il testo
   * nuovo lo calcola il riduttore, dal presente della storia.
   *
   * Non `scrivi` col testo già fatto fuori: chi lo calcolava partiva da
   * `attuale.current`, che resta indietro finché React non ridisegna, e due
   * scritture nello stesso gestore partivano dallo stesso testo — la seconda
   * disfaceva la prima (R14-1, il «+» accanto a un innesco). Qui ogni azione
   * vede il testo lasciato da quella prima, in qualunque giro arrivi.
   */
  | {
      tipo: "modifica";
      operazioni: readonly OperazioneIa[];
      etichetta: string;
      gruppo: string;
      adesso: number;
    }
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
  /** La conversazione salvata è cambiata sul disco: vedi `chatSulDisco`. */
  | { tipo: "chatSulDisco" }
  /** Mette da parte (o riprende, con `null`) il codice di una parte: vedi `codiceInSospeso`. */
  | { tipo: "codiceInSospeso"; chiave: string; testo: string | null }
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
    lettere: true,
    rigioca: 0,
    inPausa: false,
    riga: null,
    istantanee: 0,
    chatSulDisco: 0,
    codiceInSospeso: {},
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
    case "modifica": {
      // Le ragioni di quel che non si applica si perdono qui, come in
      // `scriviIn`: un controllo che chiede una cosa impossibile è un difetto
      // dello Studio, e il documento resta com'era.
      const { testo: dopo } = applicaOperazioni(testo(stato), azione.operazioni);
      return riduci(stato, {
        tipo: "scrivi",
        testo: dopo,
        etichetta: azione.etichetta,
        gruppo: azione.gruppo,
        adesso: azione.adesso,
      });
    }
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
    case "chatSulDisco":
      return { ...stato, chatSulDisco: stato.chatSulDisco + 1 };
    case "codiceInSospeso": {
      const { [azione.chiave]: tolto, ...altri } = stato.codiceInSospeso;
      if (azione.testo === null) {
        return tolto === undefined ? stato : { ...stato, codiceInSospeso: altri };
      }
      return { ...stato, codiceInSospeso: { ...altri, [azione.chiave]: azione.testo } };
    }
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
export function statoBozza(stato: StatoStudio, mancanti = 0): "salvata" | "daSalvare" | "rotta" {
  // `mancanti`: i file che il documento nomina e il pacchetto non ha
  // (`risorseMancanti`). Rompono l'installazione come un errore, e contano
  // come un errore (R14-5).
  if (stato.esito !== null && stato.esito.errori.length + mancanti > 0 && stato.validato === testo(stato))
    return "rotta";
  return daSalvare(stato) ? "daSalvare" : "salvata";
}
