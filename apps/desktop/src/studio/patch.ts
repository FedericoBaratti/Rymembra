/**
 * Modifiche strutturate sul testo del documento.
 *
 * # Perché si edita il testo e non un modello
 *
 * Perché `SkinDocument` **non è riserializzabile**, e non è una svista: sta
 * scritto sopra la sua definizione in `document.rs`. Un colore validato è una
 * quaterna di canali, e riscriverlo produrrebbe un manifest che quella stessa
 * validazione rifiuta — `rgb(9 9 13)` non è una forma che il formato accetta in
 * ingresso.
 *
 * Quindi la sorgente di verità dello Studio è il JSON scritto dall'autore, e le
 * due viste — clicca o programma — non sono due modelli da tenere sincronizzati:
 * sono due tastiere sullo stesso documento. Il file che esce è quello che si
 * mette in git.
 *
 * # Perché non un editor di JSON generico
 *
 * Perché ogni controllo dello Studio ha la forma di un tipo del crate: un
 * segmentato per un enum, un cursore per una lunghezza, un elenco riordinabile
 * per una pila di effetti. Non c'è nessun campo di testo libero dove il formato
 * vuole un valore chiuso — ed è la prima delle tre regole dello Studio. Queste
 * funzioni sono il ponte fra quei controlli e il testo.
 *
 * # Le posizioni, e perché sono arrivate dopo
 *
 * Per tutta la vita dello Studio queste funzioni hanno saputo attraversare solo
 * oggetti, e non era una dimenticanza: **nessun controllo indirizza una
 * posizione**. La pila degli effetti si riscrive intera — `Livelli` passa a
 * `onCambia` l'array completo — e lo scafale pure: `layout.shell` si sostituisce
 * da un albero già clonato, senza mai nominare una posizione per strada.
 *
 * La chat ha cambiato la domanda. Un modello che ha in mano il documento e deve
 * scaldare una fermata di un gradiente scrive
 * `["parts", "section-card", "background", "0", "stops", "0", "color"]`, ed è il
 * percorso giusto: `operazioni.rs` accetta i passi numerici apposta, perché «un
 * modello che scrive `["tokens", 0]` intende il passo 0». Quel che succedeva qui
 * è che l'array incontrato per strada diventava `{}` — la copia di livello era
 * uno `spread` di oggetto, e per un array cadeva nel ramo vuoto — e la pila
 * spariva dal documento senza che niente lo dicesse. È lo stesso guasto di
 * `__proto__` qui sotto con un'altra causa: una perdita di dati in fondo a una
 * scrittura che sembrava riuscita.
 *
 * Adesso un passo numerico dentro un elenco è una posizione; scrivere alla
 * posizione pari alla lunghezza aggiunge in fondo — che è come si mette un
 * livello in cima a una pila — e togliere una posizione fa scalare le altre.
 * Quel che non si può onorare non si onora a metà: la funzione dice **di no**,
 * e dice perché, perché quella frase è insieme quel che si mostra a chi guarda
 * e quel che torna al modello per correggersi.
 *
 * # L'ordine delle chiavi
 *
 * `JSON.stringify` conserva l'ordine di inserimento, quindi una chiave nuova
 * finisce in fondo al suo oggetto e le altre non si muovono. Un riordino
 * alfabetico produrrebbe un diff che tocca tutto il file a ogni modifica, e il
 * documento è testo apposta perché git ci lavori sopra.
 */
import type { OperazioneIa } from "../ipc";

/** Un contenitore del documento: un oggetto, oppure un elenco. */
type Contenitore = Record<string, unknown> | unknown[];

/**
 * Com'è andata una modifica, e perché no quando no.
 *
 * La ragione non è per il diario: è la stessa frase che il pannello mostra
 * sotto la proposta e che `correzione()` rimanda al modello in modalità agent.
 * Un `null` al posto suo costringerebbe a ricostruirla ripercorrendo il
 * percorso una seconda volta, cioè a scrivere due volte questa traversata.
 */
type Esito =
  | { readonly fatto: true; readonly documento: Contenitore }
  | { readonly fatto: false; readonly perche: string };

/** Il documento come oggetto, o `null` se il testo non è JSON. */
export function leggi(sorgente: string): Record<string, unknown> | null {
  try {
    const letto: unknown = JSON.parse(sorgente);
    if (letto === null || typeof letto !== "object" || Array.isArray(letto)) return null;
    return letto as Record<string, unknown>;
  } catch {
    return null;
  }
}

