import { test } from "node:test";
import assert from "node:assert/strict";

import {
  carattereDellaPosizione,
  eLaLettera,
  nomiDi,
  tastoDallaPosizione,
  tastoDi,
  type Pressione,
} from "../src/tasti";
import * as tasti from "../src/tasti";

/** Una pressione come la scrive il motore, con i modificatori spenti di serie. */
function premi(key: string, code?: string, mod: Partial<Pressione> = {}): Pressione {
  return {
    key,
    code,
    ctrlKey: false,
    shiftKey: false,
    altKey: false,
    metaKey: false,
    ...mod,
  };
}

test("il carattere, con i modificatori nell'ordine di sempre", () => {
  assert.equal(tastoDi(premi("f", "KeyF", { ctrlKey: true })), "Ctrl+f");
  assert.equal(tastoDi(premi("F", "KeyF", { shiftKey: true })), "Shift+f");
  assert.equal(tastoDi(premi(" ", "Space")), "Space");
  assert.equal(tastoDi(premi("Control", "ControlLeft", { ctrlKey: true })), null);
  assert.equal(tastoDi(premi("f", "KeyF", { metaKey: true })), null);
});

test("Ctrl+F su una tastiera russa si riconosce dalla posizione", () => {
  const russa = premi("а", "KeyF", { ctrlKey: true });
  assert.equal(tastoDi(russa), "Ctrl+а");
  assert.equal(tastoDallaPosizione(russa), "Ctrl+f");
  assert.deepEqual(nomiDi(russa), ["Ctrl+а", "Ctrl+f"]);
});

test("Ctrl+0 su una tastiera francese: la cifra sta sotto Maiusc", () => {
  assert.deepEqual(nomiDi(premi("à", "Digit0", { ctrlKey: true })), ["Ctrl+à", "Ctrl+0"]);
});

test("un carattere ASCII si prende per quel che è", () => {
  // L'apostrofo italiano sta dove l'americana ha il trattino: niente zoom.
  assert.equal(tastoDallaPosizione(premi("'", "Minus", { ctrlKey: true })), null);
  assert.deepEqual(nomiDi(premi("'", "Minus", { ctrlKey: true })), ["Ctrl+'"]);
  // La Z tedesca sta dove l'americana ha la Y: vale la Z.
  assert.deepEqual(nomiDi(premi("z", "KeyY", { ctrlKey: true })), ["Ctrl+z"]);
});

test("una lettera accentata su un tasto che non è una lettera non ha posizione", () => {
  // `ò` sulla tastiera italiana sta su `Semicolon`.
  assert.equal(tastoDallaPosizione(premi("ò", "Semicolon")), null);
  assert.equal(carattereDellaPosizione("Semicolon"), null);
  assert.equal(carattereDellaPosizione(undefined), null);
});

test("la lettera di una scorciatoia fissa, su qualunque tastiera", () => {
  assert.equal(eLaLettera(premi("a", "KeyA", { ctrlKey: true }), "a"), true);
  assert.equal(eLaLettera(premi("A", "KeyA", { ctrlKey: true, shiftKey: true }), "a"), true);
  assert.equal(eLaLettera(premi("ф", "KeyA", { ctrlKey: true }), "a"), true);
  // AZERTY: la A sta dove l'americana ha la Q, e vale la A.
  assert.equal(eLaLettera(premi("a", "KeyQ", { ctrlKey: true }), "a"), true);
  assert.equal(eLaLettera(premi("q", "KeyA", { ctrlKey: true }), "a"), false);
});

