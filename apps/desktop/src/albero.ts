/**
 * L'albero delle cartelle, appiattito: la forma che si disegna e si percorre.
 *
 * # Perché appiattito, e non annidato
 *
 * Perché il pannello è **finestrato** (`virtuale.ts`): a schermo ci sono una
 * trentina di righe, e le altre non esistono nel DOM. Un albero annidato — un
 * contenitore per ogni nodo aperto, con dentro i figli — non si può finestrare
 * senza tagliare i contenitori a metà, e un contenitore tagliato a metà è un
 * `<div>` che si chiude dove capita.
 *
 * Appiattito invece è un elenco come tutti gli altri: `righe[i]` è la riga
 * `i`-esima **visibile**, il livello è un numero dentro la riga invece che una
 * profondità nel DOM, e ↓ è `i + 1` senza dover chiedere a nessuno se il nodo
 * dopo è un fratello, un figlio o lo zio. Tutta la tastiera del pannello — →
 * che scende, ← che risale, `*` che apre i fratelli, la digitazione che salta
 * al nome — sono passeggiate su questo vettore, e nessuna di loro ha bisogno di
 * sapere che c'è un albero sotto.
 *
 * # Perché la cache dei figli e l'insieme degli aperti stanno separati
 *
 * Perché sono due fatti diversi, e confonderli è il difetto classico di questi
 * pannelli: **chiudere un nodo non butta quel che si sa di lui**. Chi chiude e
 * riapre lo fa per guardare due volte la stessa cosa — è il gesto più comune
 * che ci sia qui dentro — e se la chiusura svuotasse la cache, la riapertura
 * costerebbe un giro nel nucleo ogni volta, con la riga che compare vuota per
 * un fotogramma. Con i due insiemi separati riaprire è **sincrono**: i figli
 * ci sono già, e l'unico giro che si paga è il primo.
 *
 * Il prezzo è che la cache non si svuota da sé. È un prezzo che si può pagare:
 * quel che tiene è una `NodoCartella` per cartella *aperta almeno una volta*,
 * cioè cinque campi corti — non i brani, che non passano mai di qui. Quando la
 * libreria cambia il pannello ricomincia da capo, ed è l'unica invalidazione
 * che serve, perché è l'unico momento in cui questi dati diventano falsi.
 *
 * # Perché le chiavi non piegano le maiuscole
 *
 * Perché il nucleo l'ha già fatto. Il trie di `cartelle.rs` fonde due grafie
 * dello stesso posto **prima** di rispondere, quindi due nodi che arrivano qui
 * con percorsi diversi sono due cartelle diverse per davvero. Rifare qui la
 * piegatura vorrebbe dire una seconda idea di cosa sia lo stesso percorso, da
 * tenere d'accordo con la prima per sempre — e su Linux la piegatura è pure
 * sbagliata. Quel che si normalizza è solo il separatore, e per un caso solo:
 * i percorsi che tornano da `settings` fra un avvio e l'altro, che possono
 * essere stati scritti da una versione che fondeva le barre diversamente.
 */
import type { NodoCartella } from "./ipc";

/**
 * La chiave dei nodi di primo livello nella cache.
 *
 * Una stringa vuota e non `null`: la mappa ha una chiave sola per tutti, e il
 * livello zero è l'unico posto dell'albero che non ha un percorso proprio —
 * `cartelle_figlie` lo chiede con `percorso: null`, che è la stessa cosa detta
 * al nucleo.
 */
export const CIME = "";

/** Il numero massimo di percorsi aperti che si ricordano fra due avvii. */
export const MASSIMO_APERTI = 200;

/** Una riga dell'albero appiattito: quel che serve per disegnarla e basta. */
export interface RigaAlbero {
  /** Il percorso, nella grafia con cui i brani stanno sul disco. */
  percorso: string;
  /** Quel che si scrive: l'ultimo segmento, o il percorso intero se è una cima. */
  nome: string;
  /** Uno per i nodi di primo livello: è già `aria-level`, senza conversioni. */
  livello: number;
  /** Quanti brani ci sono qui sotto, sottocartelle comprese. */
  brani: number;
  /** È aperto adesso. */
  aperto: boolean;
  /**
   * Ha sottocartelle.
   *
   * Viene dal conteggio del nucleo e non dalla cache: si sa **prima** di aver
   * chiesto i figli, ed è quel che permette di disegnare la freccia al primo
   * colpo invece di farla comparire dopo la prima apertura.
   */
  figli: boolean;
  /** La posizione fra i fratelli, da uno: è già `aria-posinset`. */
  posizione: number;
  /** Quanti fratelli in tutto, sé compreso: è già `aria-setsize`. */
  fratelli: number;
  /** È una delle cartelle sorvegliate, cioè una su cui ha senso la sonda. */
  radice: boolean;
}

