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

/** Il valore in un percorso, o `undefined`. */
export function valoreIn(
  documento: Record<string, unknown>,
  percorso: readonly string[],
): unknown {
  let dove: unknown = documento;
  for (const passo of percorso) {
    if (dove === null || typeof dove !== "object") return undefined;
    dove = (dove as Record<string, unknown>)[passo];
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
    const dentro = dove[passo];
    const copia: Record<string, unknown> =
      dentro !== null && typeof dentro === "object" && !Array.isArray(dentro)
        ? { ...(dentro as Record<string, unknown>) }
        : {};
    dove[passo] = copia;
    dove = copia;
  }
  const ultimo = percorso[percorso.length - 1];
  if (ultimo === undefined) return sorgente;
  dove[ultimo] = valore;
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
    const sotto = copia[testa];
    if (sotto === null || typeof sotto !== "object" || Array.isArray(sotto)) return copia;
    const ripulito = pulisci(sotto as Record<string, unknown>, coda);
    if (vuoto(ripulito)) {
      delete copia[testa];
    } else {
      copia[testa] = ripulito;
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
