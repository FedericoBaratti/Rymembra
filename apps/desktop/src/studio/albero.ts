/**
 * L'albero dello scafale, dal lato dell'editor.
 *
 * # Perché si lavora sull'albero ricevuto e non sul JSON
 *
 * Quel che il nucleo manda è già **completo**: le misure sono popolate anche
 * dove il documento taceva, le manopole hanno i difetti applicati, i nomi sono
 * passati dal registro. Lavorare su quello e riscriverlo intero è più corto e
 * più sicuro di comporre ogni modifica dal documento e dalla tabella dei
 * difetti insieme — la seconda strada ha un caso «non dichiarato» per ogni
 * campo, e ognuno è un posto in cui sbagliare.
 *
 * La conseguenza va detta nell'interfaccia **prima** della prima modifica: la
 * prima volta che si tocca il layout di una skin che non lo dichiarava, nel
 * documento finisce l'albero di serie per esteso. Ed è corretto — nel momento
 * in cui tocchi il layout, il documento deve dire qual è.
 *
 * # Perché non serve indicizzare gli array in `patch.ts`
 *
 * Perché ogni gesto — spostare un widget, cambiare un gradino di aria — riscrive
 * `layout.shell` in un colpo solo. `JSON.stringify` è deterministico e le chiavi
 * escono nell'ordine in cui questo file le scrive, quindi uno scatto di cursore
 * produce comunque un diff di una riga: la proprietà di diff-piccolo che
 * `patch.ts` esiste per proteggere regge senza che `patch.ts` impari niente.
 */
import type { NodoScafale, WidgetRegistro } from "../ipc";

/** Il percorso di indici che porta a un nodo. La radice è il percorso vuoto. */
export type Via = readonly number[];

/** Il nodo a un percorso, o `null` se il percorso non esiste. */
export function nodoA(radice: NodoScafale, via: Via): NodoScafale | null {
  let dove: NodoScafale | undefined = radice;
  for (const indice of via) {
    dove = dove?.children[indice];
  }
  return dove ?? null;
}

/** Lo stesso albero con un nodo sostituito. */
export function conNodo(
  radice: NodoScafale,
  via: Via,
  nuovo: NodoScafale,
): NodoScafale {
  if (via.length === 0) return nuovo;
  const [primo, ...resto] = via;
  if (primo === undefined) return radice;
  const figlio = radice.children[primo];
  if (figlio === undefined) return radice;
  const children = [...radice.children];
  children[primo] = conNodo(figlio, resto, nuovo);
  return { ...radice, children };
}

/** Lo stesso albero senza il nodo a un percorso. */
export function senzaNodo(radice: NodoScafale, via: Via): NodoScafale {
  if (via.length === 0) return radice;
  const padre = via.slice(0, -1);
  const indice = via[via.length - 1];
  if (indice === undefined) return radice;
  const dentro = nodoA(radice, padre);
  if (dentro === null) return radice;
  return conNodo(radice, padre, {
    ...dentro,
    children: dentro.children.filter((_, i) => i !== indice),
  });
}

/** Lo stesso albero con un nodo infilato a un indice dentro una zona. */
function conInserito(
  radice: NodoScafale,
  dove: Via,
  indice: number,
  nuovo: NodoScafale,
): NodoScafale {
  const dentro = nodoA(radice, dove);
  if (dentro === null) return radice;
  const children = [...dentro.children];
  children.splice(indice, 0, nuovo);
  return conNodo(radice, dove, { ...dentro, children });
}

/**
 * Sposta un nodo dentro una zona, a un indice.
 *
 * Si toglie prima e si infila dopo, correggendo l'indice quando la rimozione
 * cade **prima** del punto d'arrivo nella stessa zona: senza quella correzione
 * trascinare un elemento di una posizione a destra non lo muove.
 */
export function spostato(
  radice: NodoScafale,
  da: Via,
  dove: Via,
  indice: number,
): NodoScafale {
  const nodo = nodoA(radice, da);
  if (nodo === null || dentroSeStesso(da, dove)) return radice;

  const padre = da.slice(0, -1);
  const daIndice = da[da.length - 1] ?? 0;
  const stessaZona =
    padre.length === dove.length && padre.every((v, i) => v === dove[i]);
  const corretto = stessaZona && daIndice < indice ? indice - 1 : indice;

  return conInserito(senzaNodo(radice, da), dove, corretto, nodo);
}

