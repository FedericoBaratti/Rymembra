/**
 * Le lingue dell'interfaccia.
 *
 * # La regola che questo file esiste per rispettare
 *
 * Aggiungere una lingua deve costare **un file solo**. Si lascia cadere
 * `de.json` qui dentro e il tedesco compare nelle impostazioni: nessun elenco
 * da allungare, nessun `import` da scrivere, nessuno `switch` da aggiornare.
 *
 * Il perno è `import.meta.glob`, che Vite risolve a build time in un oggetto con
 * una voce per file. L'elenco delle lingue disponibili non è una tabella nel
 * codice: **è il contenuto della cartella**. Un elenco scritto a mano sarebbe
 * andato fuori sincrono al primo file aggiunto da qualcun altro, e il sintomo —
 * un `de.json` che c'è e non si vede — non nomina la sua causa.
 *
 * Per la stessa ragione il nome umano della lingua («Italiano», «Deutsch»)
 * viaggia **dentro** il file, sotto la chiave riservata `_nome`: se stesse in
 * una mappa qui, aggiungere una lingua costerebbe due posti invece di uno, e la
 * promessa sarebbe falsa proprio nel punto in cui si vede.
 *
 * # A richiesta, non in anticipo

I dizionari non stanno nel pacchetto principale: erano un terzo del JavaScript
(≈454 KB su 1 288 KB), analizzati a ogni avvio anche se se ne usa uno per
volta (R21-4). `import.meta.glob` ne fa un pezzo per lingua, e `caricamento.ts`
li legge a richiesta: `main.tsx` carica la lingua del sistema prima del primo
disegno, `aspettoFinestra.ts` quella salvata prima di mostrare la finestra, e
[`impostaLingua`] carica e poi applica — così nemmeno il cambio a caldo passa
dalle chiavi nude. L'altra lingua si carica quando serve: scelta, o come
ripiego per una chiave che manca in una lingua incompleta. I nomi delle lingue
(`_nome`) non si leggono dai dizionari: li raccoglie a build time il modulo
virtuale `virtual:lingue-nomi` (`vite.config.ts`), dagli stessi file.

# Perché non una libreria
 *
 * Il progetto ha quattro dipendenze runtime in tutto, e la CI ne vieta
 * esplicitamente altre. Quel che serve qui è ricerca per chiave, interpolazione
 * testuale e il plurale: duecento righe leggibili, contro quaranta kilobyte di
 * libreria generica e un formato di catalogo che nessuno di noi ha scelto. Le
 * regole del plurale non le scriviamo noi: le ha già il motore
 * (`Intl.PluralRules`), e `plurali.ts` sceglie la forma.
 *
 * # Perché un modulo e non un contesto
 *
 * Come per la posizione di riproduzione (`riproduzione.ts`): di lingua ce n'è
 * una per finestra, e i testi servono anche **fuori** da React — dentro
 * `formato.ts`, dentro `ipc/`, dentro tabelle costruite in cima a un modulo.
 * Un contesto li renderebbe raggiungibili solo da un componente, e obbligherebbe
 * a far scendere `t` per prop attraverso settanta file: `Impostazioni` ne
 * riceve già sessantuno.
 *
 * Chi disegna si iscrive con [`useLingua`]; chi calcola e basta chiama [`t`].
 */
import { useSyncExternalStore } from "react";

import NOMI from "virtual:lingue-nomi";
import { creaCambioLingua, creaCaricatore, type Dizionario } from "./caricamento";
// Solo il tipo: le chiavi che esistono davvero. Il dizionario non entra nel pacchetto.
import type it from "./it.json";
import { formaPlurale } from "./plurali";
import { regioneDelleDate } from "./regione";
import { MOBILE } from "../piattaforma";

export type { Dizionario };

/**
 * Le chiavi che esistono davvero.
 *
 * Derivata da `it.json` e non da `Record<string, string>`: con
 * `noUncheckedIndexedAccess` una mappa aperta restituirebbe `string | undefined`
 * a ogni lettura, e ogni chiamata a `t()` avrebbe un ripiego da scrivere. Così
 * invece una chiave inventata è un errore di compilazione — che è il momento
 * giusto per scoprirla, invece che davanti a chi usa l'applicazione.
 *
 * L'italiano è la sorgente perché è la lingua in cui i testi si scrivono per
 * primi. Le altre possono essere incomplete: ripiegano (vedi [`t`]).
 */
export type Chiave = keyof typeof it;

