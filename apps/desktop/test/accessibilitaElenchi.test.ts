import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Le due violazioni di axe-core 4.14 che il collaudo del 13/11 ha trovato su
 * quasi ogni schermata (L-PC, difetti 3 e 4), lette nei sorgenti: i componenti
 * non si montano con `node --test`, quindi quel che si controlla è il filo.
 *
 * - `list` (serious): `<ol class="righe-coda queue-list">` aveva per figlio
 *   diretto `<li class="paglia" role="presentation">`, lo spaziatore della
 *   virtualizzazione. Un `li` con un ruolo diverso da `listitem` non è un
 *   figlio valido di un `ol`.
 * - `label-content-name-mismatch` (serious, WCAG 2.5.3): `.track-index` mostra
 *   il numero della riga ma il suo nome accessibile era «Play <titolo>», senza
 *   il numero: chi comanda a voce dice «clic 3» e non trova niente.
 */

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");
const lingua = (nome: "it" | "en") =>
  JSON.parse(readFileSync(new URL(`../src/lingue/${nome}.json`, import.meta.url), "utf8")) as Record<string, string>;

/** L'elemento JSX a cui appartiene `ancora`: dal `<` che lo apre al `/>` che lo chiude. */
function elementoDi(testo: string, ancora: string): string {
  const posizione = testo.indexOf(ancora);
  assert.ok(posizione >= 0, `manca «${ancora}»`);
  const da = testo.lastIndexOf("<", posizione);
  const a = testo.indexOf("/>", posizione);
  assert.ok(da >= 0 && a > da, `l'elemento di «${ancora}» non si legge`);
  return testo.slice(da, a + 2);
}

// ── list ─────────────────────────────────────────────────────────────────────

test("la paglia della coda non porta un ruolo: è nascosta, e l'ol resta valido (axe list)", () => {
  const paglia = elementoDi(sorgente("Coda.tsx"), 'className="paglia"');
  assert.match(paglia, /^<li\b/, "la paglia della coda è un li: sta dentro un ol");
  assert.doesNotMatch(paglia, /\brole=/, "un li con un ruolo che non è listitem rompe la regola `list` di axe");
  assert.match(paglia, /aria-hidden="true"/, "senza ruolo, la paglia conterebbe come una voce vuota della lista");
});

test("ogni li che fa da paglia è nascosto ai lettori di schermo", () => {
  // Lo stesso schema in un'altra lista virtualizzata: stesso difetto.
  for (const file of ["Coda.tsx", "schermate/Elenco.tsx", "schermate/Cartelle.tsx", "schermate/Libreria.tsx"]) {
    const testo = sorgente(file);
    for (const trovato of testo.matchAll(/<li\b[^>]*className="paglia"/g)) {
      const elemento = elementoDi(testo.slice(trovato.index), 'className="paglia"');
      assert.match(elemento, /aria-hidden="true"/, `${file}: una paglia li senza aria-hidden`);
    }
  }
});

// ── label-content-name-mismatch ─────────────────────────────────────────────

/**
 * La regola di axe (`isLabelContainedInName`): le parole del testo visibile
 * stanno, di seguito, fra quelle del nome accessibile.
 */
function contenuto(visibile: string, nome: string): boolean {
  const parole = (testo: string) => testo.toLowerCase().normalize("NFKD").split(/[^\p{L}\p{N}]+/u).filter(Boolean);
  const dentro = parole(nome);
  const cercate = parole(visibile);
  return dentro.some((_, i) => cercate.every((parola, k) => dentro[i + k] === parola));
}

test("il nome del tasto della riga contiene il numero che la riga mostra, in tutte e due le lingue", () => {
  for (const nome of ["it", "en"] as const) {
    const testo = lingua(nome)["list.playNumbered"];
    assert.ok(testo !== undefined, `${nome}: manca list.playNumbered`);
    assert.match(testo, /\{n\}/, `${nome}: il numero non c'è`);
    assert.match(testo, /\{titolo\}/, `${nome}: il titolo non c'è`);
    const detto = testo.replace("{n}", "3").replace("{titolo}", "Terzo");
    assert.ok(contenuto("3", detto), `${nome}: «${detto}» non contiene il 3 che si vede`);
    // Una riga col numero 3 e un titolo qualunque, non solo uno senza cifre.
    const conCifre = testo.replace("{n}", "12").replace("{titolo}", "Track 7");
    assert.ok(contenuto("12", conCifre), `${nome}: «${conCifre}» non contiene il 12 che si vede`);
  }
  // E la forma senza numero, quella di prima, non lo contiene: il difetto.
  assert.ok(!contenuto("3", lingua("en")["list.play"]!.replace("{titolo}", "Terzo")));
});

test("la riga dei brani chiama il numero che mostra, e lo stesso valore va nel nome", () => {
  const elenco = sorgente("schermate/Elenco.tsx");
  const tasto = elementoDi(elenco, 'className="indice track-index"');
  // Il nome viene dalla chiave col numero, non da quella senza.
  assert.match(tasto, /aria-label=\{t\("list\.playNumbered", \{ n: numero, titolo: brano\.title \}\)\}/);
  // E il numero è quello che il `span.numero` disegna: una costante sola, non due espressioni da tenere d'accordo.
  assert.match(elenco, /const numero = numeroTraccia \? \(brano\.trackNumber \?\? indice \+ 1\) : indice \+ 1;/);
  assert.match(elenco, /<span className="numero">\{numero\}<\/span>/);
});

test("anche la lista dei brani di un altro dispositivo, dove il numero si vede, lo dice nel nome", () => {
  const lontana = sorgente("schermate/Dispositivo.tsx");
  const tasto = elementoDi(lontana, 'className="avvia-lontana"');
  // Con la copertina al posto del numero il nome resta «Play <titolo>»: non c'è testo visibile da contenere.
  assert.match(tasto, /nelDisco\s*\?\s*t\("list\.playNumbered", \{ n: numero, titolo: brano\.title \}\)\s*:\s*t\("list\.play", \{ titolo: brano\.title \}\)/);
  assert.match(lontana, /const numero = brano\.trackNumber \?\? indice \+ 1;/);
  assert.match(lontana, /<span className="numero-lontana">\{numero\}<\/span>/);
});