/** Il bersaglio sta dentro quel che si sta spostando: sarebbe un ciclo. */
function dentroSeStesso(da: Via, dove: Via): boolean {
  return da.length <= dove.length && da.every((v, i) => v === dove[i]);
}

/** Un widget nuovo, coi difetti del registro. */
export function widgetNuovo(def: WidgetRegistro): NodoScafale {
  const options: Record<string, boolean | string | number> = {};
  for (const o of def.options) options[o.name] = o.default;
  return {
    kind: "widget",
    at: "",
    name: def.name,
    // `hug` e non la misura naturale: il registro la conosce, l'editor no, e
    // inventarne una qui vorrebbe dire tenerne due allineate. Il nucleo la
    // rimette al giro seguente, perché è lui a possedere i difetti.
    size: "hug",
    gap: null,
    align: null,
    spread: null,
    part: def.part,
    fromPrefab: null,
    slot: null,
    options,
    children: [],
  };
}

/** Una zona nuova, vuota. */
export function zonaNuova(kind: string): NodoScafale {
  return {
    kind: "zone",
    at: "",
    name: kind,
    size: "fill",
    gap: "none",
    align: "stretch",
    spread: "start",
    part: null,
    fromPrefab: null,
    slot: null,
    options: {},
    children: [],
  };
}

/** Quanti widget monta questo sottoalbero. */
export function widgetMontati(nodo: NodoScafale): string[] {
  if (nodo.kind === "widget") return [nodo.name];
  return nodo.children.flatMap(widgetMontati);
}

/** Il costo di un sottoalbero, dal registro. */
export function costoAlbero(
  nodo: NodoScafale,
  widgets: readonly WidgetRegistro[],
): number {
  return widgetMontati(nodo).reduce(
    (somma, nome) => somma + (widgets.find((w) => w.name === nome)?.cost ?? 0),
    0,
  );
}

/**
 * L'albero nella forma che il documento accetta.
 *
 * Non è una riserializzazione dell'albero validato — quella non esiste e sta
 * scritto perché — è la scrittura di un albero che l'editor ha in mano e che il
 * parser rileggerà. Le chiavi escono in un ordine fisso, così due modifiche
 * consecutive producono due diff piccoli invece di uno che tocca tutto.
 */
export function versoDocumento(nodo: NodoScafale): Record<string, unknown> {
  if (nodo.kind === "widget") {
    const fuori: Record<string, unknown> = { widget: nodo.name, size: nodo.size };
    if (Object.keys(nodo.options).length > 0) fuori["options"] = { ...nodo.options };
    return fuori;
  }
  // Un sottoalbero che viene da un prefab si riscrive come **riferimento**, non
  // come copia: riscriverne il corpo lo staccherebbe dal prefab, e la modifica
  // seguente al prefab non lo raggiungerebbe più. È l'unico punto in cui questa
  // funzione non guarda soltanto il nodo che ha davanti.
  if (nodo.fromPrefab !== null) {
    return { prefab: nodo.fromPrefab, size: nodo.size };
  }
  const fuori: Record<string, unknown> = { zone: nodo.name, size: nodo.size };
  if (nodo.gap !== null) fuori["gap"] = nodo.gap;
  if (nodo.align !== null) fuori["align"] = nodo.align;
  if (nodo.spread !== null) fuori["spread"] = nodo.spread;
  if (nodo.part !== null) fuori["part"] = nodo.part;
  fuori["children"] = nodo.children.map(versoDocumento);
  return fuori;
}

/**
 * Il corpo di un prefab: come `versoDocumento`, ma senza il riferimento.
 *
 * Serve a «fanne un prefab»: il sottoalbero che si solleva va scritto **per
 * esteso** dentro `layout.prefabs`, e solo il punto da cui viene diventa un
 * riferimento.
 */
export function corpoPrefab(nodo: NodoScafale): Record<string, unknown> {
  return versoDocumento({ ...nodo, fromPrefab: null });
}

/**
 * Un nome di prefab libero, dal nome del nodo.
 *
 * Minuscole, cifre e trattini: è l'alfabeto che il formato accetta, e proporre
 * un nome che verrebbe rifiutato sarebbe offrire l'errore.
 */
export function nomePrefabLibero(nodo: NodoScafale, presi: readonly string[]): string {
  const base = nodo.name.replace(/[^a-z0-9-]/g, "-") || "prefab";
  if (!presi.includes(base)) return base;
  for (let i = 2; i < 100; i += 1) {
    if (!presi.includes(`${base}-${i}`)) return `${base}-${i}`;
  }
  return `${base}-x`;
}
