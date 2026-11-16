import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Tre rifiniture dei fogli (R18-7, R18-15, R18-19), lette dal testo dei fogli che
 * `main.tsx` importa. Il foglio non si esegue qui: si prova quel che dice.
 */
const MAIN = readFileSync(new URL("../src/main.tsx", import.meta.url), "utf8");
const FOGLIO = [...MAIN.matchAll(/^import "\.\/(.+\.css)";\r?$/gm)]
  .map((m) => readFileSync(new URL(`../src/${m[1] ?? ""}`, import.meta.url), "utf8"))
  .join("\n")
  .replace(/\r\n/g, "\n")
  // Via i commenti: le regole si cercano nel codice, non nelle spiegazioni.
  .replace(/\/\*[\s\S]*?\*\//g, "");

/** Le regole `selettore { corpo }` del foglio, anche dentro un `@media`/`@container`. */
function regole(): { selettore: string; corpo: string }[] {
  const trovate: { selettore: string; corpo: string }[] = [];
  const re = /([^{}]+)\{([^{}]*)\}/g;
  for (let m = re.exec(FOGLIO); m !== null; m = re.exec(FOGLIO)) {
    trovate.push({ selettore: (m[1] ?? "").trim(), corpo: m[2] ?? "" });
  }
  return trovate;
}

const proprieta = (corpo: string, nome: string): string | null => {
  const m = new RegExp(`(?:^|[;\\s])${nome}\\s*:\\s*([^;]+)`).exec(corpo);
  return m === null ? null : (m[1] ?? "").trim();
};

// ── R18-7 ───────────────────────────────────────────────────────────────────

test("il segnaposto delle copertine grandi sta al centro: ogni regola che dà `display` alla classe ha la sua `.vuota` (R18-7)", () => {
  // `.vuota` mette il segnaposto al centro con `display: grid`, ma le regole più
  // specifiche di queste tre classi gli rimettevano `display: block`: la nota
  // finiva in alto, o nell'angolo in alto a sinistra.
  const tutte = regole();
  for (const classe of ["column-art", "copertina-dettaglio", "arte-np"]) {
    const conBlocco = tutte.filter(
      (r) => r.selettore.includes(`.${classe}`) && !r.selettore.includes(".vuota") && proprieta(r.corpo, "display") === "block",
    );
    assert.ok(conBlocco.length > 0, `.${classe} non dà più display: block: la prova va aggiornata`);
    const sue = tutte.filter(
      (r) =>
        r.selettore.split(",").some((s) => s.includes(`.${classe}.vuota`)) &&
        proprieta(r.corpo, "display") === "grid" &&
        proprieta(r.corpo, "place-items") === "center",
    );
    assert.ok(sue.length > 0, `manca \`.${classe}.vuota { display: grid; place-items: center }\``);
  }
});

// ── R18-15 ──────────────────────────────────────────────────────────────────

test("una copertina che sta arrivando mostra il suo fondo e il suo filo, non un buco (R18-15)", () => {
  // `.foto { opacity: 0 }` nascondeva l'`<img>` intero: anche il fondo, l'ombra e
  // il filo che devono fare da segnaposto. Il segnaposto è il bordo e il fondo
  // dell'elemento; quel che non c'è ancora sono i suoi pixel.
  for (const r of regole()) {
    const selettori = r.selettore.split(",").map((s) => s.trim());
    if (!selettori.some((s) => /(^|[\s>+~])\.foto(?![\w-])/.test(s) && !s.includes("[data-pronta]"))) continue;
    assert.notEqual(
      proprieta(r.corpo, "opacity"),
      "0",
      `«${r.selettore}» nasconde l'immagine con opacity: 0, e con lei il fondo e il filo`,
    );
  }
  const nonPronta = regole().find((r) => r.selettore.includes(".foto:not([data-pronta])"));
  assert.ok(nonPronta, "manca la regola per la copertina non ancora pronta");
  // I pixel fuori dal riquadro: il contenuto di un'immagine si ritaglia al suo
  // riquadro, e il fondo e il filo restano.
  assert.match(nonPronta.corpo, /object-position:\s*100000px 100000px/);
  assert.match(nonPronta.corpo, /object-fit:\s*none/);
  assert.match(nonPronta.corpo, /color:\s*transparent/, "il testo alternativo e l'icona d'immagine rotta non si vedono");
});

// ── R18-19 ──────────────────────────────────────────────────────────────────

test("le schede delle Impostazioni hanno una larghezza massima (R18-19)", () => {
  // A 1920 px l'interruttore di «Tieni d'occhio le cartelle» stava a 1 000 px dalla
  // sua etichetta; a 2560 la fascia era larga 1 650 px. Le note hanno già il loro
  // tetto (72ch): la colonna delle schede, no.
  const corpo = regole().find((r) => r.selettore === ".impostazioni .corpo");
  assert.ok(corpo, "manca `.impostazioni .corpo`");
  const massima = proprieta(corpo.corpo, "max-width");
  assert.ok(massima !== null, "la colonna delle schede non ha un `max-width`");
  const px = /^(\d+)px$/.exec(massima);
  assert.ok(px, `il tetto è in pixel, non «${massima}»`);
  assert.ok(Number(px[1]) >= 640 && Number(px[1]) <= 900, `il tetto ${massima} è fuori da 640–900 px`);
});
