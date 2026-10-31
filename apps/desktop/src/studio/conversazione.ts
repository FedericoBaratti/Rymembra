/**
 * La conversazione della chat, come si salva accanto alla bozza.
 *
 * Il file è `skin/bozze/<id>/chat.json`: segue la bozza — «Scarta» lo porta
 * via, una rinomina lo sposta — e non va nel backup su Drive. Qui ci sono
 * solo la forma e la lettura prudente: il file sta su disco, qualcuno ci può
 * aver messo le mani, e una conversazione illeggibile deve diventare una
 * conversazione vuota, non un pannello che non si apre.
 *
 * Le immagini non si salvano mai. La fotografia della tela è grossa, e quel
 * che ha già visto il modello l'ha visto: della battuta resta il segno che
 * c'era.
 */
import type { MessaggioIa } from "../ipc";
import type { Cambio } from "./differenze";
import { accorciato } from "./taglio";

export const VERSIONE_CHAT = 1;

/**
 * Quanto può pesare il file, in byte UTF-8, con margine sotto il tetto del
 * nucleo (2 MiB): oltre, le battute più vecchie cedono il posto.
 *
 * In **byte**, e non in caratteri: il nucleo misura i byte del testo che
 * riceve (`studio.rs`, `scrivi_chat`), e fino alla 3.0 qui si contavano le
 * unità di `String.length`. Una conversazione in italiano sta poco sopra un
 * byte per carattere; una con molti caratteri fuori dall'ASCII — un modello
 * che risponde in giapponese, una chat piena di emoji — arriva a tre o
 * quattro: un milione e mezzo di «caratteri» poteva essere cinque megabyte, e
 * il nucleo rifiutava il salvataggio a ogni battuta, per sempre.
 */
export const PESO_MASSIMO = 1_500_000;

/** Quanti byte occupa un testo in UTF-8, senza costruirne la codifica. */
export function byteUtf8(testo: string): number {
  let byte = 0;
  for (let i = 0; i < testo.length; i++) {
    const c = testo.charCodeAt(i);
    if (c < 0x80) byte += 1;
    else if (c < 0x800) byte += 2;
    else if (c >= 0xd800 && c <= 0xdbff && i + 1 < testo.length) {
      // Una coppia di surrogati è un carattere solo, da quattro byte.
      const dopo = testo.charCodeAt(i + 1);
      if (dopo >= 0xdc00 && dopo <= 0xdfff) {
        byte += 4;
        i += 1;
      } else byte += 3;
    } else byte += 3;
  }
  return byte;
}

/** Quanti cambi di una risposta si tengono per mostrarli. */
export const CAMBI_IN_VISTA = 30;

/** Quanto di un valore cambiato si mostra. */
const VALORE_IN_VISTA = 160;

/** Chi applica le modifiche: vedi `Chat.tsx`. */
export type Modalita = "proposta" | "agent";

/**
 * Un messaggio della conversazione, con quel che lo Studio sa e il modello no.
 *
 * `applicata` c'è sulle risposte del modello che portavano modifiche: `true`
 * se sono entrate nel documento, `false` se chi guardava le ha scartate o non
 * sono passate. Serve alla cronologia che torna al modello (`prompt.ts`,
 * `storiaLeggera`), che fino alla 3.0 diceva «già applicate» anche delle
 * proposte rifiutate. Non parte mai verso il servizio.
 */
export interface MessaggioChat extends MessaggioIa {
  applicata?: boolean;
}

/** Quanto è costato un turno, per quel che il servizio ne ha detto. */
export interface Gettoni {
  entrata: number | null;
  uscita: number | null;
}

/** Un valore cambiato, già in parole: `null` è «non c'era» o «non c'è più». */
export interface CambioMostrato {
  percorso: string;
  prima: string | null;
  dopo: string | null;
}

