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
 * # Perché non una libreria
 *
 * Il progetto ha quattro dipendenze runtime in tutto, e la CI ne vieta
 * esplicitamente altre. Quel che serve qui è ricerca per chiave, interpolazione
 * testuale e un plurale a due forme: duecento righe leggibili, contro
 * quaranta kilobyte di libreria generica e un formato di catalogo che nessuno
 * di noi ha scelto.
 *
 * # Perché un modulo e non un contesto
 *
 * Come per la posizione di riproduzione (`riproduzione.ts`): di lingua ce n'è
 * una per finestra, e i testi servono anche **fuori** da React — dentro
 * `formato.ts`, dentro `ipc.ts`, dentro tabelle costruite in cima a un modulo.
 * Un contesto li renderebbe raggiungibili solo da un componente, e obbligherebbe
 * a far scendere `t` per prop attraverso settanta file: `Impostazioni` ne
 * riceve già sessantuno.
 *
 * Chi disegna si iscrive con [`useLingua`]; chi calcola e basta chiama [`t`].
 */
import { useSyncExternalStore } from "react";

import it from "./it.json";

/** Un dizionario, come sta sul disco: chiave piatta → testo. */
export type Dizionario = Record<string, string>;

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

/**
 * La chiave riservata al nome della lingua nella sua lingua.
 *
 * Non è un testo dell'interfaccia: è l'etichetta che il selettore mostra, e
 * viaggia col file perché è l'unico modo di non avere una seconda tabella da
 * aggiornare.
 */
const CHIAVE_NOME = "_nome";

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
 * I file, come Vite li trova.
 *
 * `eager` perché sono qualche decina di kilobyte in tutto e servono al primo
 * fotogramma: caricarli a richiesta vorrebbe dire un `await` prima di poter
 * disegnare una sola etichetta.
 */
const FILE = import.meta.glob("./*.json", {
  eager: true,
  import: "default",
}) as Record<string, Dizionario>;

/** I dizionari, per codice. */
const DIZIONARI: Record<string, Dizionario> = {};
for (const [percorso, dizionario] of Object.entries(FILE)) {
  const codice = percorso.replace(/^\.\//, "").replace(/\.json$/, "");
  DIZIONARI[codice] = dizionario;
}

/**
 * Le lingue disponibili, in ordine di codice.
 *
 * In ordine e non nell'ordine in cui `glob` le restituisce: quello dipende dal
 * filesystem, e un selettore che cambia ordine fra una macchina e l'altra è una
 * differenza che nessuno sa spiegare.
 */
export const DISPONIBILI: Lingua[] = Object.keys(DIZIONARI)
  .sort()
  .map((codice) => ({
    codice,
    nome: DIZIONARI[codice]?.[CHIAVE_NOME] ?? codice,
  }));

/** C'è un file per questa lingua? */
export function esiste(codice: string): boolean {
  return codice in DIZIONARI;
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

function iscrivi(avvisa: () => void): () => void {
  ascoltatori.add(avvisa);
  return () => {
    ascoltatori.delete(avvisa);
  };
}

function leggi(): string {
  return attiva;
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
 * Cambia la lingua, e lo dice al documento.
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
  if (!esiste(codice) || codice === attiva) {
    if (esiste(codice)) document.documentElement.lang = codice;
    return;
  }
  attiva = codice;
  document.documentElement.lang = codice;
  for (const avvisa of ascoltatori) avvisa();
}

/**
 * Segue la lingua attiva.
 *
 * La chiama chi disegna. Restituisce il codice, così un componente che deve
 * passarlo a `Intl` non deve richiederlo a parte.
 */
export function useLingua(): string {
  return useSyncExternalStore(iscrivi, leggi, leggi);
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
function cerca(chiave: string): string | null {
  return DIZIONARI[attiva]?.[chiave] ?? DIZIONARI[RIPIEGO]?.[chiave] ?? null;
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
 * Quando fra i valori c'è `n`, si prova prima `chiave.uno` con `n === 1`. Due
 * forme bastano per italiano e inglese, e una lingua che ne ha di più può
 * aggiungere le sue senza che le due di adesso cambino: il posto da toccare è
 * questa funzione, e le altre lingue non se ne accorgono.
 *
 * # Quando manca
 *
 * Si mostra la chiave, e in sviluppo si avvisa in console. Mostrare la stringa
 * vuota nasconderebbe il buco proprio dove si vede — un bottone senza scritta
 * sembra un difetto di grafica, non una traduzione mancante.
 */
export function t(chiave: Chiave, valori?: Valori): string {
  if (valori !== undefined && valori["n"] === 1) {
    const singolare = cerca(`${chiave}.uno`);
    if (singolare !== null) return riempi(singolare, valori);
  }
  const testo = cerca(chiave);
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
  const testo = cerca(chiave);
  if (testo === null) {
    if (import.meta.env.DEV)
      console.warn(`[lingue] manca «${chiave}» in «${attiva}»`);
    return riempi(ripiego, valori);
  }
  return riempi(testo, valori);
}
