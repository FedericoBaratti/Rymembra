/**
 * La libreria unica nella finestra (`src/origine.ts`): il filtro per
 * dispositivo, le righe grigie e quel che si suona quando un dispositivo non
 * risponde.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { DispositivoLontano, StatoLontano } from "../src/ipc/lontano";
import {
  LOCALE,
  TUTTI,
  chiaviDiLibreria,
  chiaveDiOrigine,
  dispositiviDiLibreria,
  eFiltrabile,
  frasePerLaRiga,
  origineConChiavi,
  origineDi,
  origineValida,
  origineVedeIlDispositivo,
  vistaPerIlFiltro,
  perIlNucleo,
  pilloleOrigine,
  primoSpento,
  statoDellaRiga,
  suonabili,
  ugualiOrigini,
} from "../src/origine";

const PC = "aa".repeat(32);
const TELEFONO = "bb".repeat(32);
const VECCHIO = "cc".repeat(32);

function dispositivo(
  chiave: string,
  nome: string,
  piattaforma: string,
  online: boolean,
  revocato = false,
): DispositivoLontano {
  return {
    chiave,
    nome,
    piattaforma,
    dispositivo: "",
    online,
    vistoMs: null,
    associatoMs: 0,
    revocato,
    versione: null,
    insiemeAcceso: false,
    insiemeOspita: false,
    insieme: null,
    via: online ? "diretto" : null,
  };
}

function stato(...dispositivi: DispositivoLontano[]): StatoLontano {
  return {
    attivo: true,
    acceso: true,
    chiave: null,
    impronta: null,
    relay: null,
    datiMobili: "chiedi",
    sottofondo: false,
    errore: null,
    dispositivi,
    invito: null,
    richieste: [],
  };
}

const riga = (chiave: string, id: number) => ({ path: `lontano://${chiave}/${id}` });
const locale = (n: number) => ({ path: `C:\\Musica\\${n}.flac` });

test("«Tutti» non porta niente al nucleo: è l'assenza, e costa come prima", () => {
  assert.equal(perIlNucleo(TUTTI), undefined);
  assert.deepEqual(perIlNucleo(LOCALE), { tipo: "locale" });
  assert.deepEqual(perIlNucleo(origineDi(PC)), { tipo: "dispositivo", chiave: PC });
});

test("ogni origine ha la sua chiave, e due origini uguali si riconoscono", () => {
  assert.equal(chiaveDiOrigine(TUTTI), "tutti");
  assert.equal(chiaveDiOrigine(LOCALE), "locale");
  assert.equal(chiaveDiOrigine(origineDi(PC)), `dispositivo:${PC}`);
  assert.ok(ugualiOrigini(origineDi(PC), origineDi(PC)));
  assert.ok(!ugualiOrigini(origineDi(PC), origineDi(TELEFONO)));
  assert.ok(!ugualiOrigini(TUTTI, LOCALE));
});

test("S9-3: quale filtro vede le righe di un dispositivo che sta mandando il catalogo", () => {
  assert.ok(origineVedeIlDispositivo(TUTTI, PC), "«Tutti» le vede tutte");
  assert.ok(!origineVedeIlDispositivo(LOCALE, PC), "«Questo dispositivo» non ha righe di un altro");
  assert.ok(origineVedeIlDispositivo(origineDi(PC), PC));
  assert.ok(!origineVedeIlDispositivo(origineDi(TELEFONO), PC), "il filtro di un altro dispositivo no");
});

test("senza dispositivi collegati non ci sono pillole: due nomi per la stessa cosa", () => {
  assert.deepEqual(pilloleOrigine(null), []);
  assert.deepEqual(pilloleOrigine(stato()), []);
  assert.deepEqual(pilloleOrigine(stato(dispositivo(PC, "PC", "windows", true, true))), []);
});

test("le pillole sono Tutti, Questo dispositivo e un dispositivo ciascuno, anche se spento", () => {
  const pillole = pilloleOrigine(
    stato(dispositivo(TELEFONO, "Telefono", "android", false), dispositivo(PC, "Casa", "windows", true)),
  );
  assert.deepEqual(
    pillole.map((p) => chiaveDiOrigine(p.origine)),
    ["tutti", "locale", `dispositivo:${PC}`, `dispositivo:${TELEFONO}`],
    "per nome: Casa prima di Telefono"
  );
  assert.equal(pillole[0]?.dispositivo, null);
  assert.equal(pillole[3]?.dispositivo?.telefono, true);
  assert.equal(pillole[3]?.dispositivo?.online, false, "spento ma sempre scegliibile");
});

test("un dispositivo dimenticato o revocato non si può più filtrare: si torna a «Tutti»", () => {
  const collegati = stato(dispositivo(PC, "PC", "windows", true));
  assert.deepEqual(origineValida(origineDi(PC), collegati), origineDi(PC));
  assert.deepEqual(origineValida(origineDi(VECCHIO), collegati), TUTTI);
  assert.deepEqual(
    origineValida(origineDi(PC), stato(dispositivo(PC, "PC", "windows", true, true))),
    TUTTI
  );
  assert.deepEqual(origineValida(LOCALE, collegati), LOCALE);
  // Il pannello non è ancora arrivato: non dice che il dispositivo non c'è.
  assert.deepEqual(origineValida(origineDi(PC), null), origineDi(PC));
  assert.deepEqual(dispositiviDiLibreria(null), []);
});

test("una riga di qui non ha un dispositivo; una del PC sì, e dice se risponde", () => {
  const acceso = stato(dispositivo(PC, "Casa", "windows", true));
  const spento = stato(dispositivo(PC, "Casa", "windows", false));
  assert.equal(statoDellaRiga(locale(1).path, acceso), null);
  assert.equal(statoDellaRiga(null, acceso), null);
  assert.deepEqual(statoDellaRiga(riga(PC, 7).path, acceso), {
    chiave: PC,
    nome: "Casa",
    telefono: false,
    spenta: false,
  });
  assert.equal(statoDellaRiga(riga(PC, 7).path, spento)?.spenta, true);
  assert.equal(statoDellaRiga(riga(PC, 7).path, spento)?.nome, "Casa");
});

test("una riga di un dispositivo che non è più collegato è spenta e senza nome", () => {
  const altri = stato(dispositivo(TELEFONO, "Telefono", "android", true));
  assert.deepEqual(statoDellaRiga(riga(PC, 7).path, altri), {
    chiave: PC,
    nome: "",
    telefono: false,
    spenta: true,
  });
  const revocato = stato(dispositivo(PC, "Casa", "windows", true, true));
  assert.equal(statoDellaRiga(riga(PC, 7).path, revocato)?.spenta, true);
});

test("finché il pannello non c'è, nessuna riga è grigia: un grigio sarebbe un'invenzione", () => {
  assert.equal(statoDellaRiga(riga(PC, 7).path, null)?.spenta, false);
});

test("quel che si suona a liste intere salta le righe di un PC spento", () => {
  const spento = stato(
    dispositivo(PC, "Casa", "windows", false),
    dispositivo(TELEFONO, "Telefono", "android", true)
  );
  const lista = [locale(1), riga(PC, 2), riga(TELEFONO, 3), riga(PC, 4), locale(5)];
  const tutto = suonabili(lista, 0, spento);
  assert.deepEqual(tutto.brani, [lista[0], lista[2], lista[4]]);
  assert.equal(tutto.indice, 0);
  // Da una riga che resta: l'indice segue il suo posto nella lista che resta.
  assert.equal(suonabili(lista, 2, spento).indice, 1, "la riga del telefono è la seconda");
  assert.equal(suonabili(lista, 4, spento).indice, 2);
  // Da una riga che salta: si parte dalla prima suonabile dopo.
  assert.equal(suonabili(lista, 1, spento).indice, 1);
});

test("una lista tutta di un PC spento non ha niente da suonare, e dice chi", () => {
  const spento = stato(dispositivo(PC, "Casa", "windows", false));
  const lista = [riga(PC, 1), riga(PC, 2)];
  const niente = suonabili(lista, 0, spento);
  assert.deepEqual(niente.brani, []);
  assert.equal(niente.indice, 0);
  assert.equal(primoSpento(lista, spento)?.nome, "Casa");
  assert.equal(primoSpento([locale(1)], spento), null);
  // Con il PC acceso niente cambia.
  const acceso = stato(dispositivo(PC, "Casa", "windows", true));
  assert.deepEqual(suonabili(lista, 1, acceso), { brani: lista, indice: 1 });
});

test("il filtro si segue per chiavi: un dispositivo che sparisce riporta a «Tutti»", () => {
  const collegati = stato(
    dispositivo(PC, "PC", "windows", true),
    dispositivo(TELEFONO, "T", "android", false),
  );
  const chiavi = chiaviDiLibreria(collegati);
  assert.equal(chiavi, [PC, TELEFONO].sort().join(","));
  assert.deepEqual(origineConChiavi(origineDi(PC), chiavi), origineDi(PC));
  assert.deepEqual(origineConChiavi(origineDi(VECCHIO), chiavi), TUTTI);
  assert.deepEqual(origineConChiavi(origineDi(PC), null), origineDi(PC), "pannello non arrivato");
  assert.deepEqual(origineConChiavi(LOCALE, ""), LOCALE);
  assert.equal(chiaviDiLibreria(null), null);
  assert.equal(chiaviDiLibreria(stato()), "");
});

test("scegliere un dispositivo apre la libreria dov'era, se il filtro la riguarda", () => {
  assert.equal(vistaPerIlFiltro("brani"), "brani");
  assert.equal(vistaPerIlFiltro("artisti"), "artisti");
  assert.equal(vistaPerIlFiltro("preferiti"), "preferiti");
  for (const altra of ["home", "playlist", "cartelle", "impostazioni", "altro", "cerca"]) {
    assert.equal(vistaPerIlFiltro(altra), "album", altra);
  }
  assert.ok(eFiltrabile("album"));
  assert.ok(!eFiltrabile("cartelle"), "le cartelle sono di questa macchina");
  assert.ok(!eFiltrabile("playlist"), "una playlist nomina le sue righe una per una");
});

test("una riga in grazia è grigia anche col PC acceso, e dice che non la condivide più", () => {
  // Il PC non manda più il brano, e la sua voce resta nella playlist per la
  // grazia: non è un dispositivo che non risponde, e l'avviso non lo deve dire.
  const acceso = stato(dispositivo(PC, "Casa", "windows", true));
  assert.deepEqual(statoDellaRiga(riga(PC, 7).path, acceso, true), {
    chiave: PC,
    nome: "Casa",
    telefono: false,
    spenta: true,
    inGrazia: true,
  });
  // Senza il pannello non si sa il nome, ma si sa che non si suona.
  assert.equal(statoDellaRiga(riga(PC, 7).path, null, true)?.spenta, true);
  assert.equal(statoDellaRiga(riga(PC, 7).path, null, true)?.nome, "");
  // Una riga di qui non è mai in grazia.
  assert.equal(statoDellaRiga(locale(1).path, acceso, true), null);
  // Senza il segno, niente cambia.
  assert.equal(statoDellaRiga(riga(PC, 7).path, acceso, false)?.spenta, false);
  assert.equal(statoDellaRiga(riga(PC, 7).path, acceso)?.inGrazia, undefined);
});

test("una lista suonata da una playlist salta le righe in grazia, e le dice", () => {
  const acceso = stato(dispositivo(PC, "Casa", "windows", true));
  const lista = [
    { ...locale(1), inGrazia: false },
    { ...riga(PC, 2), inGrazia: true },
    { ...riga(PC, 3) },
  ];
  const tutto = suonabili(lista, 0, acceso);
  assert.deepEqual(tutto.brani, [lista[0], lista[2]]);
  assert.equal(primoSpento(lista, acceso)?.inGrazia, true);
  assert.equal(primoSpento(lista, acceso)?.nome, "Casa");
  assert.equal(primoSpento([lista[0], lista[2]], acceso), null);
});

// ── S9-14: il segno di una riga non dice «non raggiungibile» se suona ───────

test("S9-14: una riga con la copia di qui non è detta «non raggiungibile»", () => {
  // Il PC è spento e il brano ha una copia anche qui: la riga non è grigia e
  // suona (`copiaQui`), ma il suo segno — letto al sorvolo e da un lettore di
  // schermo — diceva «“PC” non raggiungibile».
  const base = { chiave: PC, nome: "Casa", telefono: false };
  assert.equal(frasePerLaRiga({ ...base, spenta: true, copiaQui: true }).chiave, "library.origin.copyHere");
  assert.deepEqual(frasePerLaRiga({ ...base, spenta: true, copiaQui: true }).valori, { nome: "Casa" });
  // Le altre non cambiano.
  assert.equal(frasePerLaRiga({ ...base, spenta: true }).chiave, "library.origin.offline");
  assert.equal(frasePerLaRiga({ ...base, spenta: false }).chiave, "library.origin.from");
  assert.equal(frasePerLaRiga({ ...base, spenta: false, inGrazia: true }).chiave, "library.origin.faded");
  assert.equal(frasePerLaRiga({ ...base, nome: "", spenta: true }).chiave, "library.origin.gone");
  // `etichettaDellaRiga` ne traduce la frase, e le due lingue ce l'hanno.
  const lontano = readFileSync(new URL("../src/dispositivoLontano.ts", import.meta.url), "utf8");
  assert.match(lontano, /const frase = frasePerLaRiga\(riga\);\s*return t\(frase\.chiave, frase\.valori\);/);
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(
      readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
    ) as Record<string, string>;
    assert.match(testi["library.origin.copyHere"] ?? "", /\{nome\}/, lingua);
    assert.doesNotMatch(testi["library.origin.copyHere"] ?? "", /raggiungibile|reachable/, lingua);
  }
});
