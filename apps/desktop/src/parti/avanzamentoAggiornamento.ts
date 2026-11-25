/**
 * L'avanzamento di un aggiornamento, come lo tiene la scheda: funzioni pure,
 * che `test/avanzamentoAggiornamento.test.ts` prova.
 *
 * # Il riduttore dello scaricamento
 *
 * Si scriveva solo da `aggiornamenti:avanzamento` e non si azzerava mai: dopo
 * «Annulla» e un nuovo «Aggiorna» la fascia mostrava subito i megabyte e la
 * quota del tentativo di prima, e li teneva finché il nucleo non mandava il
 * primo pezzo nuovo — o per tutto `SILENZIO_MASSIMO`, un minuto, se la rete
 * taceva: un avanzamento che non stava succedendo. Un tentativo nuovo, o uno
 * annullato, riparte da `null`, cioè da «Comincio…».
 *
 * Adesso calcola anche quel che la scheda mostra oltre ai megabyte: la quota,
 * la velocità e il tempo rimasto. Il nucleo manda un pezzo ogni duecento
 * millisecondi (`RESPIRO_AVANZAMENTO`), e l'orologio lo passa chi chiama: così
 * il riduttore resta puro e si prova senza aspettare.
 *
 * # Perché la velocità è una media che segue
 *
 * Il singolo intervallo oscilla: un pacchetto di rete arriva in ritardo e il
 * pezzo dopo recupera. Mostrare quel numero a ogni pezzo vorrebbe dire cifre che
 * ballano cinque volte al secondo. Una media mobile esponenziale
 * (`PESO_DEL_NUOVO`) sta ferma quando la linea è stabile e segue quando cambia.
 *
 * # Perché il tempo rimasto aspetta
 *
 * Una stima su due intervalli è un numero a caso, e sbagliarla di un fattore tre
 * all'inizio fa perdere fiducia in tutte le cifre che vengono dopo
 * (`CAMPIONI_PER_LA_STIMA`). Si dice solo se il totale si sa, e mai oltre un
 * giorno (`TETTO_RESTANTI`): a rete ferma la media scende verso zero, e il
 * tempo rimasto verso l'infinito non è un'informazione.
 */
import type { AvanzamentoAggiornamento, FaseAggiornamento } from "../ipc";

/** Quanto pesa l'ultima misura nella velocità: il resto è la media di prima. */
const PESO_DEL_NUOVO = 0.3;

/** Quanti pezzi servono, il primo compreso, prima di dire quanto manca. */
const CAMPIONI_PER_LA_STIMA = 5;

/** Oltre un giorno, «mancano» non dice più niente: si tace. */
const TETTO_RESTANTI = 24 * 3600;

/** Lo scaricamento a un pezzo, con quel che se ne ricava. */
export interface Avanzamento {
  /** I byte già scesi. */
  scaricati: number;
  /** Quanti se ne aspettano in tutto, quando il server lo dice. */
  totale: number | null;
  /**
   * Da 0 a 100, o `null` se il totale non si sa: una barra che non sa dove
   * arriva è una barra che mente.
   */
  quota: number | null;
  /** Byte al secondo, in media mobile. `null` finché non ci sono due pezzi. */
  velocita: number | null;
  /** Secondi rimasti. `null` finché la stima non regge, o se il totale manca. */
  restanti: number | null;
  /** Quando è arrivato l'ultimo pezzo, secondo l'orologio di chi chiama. */
  ultimoMs: number;
  /** Quanti pezzi si sono contati da quando lo scaricamento è cominciato. */
  campioni: number;
}

export type EventoAvanzamento =
  | { tipo: "avanzamento"; valore: AvanzamentoAggiornamento; adesso: number }
  | { tipo: "installa" }
  | { tipo: "annulla" };

/** Da quanto è sceso a quanto manca, per un totale e una velocità. */
function restantiPer(
  totale: number | null,
  scaricati: number,
  velocita: number | null,
  campioni: number,
): number | null {
  if (totale === null || velocita === null || velocita <= 0) return null;
  if (campioni < CAMPIONI_PER_LA_STIMA) return null;
  const secondi = Math.max(0, totale - scaricati) / velocita;
  return Number.isFinite(secondi) && secondi <= TETTO_RESTANTI ? secondi : null;
}

function quotaPer(scaricati: number, totale: number | null): number | null {
  return totale !== null && totale > 0
    ? Math.min(100, (scaricati / totale) * 100)
    : null;
}

export function avanzamentoDopo(
  prima: Avanzamento | null,
  evento: EventoAvanzamento,
): Avanzamento | null {
  if (evento.tipo !== "avanzamento") return null;
  const { scaricati, totale } = evento.valore;
  const { adesso } = evento;
  const quota = quotaPer(scaricati, totale);

  // Il primo pezzo, o byte che tornano indietro: è un tentativo nuovo anche se
  // nessuno ha detto «installa» — un secondo scaricamento dopo un guasto.
  if (prima === null || scaricati < prima.scaricati) {
    return {
      scaricati,
      totale,
      quota,
      velocita: null,
      restanti: null,
      ultimoMs: adesso,
      campioni: 1,
    };
  }

  const secondi = (adesso - prima.ultimoMs) / 1000;
  // Un orologio che non avanza non divide per zero: i byte si segnano, e la
  // velocità resta quella di prima.
  if (secondi <= 0) {
    return {
      ...prima,
      scaricati,
      totale,
      quota,
      restanti: restantiPer(totale, scaricati, prima.velocita, prima.campioni),
    };
  }

  const istantanea = (scaricati - prima.scaricati) / secondi;
  const velocita =
    prima.velocita === null
      ? istantanea
      : prima.velocita * (1 - PESO_DEL_NUOVO) + istantanea * PESO_DEL_NUOVO;
  const campioni = prima.campioni + 1;
  return {
    scaricati,
    totale,
    quota,
    velocita,
    restanti: restantiPer(totale, scaricati, velocita, campioni),
    ultimoMs: adesso,
    campioni,
  };
}