/** Una battuta come la mostra il pannello. */
export interface Battuta {
  /** Chi ha parlato. Il messaggio di sistema non è mai una battuta. */
  mio: boolean;
  testo: string;
  /** Il ragionamento che ha preceduto la risposta, se il modello lo mostra. */
  pensiero?: string;
  /** Mie: cosa era scelto nello Studio quando ho scritto, già in parole. */
  selezione?: string;
  /**
   * Mie: la fotografia della tela che ha accompagnato la domanda. `true`
   * dopo una rilettura: l'immagine non si salva, il segno sì.
   */
  immagine?: string | true;
  /** Del modello: quanto è costato il turno. */
  gettoni?: Gettoni;
  /** Del modello: cosa ha cambiato nel documento, quando è stato applicato. */
  cambi?: CambioMostrato[];
  /** Quanti altri cambi, oltre quelli in vista. */
  altriCambi?: number;
}

export interface Conversazione {
  modalita: Modalita;
  /** La domanda porta con sé la fotografia della tela. */
  vede: boolean;
  battute: Battuta[];
  /**
   * Quel che torna al modello: senza il messaggio di sistema, che si
   * ricostruisce a ogni turno, e senza immagini.
   */
  messaggi: MessaggioChat[];
}

export const CONVERSAZIONE_VUOTA: Conversazione = {
  modalita: "agent",
  vede: false,
  battute: [],
  messaggi: [],
};

/**
 * Un valore in una riga, tagliato.
 *
 * Senza spezzare una coppia di surrogati (`taglio.ts`): un'emoji a cavallo
 * del taglio lasciava nel file un surrogato solo, e il nucleo rifiutava
 * `chat.json` a ogni salvataggio da lì in poi.
 */
function inRiga(valore: unknown): string | null {
  if (valore === undefined) return null;
  const testo = JSON.stringify(valore) ?? "null";
  return testo.length <= VALORE_IN_VISTA ? testo : accorciato(testo, VALORE_IN_VISTA - 1);
}

/** I cambi di una risposta, pronti da mostrare e da salvare. */
export function mostraCambi(cambi: readonly Cambio[]): { cambi: CambioMostrato[]; altri: number } {
  return {
    cambi: cambi.slice(0, CAMBI_IN_VISTA).map((c) => ({
      percorso: c.percorso,
      prima: inRiga(c.prima),
      dopo: inRiga(c.dopo),
    })),
    altri: Math.max(0, cambi.length - CAMBI_IN_VISTA),
  };
}

/** La conversazione è quella di partenza: non c'è niente da salvare. */
function vuota(c: Conversazione): boolean {
  return c.battute.length === 0 && c.messaggi.length === 0 && c.modalita === "agent" && !c.vede;
}

/** Una battuta come va sul disco: l'immagine diventa un segno. */
function perIlDisco(b: Battuta): Battuta {
  return b.immagine === undefined ? b : { ...b, immagine: true };
}

/**
 * Il testo del file. Vuoto se non c'è niente da tenere: il nucleo, con un
 * testo vuoto, cancella il file.
 *
 * Oltre [`PESO_MASSIMO`] si lasciano cadere le battute e i messaggi più
 * vecchi, metà per volta, finché ci sta: il modello non ha bisogno di
 * ricordare l'inizio di una conversazione lunghissima, e il pannello
 * nemmeno.
 */