/** Quel che il pannello sa dell'albero in questo momento. */
export interface StatoAlbero {
  /**
   * Le figlie dirette di ogni nodo di cui si è già chiesto, per [`chiave`].
   *
   * Le cime stanno sotto [`CIME`]. Un nodo assente vuol dire «non ancora
   * chiesto», che è diverso da un elenco vuoto: il primo fa partire una
   * richiesta, il secondo no.
   */
  figli: ReadonlyMap<string, readonly NodoCartella[]>;
  /** I nodi aperti, per [`chiave`], dal meno recente al più recente. */
  aperti: readonly string[];
}

/** Un albero che non sa ancora niente. */
export const VUOTO: StatoAlbero = { figli: new Map(), aperti: [] };

/**
 * La chiave con cui un percorso si ritrova nella cache e fra gli aperti.
 *
 * Fonde i separatori — `\` e `/` valgono uguale, e ripetuti valgono uno — e
 * toglie quello in coda. Non tocca le maiuscole: vedi il preambolo.
 *
 * Il doppio separatore di testa si conserva, perché è quel che distingue
 * `\\server\musica` da un percorso relativo che comincia per `server`. È la
 * stessa regola di `cartelle.rs::chiave`, ed è scritta due volte perché i due
 * lati non possono chiamarsi.
 */
export function chiave(percorso: string): string {
  const unc = /^[\\/]{2}/.test(percorso);
  const segmenti = percorso.split(/[\\/]+/).filter((parte) => parte.length > 0);
  return (unc ? "//" : "") + segmenti.join("/");
}

/**
 * Le righe visibili, dall'alto in basso.
 *
 * Una pila e non la ricorsione, per la stessa ragione per cui non la usa
 * `Albero::brani_sotto` nel nucleo: la profondità di un albero di cartelle la
 * decide chi ha nominato le cartelle, e non c'è motivo di farla decidere allo
 * stack di questo processo.
 *
 * Un nodo aperto di cui i figli non sono ancora arrivati non produce niente
 * sotto di sé, e va bene così: la riga è già disegnata con `aria-expanded` a
 * `true`, le figlie compaiono quando arrivano, e nel frattempo non c'è nessuno
 * stato intermedio da inventare.
 */
export function appiattisci(stato: StatoAlbero): RigaAlbero[] {
  const aperti = new Set(stato.aperti);
  const righe: RigaAlbero[] = [];
  const pila: { elenco: readonly NodoCartella[]; indice: number; livello: number }[] = [
    { elenco: stato.figli.get(CIME) ?? [], indice: 0, livello: 1 },
  ];

  while (pila.length > 0) {
    const cima = pila[pila.length - 1];
    if (cima === undefined) break;
    const nodo = cima.elenco[cima.indice];
    if (nodo === undefined) {
      pila.pop();
      continue;
    }
    cima.indice += 1;
    const k = chiave(nodo.percorso);
    const aperto = aperti.has(k);
    righe.push({
      percorso: nodo.percorso,
      nome: nodo.nome,
      livello: cima.livello,
      brani: nodo.brani,
      aperto,
      figli: nodo.sottocartelle > 0,
      posizione: cima.indice,
      fratelli: cima.elenco.length,
      radice: nodo.radice,
    });
    if (!aperto) continue;
    const figlie = stato.figli.get(k);
    if (figlie !== undefined && figlie.length > 0) {
      pila.push({ elenco: figlie, indice: 0, livello: cima.livello + 1 });
    }
  }
  return righe;
}

/**
 * Registra le figlie di un nodo. Se c'erano già, si sovrascrivono.
 *
 * Sovrascrivere e non fondere: una risposta nuova del nucleo è più fresca di
 * quella vecchia per costruzione, e tenere l'unione delle due vorrebbe dire
 * conservare cartelle che nel frattempo sono sparite.
 */
export function conFigli(
  stato: StatoAlbero,
  percorso: string | null,
  figlie: readonly NodoCartella[],
): StatoAlbero {
  const figli = new Map(stato.figli);
  figli.set(percorso === null ? CIME : chiave(percorso), figlie);
  return { figli, aperti: stato.aperti };
}

/**
 * Apre un nodo. Aprirne uno già aperto lo porta in cima ai recenti.
 *
 * L'ordine serve al tetto dei duecento: quel che si taglia scrivendo in
 * `settings` è la testa, cioè le aperture più vecchie, e riaprire un nodo è
 * l'atto che lo rende di nuovo recente.
 */
