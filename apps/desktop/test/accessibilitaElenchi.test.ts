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
    // `{numero}` e non `{n}`: `t` formatta i numeri con la lingua attiva e
    // sceglie il plurale su `n`, mentre il numero che la riga mostra è la cifra
    // nuda (il chiamante passa `String(numero)`).
    assert.match(testo, /\{numero\}/, `${nome}: il numero non c'è`);
    assert.doesNotMatch(testo, /\{n\}/, `${nome}: «{n}» sarebbe formattato e scelto per plurale`);
    assert.match(testo, /\{titolo\}/, `${nome}: il titolo non c'è`);
    const detto = testo.replace("{numero}", "3").replace("{titolo}", "Terzo");
    assert.ok(contenuto("3", detto), `${nome}: «${detto}» non contiene il 3 che si vede`);
    // Una riga col numero 12 e un titolo con delle cifre, non solo uno senza.
    const conCifre = testo.replace("{numero}", "12").replace("{titolo}", "Track 7");
    assert.ok(contenuto("12", conCifre), `${nome}: «${conCifre}» non contiene il 12 che si vede`);
    // Una libreria grande: la riga 12345 mostra «12345», e il nome deve dirlo
    // così, non «12.345» o «12,345» (due parole per axe, nessuna delle quali è 12345).
    const grande = testo.replace("{numero}", String(12345)).replace("{titolo}", "Mille");
    assert.ok(contenuto("12345", grande), `${nome}: «${grande}» non contiene il 12345 che si vede`);
    assert.ok(
      !contenuto("12345", `${(12345).toLocaleString(nome)}, Mille`),
      `${nome}: la prova non distingue il numero formattato`,
    );
  }
  // E la forma senza numero, quella di prima («Play Terzo»), non lo contiene: il difetto.
  assert.ok(!contenuto("3", "Play Terzo"));
});

test("la riga dei brani chiama il numero che mostra, e lo stesso valore va nel nome", () => {
  const elenco = sorgente("schermate/Elenco.tsx");
  const tasto = elementoDi(elenco, 'className="indice track-index"');
  // Il nome viene dalla chiave col numero, non da quella senza.
  assert.match(tasto, /aria-label=\{t\("list\.playNumbered", \{ numero: String\(numero\), titolo: brano\.title \}\)\}/);
  // E il numero è quello che il `span.numero` disegna: una costante sola, non due espressioni da tenere d'accordo.
  assert.match(elenco, /const numero = numeroTraccia \? \(brano\.trackNumber \?\? indice \+ 1\) : indice \+ 1;/);
  assert.match(elenco, /<span className="numero">\{numero\}<\/span>/);
});

