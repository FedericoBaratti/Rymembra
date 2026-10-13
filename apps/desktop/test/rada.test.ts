import { test } from "node:test";
import assert from "node:assert/strict";

import { ANTICIPO, ArchivioRado, pagineDaPrendere, RIPROVA_MS } from "../src/rada";

/** Una richiesta che si risolve quando lo dice il test, non quando vuole lei. */
interface Sospesa {
  offset: number;
  limite: number;
  risolvi: (righe: number[]) => void;
  rifiuta: (errore: unknown) => void;
}

/** Un archivio con richieste a mano: ogni pagina chiesta finisce in `richieste`. */
function archivio(misura = 10, capienza = 40) {
  const richieste: Sospesa[] = [];
  const errori: unknown[] = [];
  let ora = 0;
  const a = new ArchivioRado<number>({
    misura,
    capienza,
    adesso: () => ora,
    onErrore: (e) => errori.push(e),
    chiedi: (offset, limite) =>
      new Promise<number[]>((risolvi, rifiuta) => {
        richieste.push({ offset, limite, risolvi, rifiuta });
      }),
  });
  return {
    a,
    richieste,
    errori,
    passa: (ms: number) => {
      ora += ms;
    },
  };
}

/** Le righe vere di un elenco di `totale` numeri, da `offset`. */
function pagina(offset: number, limite: number, totale: number): number[] {
  const righe: number[] = [];
  for (let i = offset; i < Math.min(offset + limite, totale); i += 1) righe.push(i);
  return righe;
}

/** Lascia girare le promesse risolte. */
const giro = () => new Promise((fatto) => setImmediate(fatto));

test("le pagine da prendere coprono la finestra e niente di più", () => {
  assert.deepEqual(pagineDaPrendere(0, 30, 1000, 200), [0]);
  assert.deepEqual(pagineDaPrendere(190, 230, 1000, 200), [0, 1]);
  assert.deepEqual(pagineDaPrendere(400, 401, 1000, 200), [2]);
  // Gli estremi si stringono dentro l'elenco: l'anticipo può uscire dai bordi.
  assert.deepEqual(pagineDaPrendere(-60, 30, 1000, 200), [0]);
  assert.deepEqual(pagineDaPrendere(950, 1100, 1000, 200), [4]);
  // In fondo a cinquantamila: l'ultima pagina, e non quelle di mezzo.
  assert.deepEqual(pagineDaPrendere(49_970, 50_000, 50_000, 200), [249]);
  // Vuoto: niente da chiedere.
  assert.deepEqual(pagineDaPrendere(0, 30, 0, 200), []);
  assert.deepEqual(pagineDaPrendere(10, 10, 1000, 200), []);
});

test("in fondo a cinquantamila righe si chiede solo l'ultima pagina", async () => {
  const { a, richieste } = archivio(200);
  a.imposta("brani", 50_000);
  a.richiedi(49_975, 50_000);
  // Con l'anticipo la finestra tocca la penultima pagina solo se la sfiora:
  // qui 49_975 - ANTICIPO cade ancora nell'ultima.
  assert.ok(49_975 - ANTICIPO >= 49_800);
  assert.deepEqual(
    richieste.map((r) => r.offset),
    [49_800],
  );
  richieste[0]?.risolvi(pagina(49_800, 200, 50_000));
  await giro();
  assert.equal(a.riga(49_999), 49_999);
  assert.equal(a.riga(25_000), undefined, "le pagine di mezzo non ci sono");
  assert.equal(a.pagineInMano, 1);
});

test("una pagina già in mano o in volo non si richiede", async () => {
  const { a, richieste } = archivio(10);
  a.imposta("v", 100);
  a.richiedi(0, 5);
  a.richiedi(0, 5);
  // Le pagine della finestra con il suo anticipo, una volta sola ciascuna.
  assert.equal(richieste.length, pagineDaPrendere(-ANTICIPO, 5 + ANTICIPO, 100, 10).length);
  const chieste = new Set(richieste.map((r) => r.offset));
  assert.equal(chieste.size, richieste.length, "nessuna pagina chiesta due volte");
  for (const r of richieste) r.risolvi(pagina(r.offset, r.limite, 100));
  await giro();
  const prima = richieste.length;
  a.richiedi(0, 5);
  assert.equal(richieste.length, prima, "in mano: niente richieste nuove");
});

test("la risposta di una generazione vecchia si butta", async () => {
  const { a, richieste } = archivio(10);
  a.imposta("brani:scaffale", 100);
  a.richiedi(0, 5);
  const vecchia = richieste[0];
  assert.ok(vecchia);
  // L'ordinamento cambia mentre la pagina è in volo.
  a.imposta("brani:titolo", 100);
  a.richiedi(0, 5);
  const nuova = richieste.find((r, i) => i > 0 && r.offset === 0);
  assert.ok(nuova, "la stessa pagina si richiede per la vista nuova");
  // Arrivano in ordine inverso: prima la nuova, poi la vecchia.
  nuova.risolvi([100, 101, 102, 103, 104, 105, 106, 107, 108, 109]);
  await giro();
  vecchia.risolvi([0, 1, 2, 3, 4, 5, 6, 7, 8, 9]);
  await giro();
  assert.equal(a.riga(0), 100, "la vecchia non ha scritto sopra la nuova");
});

