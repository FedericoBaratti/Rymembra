import { test } from "node:test";
import assert from "node:assert/strict";

import {
  colonneDa,
  finestraDa,
  finestraDaScala,
  larghezzaColonnaDa,
  passoGriglia,
  righeDiGriglia,
  scalaDa,
} from "../src/finestra";

test("la finestra in cima disegna lo schermo più il margine", () => {
  const f = finestraDa({
    scorrimento: 0,
    cima: 30,
    altezzaVista: 600,
    passo: 42,
    totale: 50_000,
    margine: 8,
  });
  assert.equal(f.primo, 0);
  assert.equal(f.visibileDa, 0);
  assert.equal(f.ultimo, Math.ceil(600 / 42) + 16);
});

test("in fondo a cinquantamila righe la finestra è l'ultima e non sfora", () => {
  const passo = 41.9;
  const totale = 50_000;
  const altezzaVista = 700;
  const cima = 30;
  // Lo scorrimento massimo: tutto l'elenco meno quel che si vede.
  const scorrimento = cima + totale * passo - altezzaVista;
  const f = finestraDa({ scorrimento, cima, altezzaVista, passo, totale, margine: 8 });
  assert.equal(f.ultimo, totale, "l'ultima riga è disegnata");
  assert.ok(f.primo > totale - 40, `e non molte prima: ${f.primo}`);
  assert.ok(f.visibileDa >= f.primo && f.visibileDa < f.ultimo);
});

test("uno scorrimento oltre i bordi resta dentro l'elenco", () => {
  const sopra = finestraDa({
    scorrimento: -500,
    cima: 0,
    altezzaVista: 400,
    passo: 40,
    totale: 100,
    margine: 8,
  });
  assert.equal(sopra.primo, 0);
  const sotto = finestraDa({
    scorrimento: 1e9,
    cima: 0,
    altezzaVista: 400,
    passo: 40,
    totale: 100,
    margine: 8,
  });
  assert.equal(sotto.ultimo, 100);
  assert.ok(sotto.primo <= 99);
  const vuoto = finestraDa({
    scorrimento: 0,
    cima: 0,
    altezzaVista: 400,
    passo: 40,
    totale: 0,
    margine: 8,
  });
  assert.deepEqual([vuoto.primo, vuoto.ultimo], [0, 0]);
});

test("un passo non misurato non divide per zero", () => {
  const f = finestraDa({
    scorrimento: 100,
    cima: 0,
    altezzaVista: 10,
    passo: 0,
    totale: 50,
    margine: 2,
  });
  assert.ok(Number.isFinite(f.primo) && Number.isFinite(f.ultimo));
  assert.ok(f.ultimo <= 50);
});

test("le colonne si contano dalle tracce calcolate", () => {
  assert.equal(colonneDa("171.5px 171.5px 171.5px"), 3);
  assert.equal(colonneDa("200px"), 1);
  assert.equal(colonneDa("[inizio] 180px [mezzo] 180px [fine]"), 2);
  // Una griglia non disegnata non ha tracce risolte: si tiene il numero di prima.
  assert.equal(colonneDa("repeat(auto-fill, minmax(164px, 1fr))"), null);
  assert.equal(colonneDa("none"), 1);
});

test("la larghezza della colonna è la prima traccia", () => {
  assert.equal(larghezzaColonnaDa("171.5px 171.5px"), 171.5);
  assert.equal(larghezzaColonnaDa("none"), null);
  assert.equal(larghezzaColonnaDa("repeat(3, 1fr)"), null);
});

test("le righe di griglia non contano una riga vuota in fondo", () => {
  assert.equal(righeDiGriglia(12, 4), 3);
  assert.equal(righeDiGriglia(13, 4), 4);
  assert.equal(righeDiGriglia(0, 4), 0);
  assert.equal(righeDiGriglia(5, 0), 5);
});

