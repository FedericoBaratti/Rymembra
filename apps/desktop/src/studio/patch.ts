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
 * # L'ordine delle chiavi
 *
 * `JSON.stringify` conserva l'ordine di inserimento, quindi una chiave nuova
 * finisce in fondo al suo oggetto e le altre non si muovono. Un riordino
 * alfabetico produrrebbe un diff che tocca tutto il file a ogni modifica, e il
 * documento è testo apposta perché git ci lavori sopra.
 */

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

/** Come si riscrive: due spazi, come `plain.json`. */
export function scrivi(documento: Record<string, unknown>): string {
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

/** Il valore in un percorso, o `undefined`. */
export function valoreIn(
  documento: Record<string, unknown>,
  percorso: readonly string[],
): unknown {
  let dove: unknown = documento;
  for (const passo of percorso) {
    dove = preso(dove, passo);
  }
  return dove;
}

/**
 * Scrive un valore in un percorso, creando gli oggetti che mancano.
 *
 * Restituisce il testo nuovo, e lascia quello vecchio intatto se non è JSON:
 * un documento a metà di una parentesi non si modifica a colpi di controlli —
 * l'editor lo dice e non tocca niente, invece di riscrivere sopra il lavoro di
 * chi stava scrivendo.
 */
export function scriviIn(
  sorgente: string,
  percorso: readonly string[],
  valore: unknown,
): string {
  const documento = leggi(sorgente);
  if (documento === null || percorso.length === 0) return sorgente;

  // Copia superficiale a ogni livello lungo il percorso: modificare in posto
  // andrebbe bene qui — l'oggetto arriva da `JSON.parse` e non lo condivide
  // nessuno — ma renderebbe questa funzione un'eccezione fra funzioni che non
  // mutano, e le eccezioni si scoprono al primo riuso.
  const radice: Record<string, unknown> = { ...documento };
  let dove = radice;
  for (const passo of percorso.slice(0, -1)) {
    const dentro = preso(dove, passo);
    const copia: Record<string, unknown> =
      dentro !== null && typeof dentro === "object" && !Array.isArray(dentro)
        ? { ...(dentro as Record<string, unknown>) }
        : {};
    poni(dove, passo, copia);
    dove = copia;
  }
  const ultimo = percorso[percorso.length - 1];
  if (ultimo === undefined) return sorgente;
  poni(dove, ultimo, valore);
  return scrivi(radice);
}

/**
 * Toglie un valore, e con lui gli oggetti che restano vuoti.
 *
 * La ripulitura conta: togliere l'ultimo aspetto di una parte deve togliere
 * anche la parte, altrimenti il documento si riempie di `"section-card": {}` —
 * dichiarazioni che dicono «ridisegno questa superficie» e non ridisegnano
 * niente, e che nell'albero del registro accendono il pallino dell'accento su
 * una parte intatta.
 *
 * Vale anche per gli array: `"background": []` è la stessa dichiarazione a vuoto
 * di `{}`, e la lascia chi toglie l'ultimo livello di una pila.
 */
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

export function togliDa(sorgente: string, percorso: readonly string[]): string {
  const documento = leggi(sorgente);
  if (documento === null || percorso.length === 0) return sorgente;

  const pulisci = (
    dentro: Record<string, unknown>,
    resto: readonly string[],
  ): Record<string, unknown> => {
    const [testa, ...coda] = resto;
    if (testa === undefined) return dentro;
    const copia = { ...dentro };
    if (coda.length === 0) {
      delete copia[testa];
      return copia;
    }
    const sotto = preso(copia, testa);
    if (sotto === null || typeof sotto !== "object" || Array.isArray(sotto)) return copia;
    const ripulito = pulisci(sotto as Record<string, unknown>, coda);
    if (vuoto(ripulito)) {
      delete copia[testa];
    } else {
      poni(copia, testa, ripulito);
    }
    return copia;
  };

  const ripulito = pulisci(documento, percorso);
  return scrivi(ripulito);
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
 * Il percorso JSON di un nodo dello scafale.
 *
 * `via` è il percorso di indici dei figli — lo stesso che il compilatore
 * trasforma in `data-nodo='0-1-2'` — quindi questa funzione è **formattazione**,
 * non ricerca: fra i due modi di scrivere lo stesso cammino non c'è una tabella
 * di corrispondenza da tenere allineata.
 *
 * Serve a **leggere**: portare il cursore su un errore, sapere dove si è. Le
 * modifiche riscrivono `layout.shell` intero da un albero clonato, e non hanno
 * bisogno che `patch.ts` impari a indicizzare gli array.
 */
export function percorsoNodo(via: readonly number[], campo?: string): string[] {
  const passi = ["layout", "shell"];
  for (const indice of via) {
    passi.push("children", String(indice));
  }
  if (campo !== undefined) passi.push(campo);
  return passi;
}

/** L'indirizzo `data-nodo` di un percorso di indici. */
export function indirizzoNodo(via: readonly number[]): string {
  return via.length === 0 ? "radice" : via.join("-");
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
 */
export function passiDi(
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
