/**
 * Le regole del giro guidato che non hanno bisogno di uno schermo.
 *
 * `Giro.tsx` è materia di rettangoli — il velo, il buco, il fumetto — e quel
 * che fa si vede soltanto guardandolo. Qui ci sono le quattro decisioni che si
 * possono sbagliare **senza** che si veda subito, scritte senza DOM e senza
 * React perché `node --test` le provi da sole (`test/guida.test.ts`):
 *
 * - **quando parte** ([`comeParteIlGiro`]), che ha sette condizioni e una che
 *   non è un sì o un no ma un «non ancora»;
 * - **quanto si aspetta un'ancora** ([`ATTESA_ANCORA_MS`]);
 * - **dove si posa il fumetto** ([`posaFumetto`]);
 * - **se la coda che il giro ha riempito va svuotata** alla chiusura
 *   ([`codaDaSvuotare`]).
 *
 * Un modulo a parte e non dentro `Giro.tsx` per una ragione pratica: un file
 * `.tsx` porta con sé React e il JSX, che `node --test` non sa leggere, e le
 * regole sono proprio la parte che vale la pena provare senza una finestra.
 */

/**
 * Quanto si aspetta l'ancora di un passo prima di saltarlo: un secondo e mezzo.
 *
 * In tempo e non in fotogrammi. Erano trenta fotogrammi, cioè mezzo secondo a
 * sessanta hertz — ma un secondo e mezzo a venti, e su una finestra in secondo
 * piano, che il motore fa disegnare una volta al secondo, mezzo minuto di velo
 * fermo. E il mezzo secondo non bastava al passo che ne ha più bisogno: il menù
 * di un brano aspetta che la vista dei Brani arrivi, che la prima pagina di
 * righe torni dal nucleo, e che il menù si misuri, e su una libreria fredda
 * sono più di trenta fotogrammi. Oltre il secondo e mezzo il passo si salta
 * come prima, e un velo fermo così a lungo comincerebbe a sembrare rotto.
 */
export const ATTESA_ANCORA_MS = 1_500;

/** Quanto sta il fumetto lontano dal bordo del buco, e dal bordo della finestra. */
export const DISTACCO = 12;

/** Un rettangolo in coordinate di finestra, come lo dà `getBoundingClientRect`. */
export interface Rettangolo {
  top: number;
  left: number;
  width: number;
  height: number;
}

/** Quanto è grande la finestra. Strutturale apposta: `Misura` di `dati.tsx` va bene. */
export interface Schermo {
  larghezza: number;
  altezza: number;
}

/**
 * Dove si posa il fumetto rispetto al buco.
 *
 * **Sotto** se ci sta, **sopra** altrimenti, centrato sul buco e poi rientrato
 * dentro la finestra: è la regola che rende la posizione prevedibile — chi ha
 * letto il passo prima sa già dove guardare per il prossimo. Solo quando non ci
 * sta né sotto né sopra, cioè quando il bersaglio è alto quasi quanto la
 * finestra, si prova **accanto**: a destra, poi a sinistra, centrato in
 * altezza sul buco.
 *
 * Il «accanto» c'è da quando ci sono passi così alti. Prima il ripiego era uno
 * solo — in alto, sovrapposto — e su una barra laterale alta quanto la
 * finestra voleva dire un fumetto appoggiato proprio sulle voci che stava
 * spiegando; l'albero delle Cartelle e la colonna di destra sono la stessa
 * figura. Accanto c'è quasi sempre posto, e quel posto è sotto il velo: non
 * copre niente che il passo stia mostrando. La sovrapposizione resta l'ultima
 * risorsa, per il bersaglio grande quanto tutta la finestra — lo schermo
 * intero — dove non c'è un fuori in cui stare.
 */
export function posaFumetto(
  buco: Rettangolo,
  misura: { width: number; height: number },
  schermo: Schermo,
): { top: number; left: number } {
  const stretta = (valore: number, minimo: number, massimo: number) =>
    Math.max(minimo, Math.min(valore, Math.max(minimo, massimo)));
  const rientra = (posto: { top: number; left: number }) => ({
    top: stretta(posto.top, DISTACCO, schermo.altezza - misura.height - DISTACCO),
    left: stretta(posto.left, DISTACCO, schermo.larghezza - misura.width - DISTACCO),
  });

  const alCentroX = buco.left + buco.width / 2 - misura.width / 2;
  const sotto = buco.top + buco.height + DISTACCO;
  if (sotto + misura.height + DISTACCO <= schermo.altezza)
    return rientra({ top: sotto, left: alCentroX });
  const sopra = buco.top - DISTACCO - misura.height;
  if (sopra >= DISTACCO) return rientra({ top: sopra, left: alCentroX });

  const alCentroY = buco.top + buco.height / 2 - misura.height / 2;
  const destra = buco.left + buco.width + DISTACCO;
  if (destra + misura.width + DISTACCO <= schermo.larghezza)
    return rientra({ top: alCentroY, left: destra });
  const sinistra = buco.left - DISTACCO - misura.width;
  if (sinistra >= DISTACCO) return rientra({ top: alCentroY, left: sinistra });

  // Non ci sta da nessuna parte: si appoggia in alto e si accetta la
  // sovrapposizione, che è meno peggio di un fumetto mezzo fuori dallo schermo.
  return rientra({ top: DISTACCO, left: alCentroX });
}

