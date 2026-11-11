/*
 * Un JSON di testi letto senza perdere niente in silenzio.
 *
 * `JSON.parse` di due chiavi uguali nello stesso oggetto tiene l'ultima e non
 * dice niente: in un file di testi tradotti vuol dire che la prima traduzione
 * — quella che chi l'ha scritta credeva di aver messo — non arriva mai a
 * schermo, e che i controlli sulle lingue confrontano la seconda. Qui si cerca
 * la ripetizione prima di leggere il valore.
 *
 * Lo usa `sito.js` per `site/testi/*.json`. È fatto per essere condiviso: i
 * testi dell'app (`strumenti/lingue.js`) hanno lo stesso problema.
 */

/**
 * Le chiavi ripetute dello stesso oggetto, come percorsi (`a.b`, `l[1].c`).
 *
 * Non è un lettore di JSON: segue solo le stringhe, le parentesi e le
 * virgole, quanto basta a sapere quale stringa è una chiave e di quale
 * oggetto. Il valore lo legge poi `JSON.parse`, che sul JSON malformato dice
 * il suo errore.
 */
function chiaviRipetute(testo) {
  const pila = []; // { oggetto, chiavi, attesa, chiave, indice }
  const fuori = [];
  const percorso = () =>
    pila
      .slice(0, -1)
      .map((p, i) => (p.oggetto ? (i === 0 ? p.chiave : `.${p.chiave}`) : `[${p.indice}]`))
      .join("");
  for (let i = 0; i < testo.length; i++) {
    const c = testo[i];
    if (c === '"') {
      let j = i + 1;
      while (j < testo.length && testo[j] !== '"') j += testo[j] === "\\" ? 2 : 1;
      const cima = pila[pila.length - 1];
      if (cima && cima.oggetto && cima.attesa) {
        const chiave = JSON.parse(testo.slice(i, j + 1));
        if (cima.chiavi.has(chiave)) {
          const prima = percorso();
          fuori.push(prima === "" ? chiave : `${prima}.${chiave}`);
        }
        cima.chiavi.add(chiave);
        cima.chiave = chiave;
        cima.attesa = false;
      }
      i = j;
    } else if (c === "{" || c === "[") {
      pila.push({ oggetto: c === "{", chiavi: new Set(), attesa: c === "{", chiave: null, indice: 0 });
    } else if (c === "}" || c === "]") {
      pila.pop();
    } else if (c === ",") {
      const cima = pila[pila.length - 1];
      if (cima && cima.oggetto) cima.attesa = true;
      else if (cima) cima.indice++;
    }
  }
  return fuori;
}

/** `JSON.parse`, ma una chiave ripetuta è un errore che nomina il file e la chiave. */
function leggiJson(testo, nome) {
  const ripetute = chiaviRipetute(testo);
  if (ripetute.length) {
    throw new Error(`${nome}: ${ripetute.map((k) => `«${k}»`).join(", ")} ripetut${ripetute.length === 1 ? "a" : "e"} nello stesso oggetto`);
  }
  return JSON.parse(testo);
}

module.exports = { chiaviRipetute, leggiJson };