/**
 * Come si riscrive: due spazi, come `plain.json`.
 *
 * Il tipo è largo quanto quel che la traversata restituisce, non quanto la
 * radice di un manifest: la radice arriva sempre da `leggi`, che rifiuta tutto
 * quel che non è un oggetto, e un elenco qui non ci arriva mai.
 */
export function scrivi(documento: Contenitore): string {
  return `${JSON.stringify(documento, null, 2)}\n`;
}

/**
 * `__proto__`, e perché le chiavi di questo file si leggono e si scrivono a mano.
 *
 * È l'unico nome che un autore di skin può battere in un campo di testo — il
 * nome di un colore della tavolozza, di un motivo, una rinomina — per cui
 * `oggetto[nome] = valore` **non scrive una chiave**. `Object.prototype` espone
 * un accessore con quel nome, e l'assegnazione chiama quello: cambia il
 * prototipo dell'oggetto, e `JSON.stringify` non stampa niente. Il colore
 * sparisce dal documento senza che nulla lo dica, e l'editor mostra un file che
 * non contiene quel che si è appena scritto.
 *
 * Non è l'inquinamento del prototipo globale che il nome fa temere.
 * `Object.prototype` non si tocca mai: l'oggetto scritto qui nasce sempre da
 * `JSON.parse` o da uno `spread` di queste funzioni, e il prototipo che cambia è
 * quello di quella copia — che poi viene buttata. Il difetto è la perdita di
 * dati, e finisce nelle due funzioni qui sotto.
 *
 * `defineProperty` invece dell'assegnazione perché è quel che fa `JSON.parse`
 * quando incontra `"__proto__"` in un oggetto: una proprietà propria,
 * enumerabile, che `stringify` poi ristampa. Le due direzioni tornano a
 * coincidere.
 */
function poni(
  dentro: Record<string, unknown>,
  chiave: string,
  valore: unknown,
): void {
  Object.defineProperty(dentro, chiave, {
    value: valore,
    writable: true,
    enumerable: true,
    configurable: true,
  });
}

/**
 * Il valore di una chiave **dell'oggetto**, e `undefined` per tutto il resto.
 *
 * Il rovescio di `poni`: senza la verifica, un percorso che passa per
 * `__proto__` restituirebbe `Object.prototype` — e i controlli dello Studio si
 * troverebbero a disegnare le proprietà di quello invece che del documento.
 * Vale anche per `toString` e per gli altri nomi ereditati, che non sono mai
 * chiavi di un manifest.
 */
function preso(dentro: unknown, chiave: string): unknown {
  if (dentro === null || typeof dentro !== "object") return undefined;
  return Object.prototype.hasOwnProperty.call(dentro, chiave)
    ? (dentro as Record<string, unknown>)[chiave]
    : undefined;
}

/** Vero per i due contenitori. `null` non lo è, benché `typeof` dica di sì. */
function contenitore(cosa: unknown): cosa is Contenitore {
  return cosa !== null && typeof cosa === "object";
}

/**
 * La posizione che un passo indirizza, o `null` se quel passo non è una
 * posizione.
 *
 * Scritta per esteso e in forma canonica: `"0"`, `"7"`, `"12"`. Non `"01"`, non
 * `" 1"`, non `"1.0"`, non `"1e1"`, non `"-1"` — che `Number` accetterebbe
 * tutte. Il confronto `String(n) === passo` dice in una riga la cosa giusta: un
 * passo è una posizione **solo se** riscriverlo dà di nuovo se stesso. Senza,
 * `"1e1"` diventerebbe la posizione dieci, e un nome di chiave che somiglia a
 * un numero verrebbe scambiato per un indice.
 */
function posizione(passo: string): number | null {
  const n = Number(passo);
  return Number.isInteger(n) && n >= 0 && String(n) === passo ? n : null;
}

/** Copia superficiale, che resta un elenco se era un elenco. */
function copiaDi(cosa: Contenitore): Contenitore {
  return Array.isArray(cosa) ? [...cosa] : { ...cosa };
}

