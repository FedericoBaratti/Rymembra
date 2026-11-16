/**
 * La riga di un brano di un altro dispositivo, in «Sfoglia» (R13-8).
 *
 * Sul computer il doppio clic sulla riga suona l'elenco. Ma un `stopPropagation`
 * sul `click` non ferma il `dblclick`: un doppio clic sul ⋮ (gesto comune) apriva
 * il menù **e** faceva partire l'elenco dell'altro, sostituendo la coda; sulla
 * copertina, che suona da sé, partiva tre volte. È l'aggravante di 08-3,
 * corretta nelle righe della libreria (`schermate/Elenco.tsx`) e non qui.
 *
 * I componenti non si montano con `node --test`: si legge il sorgente.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

/** Il corpo di `RigaBrano`, dalla sua dichiarazione al componente che segue. */
function rigaBrano(): string {
  const pagina = sorgente("schermate/Dispositivo.tsx");
  const da = pagina.indexOf("function RigaBrano(");
  const a = pagina.indexOf("\n/** I brani dell'altro, una riga ciascuno. */", da);
  assert.ok(da >= 0 && a > da, "non trovo RigaBrano");
  return pagina.slice(da, a);
}

test("il doppio clic sui comandi della riga non fa partire l'elenco (R13-8)", () => {
  const riga = rigaBrano();
  const doppio = /onDoubleClick=\{([\s\S]*?)\}\s*\n\s*onContextMenu/.exec(riga);
  assert.ok(doppio, "la riga ha un suo onDoubleClick");
  const gestore = doppio[1] ?? "";
  assert.match(gestore, /MOBILE\s*\?\s*undefined/, "sul telefono il doppio tocco è lo zoom: niente");
  assert.match(
    gestore,
    /closest\(["']button["']\)/,
    "il doppio clic su un tasto della riga (il ⋮, la copertina) risale fin qui e non deve suonare",
  );
});

test("la copertina suona una volta sola con un doppio clic (R13-8)", () => {
  const riga = rigaBrano();
  const avvia = riga.indexOf('className="avvia-lontana"');
  assert.ok(avvia >= 0, "manca il tasto della copertina");
  const click = riga.slice(avvia, riga.indexOf("{nelDisco ?", avvia));
  assert.match(click, /e\.stopPropagation\(\)/, "il clic non risale alla riga del telefono");
  assert.match(
    click,
    /e\.detail > 1/,
    "il secondo clic di un doppio clic non fa partire un'altra volta lo stesso elenco",
  );
});