export function serializza(c: Conversazione): string {
  if (vuota(c)) return "";
  let battute = c.battute.map(perIlDisco);
  let messaggi: MessaggioChat[] = c.messaggi.map((m) =>
    m.applicata === undefined
      ? { ruolo: m.ruolo, testo: m.testo }
      : { ruolo: m.ruolo, testo: m.testo, applicata: m.applicata },
  );
  for (;;) {
    const testo = JSON.stringify({ versione: VERSIONE_CHAT, modalita: c.modalita, vede: c.vede, battute, messaggi });
    if (byteUtf8(testo) <= PESO_MASSIMO) return testo;
    if (battute.length <= 1 && messaggi.length <= 1) {
      // Quel che resta è una battuta sola più grande del tetto: si taglia il
      // suo testo invece di consegnare al nucleo un file che rifiuterebbe a
      // ogni salvataggio. Un quarto del tetto per ognuno dei due testi, in
      // caratteri: anche a quattro byte l'uno ci stanno. Il taglio non spezza
      // una coppia di surrogati, per la stessa ragione (`taglio.ts`).
      const taglia = (t: string) => accorciato(t, PESO_MASSIMO / 16);
      battute = battute.map((b) => {
        const corta: Battuta = { ...b, testo: taglia(b.testo) };
        delete corta.pensiero;
        return corta;
      });
      messaggi = messaggi.map((m) => ({ ...m, testo: taglia(m.testo) }));
      return JSON.stringify({ versione: VERSIONE_CHAT, modalita: c.modalita, vede: c.vede, battute, messaggi });
    }
    battute = battute.slice(Math.ceil(battute.length / 2));
    messaggi = messaggi.slice(Math.ceil(messaggi.length / 2));
    // Una conversazione che riparte da una risposta non ha una domanda: si
    // riparte dalla prima domanda che resta.
    const prima = messaggi.findIndex((m) => m.ruolo === "user");
    messaggi = prima < 0 ? [] : messaggi.slice(prima);
  }
}

type Oggetto = Record<string, unknown>;
const eOggetto = (v: unknown): v is Oggetto => v !== null && typeof v === "object" && !Array.isArray(v);
const numeroONulla = (v: unknown): number | null =>
  typeof v === "number" && Number.isFinite(v) && v >= 0 ? v : null;
const testoONulla = (v: unknown): string | null => (typeof v === "string" ? v : null);

function leggiCambio(v: unknown): CambioMostrato | null {
  if (!eOggetto(v) || typeof v.percorso !== "string") return null;
  return { percorso: v.percorso, prima: testoONulla(v.prima), dopo: testoONulla(v.dopo) };
}

function leggiBattuta(v: unknown): Battuta | null {
  if (!eOggetto(v) || typeof v.mio !== "boolean" || typeof v.testo !== "string") return null;
  const b: Battuta = { mio: v.mio, testo: v.testo };
  if (typeof v.pensiero === "string") b.pensiero = v.pensiero;
  if (typeof v.selezione === "string") b.selezione = v.selezione;
  if (v.immagine === true || typeof v.immagine === "string") b.immagine = true;
  if (eOggetto(v.gettoni)) {
    b.gettoni = { entrata: numeroONulla(v.gettoni.entrata), uscita: numeroONulla(v.gettoni.uscita) };
  }
  if (Array.isArray(v.cambi)) {
    b.cambi = v.cambi.map(leggiCambio).filter((c): c is CambioMostrato => c !== null).slice(0, CAMBI_IN_VISTA);
  }
  const altri = numeroONulla(v.altriCambi);
  if (altri !== null && altri > 0) b.altriCambi = Math.round(altri);
  return b;
}

function leggiMessaggio(v: unknown): MessaggioChat | null {
  if (!eOggetto(v) || typeof v.testo !== "string") return null;
  // Il sistema non si salva, e uno che dicesse di esserlo non si rimanda:
  // le istruzioni le scrive solo lo Studio.
  if (v.ruolo !== "user" && v.ruolo !== "assistant") return null;
  return typeof v.applicata === "boolean"
    ? { ruolo: v.ruolo, testo: v.testo, applicata: v.applicata }
    : { ruolo: v.ruolo, testo: v.testo };
}

/**
 * Legge il file. Quel che non ha la forma giusta si lascia cadere, voce per
 * voce; un file che non si legge affatto è una conversazione vuota.
 */
