/**
 * Il JSON, colorato. Una riga alla volta.
 *
 * # Perché non una libreria
 *
 * Perché il documento è JSON e basta, e un evidenziatore per JSON è un ciclo su
 * una riga. Le librerie che si userebbero al suo posto — un editor intero,
 * grammatiche, temi — pesano più di tutta l'applicazione e portano dentro una
 * seconda idea di come si scrive il codice, che poi si vede.
 *
 * # Perché riga per riga si può
 *
 * In JSON una stringa **non** può contenere un a capo letterale: il formato
 * pretende `\n`. Quindi nessun costrutto attraversa una riga, e ogni riga si
 * legge da sola senza portarsi dietro uno stato. È la proprietà che rende questo
 * file venti righe invece di un tokenizzatore con gli stati.
 *
 * # Questo non valida niente
 *
 * A dire se un documento è buono è `parse_skin_json`, sull'intero documento,
 * centoventi millisecondi dopo. Qui si colora anche quel che è rotto — anzi
 * soprattutto: è mentre si scrive che i colori servono.
 */

/** Che cos'è un pezzo di riga. */
export type Genere = "chiave" | "stringa" | "numero" | "letterale" | "segno";

/** Un pezzo di riga, già classificato. */
export interface Pezzo {
  genere: Genere;
  testo: string;
}

/** Dove finisce una stringa che comincia a `da`, virgolette comprese. */
function fineStringa(riga: string, da: number): number {
  for (let i = da + 1; i < riga.length; i += 1) {
    const c = riga[i];
    if (c === "\\") {
      i += 1;
      continue;
    }
    if (c === '"') return i + 1;
  }
  // Una stringa aperta e non chiusa è il caso normale a metà di una parola.
  return riga.length;
}

/** La stringa che comincia qui è una chiave? Lo dice il primo segno dopo. */
function eUnaChiave(riga: string, dopo: number): boolean {
  for (let i = dopo; i < riga.length; i += 1) {
    const c = riga[i];
    if (c === " " || c === "\t") continue;
    return c === ":";
  }
  return false;
}

const LETTERALI = ["true", "false", "null"] as const;

/** I pezzi di una riga, in ordine. Concatenarli ridà la riga esatta. */
export function evidenzia(riga: string): Pezzo[] {
  const pezzi: Pezzo[] = [];
  let segno = "";
  const chiudiSegno = () => {
    if (segno !== "") {
      pezzi.push({ genere: "segno", testo: segno });
      segno = "";
    }
  };

  let i = 0;
  while (i < riga.length) {
    const c = riga[i] ?? "";

    if (c === '"') {
      const fine = fineStringa(riga, i);
      chiudiSegno();
      pezzi.push({
        genere: eUnaChiave(riga, fine) ? "chiave" : "stringa",
        testo: riga.slice(i, fine),
      });
      i = fine;
      continue;
    }

    if (c >= "0" && c <= "9") {
      let fine = i;
      while (fine < riga.length && /[0-9.eE+-]/.test(riga[fine] ?? "")) fine += 1;
      chiudiSegno();
      pezzi.push({ genere: "numero", testo: riga.slice(i, fine) });
      i = fine;
      continue;
    }

    const parola = LETTERALI.find((quale) => riga.startsWith(quale, i));
    if (parola !== undefined) {
      chiudiSegno();
      pezzi.push({ genere: "letterale", testo: parola });
      i += parola.length;
      continue;
    }

    segno += c;
    i += 1;
  }
  chiudiSegno();
  return pezzi;
}

/**
 * Su quale riga sta l'errore, contando da zero.
 *
 * Il nucleo dà il percorso nel documento (`parts.section-cards`), non una
 * posizione nel testo: `SkinDocument` è un albero e non sa più da che carattere
 * veniva. Una chiave del percorso però è scritta nel file **fra virgolette**, e
 * cercarla è il modo più corto per arrivare alla riga giusta.
 *
 * # Perché non basta l'ultimo pezzo
 *
 * Perché i punti nel percorso separano i livelli, ma stanno **anche dentro** i
 * nomi: `tokens.color.accent` è il token `color.accent` dentro `tokens`, non un
 * `accent` dentro un `color`. Prendere l'ultimo pezzo e basta cercherebbe
 * `"accent"`, che nel file non c'è.
 *
 * Quindi si provano tutti i sotto-percorsi dal più lungo al più corto e si tiene
 * il primo che si trova: il più lungo è anche il più specifico, quindi il primo
 * a corrispondere è quello giusto.
 */
export function rigaDi(sorgente: string, percorso: string): number | null {
  const pezzi = percorso.split(".");
  for (let da = 0; da < pezzi.length; da += 1) {
    const dove = sorgente.indexOf(`"${pezzi.slice(da).join(".")}"`);
    if (dove >= 0) return sorgente.slice(0, dove).split("\n").length - 1;
  }
  return null;
}

/** Riga e colonna di un carattere, contando da uno: è come si leggono. */
export function posizione(sorgente: string, indice: number): {
  riga: number;
  colonna: number;
} {
  const prima = sorgente.slice(0, indice).split("\n");
  return {
    riga: prima.length,
    colonna: (prima[prima.length - 1] ?? "").length + 1,
  };
}
