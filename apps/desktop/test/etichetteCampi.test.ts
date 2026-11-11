import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * I campi delle schede di Scrobbling, dei modelli IA e dell'equalizzatore
 * hanno un nome (R13-13).
 *
 * Avevano soltanto il segnaposto, che sparisce appena si scrive e che molti
 * lettori di schermo non leggono come nome: «campo di modifica, protetto» e
 * basta. E il cursore di ogni banda si chiamava «31 Hz, 3.5 dB», col valore
 * dentro il nome — detto prima del valore, che il lettore poi ripete, e col
 * punto inglese anche in italiano.
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");

/** Ogni `<input …/>` del sorgente, con un sì se ha un nome. */
function campi(sorgente: string): { tag: string; conNome: boolean }[] {
  const trovati: { tag: string; conNome: boolean }[] = [];
  for (const m of sorgente.matchAll(/<input\b/g)) {
    const da = m.index;
    const tag = sorgente.slice(da, sorgente.indexOf("/>", da) + 2);
    const prima = sorgente.slice(0, da);
    // Dentro una `<label>` ancora aperta: il nome è il suo testo.
    const dentroEtichetta = prima.lastIndexOf("<label") > prima.lastIndexOf("</label>");
    const conNome = /aria-label(ledby)?=/.test(tag) || dentroEtichetta;
    trovati.push({ tag, conNome });
  }
  return trovati;
}

test("ogni campo ha un nome, non solo un segnaposto", () => {
  for (const file of ["Scrobbling.tsx", "ModelliIA.tsx", "Equalizzatore.tsx"]) {
    const elenco = campi(leggi(`../src/parti/${file}`));
    assert.ok(elenco.length > 0, `${file}: nessun campo, la prova va aggiornata`);
    for (const { tag, conNome } of elenco) {
      assert.ok(conNome, `${file}: campo senza nome:\n${tag}`);
    }
  }
});

test("il cursore di una banda si chiama con la banda, e il valore lo dice aria-valuetext", () => {
  const sorgente = leggi("../src/parti/Equalizzatore.tsx");
  const cursore = sorgente.slice(sorgente.indexOf('type="range"'));
  const tag = cursore.slice(0, cursore.indexOf("/>"));
  assert.doesNotMatch(tag, /aria-label=\{t\("eq\.band\.aria"/);
  assert.match(tag, /aria-label=\{etichettaBanda\(hz\)\}/);
  assert.match(tag, /aria-valuetext=/);
});

test("i tasti di Scrobbling che lavorano non si spengono sotto il fuoco", () => {
  const sorgente = leggi("../src/parti/Scrobbling.tsx");
  const interruttore = sorgente.slice(sorgente.indexOf('aria-label={t("scrobble.toggle")}'));
  const spento = /disabled=\{([^}]*)\}/.exec(interruttore)?.[1] ?? "";
  assert.ok(!spento.includes("inVolo"), `interruttore: disabled={${spento}}`);
  const dove = sorgente.indexOf('t("scrobble.sendNow")');
  const invia = sorgente.slice(sorgente.lastIndexOf("<button", dove), dove);
  assert.doesNotMatch(invia, /disabled=\{inVolo\}/);
  assert.match(invia, /aria-busy=/);
});
