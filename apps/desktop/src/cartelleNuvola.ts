/**
 * I percorsi del cloud, dal lato della finestra: riconoscerli, e dirli.
 *
 * # Perché un modulo solo
 *
 * Una cartella di OneDrive sta fra le sorvegliate come `nuvola://onedrive-
 * k3x7q2ab/Musica/Rock`, e un brano sotto di lei come lo stesso percorso con
 * il file in fondo. È la grammatica di `rymembra_domain::nuvola`, e da questa
 * parte del confine la si legge in una dozzina di posti: l'elenco delle
 * cartelle nelle Impostazioni, l'albero di «Cartelle», il menù di un brano, la
 * domanda prima di togliere una cartella, la riga che dice quali radici una
 * scansione ha saltato. Se ognuno avesse il suo `startsWith`, il primo a
 * sbagliare la grafia mostrerebbe a qualcuno un `nuvola://` nudo — che non
 * dice niente a chi ascolta musica — o, peggio, offrirebbe «Mostra nella
 * cartella» su un file che una cartella sul disco non ce l'ha.
 *
 * Quindi qui c'è **un** `eNuvola`, come nel nucleo c'è un `e_nuvola`, e un solo
 * modo di scrivere una radice per una persona: [`nomeRadice`].
 *
 * Niente React, niente nucleo, niente lingue: `node --test` lo carica da solo
 * (`test/cartelleNuvola.test.ts`). I nomi dei servizi non si traducono — sono
 * nomi propri — e stanno qui scritti come li scrive `Servizio::nome`.
 */
import type { ServizioNuvola } from "./ipc";
import type { NomeIcona } from "./parti/Icone";

/** Come comincia un percorso del cloud: `rymembra_domain::nuvola::SCHEMA`. */
export const SCHEMA = "nuvola://";

/** Il separatore fra due cartelle quando si scrive un percorso per una persona. */
export const FRECCIA = " › ";

/** Il separatore fra il servizio e la cartella: «OneDrive · Musica › Rock». */
export const PUNTO = " · ";

/**
 * Il nome di ogni servizio, come lo scrive `Servizio::nome` nel nucleo.
 *
 * Il link di MEGA è MEGA anche lui: per chi lo guarda è la stessa cosa, e la
 * differenza — un account o una cartella condivisa — la dice il nome del conto.
 */
export const NOMI_SERVIZI: Readonly<Record<ServizioNuvola, string>> = {
  onedrive: "OneDrive",
  gdrive: "Google Drive",
  dropbox: "Dropbox",
  webdav: "WebDAV",
  mega: "MEGA",
  megalink: "MEGA",
};

/** È un servizio che questa finestra conosce? */
function eServizio(chiave: string): chiave is ServizioNuvola {
  return Object.hasOwn(NOMI_SERVIZI, chiave);
}

/**
 * Il percorso è nel cloud?
 *
 * Accetta anche `null` e `undefined` perché `Brano.path` può mancare — un
 * brano di catalogo — e chi chiede vuole un sì o un no, non un controllo in più.
 */
export function eNuvola(percorso: string | null | undefined): boolean {
  return typeof percorso === "string" && percorso.startsWith(SCHEMA);
}

/** Il servizio di un conto (`onedrive-k3x7q2ab` → `onedrive`), o `null`. */
export function servizioDelConto(conto: string): ServizioNuvola | null {
  const trattino = conto.indexOf("-");
  if (trattino <= 0) return null;
  const chiave = conto.slice(0, trattino);
  return eServizio(chiave) ? chiave : null;
}

/** Il conto di un percorso del cloud, o `null` se il percorso è sul disco. */
export function contoDi(percorso: string): string | null {
  if (!eNuvola(percorso)) return null;
  const resto = percorso.slice(SCHEMA.length);
  const barra = resto.indexOf("/");
  const conto = barra < 0 ? resto : resto.slice(0, barra);
  return conto.length > 0 ? conto : null;
}

/** Il servizio di un percorso del cloud, o `null` se il percorso è sul disco. */
export function servizioDi(percorso: string | null | undefined): ServizioNuvola | null {
  if (typeof percorso !== "string") return null;
  const conto = contoDi(percorso);
  return conto === null ? null : servizioDelConto(conto);
}

/** Il nome del servizio di un percorso, o `null` se il percorso è sul disco. */
export function nomeServizioDi(percorso: string | null | undefined): string | null {
  const servizio = servizioDi(percorso);
  return servizio === null ? null : NOMI_SERVIZI[servizio];
}