test("il / di una tastiera italiana si riconosce anche senza Maiusc", () => {
  // Su IT, DE e ES «/» è Maiusc+7, su FR Maiusc sul tasto dei due punti: il
  // carattere arriva già fatto, e Maiusc è stato «speso» per farlo.
  assert.deepEqual(nomiDi(premi("/", "Digit7", { shiftKey: true })), ["Shift+/", "/"]);
  assert.deepEqual(nomiDi(premi("/", "Period", { shiftKey: true })), ["Shift+/", "/"]);
  // Le lettere no: Maiusc+F resta un accordo suo.
  assert.deepEqual(nomiDi(premi("F", "KeyF", { shiftKey: true })), ["Shift+f"]);
  // E lo spazio nemmeno: Maiusc+Spazio non diventa `alterna`.
  assert.deepEqual(nomiDi(premi(" ", "Space", { shiftKey: true })), ["Shift+Space"]);
  // Con Ctrl il nome senza Maiusc tiene il Ctrl.
  assert.deepEqual(nomiDi(premi("+", "BracketRight", { ctrlKey: true, shiftKey: true })), [
    "Ctrl+Shift++",
    "Ctrl++",
  ]);
});

test("gli interruttori non si ripetono tenendo premuto", () => {
  // Windows ripete il `keydown` dopo mezzo secondo, poi trenta volte al
  // secondo: Spazio tenuto un po' di più alternava pausa e ripresa a caso, F11
  // entrava e usciva dallo schermo intero.
  const { siRipete } = tasti;
  assert.equal(typeof siRipete, "function");
  for (const interruttore of [
    "alterna",
    "schermoIntero",
    "muto",
    "inRiproduzione",
    "importazioni",
    "incollaLink",
    "cerca",
    "precedente",
    "prossimo",
    "zoomNormale",
  ]) {
    assert.equal(siRipete(interruttore), false, interruttore);
  }
  // Dove la ripetizione è voluta: i cinque secondi, il volume, lo zoom.
  for (const passo of ["avanti", "indietro", "volumeSu", "volumeGiu", "ingrandisci", "rimpicciolisci"]) {
    assert.equal(siRipete(passo), true, passo);
  }
});

test("le frecce sul cursore della posizione saltano di cinque secondi, come le scorciatoie", () => {
  const { passoFreccia, PASSO_FRECCIA_MS } = tasti;
  assert.equal(typeof passoFreccia, "function");
  assert.equal(PASSO_FRECCIA_MS, 5_000);
  assert.equal(passoFreccia("ArrowRight", 10_000, 200_000), 15_000);
  assert.equal(passoFreccia("ArrowUp", 10_000, 200_000), 15_000);
  assert.equal(passoFreccia("ArrowLeft", 10_000, 200_000), 5_000);
  assert.equal(passoFreccia("ArrowDown", 3_000, 200_000), 0);
  assert.equal(passoFreccia("ArrowRight", 198_000, 200_000), 200_000);
  // Inizio, Fine e Pag↑/Pag↓ restano al cursore, che li sa fare da sé.
  assert.equal(passoFreccia("Home", 10_000, 200_000), null);
  assert.equal(passoFreccia("PageUp", 10_000, 200_000), null);
});

test("altgr_e_tasti_morti_non_sono_un_tasto", () => {
  // R13d-9. Su una tastiera italiana AltGr arriva come Ctrl+Alt+AltGraph, e
  // «Assegna» si chiudeva su quello; un tasto morto si registrava come
  // `Dead`, che poi scattava per ogni tasto morto. Anche Bloc Maiusc, Bloc
  // Num, il tasto del menù e la composizione dell'IME diventavano scorciatoie.
  const nudo = { ctrlKey: false, shiftKey: false, altKey: false, metaKey: false };
  assert.equal(tastoDi({ ...nudo, key: "AltGraph", ctrlKey: true, altKey: true }), null);
  for (const key of [
    "AltGraph",
    "Dead",
    "Process",
    "Unidentified",
    "CapsLock",
    "NumLock",
    "ScrollLock",
    "Fn",
    "FnLock",
    "ContextMenu",
  ]) {
    assert.equal(tastoDi({ ...nudo, key }), null, key);
    assert.deepEqual(nomiDi({ ...nudo, key }), [], key);
  }
  // Il carattere composto con AltGr resta un tasto, con gli stessi
  // modificatori che vede chi lo preme: cattura e confronto usano la stessa
  // funzione, quindi «Ctrl+Alt+[» assegnato scatta con AltGr+è.
  assert.equal(tastoDi({ ...nudo, key: "[", ctrlKey: true, altKey: true }), "Ctrl+Alt+[");
});
