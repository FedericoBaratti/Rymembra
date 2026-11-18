/**
 * La libreria unica vista dalla finestra: da dove si guarda, quali righe sono
 * di un altro dispositivo, e quali di loro adesso non si possono suonare.
 *
 * # Il filtro
 *
 * Con i dispositivi collegati la libreria è una sola (§4 della guida delle
 * modifiche del 18/11): le righe del PC stanno in `tracks` accanto a quelle di
 * qui. Il filtro per dispositivo — «Tutti · Questo dispositivo · <nome>» — dice
 * al nucleo da dove leggere (`OrigineLibreria`): «Tutti» è la libreria unica,
 * una voce per brano; «Questo dispositivo» tutte le righe di qui, anche quelle
 * che «Tutti» nasconde dietro una copia migliore di un altro; «<nome>» tutte le
 * righe di quel dispositivo. È quel che prima era la pagina «Sfoglia».
 *
 * Le pillole non ci sono finché non c'è un dispositivo collegato: un filtro che
 * può scegliere solo fra «Tutti» e «Questo dispositivo» è due nomi per la stessa
 * cosa.
 *
 * # Il grigio
 *
 * Un dispositivo spento non toglie i suoi brani: restano in libreria, in grigio
 * (`statoDellaRiga`). Sul dispositivo resta l'elenco, non l'audio. Un tocco su
 * una riga grigia dice che quel dispositivo non risponde, e non suona niente: è
 * un avviso, non un errore. Quel che si suona a liste intere — «Riproduci»,
 * un album, la selezione — salta le righe grige (`suonabili`).
 *
 * Senza React e senza il pannello: le prove di `test/` lo caricano con
 * `node --test`.
 */
import { dispositivoDellaRiga, type OrigineLibreria } from "./ipc/libreria";
import type { StatoLontano } from "./ipc/lontano";

/** «Tutti»: la libreria unica. */
export const TUTTI: OrigineLibreria = { tipo: "tutti" };

/** «Questo dispositivo»: tutte le righe di qui. */
export const LOCALE: OrigineLibreria = { tipo: "locale" };

/** Il filtro di un dispositivo, dalla sua chiave. */
export function origineDi(chiave: string): OrigineLibreria {
  return { tipo: "dispositivo", chiave };
}

/** Una stringa per origine: la chiave di una lista rada, di una cache. */
export function chiaveDiOrigine(origine: OrigineLibreria): string {
  return origine.tipo === "dispositivo" ? `dispositivo:${origine.chiave}` : origine.tipo;
}

/** Due origini dicono lo stesso filtro. */
export function ugualiOrigini(a: OrigineLibreria, b: OrigineLibreria): boolean {
  return chiaveDiOrigine(a) === chiaveDiOrigine(b);
}

/**
 * L'origine da mandare al nucleo: «Tutti» è l'assenza, e costa a chi non ha
 * dispositivi quel che costava prima (`origine` non si legge).
 */
export function perIlNucleo(origine: OrigineLibreria): OrigineLibreria | undefined {
  return origine.tipo === "tutti" ? undefined : origine;
}

/** Un dispositivo come lo mostra un filtro o una riga. */
export interface DispositivoDiLibreria {
  chiave: string;
  nome: string;
  telefono: boolean;
  /** Risponde adesso. */
  online: boolean;
}

/**
 * I dispositivi che hanno (o possono avere) righe in libreria: i collegati non
 * revocati, per nome. Anche quelli spenti — sono quelli che la riga grigia
 * mostra — e anche quelli che non hanno ancora mandato niente.
 */
export function dispositiviDiLibreria(stato: StatoLontano | null): DispositivoDiLibreria[] {
  if (stato === null) return [];
  return stato.dispositivi
    .filter((d) => !d.revocato)
    .map((d) => ({
      chiave: d.chiave,
      nome: d.nome,
      telefono: d.piattaforma === "android",
      online: d.online,
    }))
    .sort((a, b) => a.nome.localeCompare(b.nome));
}

/**
 * Il filtro che vale con questi dispositivi: un dispositivo che non c'è più —
 * dimenticato, o revocato dall'altra parte — non si può filtrare, e si torna a
 * «Tutti».
 */
export function origineValida(
  origine: OrigineLibreria,
  stato: StatoLontano | null,
): OrigineLibreria {
  if (origine.tipo !== "dispositivo") return origine;
  // Il pannello che non è ancora arrivato non dice che il dispositivo non c'è.
  if (stato === null) return origine;
  const trovato = dispositiviDiLibreria(stato).some((d) => d.chiave === origine.chiave);
  return trovato ? origine : TUTTI;
}

/** Una pillola del filtro: l'origine che sceglie. */
export interface PillolaOrigine {
  origine: OrigineLibreria;
  /** Il nome del dispositivo; assente per «Tutti» e «Questo dispositivo». */
  dispositivo: DispositivoDiLibreria | null;
}

/**
 * Le pillole del filtro: «Tutti», «Questo dispositivo» e un dispositivo
 * ciascuno. Nessuna se non c'è un dispositivo collegato.
 */
