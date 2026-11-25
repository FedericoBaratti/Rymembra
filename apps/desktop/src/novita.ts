/**
 * Le Novità: i passi della finestra illustrata che si apre una volta, dopo un
 * aggiornamento, e le regole per aprirla.
 *
 * # Scritte a mano, dentro l'app
 *
 * Il contenuto non viene da nessun server e non è il CHANGELOG: sono due, tre,
 * quattro schermate per versione, scritte da chi fa il rilascio, in parole
 * semplici — quel che chi usa Rymembra nota, non quel che i programmatori hanno
 * toccato. I testi stanno in `lingue/it.json` e `en.json` sotto
 * `whatsNew.<versione>.<n>.title` e `.text`, e `check:lingue` garantisce la
 * parità. Si leggono anche offline, e anche sul telefono.
 *
 * # Un passo
 *
 * `{ illustrazione, titoloKey, testoKey }`. L'illustrazione è una scena SVG
 * leggera (`parti/Novita.tsx`: poche forme, nessuna immagine da caricare) che
 * sceglie un nome di questa lista, e le chiavi sono `Chiave`: un refuso è un
 * errore di compilazione.
 *
 * # Senza novità, niente finestra
 *
 * Una versione che non è qui dentro non ha novità, e la finestra non compare:
 * non è un errore, è una versione correttiva. `strumenti/novita.js` avvisa al
 * rilascio se la versione che si pubblica non c'è — la dimenticanza che questo
 * modulo non può vedere da solo.
 *
 * # La chiave è la versione di `package.json`
 *
 * Quando la versione cambia (`npm run version:set`), la chiave qui sotto e le
 * chiavi `whatsNew.<versione>.*` dei due file di lingua **vanno spostate** alla
 * versione nuova, insieme ai testi: è il passo «scrivere le novità in it e en»
 * del kit di rilascio.
 */
import type { Chiave } from "./lingue";
import { esitoStrisciata } from "./gesti";

/** Le scene che la finestra sa disegnare. */
export const ILLUSTRAZIONI = ["libreria", "sottofondo", "auto", "aggiornamenti"] as const;

export type Illustrazione = (typeof ILLUSTRAZIONI)[number];

export interface PassoNovita {
  illustrazione: Illustrazione;
  titoloKey: Chiave;
  testoKey: Chiave;
}

/**
 * Le novità, per versione.
 *
 * Quattro passi per questa, scritti per chi arriva dalla 3.2.0 (la 3.2.1 non è
 * mai uscita): i dispositivi collegati e la libreria unica, il sottofondo con
 * l'avvio con Windows e la X nel vassoio, Android con Android Auto, gli
 * aggiornamenti. **Ogni frase dice quel che c'è nel codice** — e chi rilascia
 * la rilegge contro quel che è stato unito davvero prima di pubblicare.
 */
export const NOVITA: Readonly<Record<string, readonly PassoNovita[]>> = {
  "3.3.0": [
    {
      illustrazione: "libreria",
      titoloKey: "whatsNew.3.3.0.1.title",
      testoKey: "whatsNew.3.3.0.1.text",
    },
    {
      illustrazione: "sottofondo",
      titoloKey: "whatsNew.3.3.0.2.title",
      testoKey: "whatsNew.3.3.0.2.text",
    },
    {
      illustrazione: "auto",
      titoloKey: "whatsNew.3.3.0.3.title",
      testoKey: "whatsNew.3.3.0.3.text",
    },
    {
      illustrazione: "aggiornamenti",
      titoloKey: "whatsNew.3.3.0.4.title",
      testoKey: "whatsNew.3.3.0.4.text",
    },
  ],
};

/**
 * Le versioni che hanno novità: è quel che la finestra manda al nucleo, che
 * confronta con la sua versione (`novita_da_mostrare`).
 */
export const VERSIONI_CON_NOVITA: readonly string[] = Object.keys(NOVITA);

/** I passi di una versione, o `null` se non ne ha. */
export function novitaDi(versione: string): readonly PassoNovita[] | null {
  return NOVITA[versione] ?? null;
}

// ── quando si apre ───────────────────────────────────────────────────────────

/** Quel che serve per decidere se la finestra delle Novità si apre adesso. */
export interface CondizioniDelleNovita {
  /** Il nucleo ha detto di mostrarle, e non sono ancora state chiuse. */
  daMostrare: boolean;
  /** Sono aperte adesso. */
  aperta: boolean;
  /** L'applicazione ha i suoi dati. */
  avviato: boolean;
  /** Il primo avvio è sullo schermo. */
  primoVisibile: boolean;
  /** Lo Studio delle skin è sullo schermo. */
  studioVisibile: boolean;
  /** Il giro guidato è aperto. */
  giroAperto: boolean;
  /** Un altro dialogo, o un menù, aspetta una risposta. */
  modaleAperto: boolean;
}

/**
 * Se le Novità si aprono, aspettano, o non adesso.
 *
 * **Mai sopra il primo avvio, lo Studio o il giro**: lì c'è già qualcuno che sta
 * facendo un'altra cosa, e la finestra riprova quando cambia lo stato che l'ha
 * fermata. **Sopra un altro dialogo aspetta** — la copia del Store che propone
 * la libreria di prima, per esempio — e si apre quando si chiude: stessa regola
 * del giro (`comeParteIlGiro`), che dal canto suo aspetta le Novità. La
 * schermata di attesa della libreria non c'entra: `App` non è nemmeno montata
 * finché la libreria si apre (`AttesaDellaLibreria`).
 */
export function comeSiApreNovita(c: CondizioniDelleNovita): "apri" | "aspetta" | "no" {
  if (!c.daMostrare || c.aperta || !c.avviato) return "no";
  if (c.primoVisibile || c.studioVisibile || c.giroAperto) return "no";
  return c.modaleAperto ? "aspetta" : "apri";
}

// ── come si sfoglia ──────────────────────────────────────────────────────────

export type Mossa = "avanti" | "indietro";

/** Il passo dopo una mossa: mai fuori dai passi. */
export function passoDopo(corrente: number, totale: number, mossa: Mossa): number {
  if (totale <= 0) return 0;
  const dopo = mossa === "avanti" ? corrente + 1 : corrente - 1;
  return Math.min(totale - 1, Math.max(0, dopo));
}

/**
 * Una strisciata del dito: a sinistra si va avanti, a destra indietro, come si
 * sfogliano le pagine. Vale la regola della copertina (`esitoStrisciata`): oltre
 * un terzo della larghezza, o con un colpo nella stessa direzione.
 */
export function mossaDelDito(strisciata: {
  dx: number;
  vx: number;
  larghezza: number;
}): Mossa | null {
  const esito = esitoStrisciata(strisciata);
  if (esito === null) return null;
  return esito === "prossimo" ? "avanti" : "indietro";
}

/** Le frecce della tastiera sfogliano; ogni altro tasto è di chi l'aspetta. */
export function mossaDaTasto(tasto: string): Mossa | null {
  if (tasto === "ArrowRight") return "avanti";
  if (tasto === "ArrowLeft") return "indietro";
  return null;
}
