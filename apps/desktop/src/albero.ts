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
 * # Le foglie sono righe come le altre
 *
 * Aprendo una cartella compaiono, dopo le sottocartelle, i suoi file: le foglie
 * del pannello «Folders» di foobar2000. Stanno nello **stesso** vettore, con lo
 * stesso livello delle sorelle cartelle e lo stesso conto di `aria-posinset` /
 * `aria-setsize`, perché per chi percorre l'albero — con le frecce o con un
 * lettore di schermo — un file è un nodo come gli altri, solo senza figli. La
 * differenza la porta `brano`: un numero per i file, `null` per le cartelle.
 *
 * L'ordine delle foglie lo decide questo modulo e non il nucleo, ed è l'ordine
 * del **nome del file** letto come lo legge chi guarda: «2» prima di «10»
 * ([`ordinaFile`]). È una scelta di disegno — dentro la cartella si riproduce
 * comunque nell'ordine dell'album, che è del nucleo — e sta dove si disegna.
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
 * cioè cinque campi corti, più le foglie delle cartelle aperte — un album sono
 * dieci righe. Quando la libreria cambia il pannello ricomincia da capo, ed è
 * l'unica invalidazione che serve, perché è l'unico momento in cui questi dati
 * diventano falsi.
 *
 * # Il filtro è una lente, non un secondo albero
 *
 * Il filtro arriva dal nucleo già potato: per ogni cartella da tenere aperta,
 * i soli figli che portano a una corrispondenza ([`Lente`]). Non sostituisce la
 * cache: ci si **appoggia sopra** ([`attraversoLente`]). I rami potati coprono
 * quel che la cache sa di quelle cartelle, gli aperti sono quelli della lente,
 * e tutto il resto passa: una cartella trovata e aperta a mano mostra il suo
 * contenuto vero, dalla cache, come fuori dal filtro. Cancellato il filtro, la
 * lente se ne va e l'albero di prima è lì com'era — aperti compresi.
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
import type { CartelleFiltrate, FileCartella, NodoCartella } from "./ipc";

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
  /**
   * Il percorso, nella grafia con cui i brani stanno sul disco. Per una foglia
   * è quello del file: cartelle e file non si incontrano mai sullo stesso
   * percorso, quindi la chiave basta a tutti e due.
   */
  percorso: string;
  /**
   * Quel che si scrive: l'ultimo segmento, o il percorso intero se è una cima.
   * Per una foglia è il titolo del brano, o il nome del file se il titolo
   * manca.
   */
  nome: string;
  /** Uno per i nodi di primo livello: è già `aria-level`, senza conversioni. */
  livello: number;
  /** Quanti brani ci sono qui sotto, sottocartelle comprese. Uno per un file. */
  brani: number;
  /** È aperto adesso. */
  aperto: boolean;
  /**
   * Si può aprire: ha sottocartelle o file.
   *
   * Viene dai conteggi del nucleo e non dalla cache: si sa **prima** di aver
   * chiesto i figli, ed è quel che permette di disegnare la freccia al primo
   * colpo invece di farla comparire dopo la prima apertura.
   */
  figli: boolean;
  /** La posizione fra i fratelli, da uno: è già `aria-posinset`. */
  posizione: number;
  /** Quanti fratelli in tutto, sé compreso, file compresi: `aria-setsize`. */
  fratelli: number;
  /** È una delle cartelle sorvegliate, cioè una su cui ha senso la sonda. */
  radice: boolean;
  /**
   * Quanti file stanno direttamente in questa cartella: dice se aprendola
   * vanno chieste anche le foglie. Zero per un file, e per i nodi finti dello
   * Studio che il conteggio non lo portano.
   */
  file: number;
  /** Il brano, se la riga è un file; `null` se è una cartella. */
  brano: number | null;
  /** La durata in millisecondi, se la riga è un file. */
  durataMs: number | null;
  /**
   * Sta sotto una cartella sorvegliata, o lo è.
   *
   * Lo eredita dalla sua cima. Serve a «Mostra nella cartella»: il nucleo
   * apre soltanto cartelle che stanno sotto una radice vera, e una voce di
   * menù che porta a un rifiuto non va mostrata.
   */
  sorvegliata: boolean;
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
  /** Le foglie di ogni nodo di cui si è già chiesto, già in ordine di nome. */
  file: ReadonlyMap<string, readonly FileCartella[]>;
  /** I nodi aperti, per [`chiave`], dal meno recente al più recente. */
  aperti: readonly string[];
}

/** Un albero che non sa ancora niente. */
export const VUOTO: StatoAlbero = { figli: new Map(), file: new Map(), aperti: [] };

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

/** La chiave di una riga: la stessa per cartelle e file, vedi `percorso`. */
export function chiaveRiga(riga: RigaAlbero): string {
  return chiave(riga.percorso);
}