/**
 * Il valore di un passo dentro un contenitore, e `undefined` per tutto il resto.
 *
 * Dentro un elenco vale **solo** una posizione: `preso` da solo direbbe di sì
 * anche a `"length"`, che è una proprietà propria di ogni array e non è mai un
 * passo di un percorso. Quel che ne uscirebbe è un numero, cioè qualcosa su cui
 * i controlli si metterebbero a disegnare.
 */
function dentroA(dove: unknown, passo: string): unknown {
  if (!contenitore(dove)) return undefined;
  if (!Array.isArray(dove)) return preso(dove, passo);
  const dritto = posizione(passo);
  return dritto === null ? undefined : dove[dritto];
}

/** Il percorso, o un suo prefisso, come lo legge un modello. */
function via(percorso: readonly string[], fino?: number): string {
  return JSON.stringify(fino === undefined ? percorso : percorso.slice(0, fino));
}

/**
 * Scrive un passo dentro un contenitore. La ragione, quando non si può.
 *
 * Dentro un elenco si sostituisce una posizione che c'è, oppure si **aggiunge**
 * scrivendo alla posizione pari alla lunghezza — che è il modo in cui si mette
 * un livello in cima a una pila. Una posizione più in là aprirebbe dei buchi, e
 * un buco `JSON.stringify` lo stampa `null`: il documento ne uscirebbe con un
 * livello nullo dentro la pila, e il validatore direbbe una cosa vera su una
 * modifica che nessuno ha chiesto.
 */
function poniIn(
  dove: Contenitore,
  passo: string,
  valore: unknown,
  percorso: readonly string[],
  fino: number,
): string | null {
  if (!Array.isArray(dove)) {
    poni(dove, passo, valore);
    return null;
  }
  const dritto = posizione(passo);
  if (dritto === null) {
    return `${via(percorso, fino)} è un elenco, e «${passo}» non è una posizione`;
  }
  if (dritto > dove.length) {
    return `${via(percorso, fino)} ha ${dove.length} voci: alla posizione ${dritto} resterebbe un buco (per aggiungere in fondo, scrivi alla ${dove.length})`;
  }
  dove[dritto] = valore;
  return null;
}

/**
 * Toglie un passo da un contenitore. La ragione, quando non c'era niente.
 *
 * `splice` e non `delete`: un `delete` su una posizione lascia un buco, e i
 * buchi `JSON.stringify` li stampa `null`. Togliere il primo livello di una
 * pila deve far scalare gli altri, non lasciare un `null` al suo posto.
 */
function togliIn(
  dove: Contenitore,
  passo: string,
  percorso: readonly string[],
  fino: number,
): string | null {
  if (!Array.isArray(dove)) {
    if (!Object.prototype.hasOwnProperty.call(dove, passo)) {
      return `${via(percorso, fino)} non ha «${passo}»`;
    }
    delete dove[passo];
    return null;
  }
  const dritto = posizione(passo);
  if (dritto === null) {
    return `${via(percorso, fino)} è un elenco, e «${passo}» non è una posizione`;
  }
  if (dritto >= dove.length) {
    return `${via(percorso, fino)} ha ${dove.length} voci: la posizione ${dritto} non c'è`;
  }
  dove.splice(dritto, 1);
  return null;
}

/**
 * Scrive un valore in un percorso, dentro una copia del documento.
 *
 * Copia superficiale a ogni livello lungo il percorso: modificare in posto
 * andrebbe bene qui — l'oggetto arriva da `JSON.parse` e non lo condivide
 * nessuno — ma renderebbe questa funzione un'eccezione fra funzioni che non
 * mutano, e le eccezioni si scoprono al primo riuso.
 *
 * Quel che **manca** lungo la strada si crea, ed è il caso di chi scrive il
 * primo aspetto di una parte che non c'era. Quel che c'è si copia com'è, e un
 * elenco resta un elenco. Quel che c'è e non è un contenitore — un colore, un
 * numero — ferma tutto: sostituirlo con un oggetto vuoto cancellerebbe un
 * valore del documento per arrivare a scriverne un altro, che è la stessa
 * perdita di dati silenziosa che questa riscrittura esiste per togliere.
 */