// ── i passi ───────────────────────────────────────────────────────────────────

/** Dove gira la scheda: i passi sono quattro sul computer e tre sul telefono. */
export type Forma = "computer" | "telefono";

export type ChiavePasso = "scarico" | "verifico" | "installo" | "riavvio" | "installa";

export interface Passo {
  chiave: ChiavePasso;
  /** `fatto`, `corrente` o `attesa`: l'unica cosa che la scheda deve sapere. */
  stato: "fatto" | "corrente" | "attesa";
}

const PASSI: Record<Forma, ChiavePasso[]> = {
  computer: ["scarico", "verifico", "installo", "riavvio"],
  // Sul telefono la sequenza si ferma a «Installa»: la schermata di conferma è
  // di Android, e quel passo lo fa chi usa il telefono.
  telefono: ["scarico", "verifico", "installa"],
};

/** Il passo a cui corrisponde una fase, sul computer e sul telefono. */
function posizioneDi(fase: FaseAggiornamento, forma: Forma): number | null {
  if (forma === "computer") {
    // Un pacchetto già verificato che aspetta la chiusura non è un passo: è una
    // sosta, e la scheda la racconta a parole.
    return fase === "pronto" ? null : PASSI.computer.indexOf(fase as ChiavePasso);
  }
  switch (fase) {
    case "scarico":
      return 0;
    case "verifico":
      return 1;
    default:
      return 2;
  }
}

/** I passi da disegnare per una fase: vuoto se non ce n'è uno da mostrare. */
export function passiDi(fase: FaseAggiornamento | null, forma: Forma): Passo[] {
  if (fase === null) return [];
  const corrente = posizioneDi(fase, forma);
  if (corrente === null || corrente < 0) return [];
  return PASSI[forma].map((chiave, i) => ({
    chiave,
    stato: i < corrente ? "fatto" : i === corrente ? "corrente" : "attesa",
  }));
}

// ── l'attesa del pacchetto ────────────────────────────────────────────────────

const GIORNO_MS = 24 * 3600 * 1000;

/**
 * Dopo quanto la scheda ripropone «Installa ora».
 *
 * Con l'avvio con Windows e la X nel vassoio il programma può non uscire mai, e
 * un pacchetto che aspetta «la chiusura» aspetterebbe per sempre. Tre giorni:
 * abbastanza perché chi chiude il computer la sera lo abbia già installato da
 * sé, e pochi perché un aggiornamento di sicurezza non resti in attesa una
 * settimana.
 */
export const SOLLECITO_MS = 3 * GIORNO_MS;

/**
 * Il pacchetto aspetta da abbastanza perché valga la pena chiedere di nuovo.
 *
 * Senza un momento — o con un orologio spostato indietro, che farebbe una
 * differenza negativa — non si sollecita: sollecitare per un'ora sbagliata è
 * peggio che non farlo.
 */
export function attesaLunga(dalMs: number | null, adesso: number): boolean {
  return dalMs !== null && adesso - dalMs >= SOLLECITO_MS;
}

// ── il tempo che manca, a parole ─────────────────────────────────────────────

/**
 * Il tempo rimasto in una misura che si legge: secondi sotto il minuto, minuti
 * sopra, sempre per eccesso. «Mancano 0 s» non si dice mai.
 */
export function quantoManca(secondi: number): { unita: "s" | "min"; n: number } {
  const n = Math.max(1, Math.ceil(secondi));
  return n <= 59 ? { unita: "s", n } : { unita: "min", n: Math.ceil(n / 60) };
}

// ── l'errore che la scheda mostra ────────────────────────────────────────────

/**
 * L'errore da mostrare sulla scheda, o `null`.
 *
 * Il nucleo scrive `ultimo_errore` a ogni controllo che non trova la rete —
 * ogni mezz'ora sul computer, ogni sei ore sul telefono — **anche con un
 * pacchetto già scaricato** che aspetta la chiusura: il portatile in sospensione,
 * una VPN che cambia, il wifi che si riaggancia. Quell'errore non dice niente di
 * quel che la scheda racconta, e la scheda «È pronta» non lo mostra: l'errore
 * appartiene a un tentativo — qualcuno ha premuto «Aggiorna» e il pacchetto non è
 * arrivato — e si vede solo quando **non c'è nessun passo**, né uno in corso né
 * un pacchetto in sosta.
 *
 * Con un passo in corso la barra va avanti, e il guasto che ne verrà arriverà con
 * il passo che finisce.
 */
export function guastoDaMostrare<E>(
  tentato: boolean,
  fase: FaseAggiornamento | null,
  errore: E | null | undefined,
): E | null {
  return tentato && fase === null && errore != null ? errore : null;
}