/**
 * Il confronto dei nomi di file: le cifre come numeri, le maiuscole no.
 *
 * `Intl.Collator` con `numeric` e non un confronto scritto a mano: è lo stesso
 * ordine di Esplora risorse — «2» prima di «10», «brano» accanto a «Brano» — e
 * la lingua dell'interfaccia decide dove stanno le lettere accentate, che è
 * una cosa che un confronto per codice sbaglierebbe. Uno solo, costruito una
 * volta: crearne uno a confronto costerebbe più dell'ordinamento intero.
 */
const COLLAZIONE = new Intl.Collator(undefined, { numeric: true, sensitivity: "base" });

/**
 * Le foglie in ordine di nome di file, letto come lo legge chi guarda.
 *
 * A parità di nome — «Brano.mp3» e «brano.mp3» per la collazione sono uguali —
 * decide il percorso, così l'ordine non dipende da come sono arrivate.
 */
export function ordinaFile(file: readonly FileCartella[]): FileCartella[] {
  return [...file].sort(
    (a, b) =>
      COLLAZIONE.compare(a.nome, b.nome) ||
      (a.percorso < b.percorso ? -1 : a.percorso > b.percorso ? 1 : 0),
  );
}

/** Una voce di un elenco di fratelli: una cartella o un file. */
type Voce = { cartella: NodoCartella; file?: undefined } | { cartella?: undefined; file: FileCartella };

/** I figli di un nodo come li mostra l'albero: prima le cartelle, poi i file. */
function vociDi(stato: StatoAlbero, k: string): readonly Voce[] {
  const cartelle = stato.figli.get(k) ?? [];
  const file = stato.file.get(k) ?? [];
  if (file.length === 0) return cartelle.map((cartella) => ({ cartella }));
  return [
    ...cartelle.map((cartella) => ({ cartella })),
    ...file.map((f) => ({ file: f })),
  ];
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
  const pila: {
    elenco: readonly Voce[];
    indice: number;
    livello: number;
    /** Ereditato dalla cima; `null` al primo livello, dove lo dice il nodo. */
    sorvegliata: boolean | null;
  }[] = [{ elenco: vociDi(stato, CIME), indice: 0, livello: 1, sorvegliata: null }];

  while (pila.length > 0) {
    const cima = pila[pila.length - 1];
    if (cima === undefined) break;
    const voce = cima.elenco[cima.indice];
    if (voce === undefined) {
      pila.pop();
      continue;
    }
    cima.indice += 1;

    if (voce.file !== undefined) {
      const f = voce.file;
      righe.push({
        percorso: f.percorso,
        // Un titolo vuoto è un file senza tag: il nome del file è l'unica cosa
        // che lo distingue dai suoi vicini.
        nome: f.titolo.trim().length > 0 ? f.titolo : f.nome,
        livello: cima.livello,
        brani: 1,
        aperto: false,
        figli: false,
        posizione: cima.indice,
        fratelli: cima.elenco.length,
        radice: false,
        file: 0,
        brano: f.brano,
        durataMs: f.durataMs,
        sorvegliata: cima.sorvegliata ?? false,
      });
      continue;
    }

    const nodo = voce.cartella;
    const k = chiave(nodo.percorso);
    const aperto = aperti.has(k);
    const sorvegliata = cima.sorvegliata ?? nodo.radice;
    righe.push({
      percorso: nodo.percorso,
      nome: nodo.nome,
      livello: cima.livello,
      brani: nodo.brani,
      aperto,
      // `file` manca solo nei dati finti dello Studio, che lo precedono: lì la
      // cartella si apre quando ha sottocartelle, come prima delle foglie.
      figli: nodo.sottocartelle > 0 || (nodo.file ?? 0) > 0,
      posizione: cima.indice,
      fratelli: cima.elenco.length,
      radice: nodo.radice,
      file: nodo.file ?? 0,
      brano: null,
      durataMs: null,
      sorvegliata,
    });
    if (!aperto) continue;
    const sotto = vociDi(stato, k);
    if (sotto.length > 0) {
      pila.push({ elenco: sotto, indice: 0, livello: cima.livello + 1, sorvegliata });
    }
  }
  return righe;
}

/**
 * Registra le figlie di un nodo, e le sue foglie se ci sono. Se c'erano già,
 * si sovrascrivono.
 *
 * Sovrascrivere e non fondere: una risposta nuova del nucleo è più fresca di
 * quella vecchia per costruzione, e tenere l'unione delle due vorrebbe dire
 * conservare cartelle che nel frattempo sono sparite.
 *
 * `file` assente lascia le foglie come sono: le cime non ne hanno, e le
 * chiede solo chi apre una cartella che dice di averne.
 */