function scriviNel(
  documento: Contenitore,
  percorso: readonly string[],
  valore: unknown,
): Esito {
  const ultimo = percorso[percorso.length - 1];
  if (ultimo === undefined) return { fatto: false, perche: "il percorso è vuoto" };

  const radice = copiaDi(documento);
  let dove: Contenitore = radice;
  for (const [quanti, passo] of percorso.slice(0, -1).entries()) {
    const dentro = dentroA(dove, passo);
    if (dentro !== undefined && !contenitore(dentro)) {
      return {
        fatto: false,
        perche: `${via(percorso, quanti + 1)} è un valore, non un contenitore: non ci si può scrivere dentro`,
      };
    }
    const passa: Contenitore = dentro === undefined ? {} : copiaDi(dentro);
    const perche = poniIn(dove, passo, passa, percorso, quanti);
    if (perche !== null) return { fatto: false, perche };
    dove = passa;
  }

  const perche = poniIn(dove, ultimo, valore, percorso, percorso.length - 1);
  return perche === null
    ? { fatto: true, documento: radice }
    : { fatto: false, perche };
}

/**
 * Toglie un valore, e con lui i contenitori che restano vuoti.
 *
 * La ripulitura conta: togliere l'ultimo aspetto di una parte deve togliere
 * anche la parte, altrimenti il documento si riempie di `"section-card": {}` —
 * dichiarazioni che dicono «ridisegno questa superficie» e non ridisegnano
 * niente, e che nell'albero del registro accendono il pallino dell'accento su
 * una parte intatta.
 *
 * Vale anche per gli elenchi: `"background": []` è la stessa dichiarazione a
 * vuoto di `{}`, e la lascia chi toglie l'ultimo livello di una pila.
 *
 * `fino` invece della coda del percorso perché le ragioni nominano il prefisso,
 * e un percorso che a ogni giro perde la testa non saprebbe più dire da dove
 * era partito.
 */
function togliNel(
  documento: Contenitore,
  percorso: readonly string[],
  fino = 0,
): Esito {
  const testa = percorso[fino];
  if (testa === undefined) return { fatto: false, perche: "il percorso è vuoto" };

  const copia = copiaDi(documento);
  if (fino < percorso.length - 1) {
    const sotto = dentroA(copia, testa);
    if (!contenitore(sotto)) {
      return {
        fatto: false,
        perche: `${via(percorso, fino + 1)} non c'è, o non è un contenitore`,
      };
    }
    const dentro = togliNel(sotto, percorso, fino + 1);
    if (!dentro.fatto) return dentro;
    if (!vuoto(dentro.documento)) {
      const perche = poniIn(copia, testa, dentro.documento, percorso, fino);
      return perche === null
        ? { fatto: true, documento: copia }
        : { fatto: false, perche };
    }
    // Il contenitore è rimasto vuoto: cade nel ramo qui sotto, che lo toglie.
  }

  const perche = togliIn(copia, testa, percorso, fino);
  return perche === null
    ? { fatto: true, documento: copia }
    : { fatto: false, perche };
}

/**
 * Una dichiarazione che non dichiara niente: `{}` oppure `[]`.
 *
 * Serve in due posti, ed è lo stesso concetto in entrambi: qui, per non lasciare
 * un contenitore vuoto quando se ne toglie l'ultima voce; e in `Studio`, per non
 * scriverne uno in partenza. Un `"background": []` dice «ridisegno questa
 * superficie» e non ridisegna niente — nell'albero del registro accende il
 * pallino dell'accento su una parte intatta.
 */
export function vuoto(cosa: unknown): boolean {
  if (Array.isArray(cosa)) return cosa.length === 0;
  if (cosa === null || typeof cosa !== "object") return false;
  return Object.keys(cosa).length === 0;
}

/** Il valore in un percorso, o `undefined`. */
export function valoreIn(
  documento: Record<string, unknown>,
  percorso: readonly string[],
): unknown {
  let dove: unknown = documento;
  for (const passo of percorso) {
    dove = dentroA(dove, passo);
  }
  return dove;
}

/**
 * Scrive un valore in un percorso, creando quel che manca.
 *
 * Restituisce il testo nuovo, e lascia quello vecchio intatto se non è JSON o
 * se il percorso non si può onorare: un documento a metà di una parentesi non
 * si modifica a colpi di controlli — l'editor lo dice e non tocca niente,
 * invece di riscrivere sopra il lavoro di chi stava scrivendo.
 *
 * La ragione del rifiuto si perde qui, e va bene: un controllo che chiede una
 * cosa impossibile è un difetto dello Studio, non una frase da mostrare. Chi la
 * ragione la vuole — la chat — passa da [`applicaOperazioni`].
 */