/*
 * La chiave riservata `_nome` è il nome della lingua nella sua lingua.
 *
 * Non è un testo dell'interfaccia: è l'etichetta che il selettore mostra, e
 * viaggia col file perché è l'unico modo di non avere una seconda tabella da
 * aggiornare. La legge `vite.config.ts` a build time (`virtual:lingue-nomi`), e
 * non questo modulo: i dizionari qui non si caricano per scrivere il selettore.
 */

/**
 * La lingua di ripiego, per due mestieri diversi.
 *
 * Chi arriva con un sistema in svedese e non trova `sv.json` parte in inglese;
 * e una chiave che manca dentro un `de.json` incompleto si legge in inglese.
 * Sono due cose distinte e ripiegano sulla stessa lingua per la stessa ragione:
 * è quella che più persone leggono. **Non** è l'italiano, che pure è la lingua
 * in cui il codice è scritto — i due ruoli non vanno confusi.
 */
export const RIPIEGO = "en";

/** Una lingua disponibile. */
export interface Lingua {
  /** Il codice ISO, che è il nome del file senza estensione. */
  codice: string;
  /** Come si chiama nella propria lingua. Arriva da `_nome`. */
  nome: string;
}

/**
 * I file, come Vite li trova: per ognuno, chi lo carica.
 *
 * Non `eager`: ogni lingua è un pezzo a parte, che si legge quando serve
 * (`caricamento.ts`). Quel che qui si sa subito è **quali** lingue ci sono: i
 * codici sono i nomi dei file.
 */
const FILE = import.meta.glob("./*.json", {
  import: "default",
}) as Record<string, () => Promise<Dizionario>>;