test("il fuoco nella griglia: frecce, bordi, Inizio e Fine", () => {
  // Quattro colonne, dieci celle: l'ultima riga ha due celle (8 e 9).
  const passo = (i: number, tasto: string) => passoGriglia(i, tasto, 4, 10);
  assert.equal(passo(0, "ArrowRight"), 1);
  assert.equal(passo(9, "ArrowRight"), null, "contro il bordo si resta");
  assert.equal(passo(0, "ArrowLeft"), null);
  assert.equal(passo(5, "ArrowLeft"), 4, "a sinistra si va anche a capo di riga");
  assert.equal(passo(1, "ArrowDown"), 5);
  assert.equal(passo(5, "ArrowUp"), 1);
  assert.equal(passo(2, "ArrowUp"), null);
  // Sotto la 6 non c'è niente in verticale, ma una riga sotto sì: l'ultima cella.
  assert.equal(passo(6, "ArrowDown"), 9);
  // Dall'ultima riga non si scende.
  assert.equal(passo(9, "ArrowDown"), null);
  assert.equal(passo(5, "Home"), 0);
  assert.equal(passo(5, "End"), 9);
  assert.equal(passo(5, "Enter"), null);
  assert.equal(passoGriglia(0, "End", 4, 0), null, "una griglia vuota non ha dove");
});

test("la scala somma le righe misurate e stima le altre", () => {
  const scala = scalaDa(
    new Map([
      [1, 60],
      [3, 80],
    ]),
    5,
    40,
  );
  assert.deepEqual([...scala], [0, 40, 100, 140, 220, 260]);
  assert.deepEqual([...scalaDa(new Map(), 0, 40)], [0], "un elenco vuoto è alto zero");
  // Una misura oltre la fine (la coda si è accorciata) non conta.
  assert.deepEqual([...scalaDa(new Map([[7, 99]]), 2, 40)], [0, 40, 80]);
});

/** Righe alte 40 e 120 a turno: la riga 2k comincia a 160k, la 2k+1 a 160k+40. */
function alterne(totale: number): Float64Array {
  const altezze = new Map<number, number>();
  for (let i = 0; i < totale; i++) altezze.set(i, i % 2 === 0 ? 40 : 120);
  return scalaDa(altezze, totale, 40);
}

test("su righe diverse la finestra trova la riga in cima senza un passo", () => {
  const f = finestraDaScala({
    scorrimento: 880,
    cima: 30,
    altezzaVista: 600,
    scala: alterne(1000),
    margine: 8,
  });
  // 850 dalla cima cade dentro la riga 11 (da 840 a 960).
  assert.equal(f.visibileDa, 11);
  assert.equal(f.primo, 3);
  // Il fondo della vista, 1450, cade nella riga 18 (da 1440 a 1480).
  assert.equal(f.ultimo, 18 + 1 + 8);
  // Dalla 11 alla 17 entrano intere: finiscono a 1440, la vista a 1440.
  assert.equal(f.visibili, 7);
});

test("su righe alte uguale la scala dà la finestra del passo", () => {
  const passo = 42;
  const altezze = new Map<number, number>();
  for (let i = 0; i < 5000; i++) altezze.set(i, passo);
  const m = { scorrimento: 5000, cima: 30, altezzaVista: 600, margine: 8 };
  const conPasso = finestraDa({ ...m, passo, totale: 5000 });
  const conScala = finestraDaScala({ ...m, scala: scalaDa(altezze, 5000, passo) });
  assert.equal(conScala.visibileDa, conPasso.visibileDa);
  assert.equal(conScala.primo, conPasso.primo);
  assert.equal(conScala.visibili, Math.floor(600 / passo));
});