/** Quel che serve sapere per decidere se il giro parte. */
export interface CondizioniDelGiro {
  /** Il copione di adesso non è ancora stato visto: vedi `giro.rs`. */
  daFare: boolean;
  /** Il giro è già aperto. */
  aperto: boolean;
  /** `avvio` è arrivato: senza, non si sa nemmeno quanti brani ci sono. */
  avviato: boolean;
  /** La schermata del primo avvio è a schermo. */
  primoVisibile: boolean;
  /** Quanti brani ha la libreria. */
  brani: number;
  /** Si sta guardando Esplora. */
  inEsplora: boolean;
  /** Lo Studio copre la finestra. */
  studioVisibile: boolean;
  /** Un dialogo modale o un menù è aperto adesso. */
  modaleAperto: boolean;
}

/**
 * Cosa fa il giro adesso: parte, aspetta che un modale si chiuda, o niente.
 *
 * - `parti`: tutte le condizioni ci sono.
 * - `aspetta`: ci sono tutte tranne una, e quella è un dialogo o un menù
 *   aperto. Non è un «no»: chi lo ha aperto lo chiuderà, e il giro parte
 *   allora. Una cosa da guardare nel documento, perché i modali non stanno
 *   tutti in `App` — l'equalizzatore, la sincronia, le conferme delle Cartelle
 *   tengono il proprio stato — e l'attributo `aria-modal` è l'unica cosa che
 *   hanno in comune.
 * - `no`: manca qualcosa che è uno stato di `App`, e quando cambierà la
 *   domanda si rifà da sé.
 *
 * Perché non sopra un modale: il giro prende il fuoco e il velo prende i clic,
 * quindi una domanda aperta resterebbe sotto, senza risposta e senza modo di
 * dargliela — e un fumetto che spiega la barra laterale sopra «Eliminare 40
 * brani dal disco?» è il peggior momento possibile per un benvenuto. Lo
 * Studio per la stessa ragione, e per una in più: sta sopra il velo del giro,
 * che finirebbe tutto sotto, invisibile e col fuoco in mano.
 *
 * Le altre condizioni sono quelle di sempre, e il perché di ciascuna sta
 * accanto all'effetto che la usa in `App.tsx`.
 */
export function comeParteIlGiro(
  c: CondizioniDelGiro,
): "parti" | "aspetta" | "no" {
  if (!c.daFare || c.aperto || !c.avviato) return "no";
  if (c.primoVisibile || c.brani === 0 || c.inEsplora || c.studioVisibile)
    return "no";
  return c.modaleAperto ? "aspetta" : "parti";
}

/** Della riproduzione, quel che dice se la coda del giro è stata toccata. */
export interface CodaAdesso {
  coda: readonly number[];
  posizioneCoda: number | null;
  posizioneMs: number;
}

/**
 * Se la coda che il giro ha messo in una coda vuota va tolta, chiudendolo.
 *
 * Il giro riempie la coda **solo se era vuota** (`coda_prepara`), perché
 * quattro passi parlano del lettore e il lettore si vede soltanto con un
 * brano. Chiudendo si rimette com'era — vuota — ma soltanto se nessuno l'ha
 * usata nel frattempo: un brano fatto partire durante il giro, uno saltato, un
 * brano in più messo in coda sono ascolto vero, e toglierli sarebbe togliere
 * qualcosa che qualcuno ha scelto.
 *
 * «Non toccata» vuol dire: gli stessi brani (in qualunque ordine — con lo
 * shuffle acceso il nucleo li mescola), ancora al primo, e la puntina ancora a
 * zero. È una lettura dalla finestra, e può sbagliare di un istante — un
 * «riproduci» premuto un attimo prima di chiudere, il cui primo battito non è
 * ancora arrivato. Il nucleo però ha l'ultima parola: «Svuota» su una coda che
 * non è più quella del giro manda l'avviso con «Annulla»
 * (`ricorda_coda_di_prima`), quindi l'errore possibile ha già il suo rimedio.
 */
export function codaDaSvuotare(
  messa: readonly number[] | null,
  adesso: CodaAdesso,
): boolean {
  if (messa === null || messa.length === 0) return false;
  if (adesso.posizioneMs !== 0) return false;
  if (adesso.posizioneCoda !== null && adesso.posizioneCoda !== 0) return false;
  if (adesso.coda.length !== messa.length) return false;
  const ordinati = (elenco: readonly number[]) => [...elenco].sort((x, y) => x - y);
  const a = ordinati(messa);
  const b = ordinati(adesso.coda);
  return a.every((id, i) => id === b[i]);
}