/** I caricatori, per codice. */
const CARICATORI: Record<string, () => Promise<Dizionario>> = {};
for (const [percorso, carica] of Object.entries(FILE)) {
  const codice = percorso.replace(/^\.\//, "").replace(/\.json$/, "");
  CARICATORI[codice] = carica;
}
const caricatore = creaCaricatore(CARICATORI);

/**
 * Le lingue disponibili, in ordine di codice.
 *
 * In ordine e non nell'ordine in cui `glob` le restituisce: quello dipende dal
 * filesystem, e un selettore che cambia ordine fra una macchina e l'altra è una
 * differenza che nessuno sa spiegare. Il nome viene da `_nome` dentro il file,
 * raccolto a build time (`virtual:lingue-nomi`): non serve caricare un
 * dizionario per scrivere il selettore.
 */
export const DISPONIBILI: Lingua[] = Object.keys(CARICATORI)
  .sort()
  .map((codice) => ({
    codice,
    nome: NOMI[codice] ?? codice,
  }));

/** C'è un file per questa lingua? */
export function esiste(codice: string): boolean {
  return Object.hasOwn(CARICATORI, codice);
}

/**
 * Riduce `de-DE` a `de`.
 *
 * Senza, un sistema tedesco non troverebbe mai `de.json`: `navigator.language`
 * porta quasi sempre la regione, e i file portano la lingua. Chi un giorno
 * vorrà distinguere `pt-BR` da `pt-PT` aggiunga `pt-BR.json` e cambi questa
 * funzione perché provi prima il codice intero — è l'unico punto da toccare.
 */
function radice(codice: string): string {
  const tagliato = codice.trim().toLowerCase().split(/[-_]/)[0];
  return tagliato ?? "";
}

/**
 * Quale lingua usare.
 *
 * Nell'ordine: la scelta salvata, la lingua del sistema, l'inglese. La scelta
 * salvata può nominare una lingua il cui file non c'è più — un `de.json`
 * cancellato, un profilo arrivato da un'installazione con più lingue — e in quel
 * caso vale come se non ci fosse, invece di lasciare l'interfaccia con le sole
 * chiavi a schermo.
 */
export function scegli(salvata: string | null, sistema: string): string {
  if (salvata !== null && esiste(salvata)) return salvata;
  const dal = radice(sistema);
  if (esiste(dal)) return dal;
  return esiste(RIPIEGO) ? RIPIEGO : (DISPONIBILI[0]?.codice ?? RIPIEGO);
}

// ── La lingua attiva, fuori da React ────────────────────────────────────────

let attiva: string = esiste(RIPIEGO)
  ? RIPIEGO
  : (DISPONIBILI[0]?.codice ?? RIPIEGO);
const ascoltatori = new Set<() => void>();
/**
 * Cambia quando cambia quel che si legge: la lingua attiva, o — per una lingua
 * incompleta — l'arrivo del ripiego. È lo stato che `useLingua` segue, e non il
 * solo codice: il codice non cambia quando arriva il ripiego, e il testo sì.
 */
let versione = 0;

function iscrivi(avvisa: () => void): () => void {
  ascoltatori.add(avvisa);
  return () => {
    ascoltatori.delete(avvisa);
  };
}

function leggi(): number {
  return versione;
}

function avvisa(): void {
  versione += 1;
  for (const chi of ascoltatori) chi();
}

/**
 * Il codice da passare a `Intl`.
 *
 * La legge **adesso**, senza iscriversi: `formato.ts` la chiede a ogni numero
 * che formatta, e formattare non è disegnare. Restituisce la lingua attiva, ma
 * ha un nome suo perché il posto in cui si chiede «come si scrive un numero»
 * non è lo stesso in cui si chiede «in che lingua parliamo»: se un giorno le
 * due cose divergeranno — un'interfaccia in inglese con i numeri all'italiana —
 * divergeranno qui e in nessun altro punto.
 */
export function locale(): string {
  return attiva;
}

/**
 * Il codice da passare a `Intl` per le date e le ore.
 *
 * È il primo punto in cui le due cose qui sopra divergono (D-42): la lingua
 * dell'interfaccia, ma con la regione del sistema quando parla la stessa
 * lingua — `en-GB` sotto un'interfaccia inglese, e la data del 10 novembre si
 * scrive `10/11/2026` invece di `11/10/2026`. La regola sta in `regione.ts`.
 * I numeri restano a `locale`.
 */
export function localeDate(): string {
  const delSistema =
    typeof navigator === "undefined"
      ? []
      : navigator.languages.length > 0
        ? navigator.languages
        : [navigator.language];
  return regioneDelleDate(attiva, delSistema);
}

/**
 * Cambia la lingua, e lo dice al documento. Il dizionario deve essere già in
 * mano: chi parte da un codice usa [`impostaLingua`], che prima lo carica.
 *
 * `document.documentElement.lang` non è una formalità: da lì passano la
 * sillabazione, la sintesi vocale e il correttore. `index.html` lo nasce
 * neutro apposta, perché un `lang="it"` fisso su una pagina inglese è una bugia
 * che solo chi usa uno screen reader sente.
 *
 * Una lingua senza file si ignora invece di essere applicata: applicarla
 * lascerebbe a schermo le chiavi, che è peggio della lingua sbagliata.
 */
export function applicaLingua(codice: string): void {
  if (!esiste(codice)) return;
  // Applicare un dizionario che non c'è lascerebbe a schermo le chiavi nude.
  if (caricatore.caricato(codice) === undefined) {
    if (import.meta.env.DEV) console.warn(`[lingue] «${codice}» non è ancora caricata`);
    return;
  }
  document.documentElement.lang = codice;
  ricordaLaLingua(codice);
  if (codice === attiva) return;
  attiva = codice;
  avvisa();
}

/**
 * Carica una lingua e poi la applica: è il modo di cambiarla.
 *
 * Se mentre si carica ne viene chiesta un'altra, vince l'ultima. Un guasto del
 * caricamento arriva a chi ha chiesto e la lingua resta com'era: mai le chiavi
 * nude. Una lingua senza file si ignora, come con [`applicaLingua`].
 */
export const impostaLingua: (codice: string) => Promise<void> = creaCambioLingua(
  caricatore,
  applicaLingua,
);

/**
 * Dove si ricorda l'ultima lingua applicata, per il primo disegno dell'avvio
 * dopo (L2-1, collaudo sugli emulatori).
 *
 * La lingua scelta sta nel database e arriva con `avvio()`, che è una chiamata:
 * prima, la finestra può disegnare soltanto con quel che il browser sa già. Sul
 * computer non si vede, perché la finestra resta nascosta finché la lingua non è
 * pronta (`pronto` in `aspettoFinestra.ts`); sul telefono la WebView si vede da
 * subito, e un telefono in inglese con Rymembra in italiano mostrava per tre o
 * quattro decimi di secondo la Home in inglese. Il ricordo è solo un indizio per
 * quel primo disegno: la lingua vera resta quella del database, che `avvio()`
 * applica subito dopo come prima. Non è un dato personale, e non viaggia né nel
 * backup né nel profilo (la scelta li ha già, in `settings`).
 */
const CHIAVE_RICORDATA = "rymembra.linguaRicordata";

function ricordaLaLingua(codice: string): void {
  try {
    globalThis.localStorage?.setItem(CHIAVE_RICORDATA, codice);
  } catch {
    // Un archivio pieno o spento: al prossimo avvio si parte dal sistema.
  }
}

/** L'ultima lingua applicata, se c'è: vedi `CHIAVE_RICORDATA`. */
export function linguaRicordata(): string | null {
  try {
    return globalThis.localStorage?.getItem(CHIAVE_RICORDATA) ?? null;
  } catch {
    return null;
  }
}

/**
 * La lingua del primo disegno: l'ultima applicata (`ricordata`), se il suo file
 * c'è; altrimenti quella del sistema, o l'inglese. Se nemmeno quella si legge,
 * l'interfaccia mostra le chiavi — e il guasto arriva a chi ha chiesto.
 */
export async function impostaLinguaIniziale(
  sistema: string,
  ricordata: string | null = null,
): Promise<void> {
  const scelta = scegli(ricordata, sistema);
  try {
    await impostaLingua(scelta);
  } catch (errore) {
    if (scelta === RIPIEGO) throw errore;
    await impostaLingua(RIPIEGO);
  }
}

/**
 * Segue la lingua attiva.
 *
 * La chiama chi disegna. Restituisce il codice, così un componente che deve
 * passarlo a `Intl` non deve richiederlo a parte.
 */
export function useLingua(): string {
  useSyncExternalStore(iscrivi, leggi, leggi);
  return attiva;
}

// ── I testi ────────────────────────────────────────────────────────────────

/** Quel che si può infilare dentro un testo. */
export type Valori = Record<string, string | number>;

/**
 * Cerca una chiave nella lingua attiva, poi nel ripiego, poi si arrende.
 *
 * Arrendersi vuol dire restituire `null`, non la chiave: chi chiama decide cosa
 * farne, e [`t`] mostra la chiave apposta — a schermo è brutta abbastanza da
 * essere segnalata, che è esattamente il servizio che deve rendere.
 */
function cerca(chiave: string, chiediIlRipiego = true): string | null {
  const trovato = caricatore.caricato(attiva)?.[chiave];
  if (trovato !== undefined) return trovato;
  const ripiego = ripiegoPerChiave(chiediIlRipiego);
  return ripiego?.[chiave] ?? null;
}

/**
 * Il dizionario di ripiego, se è in mano; altrimenti si comincia a caricarlo,
 * e quando arriva chi disegna si ridisegna (`avvisa`).
 *
 * Serve solo a una lingua incompleta: l'italiano e l'inglese hanno le stesse
 * chiavi (`check:lingue`), e per loro il ripiego non si carica mai. Una lingua
 * con dei buchi mostra la chiave per il tempo della lettura, poi l'inglese.
 * `chiedi` falso per [`tSe`]: le sue chiavi vengono dal nucleo, e una che non
 * c'è è prevista — non vale una lettura.
 */
function ripiegoPerChiave(chiedi: boolean): Dizionario | undefined {
  if (attiva === RIPIEGO) return undefined;
  const ripiego = caricatore.caricato(RIPIEGO);
  if (ripiego !== undefined || !chiedi || !esiste(RIPIEGO) || ripiegoInArrivo) return ripiego;
  ripiegoInArrivo = true;
  caricatore
    .carica(RIPIEGO)
    .then(avvisa)
    .catch(() => undefined)
    .finally(() => {
      ripiegoInArrivo = false;
    });
  return undefined;
}

/** Il ripiego è già stato chiesto: una lettura sola, qualunque numero di chiavi manchi. */
let ripiegoInArrivo = false;

/**
 * Come [`cerca`], ma la forma giusta per il numero `n`.
 *
 * Dizionario per dizionario, e non forma per forma: vedi «L'ordine in cui si
 * cerca» in `plurali.ts`. Cercare `.one` prima nella lingua attiva e poi
 * nell'inglese, e solo dopo la chiave nuda tedesca, metterebbe una parola
 * inglese in mezzo a una frase tedesca che c'era tutta.
 */
function cercaPer(chiave: string, n: number, chiediIlRipiego = true): string | null {
  const suo = caricatore.caricato(attiva);
  if (suo !== undefined) {
    const trovato = formaPlurale((k) => suo[k] ?? null, attiva, chiave, n);
    if (trovato !== null) return trovato;
  }
  if (attiva === RIPIEGO) return null;
  const ripiego = ripiegoPerChiave(chiediIlRipiego);
  if (ripiego === undefined) return null;
  return formaPlurale((k) => ripiego[k] ?? null, RIPIEGO, chiave, n);
}

/**
 * Il testo da riempire: la forma plurale quando fra i valori c'è un numero `n`.
 *
 * Un `n` scritto come stringa non sceglie la forma, e non può: «1.234» è
 * milleduecentotrentaquattro in italiano e uno virgola due in inglese. Chi
 * vuole il plurale passa il numero, e la formattazione la fa [`riempi`].
 */
function testoDi(
  chiave: string,
  valori: Valori | undefined,
  chiediIlRipiego: boolean,
): string | null {
  if (MOBILE) {
    // La variante del telefono quasi sempre non c'è: la sua assenza non è un
    // buco, e non vale la lettura del ripiego.
    const delTelefono = testoDiChiave(`${chiave}${SUL_TELEFONO}`, valori, false);
    if (delTelefono !== null) return delTelefono;
  }
  return testoDiChiave(chiave, valori, chiediIlRipiego);
}

/**
 * La variante di una frase per il telefono: `chiave.telefono`, se c'è.
 *
 * # Perché qui e non nei componenti
 *
 * Perché le frasi che sul telefono direbbero una cosa falsa — «questo PC»,
 * «Gestione credenziali di Windows», «un'unità di rete» — stanno sparse in
 * decine di schermate, e una scelta `MOBILE ? a : b` in ognuna sarebbe una
 * scelta da ricordarsi in ognuna. Così la variante si scrive solo nei file
 * delle lingue, accanto alla frase che corregge, e vale ovunque quella frase
 * si disegni. Senza variante si disegna la frase di sempre: la maggior parte
 * delle frasi è vera su tutti e due.
 *
 * Il plurale vale anche per lei: `brani.telefono.uno` come `brani.uno`.
 */
const SUL_TELEFONO = ".telefono";

/** [`testoDi`] per una chiave sola, senza la variante del telefono. */
function testoDiChiave(
  chiave: string,
  valori: Valori | undefined,
  chiediIlRipiego: boolean,
): string | null {
  const n = valori?.["n"];
  return typeof n === "number"
    ? cercaPer(chiave, n, chiediIlRipiego)
    : cerca(chiave, chiediIlRipiego);
}

/**
 * Sostituisce `{nome}` con i valori.
 *
 * I numeri passano da `toLocaleString` con la lingua attiva, e non è una
 * comodità: «1.234» vuol dire milleduecentotrentaquattro in italiano e uno
 * virgola due in inglese. Farlo qui significa che nessun punto di chiamata deve
 * ricordarsene — ed è esattamente il genere di cosa di cui, su
 * quarantacinque punti di chiamata, qualcuno non si ricorderebbe.
 */
function riempi(testo: string, valori: Valori | undefined): string {
  if (valori === undefined) return testo;
  return testo.replace(/\{(\w+)\}/g, (intero, nome: string) => {
    const valore = valori[nome];
    if (valore === undefined) return intero;
    return typeof valore === "number" ? valore.toLocaleString(attiva) : valore;
  });
}

/**
 * Il testo di una chiave.
 *
 * # Il plurale
 *
 * Quando fra i valori c'è un numero `n`, la forma la sceglie `plurali.ts`:
 * `chiave.uno` per esattamente uno, poi la categoria della lingua (`.one`,
 * `.few`, `.many`…), poi `.other`, poi la chiave nuda. L'italiano e l'inglese
 * hanno bisogno solo di `.uno`; una lingua con più forme aggiunge le sue, e le
 * chiavi di adesso non se ne accorgono.
 *
 * # Quando manca
 *
 * Si mostra la chiave, e in sviluppo si avvisa in console. Mostrare la stringa
 * vuota nasconderebbe il buco proprio dove si vede — un bottone senza scritta
 * sembra un difetto di grafica, non una traduzione mancante.
 */
export function t(chiave: Chiave, valori?: Valori): string {
  const testo = testoDi(chiave, valori, true);
  if (testo === null) {
    if (import.meta.env.DEV)
      console.warn(`[lingue] manca «${chiave}» in «${attiva}»`);
    return chiave;
  }
  return riempi(testo, valori);
}

/**
 * Come [`t`], ma per chiavi calcolate a runtime.
 *
 * Le chiavi degli errori arrivano dal nucleo (`ErroreIpc.i18nKey`) e i codici
 * dello Studio pure: sono stringhe che TypeScript non può controllare, e
 * chiedergli di fingere il contrario con un cast in ogni punto di chiamata
 * renderebbe indistinguibili i due casi. Qui il ripiego è esplicito e
 * obbligatorio, ed è quel che rende accettabile la chiave non verificata.
 */
export function tSe(chiave: string, ripiego: string, valori?: Valori): string {
  const testo = testoDi(chiave, valori, false);
  if (testo === null) {
    if (import.meta.env.DEV)
      console.warn(`[lingue] manca «${chiave}» in «${attiva}»`);
    return riempi(ripiego, valori);
  }
  return riempi(testo, valori);
}