test("invalidare tiene le righe a schermo e le richiede", async () => {
  const { a, richieste } = archivio(10);
  a.imposta("preferiti", 30);
  a.richiedi(0, 5);
  for (const r of richieste) r.risolvi(pagina(r.offset, r.limite, 30));
  await giro();
  const prima = richieste.length;
  const generazione = a.generazione;

  a.invalida();
  assert.equal(a.generazione, generazione + 1);
  assert.equal(a.riga(3), 3, "vecchia ma ancora a schermo: niente scheletri");
  a.richiedi(0, 5);
  assert.ok(richieste.length > prima, "e si richiede");
  const dinuovo = richieste.slice(prima).find((r) => r.offset === 0);
  assert.ok(dinuovo);
  dinuovo.risolvi(pagina(0, 10, 30).map((n) => n + 1000));
  await giro();
  assert.equal(a.riga(3), 1003);
});

test("una chiave nuova butta tutto, un totale nuovo no", async () => {
  const { a, richieste } = archivio(10);
  a.imposta("v", 30);
  a.richiedi(0, 5);
  for (const r of richieste) r.risolvi(pagina(r.offset, r.limite, 30));
  await giro();
  a.imposta("v", 29);
  assert.equal(a.riga(0), 0, "lo stesso elenco, cambiato sotto: le righe restano");
  assert.equal(a.totale, 29);
  a.imposta("w", 29);
  assert.equal(a.riga(0), undefined, "un altro elenco: niente di quello di prima");
});

test("il totale si corregge da sé quando l'ultima pagina è più corta", async () => {
  const { a, richieste } = archivio(10);
  // Il conteggio dice 100, ma il nucleo ne ha 95.
  a.imposta("v", 100);
  a.richiedi(90, 100);
  const ultima = richieste.find((r) => r.offset === 90);
  assert.ok(ultima);
  ultima.risolvi(pagina(90, 10, 95));
  await giro();
  assert.equal(a.totale, 95);
  // E il conteggio vecchio, ripetuto da chi possiede la lista, non lo riporta
  // indietro: conta solo quando cambia.
  a.imposta("v", 100);
  assert.equal(a.totale, 95);
  // Un elenco cresciuto: la pagina porta più righe del previsto.
  const { a: b, richieste: rb } = archivio(10);
  b.imposta("v", 15);
  b.richiedi(10, 15);
  rb.find((r) => r.offset === 10)?.risolvi(pagina(10, 10, 18));
  await giro();
  assert.equal(b.totale, 18);
});

test("oltre la capienza si dimenticano le pagine guardate meno di recente", async () => {
  // Pagine da duecento, così l'anticipo resta dentro la pagina della riga.
  const { a, richieste } = archivio(200, 2);
  a.imposta("v", 1000);
  const chiedi = async (primo: number) => {
    a.richiedi(primo, primo + 1);
    for (const r of richieste.splice(0)) r.risolvi(pagina(r.offset, r.limite, 1000));
    await giro();
  };
  await chiedi(500);
  await chiedi(700);
  await chiedi(900);
  assert.equal(a.pagineInMano, 2);
  // La prima guardata è la prima dimenticata.
  assert.equal(a.riga(500), undefined);
  assert.equal(a.riga(900), 900);
});

test("aggiorna riscrive le righe in mano, e toglierne accorcia la pagina", async () => {
  const { a, richieste } = archivio(10);
  a.imposta("v", 20);
  a.richiedi(0, 20);
  for (const r of richieste) r.risolvi(pagina(r.offset, r.limite, 20));
  await giro();
  a.aggiorna((righe) => righe.map((n) => n * 2));
  assert.equal(a.riga(3), 6);
  a.aggiorna((righe) => righe.filter((n) => n !== 6));
  assert.equal(a.riga(3), 8, "le righe dopo salgono di un posto");
  assert.equal(a.riga(9), undefined, "e in fondo alla pagina resta un buco");
  assert.equal(a.riga(10), 20, "le pagine dopo restano dove sono");
});

test("una pagina fallita si dice una volta e si riprova dopo un po'", async () => {
  const { a, richieste, errori, passa } = archivio(10);
  a.imposta("v", 30);
  a.richiedi(0, 5);
  const n = richieste.length;
  for (const r of richieste) r.rifiuta(new Error("occupato"));
  await giro();
  assert.equal(errori.length, n);
  a.richiedi(0, 5);
  assert.equal(richieste.length, n, "subito dopo non si riprova");
  passa(RIPROVA_MS + 1);
  a.richiedi(0, 5);
  assert.ok(richieste.length > n, "dopo un po' sì");
});

test("chi ascolta sente gli arrivi, e l'istantanea cambia", async () => {
  const { a, richieste } = archivio(10);
  let sentiti = 0;
  const smetti = a.iscrivi(() => {
    sentiti += 1;
  });
  const prima = a.istantanea();
  a.imposta("v", 10);
  a.richiedi(0, 10);
  richieste[0]?.risolvi(pagina(0, 10, 10));
  await giro();
  assert.ok(sentiti >= 2);
  assert.notEqual(a.istantanea(), prima);
  smetti();
  const dopo = sentiti;
  a.invalida();
  assert.equal(sentiti, dopo);
});

test("l'intervallo chiede le pagine che mancano e le aspetta", async () => {
  const { a, richieste } = archivio(10);
  a.imposta("v", 100);
  const promessa = a.intervallo(5, 24);
  assert.deepEqual(
    richieste.map((r) => r.offset).sort((x, y) => x - y),
    [0, 10, 20],
  );
  for (const r of richieste) r.risolvi(pagina(r.offset, r.limite, 100));
  const righe = await promessa;
  assert.equal(righe.length, 20);
  assert.equal(righe[0], 5);
  assert.equal(righe[19], 24);
});
