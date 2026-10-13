/**
 * Chi richiama che cosa, e come rinominare senza rompere i richiami.
 *
 * Una skin nomina le sue cose e poi le richiama per nome: un colore della
 * tavolozza con `{ "$palette": "etichetta" }`, un motivo con `$pattern`,
 * un'animazione scrivendone il nome in `parts.<p>.motion.<innesco>`, una curva
 * scrivendone il nome in un campo `easing`. Rinominare solo la dichiarazione
 * lascia ogni richiamo a puntare nel vuoto — e il validatore lo direbbe, una
 * riga per richiamo. Qui si rinomina la voce **e** chi la usa, in una
 * scrittura sola.
 *
 * I richiami stanno anche dentro gli array — gli sfondi a livelli, le fermate
 * di un gradiente — dove `patch.ts` non indicizza. Si lavora sull'oggetto
 * intero e si riscrive il testo: è un passo della storia come un altro.
 */

type Json = unknown;

const eOggetto = (v: Json): v is Record<string, Json> =>
  v !== null && typeof v === "object" && !Array.isArray(v);

/** I percorsi puntati di tutti i valori per cui `trova` dice sì. */
export function chiRichiama(documento: Json, trova: (valore: Json, chiave: string) => boolean): string[] {
  const fuori: string[] = [];
  const visita = (v: Json, via: string[], chiave: string) => {
    if (via.length > 0 && trova(v, chiave)) {
      fuori.push(via.join("."));
      return;
    }
    if (Array.isArray(v)) v.forEach((x, i) => visita(x, [...via, String(i)], String(i)));
    else if (eOggetto(v)) for (const [k, x] of Object.entries(v)) visita(x, [...via, k], k);
  };
  visita(documento, [], "");
  return fuori;
}

/** Chi richiama un colore della tavolozza. */
export function chiUsaColore(documento: Json, nome: string): string[] {
  return chiRichiama(documento, (v) => eOggetto(v) && v["$palette"] === nome);
}

/** Chi richiama un motivo. */
export function chiUsaMotivo(documento: Json, nome: string): string[] {
  return chiRichiama(documento, (v) => eOggetto(v) && v["$pattern"] === nome);
}

/** Chi richiama un token. */
export function chiUsaToken(documento: Json, id: string): string[] {
  return chiRichiama(documento, (v) => eOggetto(v) && v["$token"] === id);
}

/** Le parti e gli inneschi che richiamano un'animazione. */
export function chiUsaAnimazione(documento: Json, nome: string): string[] {
  const parti = eOggetto(documento) && eOggetto(documento["parts"]) ? documento["parts"] : {};
  const fuori: string[] = [];
  for (const [parte, stile] of Object.entries(parti)) {
    const movimento = eOggetto(stile) && eOggetto(stile["motion"]) ? stile["motion"] : {};
    for (const [innesco, richiamata] of Object.entries(movimento)) {
      if (richiamata === nome) fuori.push(`parts.${parte}.motion.${innesco}`);
    }
  }
  return fuori;
}

/** I campi `easing` che richiamano una curva per nome. */
export function chiUsaCurva(documento: Json, nome: string): string[] {
  return chiRichiama(documento, (v, chiave) => chiave === "easing" && v === nome);
}

/** Una copia profonda, con `cambia` applicato a ogni valore dal basso. */
function trasforma(v: Json, cambia: (valore: Json, chiave: string) => Json, chiave = ""): Json {
  let dentro: Json = v;
  if (Array.isArray(v)) dentro = v.map((x, i) => trasforma(x, cambia, String(i)));
  else if (eOggetto(v)) {
    dentro = Object.fromEntries(Object.entries(v).map(([k, x]) => [k, trasforma(x, cambia, k)]));
  }
  return cambia(dentro, chiave);
}

/** Lo stesso oggetto con una chiave rinominata al suo posto, o `null` se non si può. */
function conChiave(oggetto: Json, da: string, a: string): Record<string, Json> | null {
  if (!eOggetto(oggetto) || !(da in oggetto) || a in oggetto) return null;
  return Object.fromEntries(Object.entries(oggetto).map(([k, v]) => [k === da ? a : k, v]));
}

/** Rinomina la chiave di una sezione, nella base e nella variante se c'è. */
function rinominaSezione(documento: Record<string, Json>, sezione: readonly string[], da: string, a: string) {
  const scendi = (radice: Record<string, Json>, via: readonly string[]): boolean => {
    const [primo, ...resto] = via;
    if (primo === undefined) return false;
    const figlio = radice[primo];
    if (resto.length === 0) {
      const nuovo = conChiave(figlio, da, a);
      if (nuovo === null) return false;
      radice[primo] = nuovo;
      return true;
    }
    if (!eOggetto(figlio)) return false;
    const copia = { ...figlio };
    const fatto = scendi(copia, resto);
    if (fatto) radice[primo] = copia;
    return fatto;
  };
  const fatto = scendi(documento, sezione);
  const variante = documento["variant"];
  if (eOggetto(variante)) {
    const copia = { ...variante };
    if (scendi(copia, sezione)) documento["variant"] = copia;
  }
  return fatto;
}

export type Genere = "colore" | "motivo" | "animazione" | "curva";

/**
 * Il documento con una voce rinominata e ogni suo richiamo aggiornato, o
 * `null` se il nome nuovo è già preso o quello vecchio non c'è.
 */
export function rinomina(documento: Json, genere: Genere, da: string, a: string): Record<string, Json> | null {
  if (!eOggetto(documento) || da === a) return null;
  const copia = { ...documento };
  const sezione =
    genere === "colore"
      ? ["palette"]
      : genere === "motivo"
        ? ["patterns"]
        : genere === "animazione"
          ? ["motion", "animations"]
          : ["motion", "easings"];
  if (!rinominaSezione(copia, sezione, da, a)) return null;

  switch (genere) {
    case "colore":
      return trasforma(copia, (v) =>
        eOggetto(v) && v["$palette"] === da ? { ...v, $palette: a } : v,
      ) as Record<string, Json>;
    case "motivo":
      return trasforma(copia, (v) =>
        eOggetto(v) && v["$pattern"] === da ? { ...v, $pattern: a } : v,
      ) as Record<string, Json>;
    case "curva":
      return trasforma(copia, (v, chiave) => (chiave === "easing" && v === da ? a : v)) as Record<
        string,
        Json
      >;
    case "animazione": {
      const parti = copia["parts"];
      if (!eOggetto(parti)) return copia;
      copia["parts"] = Object.fromEntries(
        Object.entries(parti).map(([parte, stile]) => {
          if (!eOggetto(stile) || !eOggetto(stile["motion"])) return [parte, stile];
          const movimento = Object.fromEntries(
            Object.entries(stile["motion"]).map(([k, v]) => [k, v === da ? a : v]),
          );
          return [parte, { ...stile, motion: movimento }];
        }),
      );
      return copia;
    }
  }
}
