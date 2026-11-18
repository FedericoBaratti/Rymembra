/**
 * La finestra delle Novità: i passi scritti a mano, le regole per aprirla, e il
 * cablaggio in `App.tsx`. La tabella «mostra / non mostra» sta in Rust
 * (`preferenze::novita_da_mostrare`); qui si prova quel che sta nella finestra.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import * as modulo from "../src/novita";

const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const it = JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>;
const en = JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>;
const pacchetto = JSON.parse(leggi("../package.json")) as { version: string };
const app = leggi("../src/App.tsx");

test("la versione di oggi ha le sue novità, da due a quattro passi", () => {
  const { novitaDi, VERSIONI_CON_NOVITA } = modulo;
  assert.ok(
    VERSIONI_CON_NOVITA.includes(pacchetto.version),
    `la versione di package.json (${pacchetto.version}) non ha novità: a «version:set» la chiave va spostata`,
  );
  const passi = novitaDi(pacchetto.version);
  assert.ok(passi !== null, "novitaDi non trova la versione di oggi");
  assert.ok(passi.length >= 2 && passi.length <= 4, `${passi.length} passi: ne servono da 2 a 4`);
});

test("una versione senza novità non ne ha, e non è un errore", () => {
  const { novitaDi } = modulo;
  assert.equal(novitaDi("0.0.0"), null);
  assert.equal(novitaDi(""), null);
});

test("ogni passo ha il suo titolo e il suo testo, in italiano e in inglese, con la chiave per versione e numero", () => {
  const { NOVITA } = modulo;
  for (const [versione, passi] of Object.entries(NOVITA)) {
    passi.forEach((passo, i) => {
      const base = `whatsNew.${versione}.${i + 1}`;
      assert.equal(passo.titoloKey, `${base}.title`, "la chiave del titolo non è whatsNew.<versione>.<n>.title");
      assert.equal(passo.testoKey, `${base}.text`, "la chiave del testo non è whatsNew.<versione>.<n>.text");
      for (const [lingua, testi] of [
        ["it", it],
        ["en", en],
      ] as const) {
        assert.ok((testi[passo.titoloKey] ?? "").trim().length > 0, `${lingua}: manca ${passo.titoloKey}`);
        assert.ok((testi[passo.testoKey] ?? "").trim().length > 0, `${lingua}: manca ${passo.testoKey}`);
      }
    });
  }
});

test("le novità non promettono quel che non c'è: nessuna parola da «presto» o «in arrivo»", () => {
  const { NOVITA } = modulo;
  for (const passi of Object.values(NOVITA)) {
    for (const passo of passi) {
      for (const testi of [it, en]) {
        const frase = `${testi[passo.titoloKey] ?? ""} ${testi[passo.testoKey] ?? ""}`;
        assert.doesNotMatch(frase, /\b(presto|in arrivo|prossimamente|soon|coming)\b/i, frase);
      }
    }
  }
});

test("le illustrazioni sono quelle che la finestra sa disegnare", () => {
  const { NOVITA, ILLUSTRAZIONI } = modulo;
  for (const passi of Object.values(NOVITA)) {
    for (const passo of passi) assert.ok(ILLUSTRAZIONI.includes(passo.illustrazione), passo.illustrazione);
  }
});

// ── quando si apre ───────────────────────────────────────────────────────────

const PRONTA = {
  daMostrare: true,
  aperta: false,
  avviato: true,
  primoVisibile: false,
  studioVisibile: false,
  giroAperto: false,
  modaleAperto: false,
};

test("la finestra si apre quando il nucleo ha detto di sì e nessuno ha la scena", () => {
  const { comeSiApreNovita } = modulo;
  assert.equal(comeSiApreNovita(PRONTA), "apri");
});

test("non si apre senza un sì del nucleo, né se è già aperta, né prima che l'app abbia i dati", () => {
  const { comeSiApreNovita } = modulo;
  assert.equal(comeSiApreNovita({ ...PRONTA, daMostrare: false }), "no");
  assert.equal(comeSiApreNovita({ ...PRONTA, aperta: true }), "no");
  assert.equal(comeSiApreNovita({ ...PRONTA, avviato: false }), "no");
});

test("mai sopra il primo avvio, lo Studio o il giro", () => {
  const { comeSiApreNovita } = modulo;
  assert.equal(comeSiApreNovita({ ...PRONTA, primoVisibile: true }), "no");
  assert.equal(comeSiApreNovita({ ...PRONTA, studioVisibile: true }), "no");
  assert.equal(comeSiApreNovita({ ...PRONTA, giroAperto: true }), "no");
});

test("sopra un altro dialogo aspetta, e non si apre", () => {
  const { comeSiApreNovita } = modulo;
  assert.equal(comeSiApreNovita({ ...PRONTA, modaleAperto: true }), "aspetta");
  // Ma il primo avvio e lo Studio vincono: lì non si aspetta, si rinuncia per ora.
  assert.equal(comeSiApreNovita({ ...PRONTA, modaleAperto: true, primoVisibile: true }), "no");
});

// ── i passi ──────────────────────────────────────────────────────────────────

test("avanti e indietro non escono dai passi", () => {
  const { passoDopo } = modulo;
  assert.equal(passoDopo(0, 4, "avanti"), 1);
  assert.equal(passoDopo(3, 4, "avanti"), 3, "oltre l'ultimo non si va");
  assert.equal(passoDopo(3, 4, "indietro"), 2);
  assert.equal(passoDopo(0, 4, "indietro"), 0, "prima del primo non si va");
  assert.equal(passoDopo(0, 0, "avanti"), 0, "senza passi non si sposta niente");
});

test("la strisciata del dito sfoglia: a sinistra avanti, a destra indietro, poco niente", () => {
  const { mossaDelDito } = modulo;
  assert.equal(mossaDelDito({ dx: -200, vx: -0.1, larghezza: 360 }), "avanti");
  assert.equal(mossaDelDito({ dx: 200, vx: 0.1, larghezza: 360 }), "indietro");
  assert.equal(mossaDelDito({ dx: -20, vx: -0.05, larghezza: 360 }), null);
  // Un colpo veloce vale anche se corto.
  assert.equal(mossaDelDito({ dx: -40, vx: -0.9, larghezza: 360 }), "avanti");
});

test("le frecce della tastiera sfogliano, e solo quelle", () => {
  const { mossaDaTasto } = modulo;
  assert.equal(mossaDaTasto("ArrowRight"), "avanti");
  assert.equal(mossaDaTasto("ArrowLeft"), "indietro");
  assert.equal(mossaDaTasto("Enter"), null);
  assert.equal(mossaDaTasto("Tab"), null);
});

// ── il cablaggio in App ──────────────────────────────────────────────────────

test("App monta le Novità una volta, e la proposta del sottofondo aspetta che si chiudano", () => {
  assert.equal((app.match(/<NovitaAllAvvio\b/g) ?? []).length, 1, "le Novità non sono montate una volta sola");
  const riga = app.match(/const propostaPuoMostrarsi =[^;]+;/)?.[0] ?? "";
  assert.match(riga, /!novitaInAttesa/, "la proposta non aspetta le Novità");
  assert.match(riga, /!primoVisibile/);
  assert.match(riga, /!giroDaFare/);
  assert.match(riga, /!giroAperto/);
});

test("il giro aspetta che le Novità si siano chiuse, perché non ci siano due veli insieme", () => {
  assert.match(app, /daFare: giroDaFare && !novitaInAttesa/);
});

test("le Novità usano i due comandi del nucleo, e si segnano viste comunque si chiudano", () => {
  const finestra = leggi("../src/parti/Novita.tsx");
  assert.match(finestra, /\.novitaDaMostrare\(VERSIONI_CON_NOVITA\)/);
  assert.match(finestra, /\.novitaViste\(/);
  // Si segna «viste» comunque si chiuda: l'ultimo passo, «Chiudi», Esc.
  assert.match(finestra, /onChiudi=\{chiudi\}/);
});

test("la finestra è un Dialogo modale, e si sfoglia col dito", () => {
  const finestra = leggi("../src/parti/Novita.tsx");
  assert.match(finestra, /<Dialogo\b/);
  assert.match(finestra, /useDito\(/);
  assert.match(finestra, /mossaDelDito\(/);
});