export function pilloleOrigine(stato: StatoLontano | null): PillolaOrigine[] {
  const dispositivi = dispositiviDiLibreria(stato);
  if (dispositivi.length === 0) return [];
  return [
    { origine: TUTTI, dispositivo: null },
    { origine: LOCALE, dispositivo: null },
    ...dispositivi.map((d) => ({ origine: origineDi(d.chiave), dispositivo: d })),
  ];
}

/** Di quale dispositivo è una riga, e se adesso risponde. */
export interface StatoDellaRiga {
  chiave: string;
  nome: string;
  telefono: boolean;
  /** Non si può suonare adesso: il dispositivo non risponde, o non è più collegato. */
  spenta: boolean;
}

/**
 * Lo stato di una riga della libreria unica, o `null` per una riga di qui.
 *
 * Una riga di un dispositivo che non è fra i collegati — l'hanno dimenticato
 * mentre la finestra aveva ancora la lista — è spenta, e di nome non ne ha.
 * Con il pannello che non è ancora arrivato la riga non si dice spenta: finché
 * non si sa, un grigio sarebbe un'invenzione.
 */
export function statoDellaRiga(
  percorso: string | null | undefined,
  stato: StatoLontano | null,
): StatoDellaRiga | null {
  const chiave = dispositivoDellaRiga(percorso);
  if (chiave === null) return null;
  if (stato === null) return { chiave, nome: "", telefono: false, spenta: false };
  const suo = stato.dispositivi.find((d) => d.chiave === chiave && !d.revocato);
  if (suo === undefined) return { chiave, nome: "", telefono: false, spenta: true };
  return {
    chiave,
    nome: suo.nome,
    telefono: suo.piattaforma === "android",
    spenta: !suo.online,
  };
}

/**
 * Di una lista da suonare, le righe che adesso si possono suonare: quelle di un
 * dispositivo che non risponde restano fuori. `da` è dove si parte nella lista
 * intera; l'indice che si ritorna è il posto, nella lista che resta, della
 * prima riga suonabile da lì in poi.
 *
 * Una lista senza niente di suonabile torna vuota: chi chiama lo dice — con
 * l'avviso, non con un errore — e non manda niente al lettore.
 */
export function suonabili<T extends { path: string | null }>(
  brani: readonly T[],
  da: number,
  stato: StatoLontano | null,
): { brani: T[]; indice: number } {
  const tenuti: T[] = [];
  let indice = 0;
  brani.forEach((brano, posto) => {
    if (statoDellaRiga(brano.path, stato)?.spenta === true) return;
    if (posto < da) indice += 1;
    tenuti.push(brano);
  });
  return { brani: tenuti, indice: Math.min(indice, Math.max(tenuti.length - 1, 0)) };
}

/**
 * Il nome del dispositivo che sta dietro il primo di questi brani spenti, per
 * dire «<PC> non raggiungibile»: il primo che ha un nome, o `null` se nessuno
 * dei brani è spento.
 */
export function primoSpento<T extends { path: string | null }>(
  brani: readonly T[],
  stato: StatoLontano | null,
): StatoDellaRiga | null {
  for (const brano of brani) {
    const riga = statoDellaRiga(brano.path, stato);
    if (riga?.spenta === true) return riga;
  }
  return null;
}

/** Le viste della libreria che il filtro per dispositivo riguarda. */
export const VISTE_FILTRABILI = ["album", "artisti", "brani", "preferiti"] as const;

/** Una vista della libreria che il filtro riguarda. */
export type VistaFiltrabile = (typeof VISTE_FILTRABILI)[number];

/** La vista è una di quelle che il filtro riguarda? */
export function eFiltrabile(vista: string): vista is VistaFiltrabile {
  return (VISTE_FILTRABILI as readonly string[]).includes(vista);
}

/**
 * Dove si apre la libreria quando si sceglie un dispositivo (la barra laterale,
 * Altro, le Impostazioni): dov'era, se è una vista che il filtro riguarda; se
 * no, gli Album. Prima era «Sfoglia», che apriva un'altra pagina.
 */
export function vistaPerIlFiltro(vista: string): VistaFiltrabile {
  return eFiltrabile(vista) ? vista : "album";
}

/**
 * Come [`origineValida`], con le chiavi dei dispositivi collegati già in mano
 * (separate da virgole, o `null` finché il pannello non c'è): per chi segue
 * quelle e non il pannello intero.
 */
export function origineConChiavi(origine: OrigineLibreria, chiavi: string | null): OrigineLibreria {
  if (origine.tipo !== "dispositivo" || chiavi === null) return origine;
  return chiavi.split(",").includes(origine.chiave) ? origine : TUTTI;
}

/** Le chiavi dei dispositivi di libreria, in una stringa che si confronta con `===`. */
export function chiaviDiLibreria(stato: StatoLontano | null): string | null {
  if (stato === null) return null;
  return dispositiviDiLibreria(stato)
    .map((d) => d.chiave)
    .sort()
    .join(",");
}