export function leggiSalvata(testo: string | null): Conversazione {
  if (testo === null || testo === "") return CONVERSAZIONE_VUOTA;
  let letto: unknown;
  try {
    letto = JSON.parse(testo);
  } catch {
    return CONVERSAZIONE_VUOTA;
  }
  if (!eOggetto(letto) || letto.versione !== VERSIONE_CHAT) return CONVERSAZIONE_VUOTA;
  const battute = Array.isArray(letto.battute)
    ? letto.battute.map(leggiBattuta).filter((b): b is Battuta => b !== null)
    : [];
  const messaggi = Array.isArray(letto.messaggi)
    ? letto.messaggi.map(leggiMessaggio).filter((m): m is MessaggioChat => m !== null)
    : [];
  return {
    modalita: letto.modalita === "proposta" ? "proposta" : "agent",
    vede: letto.vede === true,
    battute,
    messaggi,
  };
}

/**
 * Quanti giri può fare la modalità agent.
 *
 * Quattro. Sotto, un modello che sbaglia un nome al primo giro non fa in tempo
 * a correggersi; sopra, un modello che non ci arriva brucia gettoni e minuti
 * ripetendo la stessa cosa — e su un servizio a pagamento quei giri si pagano.
 */
export const GIRI_MASSIMI = 4;

/** Com'è finito un giro dell'agent, per decidere se aprirne un altro. */
export interface FineGiro {
  /** Il ciclo è ancora vivo: nessuno ha premuto «Ferma». */
  ciclo: boolean;
  giro: number;
  /** Errori (o ragioni di scarto) da rimandare al modello. */
  restano: number;
  tagliato: boolean;
  impuntato: boolean;
}

/**
 * Si apre un altro giro, a pagamento.
 *
 * `ciclo` prima di tutto. Fino alla 3.2 «Ferma» azzerava il giro e basta, col
 * commento che diceva che bastava: ma un turno già chiuso — «Ferma» un attimo
 * dopo l'ultimo gettone — arriva come «finito», e con il giro a zero la
 * condizione `giro >= GIRI_MASSIMI` era falsa: ripartiva un ciclo intero. Lo
 * stesso passando ad «agent» mentre arrivava una risposta in «proposta». Il
 * ciclo adesso è una bandiera sua (`Chat.tsx`, `cicloAgent`), alzata solo da
 * una domanda fatta in agent e abbassata da «Ferma», da un guasto e da ogni
 * uscita.
 */
export function apreUnAltroGiro(f: FineGiro): boolean {
  return f.ciclo && f.restano > 0 && !f.tagliato && !f.impuntato && f.giro < GIRI_MASSIMI;
}

/**
 * La conversazione dopo una domanda che non è partita, o che è caduta prima
 * della risposta.
 *
 * `manda` la aggiunge prima di invocare il servizio — è lì che deve stare se
 * tutto va bene — e fino alla 3.2 un guasto ce la lasciava: un server locale
 * non ancora avviato, e la domanda dopo partiva come [system, user, user].
 * Alcuni modelli (i modelli di Gemma, certi Mistral) rifiutano due «user» di
 * fila, e da lì ogni domanda della conversazione falliva; gli altri
 * eseguivano anche quella vecchia. La risposta si aggiunge solo a turno
 * finito, quindi a un guasto l'ultimo messaggio è sempre quella domanda.
 */
export function senzaDomandaFallita(messaggi: readonly MessaggioChat[]): MessaggioChat[] {
  return messaggi[messaggi.length - 1]?.ruolo === "user" ? messaggi.slice(0, -1) : [...messaggi];
}

/** Le misure di scorrimento di un pannello. */
export interface Scorrimento {
  scrollTop: number;
  clientHeight: number;
  scrollHeight: number;
}

/**
 * Il pannello è in fondo, o quasi: quel che arriva lo deve seguire.
 *
 * Fino alla 3.2 la chat tornava in fondo a ogni pezzo della risposta — fino
 * a venti volte al secondo — e chi risaliva per rileggere la proposta o i
 * cambi di prima veniva riportato giù finché il turno non finiva. Adesso
 * segue solo chi era già in fondo (`Chat.tsx`, che lo misura a ogni
 * scorrimento).
 */
export function vicinoAlFondo(m: Scorrimento, soglia = 48): boolean {
  return m.scrollHeight - (m.scrollTop + m.clientHeight) <= soglia;
}
