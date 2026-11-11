import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Prove sui sorgenti delle schermate, dove la regola sta nel cablaggio: chi
 * riceve un errore, che cosa fa un tasto, che nome ha un comando. Come
 * `impostazioni.test.ts`, si legge il sorgente e si cerca il punto in cui la
 * regola è scritta.
 */
const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

test("Impostazioni, scheda dei testi: le scritture che falliscono si dicono", () => {
  const impostazioni = sorgente("schermate/Impostazioni.tsx");
  for (const comando of ["testiRete", "testiAccanto"]) {
    const dopo = impostazioni.slice(impostazioni.indexOf(`.${comando}(`));
    const catena = dopo.slice(0, dopo.indexOf(";"));
    assert.doesNotMatch(catena, /\.catch\(\(\) => \{\}\)/, comando);
    assert.match(catena, /\.catch\(onErrore\)/, comando);
  }
  const riempi = impostazioni.slice(impostazioni.indexOf("ipc.testiFerma() : ipc.testiRiempi()"));
  assert.match(riempi.slice(0, riempi.indexOf("}}")), /\.catch\(onErrore\)/);
  assert.match(impostazioni, /<SchedaTesti onErrore=\{onErrore\} \/>/);
});

test("Impostazioni, ricerca: Invio fa quel che fa il clic, scheda compresa", () => {
  const impostazioni = sorgente("schermate/Impostazioni.tsx");
  const invio = impostazioni.slice(impostazioni.indexOf('if (e.key === "Enter" && prima)'));
  assert.match(invio.slice(0, invio.indexOf("\n")), /apriVoce\(prima\)/);
  assert.match(impostazioni, /onClick=\{\(\) => apriVoce\(v\)\}/);
  const apri = impostazioni.slice(impostazioni.indexOf("const apriVoce"));
  assert.match(apri.slice(0, apri.indexOf("};")), /vaiAllaScheda\(t\(v\.scheda\)\)/);
});

test("il posto del contenuto è il punto di riferimento main della finestra", () => {
  // R16-10: axe `landmark-one-main` su tutte le schermate. Lo scafale
  // disegnava `nav`, `aside` e `header`, mai `main`.
  const impaginazione = sorgente("Impaginazione.tsx");
  assert.match(
    impaginazione,
    /const Involucro = nodo\.name === "content" && ctx\.stato === undefined \? "main" : "div";/,
  );
  assert.match(impaginazione, /<Involucro className=\{classi\(nodo\)\} data-nodo=\{nodo\.at\}>/);
});

test("cuore e muto: nome fisso e stato in aria-pressed, mai tutti e due che cambiano", () => {
  // R16-11. «Togli dai preferiti, premuto»: lo stato detto due volte, in due
  // modi che si contraddicono.
  for (const file of ["parti/Giudizio.tsx", "parti/Ora.tsx", "Lettore.tsx", "schermate/InRiproduzioneTelefono.tsx"]) {
    const testo = sorgente(file);
    assert.doesNotMatch(testo, /aria-label=\{brano\.liked \?/, file);
    assert.doesNotMatch(testo, /aria-label=\{stato\.muto \?/, file);
  }
  assert.match(sorgente("parti/Giudizio.tsx"), /aria-pressed=\{brano\.liked\}\s+aria-label=\{t\("track\.favorite"\)\}/);
  assert.match(sorgente("parti/Giudizio.tsx"), /aria-pressed=\{stato\.muto\}\s+aria-label=\{t\("player\.muted"\)\}/);
});

test("trasporto spento in sessione: raggiungibile, con la ragione detta", () => {
  // R16-11. `disabled` + `title`: il tasto non prendeva il fuoco e la
  // ragione stava solo nel suggerimento, per chi ha il puntatore.
  const trasporto = sorgente("parti/Trasporto.tsx");
  assert.doesNotMatch(trasporto, /\sdisabled=\{/);
  assert.equal([...trasporto.matchAll(/aria-disabled=\{!(trasporto|mescola) \|\| undefined\}/g)].length, 5);
  assert.equal([...trasporto.matchAll(/aria-describedby=\{(trasporto|mescola) \? undefined : id(Spento|SenzaCasuale)\}/g)].length, 5);
  // Premuto da spento non fa niente.
  assert.equal([...trasporto.matchAll(/onClick=\{seAcceso\((trasporto|mescola), ipc\.\w+\)\}/g)].length, 5);
});