/** I caratteri che il nucleo codifica in un segmento: `DA_CODIFICARE`, più il punto. */
const FUGHE = new Set(["%", "/", "\\", "|", "."]);

/**
 * Il nome com'era sul servizio, da un segmento del percorso.
 *
 * La stessa regola di `decodifica_segmento`: si sciolgono solo le fughe che il
 * nucleo produce, e un `%` seguito da altro resta un `%`. Un nome come
 * «100% Hits» torna uguale, e un segmento scritto da una versione futura con
 * più fughe si legge lo stesso invece di diventare un errore.
 */
export function decodificaSegmento(segmento: string): string {
  let fuori = "";
  let i = 0;
  while (i < segmento.length) {
    const c = segmento[i];
    if (c === "%") {
      const codice = Number.parseInt(segmento.slice(i + 1, i + 3), 16);
      const carattere =
        segmento.length >= i + 3 && Number.isFinite(codice)
          ? String.fromCharCode(codice)
          : "";
      if (FUGHE.has(carattere)) {
        fuori += carattere;
        i += 3;
        continue;
      }
    }
    fuori += c;
    i += 1;
  }
  return fuori;
}

/** I nomi, decodificati, delle cartelle dopo il conto: `segmenti` del nucleo. */
export function segmentiDi(percorso: string): string[] {
  if (!eNuvola(percorso)) return [];
  return percorso
    .slice(SCHEMA.length)
    .split("/")
    .slice(1)
    .filter((s) => s.length > 0)
    .map(decodificaSegmento);
}

/**
 * Una radice, scritta per una persona.
 *
 * Una cartella del disco resta il suo percorso: è quel che si riconosce, e
 * accorciarlo vorrebbe dire nascondere proprio la parte che distingue
 * `D:\Musica` da `E:\Musica`. Una del cloud diventa «OneDrive · Musica ›
 * Rock», cioè quel che il nucleo mette in `NodoCartella.nome` per le radici —
 * ricavato qui dal percorso, che c'è sempre, invece che dall'elenco dei conti,
 * che può non essere ancora arrivato. La cima di un drive è il servizio e
 * basta.
 *
 * Un percorso del cloud di un servizio che questa finestra non conosce — una
 * versione futura, un profilo da un altro PC — si scrive col nome del conto:
 * brutto, ma vero, e meglio di un `nuvola://`.
 */
export function nomeRadice(percorso: string): string {
  if (!eNuvola(percorso)) return percorso;
  const servizio = nomeServizioDi(percorso) ?? contoDi(percorso) ?? percorso;
  const cartelle = segmentiDi(percorso);
  return cartelle.length === 0 ? servizio : `${servizio}${PUNTO}${cartelle.join(FRECCIA)}`;
}

/**
 * La prima cartella del disco, fra le sorvegliate.
 *
 * Dove finiscono i brani scaricati, quando non lo si è scelto, e da dove parte
 * il dialogo che lo sceglie: in tutti e due i casi una cartella del cloud non
 * va bene — i download non ci si scrivono, e il dialogo di sistema non sa
 * aprirla.
 */
export function primaLocale(cartelle: readonly string[]): string | undefined {
  return cartelle.find((c) => !eNuvola(c));
}

/** Le cartelle del disco, fra le sorvegliate. */
export function soloLocali(cartelle: readonly string[]): string[] {
  return cartelle.filter((c) => !eNuvola(c));
}

/**
 * L'icona di un servizio: generica, mai il marchio.
 *
 * Un server per WebDAV — è un indirizzo, non un'azienda — e una nuvola per
 * gli altri. Niente loghi: non sono nostri da disegnare, e una fila di cinque
 * marchi colorati nella finestra dice «pubblicità» prima di dire «cartelle».
 * Il nome scritto accanto basta a distinguerli.
 */
export function iconaServizio(servizio: ServizioNuvola | null): NomeIcona {
  if (servizio === null) return "i-folder";
  return servizio === "webdav" ? "i-server" : "i-nuvola";
}

/**
 * La linguetta di un servizio nel selettore.
 *
 * Il link di MEGA sta sotto MEGA: chi ha un link in mano cerca «MEGA», e due
 * linguette per lo stesso nome sarebbero una domanda a cui chi guarda non sa
 * rispondere prima di aver provato.
 */
export type Scheda = Exclude<ServizioNuvola, "megalink">;

/** Vedi [`Scheda`]. */
export function schedaDi(servizio: ServizioNuvola): Scheda {
  return servizio === "megalink" ? "mega" : servizio;
}