export function scriviIn(
  sorgente: string,
  percorso: readonly string[],
  valore: unknown,
): string {
  const documento = leggi(sorgente);
  if (documento === null) return sorgente;
  const esito = scriviNel(documento, percorso, valore);
  return esito.fatto ? scrivi(esito.documento) : sorgente;
}

/** Toglie un valore, e con lui i contenitori che restano vuoti. */
export function togliDa(sorgente: string, percorso: readonly string[]): string {
  const documento = leggi(sorgente);
  if (documento === null) return sorgente;
  const esito = togliNel(documento, percorso);
  return esito.fatto ? scrivi(esito.documento) : sorgente;
}

/**
 * Le operazioni di un modello, applicate al documento in un colpo solo.
 *
 * # Perché qui e non nella chat
 *
 * Perché è la stessa traversata dei controlli, e la promessa scritta in `ia.rs`
 * è che una proposta non abbia poteri che un cursore non abbia: quel che
 * `scriviNel` rifiuta a un controllo lo rifiuta anche a un modello, e quel che
 * passa finisce nello stesso `scriviSorgente`, cioè nello stesso annulla.
 *
 * # Perché una lettura sola e una scrittura sola
 *
 * Perché prima erano una per operazione. Dieci modifiche su un manifest da
 * trenta kilobyte volevano dire venti attraversamenti del documento intero per
 * cambiare venti caratteri, e ogni giro ripassava dal testo — cioè da un
 * `JSON.parse` di quel che si era appena stampato.
 *
 * # Perché quel che non si applica si nomina
 *
 * Per la stessa ragione di `operazioni::estrai`: nove modifiche buone e una
 * storta valgono nove. Le ragioni tornano indietro per due usi — mostrarle a chi
 * guarda, e rimandarle al modello in modalità agent, dove sono esattamente
 * l'informazione che gli serve per correggersi. Sono in italiano come quelle del
 * nucleo, e per lo stesso motivo: finiscono nello stesso elenco, e un elenco in
 * due lingue si legge peggio di uno in una sola.
 */
export function applicaOperazioni(
  sorgente: string,
  operazioni: readonly OperazioneIa[],
): { testo: string; scartate: string[] } {
  const documento = leggi(sorgente);
  if (documento === null) {
    // Prima lo si scopriva applicando, e non lo si scopriva: `scriviIn`
    // restituiva la sorgente intatta, il confronto usciva vuoto e il pannello
    // mostrava «N modifiche» con un «Applica» che non faceva niente. In
    // modalità agent erano quattro giri a rimandare lo stesso documento
    // immutato.
    return {
      testo: sorgente,
      scartate: [
        "il documento non è JSON valido: non si è applicato niente. Chiudi la parentesi che manca e riprova.",
      ],
    };
  }

  const scartate: string[] = [];
  let dove: Contenitore = documento;
  let qualcosa = false;
  for (const op of operazioni) {
    // Il tipo scritto, e non dedotto: `dove` si riassegna da `esito.documento`,
    // quindi dedurre `esito` vorrebbe dire dedurre prima `dove` — che dipende da
    // `esito`. TypeScript chiude il giro con un `any` implicito e un TS7022.
    const esito: Esito = op.togli
      ? togliNel(dove, op.percorso)
      : scriviNel(dove, op.percorso, op.valore);
    if (!esito.fatto) {
      scartate.push(
        `«${op.togli ? "togli" : "scrivi"} ${via(op.percorso)}»: ${esito.perche}`,
      );
      continue;
    }
    dove = esito.documento;
    qualcosa = true;
  }

  // La sorgente intatta, e non una ristampa identica: `scrivi` normalizza il
  // rientro, e un testo riscritto senza che nessuna modifica sia passata
  // aprirebbe un passo di annullo che non annulla niente.
  return { testo: qualcosa ? scrivi(dove) : sorgente, scartate };
}

/**
 * Il percorso di una proprietà d'aspetto, base o di uno stato.
 *
 * Un posto solo per la regola: base sta in `parts.<nome>.<campo>`, uno stato in
 * `parts.<nome>.states.<stato>.<campo>`. Comporla nei punti in cui serve
 * vorrebbe dire scriverla cinque volte e sbagliarla una.
 */