test("la finestra su righe diverse copre sempre la vista", () => {
  // Altezze da una, due e tre righe di testo, in un ordine che non si ripete.
  const totale = 18_000;
  const altezze = new Map<number, number>();
  let seme = 7;
  for (let i = 0; i < totale; i++) {
    seme = (seme * 48_271) % 2_147_483_647;
    altezze.set(i, [38, 54, 70][seme % 3] ?? 38);
  }
  const scala = scalaDa(altezze, totale, 38);
  const cima = 8;
  const altezzaVista = 640;
  const fondo = (scala[totale] ?? 0) + cima - altezzaVista;
  for (let scorrimento = 0; scorrimento <= fondo; scorrimento += 997) {
    const f = finestraDaScala({ scorrimento, cima, altezzaVista, scala, margine: 8 });
    // In cima la vista comincia sopra la prima riga, nel bordo dell'elenco.
    const da = scorrimento - cima;
    assert.ok((scala[f.primo] ?? 0) <= Math.max(da, 0), `sopra la vista a ${scorrimento}`);
    assert.ok(
      f.ultimo === totale || (scala[f.ultimo] ?? 0) >= da + altezzaVista,
      `sotto la vista a ${scorrimento}`,
    );
    assert.ok(f.visibileDa >= f.primo && f.visibileDa < f.ultimo);
    assert.ok(f.ultimo - f.primo < 60, `e non molte di più: ${f.ultimo - f.primo}`);
  }
  // In fondo: l'ultima riga è disegnata, e non molte prima.
  const ultima = finestraDaScala({ scorrimento: fondo, cima, altezzaVista, scala, margine: 8 });
  assert.equal(ultima.ultimo, totale);
  assert.ok(ultima.primo > totale - 40, `${ultima.primo}`);
});

test("su righe diverse i bordi, l'elenco vuoto e una riga più alta della vista", () => {
  const scala = alterne(100);
  const sopra = finestraDaScala({ scorrimento: -500, cima: 0, altezzaVista: 400, scala, margine: 8 });
  assert.deepEqual([sopra.primo, sopra.visibileDa], [0, 0]);
  const sotto = finestraDaScala({ scorrimento: 1e9, cima: 0, altezzaVista: 400, scala, margine: 8 });
  assert.equal(sotto.ultimo, 100);
  assert.ok(sotto.primo <= 99 && sotto.visibileDa <= 99);
  const vuoto = finestraDaScala({
    scorrimento: 0,
    cima: 0,
    altezzaVista: 400,
    scala: scalaDa(new Map(), 0, 40),
    margine: 8,
  });
  assert.deepEqual([vuoto.primo, vuoto.ultimo, vuoto.visibili], [0, 0, 1]);
  // Una riga sola più alta della vista: PagGiù deve muoversi lo stesso.
  const alta = finestraDaScala({
    scorrimento: 0,
    cima: 0,
    altezzaVista: 400,
    scala: scalaDa(new Map([[0, 1000]]), 3, 40),
    margine: 8,
  });
  assert.equal(alta.visibili, 1);
});

test("la finestra disegnata resta finché copre la vista con mezzo margine", () => {
  const scala = alterne(1000);
  const m = { cima: 8, altezzaVista: 300, scala, margine: 8 };
  const prima = finestraDaScala({ ...m, scorrimento: 4000 });
  // Un pixel di dispositivo avanti e indietro, attorno a dove si è: la stessa
  // finestra, qualunque confine di riga cada lì in mezzo.
  for (const scarto of [1.11, -1.11, 2.22, -2.22]) {
    const dopo = finestraDaScala({ ...m, scorrimento: 4000 + scarto, tenere: prima });
    assert.deepEqual([dopo.primo, dopo.ultimo], [prima.primo, prima.ultimo], `a ${scarto}`);
  }
  // Ma non oltre il mezzo margine: lì la finestra si sposta, e copre la vista.
  const lontano = finestraDaScala({ ...m, scorrimento: 4000 + 6 * 160, tenere: prima });
  assert.ok(lontano.primo > prima.primo);
  assert.ok((scala[lontano.primo] ?? 0) <= 4000 + 6 * 160 - 8);
  assert.ok((scala[lontano.ultimo] ?? 0) >= 4000 + 6 * 160 - 8 + 300);
  // Una finestra di un elenco più lungo di adesso non si tiene.
  const corta = finestraDaScala({ ...m, scorrimento: 0, tenere: { primo: 0, ultimo: 2000 } });
  assert.ok(corta.ultimo <= 1000);
});