export function apri(stato: StatoAlbero, percorso: string): StatoAlbero {
  const k = chiave(percorso);
  const senza = stato.aperti.filter((altro) => altro !== k);
  const aperti = [...senza, k];
  return {
    figli: stato.figli,
    aperti: aperti.length > MASSIMO_APERTI ? aperti.slice(-MASSIMO_APERTI) : aperti,
  };
}

/** Chiude un nodo. **Non** tocca la cache dei figli: vedi il preambolo. */
export function chiudi(stato: StatoAlbero, percorso: string): StatoAlbero {
  const k = chiave(percorso);
  if (!stato.aperti.includes(k)) return stato;
  return { figli: stato.figli, aperti: stato.aperti.filter((altro) => altro !== k) };
}

/** Apre in un colpo solo un elenco di percorsi: serve a `*` e al ripristino. */
export function apriTutti(stato: StatoAlbero, percorsi: readonly string[]): StatoAlbero {
  return percorsi.reduce(apri, stato);
}

/** È aperto adesso. */
export function eAperto(stato: StatoAlbero, percorso: string): boolean {
  return stato.aperti.includes(chiave(percorso));
}

/** I figli già arrivati, o `undefined` se non sono ancora stati chiesti. */
export function figliDi(
  stato: StatoAlbero,
  percorso: string | null,
): readonly NodoCartella[] | undefined {
  return stato.figli.get(percorso === null ? CIME : chiave(percorso));
}

/**
 * L'indice del padre di `righe[da]`, o `-1` se è una cima.
 *
 * Si cerca all'indietro il primo nodo di livello minore: nell'albero appiattito
 * è il padre per costruzione, perché i figli seguono sempre il loro padre senza
 * niente in mezzo che stia più in alto.
 */
export function indicePadre(righe: readonly RigaAlbero[], da: number): number {
  const mio = righe[da]?.livello;
  if (mio === undefined || mio <= 1) return -1;
  for (let i = da - 1; i >= 0; i -= 1) {
    const livello = righe[i]?.livello;
    if (livello !== undefined && livello < mio) return i;
  }
  return -1;
}

/**
 * I percorsi dei fratelli di `righe[da]`, sé compreso.
 *
 * Serve al tasto `*`, che apre tutto un livello in una volta. I fratelli sono
 * contigui **solo** se nessuno di loro è aperto: appena uno lo è, i suoi figli
 * si infilano in mezzo. Quindi non si prende una fetta, si scorre — in avanti
 * fino a incontrare un livello più basso del proprio, e indietro allo stesso
 * modo.
 */
export function fratelliDi(righe: readonly RigaAlbero[], da: number): string[] {
  const mia = righe[da];
  if (mia === undefined) return [];
  const percorsi: string[] = [];
  for (let i = da; i >= 0; i -= 1) {
    const riga = righe[i];
    if (riga === undefined || riga.livello < mia.livello) break;
    if (riga.livello === mia.livello) percorsi.unshift(riga.percorso);
  }
  for (let i = da + 1; i < righe.length; i += 1) {
    const riga = righe[i];
    if (riga === undefined || riga.livello < mia.livello) break;
    if (riga.livello === mia.livello) percorsi.push(riga.percorso);
  }
  return percorsi;
}

/**
 * La riga il cui nome comincia per `prefisso`, cercando **dopo** `da` e poi
 * ricominciando da capo.
 *
 * L'anello è quel che rende utile premere due volte la stessa lettera: con tre
 * cartelle che cominciano per «R», la seconda «r» porta alla seconda e la
 * quarta torna alla prima. Senza, si resterebbe fermi sulla prima per sempre.
 *
 * Il confronto è insensibile alle maiuscole con `localeCompare`, e non con un
 * `toLowerCase` per parte: la lingua dell'interfaccia decide come si piega una
 * lettera, e in turco la I maiuscola non diventa la i che ci si aspetta.
 */
export function saltaA(
  righe: readonly RigaAlbero[],
  prefisso: string,
  da: number,
): number {
  if (prefisso.length === 0 || righe.length === 0) return -1;
  const combacia = (nome: string) =>
    nome
      .slice(0, prefisso.length)
      .localeCompare(prefisso, undefined, { sensitivity: "base" }) === 0;
  for (let salto = 1; salto <= righe.length; salto += 1) {
    const i = (da + salto) % righe.length;
    const riga = righe[i];
    if (riga !== undefined && combacia(riga.nome)) return i;
  }
  // Anche la riga di partenza, per ultima: con una sola cartella che comincia
  // per «R», digitare «ro» dopo «r» deve restare lì invece di non trovare
  // niente e far tornare il fuoco chissà dove.
  const qui = righe[da];
  return qui !== undefined && combacia(qui.nome) ? da : -1;
}