export function conFigli(
  stato: StatoAlbero,
  percorso: string | null,
  figlie: readonly NodoCartella[],
  file?: readonly FileCartella[],
): StatoAlbero {
  const k = percorso === null ? CIME : chiave(percorso);
  const figli = new Map(stato.figli);
  figli.set(k, figlie);
  if (file === undefined) return { figli, file: stato.file, aperti: stato.aperti };
  const foglie = new Map(stato.file);
  foglie.set(k, ordinaFile(file));
  return { figli, file: foglie, aperti: stato.aperti };
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
    file: stato.file,
    aperti: aperti.length > MASSIMO_APERTI ? aperti.slice(-MASSIMO_APERTI) : aperti,
  };
}

/** Chiude un nodo. **Non** tocca la cache dei figli: vedi il preambolo. */
export function chiudi(stato: StatoAlbero, percorso: string): StatoAlbero {
  const k = chiave(percorso);
  if (!stato.aperti.includes(k)) return stato;
  return {
    figli: stato.figli,
    file: stato.file,
    aperti: stato.aperti.filter((altro) => altro !== k),
  };
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

// ── il filtro ───────────────────────────────────────────────────────────────

/** Quel che resta dell'albero sotto un filtro: vedi il preambolo. */
export interface Lente {
  /** Le figlie potate delle cartelle che il filtro apre, per [`chiave`]. */
  figli: ReadonlyMap<string, readonly NodoCartella[]>;
  /** Le foglie che corrispondono, per cartella, già in ordine di nome. */
  file: ReadonlyMap<string, readonly FileCartella[]>;
  /**
   * Gli aperti sotto il filtro: all'inizio la strada verso ogni
   * corrispondenza, poi quel che si apre e si chiude a mano. Gli aperti di
   * fuori non si toccano.
   */
  aperti: readonly string[];
  /** Le cartelle il cui nome corrisponde, per [`chiave`]. */
  trovate: ReadonlySet<string>;
  /** Quante corrispondenze c'erano in tutto. */
  corrispondenze: number;
  /** Erano più di quelle arrivate. */
  troncato: boolean;
}

/** La lente di un filtro, da quel che risponde il nucleo. */
export function lente(filtrate: CartelleFiltrate): Lente {
  const figli = new Map<string, readonly NodoCartella[]>();
  const file = new Map<string, readonly FileCartella[]>();
  const aperti: string[] = [];
  for (const ramo of filtrate.rami) {
    const k = ramo.percorso === null ? CIME : chiave(ramo.percorso);
    figli.set(k, ramo.cartelle);
    // Anche vuoto: un ramo senza foglie che corrispondono deve **coprire** le
    // foglie che la cache sa di quella cartella, non lasciarle passare.
    file.set(k, ordinaFile(ramo.file));
    if (ramo.percorso !== null) aperti.push(k);
  }
  return {
    figli,
    file,
    aperti,
    trovate: new Set(filtrate.trovate.map(chiave)),
    corrispondenze: filtrate.corrispondenze,
    troncato: filtrate.troncato,
  };
}

/**
 * L'albero visto attraverso la lente: i rami potati sopra la cache, e gli
 * aperti della lente al posto di quelli di fuori.
 */
export function attraversoLente(stato: StatoAlbero, filtro: Lente): StatoAlbero {
  const figli = new Map(stato.figli);
  for (const [k, figlie] of filtro.figli) figli.set(k, figlie);
  const file = new Map(stato.file);
  for (const [k, foglie] of filtro.file) file.set(k, foglie);
  return { figli, file, aperti: filtro.aperti };
}

// ── la selezione ────────────────────────────────────────────────────────────

/**
 * Le righe scelte, e da dove parte un intervallo.
 *
 * Per [`chiave`] e non per indice: le righe si spostano — un nodo che si apre
 * sopra ne fa scendere cento — e una selezione per indice sceglierebbe righe
 * diverse da quelle cliccate. L'àncora invece è un indice, ed è giusto così:
 * Maiusc prende «da lì a qui» in quel che si vede adesso, come nell'elenco dei
 * brani.
 */
export interface Selezione {
  chiavi: ReadonlySet<string>;
  ancora: number | null;
}

/** Niente di scelto. */
export const NESSUNA: Selezione = { chiavi: new Set(), ancora: null };

/**
 * Il gesto che cambia la selezione, coi tasti dell'elenco dei brani.
 *
 * - `azzera`: il clic senza modificatori — toglie la selezione e mette l'àncora
 *   qui, come nell'elenco: un clic semplice serve ad aprire e a spostarsi;
 * - `alterna`: Ctrl+clic, o Spazio — aggiunge o toglie questa riga;
 * - `intervallo`: Maiusc+clic, o Maiusc+↑↓ — tutte le righe fra l'àncora e qui.
 */
export type GestoSelezione = "azzera" | "alterna" | "intervallo";

/** La selezione dopo un gesto su `righe[indice]`. */
export function seleziona(
  prima: Selezione,
  righe: readonly RigaAlbero[],
  indice: number,
  gesto: GestoSelezione,
): Selezione {
  const riga = righe[indice];
  if (riga === undefined) return prima;
  if (gesto === "azzera") return { chiavi: new Set(), ancora: indice };
  if (gesto === "intervallo" && prima.ancora !== null) {
    const da = Math.max(0, Math.min(prima.ancora, indice));
    const a = Math.min(righe.length - 1, Math.max(prima.ancora, indice));
    return {
      chiavi: new Set(righe.slice(da, a + 1).map(chiaveRiga)),
      ancora: prima.ancora,
    };
  }
  // Alterna, o un intervallo senza àncora: senza un «da dove» l'intervallo è la
  // riga sola, ed è il punto da cui partirà il prossimo.
  const k = chiaveRiga(riga);
  const chiavi = new Set(prima.chiavi);
  if (!chiavi.delete(k)) chiavi.add(k);
  return { chiavi, ancora: indice };
}

/**
 * Su cosa agisce un comando: le righe scelte e visibili, nell'ordine
 * dell'albero, **senza** quelle che stanno dentro una cartella scelta.
 *
 * Una cartella scelta porta già tutto quel che ha sotto; contare anche il file
 * scelto dentro di lei lo metterebbe in coda due volte. Le scelte che un nodo
 * chiuso nasconde non contano: si agisce su quel che si vede.
 */
export function bersagli(
  righe: readonly RigaAlbero[],
  chiavi: ReadonlySet<string>,
): RigaAlbero[] {
  const scelte: RigaAlbero[] = [];
  /** Il livello della cartella scelta che copre le righe successive. */
  let coperte: number | null = null;
  for (const riga of righe) {
    if (coperte !== null && riga.livello > coperte) continue;
    coperte = null;
    if (!chiavi.has(chiaveRiga(riga))) continue;
    scelte.push(riga);
    if (riga.brano === null) coperte = riga.livello;
  }
  return scelte;
}

/**
 * La coda che parte da una foglia: tutti i file della sua cartella, nell'ordine
 * in cui si vedono, e la posizione di quello scelto.
 *
 * È il doppio clic di foobar2000 su un file: suona la cartella **da lì**. I
 * file di una cartella sono contigui — vengono dopo tutte le sottocartelle, e
 * una foglia non ha figli — quindi basta allargarsi dalla riga finché si
 * incontrano file dello stesso livello. `null` se la riga non è un file.
 */
export function codaDaFile(
  righe: readonly RigaAlbero[],
  indice: number,
): { brani: number[]; partenza: number } | null {
  const scelta = righe[indice];
  if (scelta === undefined || scelta.brano === null) return null;
  const sorella = (riga: RigaAlbero | undefined): riga is RigaAlbero & { brano: number } =>
    riga !== undefined && riga.brano !== null && riga.livello === scelta.livello;
  let da = indice;
  while (sorella(righe[da - 1])) da -= 1;
  let a = indice;
  while (sorella(righe[a + 1])) a += 1;
  const brani: number[] = [];
  for (let i = da; i <= a; i += 1) {
    const riga = righe[i];
    if (sorella(riga)) brani.push(riga.brano);
  }
  return { brani, partenza: indice - da };
}

// ── le passeggiate della tastiera ──────────────────────────────────────────

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
 * I percorsi delle cartelle sorelle di `righe[da]`, sé compresa se è una
 * cartella.
 *
 * Serve al tasto `*`, che apre tutto un livello in una volta. I fratelli sono
 * contigui **solo** se nessuno di loro è aperto: appena uno lo è, i suoi figli
 * si infilano in mezzo. Quindi non si prende una fetta, si scorre — in avanti
 * fino a incontrare un livello più basso del proprio, e indietro allo stesso
 * modo. I file si saltano: non si aprono, e messi fra gli aperti finirebbero
 * nel ricordo del pannello a occupare posti che sono delle cartelle.
 */
export function fratelliDi(righe: readonly RigaAlbero[], da: number): string[] {
  const mia = righe[da];
  if (mia === undefined) return [];
  const percorsi: string[] = [];
  const cartella = (riga: RigaAlbero) =>
    riga.livello === mia.livello && riga.brano === null;
  for (let i = da; i >= 0; i -= 1) {
    const riga = righe[i];
    if (riga === undefined || riga.livello < mia.livello) break;
    if (cartella(riga)) percorsi.unshift(riga.percorso);
  }
  for (let i = da + 1; i < righe.length; i += 1) {
    const riga = righe[i];
    if (riga === undefined || riga.livello < mia.livello) break;
    if (cartella(riga)) percorsi.push(riga.percorso);
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