export function percorsoParte(
  parte: string,
  stato: string | null,
  campo: string,
): string[] {
  return stato === null
    ? ["parts", parte, campo]
    : ["parts", parte, "states", stato, campo];
}

/**
 * Il percorso di un errore, sciolto in passi veri.
 *
 * Il validatore scrive i percorsi col punto — `parts.section-card.radius` — e i
 * nomi delle chiavi contengono a loro volta dei punti: `tokens.color.surface.0`
 * è **due** passi, non quattro. Spezzare sul punto e basta dà il risultato
 * giusto per le parti e sbagliato per i token, che è il modo peggiore di
 * sbagliare: funziona finché non si prova col caso che conta.
 *
 * Si scioglie guardando il documento, non indovinando: a ogni livello si prende
 * il prefisso **più lungo** che è davvero una chiave lì dentro. `null` se il
 * percorso non porta da nessuna parte.
 *
 * Gli elenchi non si attraversano, e qui è giusto così: l'unico chiamante è la
 * rinomina di una chiave sbagliata, e una posizione non ha un nome da correggere.
 */
function passiDi(
  documento: Record<string, unknown>,
  percorso: string,
): string[] | null {
  const pezzi = percorso.split(".");
  const passi: string[] = [];
  let dove: unknown = documento;
  let da = 0;

  while (da < pezzi.length) {
    if (dove === null || typeof dove !== "object" || Array.isArray(dove))
      return null;
    const dentro = dove as Record<string, unknown>;
    let trovato: string | null = null;
    // Dal più lungo: `color.surface.0` prima di `color`, altrimenti un token si
    // fermerebbe al primo pezzo e il resto diventerebbe un cammino inesistente.
    for (let fino = pezzi.length; fino > da; fino -= 1) {
      const candidato = pezzi.slice(da, fino).join(".");
      if (Object.prototype.hasOwnProperty.call(dentro, candidato)) {
        trovato = candidato;
        da = fino;
        break;
      }
    }
    if (trovato === null) return null;
    passi.push(trovato);
    dove = dentro[trovato];
  }

  return passi;
}

/**
 * Rinomina una chiave, al suo posto e senza spostarla.
 *
 * È la correzione di «forse volevi dire…», e prima era una
 * `String.replace('"sbagliato"', '"giusto"')` sul testo intero: sostituiva la
 * **prima** occorrenza ovunque fosse, quindi bastava che quella parola comparisse
 * prima da qualche altra parte — dentro una descrizione, in `meta`, in un altro
 * blocco — perché il bottone correggesse la cosa sbagliata e lasciasse
 * l'errore dov'era.
 *
 * Qui si passa dal percorso dell'errore, che il validatore dà già preciso.
 *
 * La chiave resta **dov'era**: ricostruire l'oggetto con uno `spread` la
 * sposterebbe in fondo, e un diff che muove un blocco per una lettera cambiata
 * è un diff che nessuno rilegge.
 */
export function rinominaChiave(
  sorgente: string,
  percorso: string,
  nuovo: string,
): string {
  const documento = leggi(sorgente);
  if (documento === null) return sorgente;
  const passi = passiDi(documento, percorso);
  if (passi === null || passi.length === 0) return sorgente;

  const vecchio = passi[passi.length - 1];
  if (vecchio === undefined || vecchio === nuovo) return sorgente;
  const versoIlPadre = passi.slice(0, -1);

  const padre = valoreIn(documento, versoIlPadre);
  if (padre === null || typeof padre !== "object" || Array.isArray(padre))
    return sorgente;
  const dentro = padre as Record<string, unknown>;
  // Rinominare su una chiave che esiste già fonderebbe due dichiarazioni in
  // silenzio: meglio non fare niente, e lasciare l'errore a dirlo.
  if (Object.prototype.hasOwnProperty.call(dentro, nuovo)) return sorgente;

  const rinominato = Object.fromEntries(
    Object.entries(dentro).map(([chiave, valore]) => [
      chiave === vecchio ? nuovo : chiave,
      valore,
    ]),
  );

  return versoIlPadre.length === 0
    ? scrivi(rinominato)
    : scriviIn(sorgente, versoIlPadre, rinominato);
}
