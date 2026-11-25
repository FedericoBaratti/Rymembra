/**
 * «Scarica qui» su un insieme grande chiede prima (FR2, punto 1).
 *
 * F3c lo scrisse fra i rischi: «Scarica qui» sull'artista accodava tutti i brani
 * remoti senza chiedere, e con migliaia di brani — migliaia di file, i gigabyte
 * del disco di chi scarica — il gesto era un clic troppo facile. Passano tutti da
 * `scaricaQui` (il menu di una riga o di una selezione, l'album, la playlist,
 * l'artista), e la conferma sta lì, in un posto solo.
 *
 * Senza un DOM: la decisione e le parole sono funzioni pure di `lontano.ts`, e il
 * resto si guarda nel sorgente di `App.tsx`, nel comando del nucleo e nei file
 * delle lingue.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import {
  PER_VOLTA_SCARICO,
  SOGLIA_CONFERMA_SCARICO,
  chiedeConfermaPerLoScarico,
  domandaDelloScarico,
  inBlocchi,
} from "../src/lontano";

const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, import.meta.url), "utf8").replace(/\r\n/g, "\n");
const APP = leggi("../src/App.tsx");
const SCARICO_RS = leggi("../src-tauri/src/lontano/scarico.rs");
const IT = JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>;
const EN = JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>;

/** I formattatori della finestra, finti: la prova guarda quale frase si sceglie. */
const FORMATI = {
  brani: (n: number) => `${n} brani`,
  peso: (byte: number) => `${Math.round(byte / 1e6)} MB`,
};

test("la conferma serve da cinquanta brani in su, e non prima", () => {
  assert.equal(SOGLIA_CONFERMA_SCARICO, 50);
  assert.equal(chiedeConfermaPerLoScarico(0), false);
  assert.equal(chiedeConfermaPerLoScarico(1), false, "un album, una selezione: si scarica e basta");
  assert.equal(chiedeConfermaPerLoScarico(SOGLIA_CONFERMA_SCARICO - 1), false);
  assert.equal(chiedeConfermaPerLoScarico(SOGLIA_CONFERMA_SCARICO), true);
  assert.equal(chiedeConfermaPerLoScarico(12_000), true);
});

test("la domanda dice quanti brani e quanto spazio, quando lo spazio è noto", () => {
  const domanda = domandaDelloScarico(
    { brani: 1340, byte: 8_200_000_000, senzaDimensione: 0 },
    FORMATI,
  );
  assert.deepEqual(domanda.titolo, {
    chiave: "devices.download.confirm.title",
    valori: { brani: "1340 brani" },
  });
  assert.deepEqual(domanda.descrizione, {
    chiave: "devices.download.confirm.body",
    valori: { spazio: "8200 MB" },
  });
});

test("con qualche dimensione ignota si dice «almeno», e non si dà una cifra per intera", () => {
  const domanda = domandaDelloScarico(
    { brani: 900, byte: 3_000_000_000, senzaDimensione: 40 },
    FORMATI,
  );
  assert.equal(domanda.descrizione.chiave, "devices.download.confirm.bodyAtLeast");
  assert.deepEqual(domanda.descrizione.valori, { spazio: "3000 MB" });
});

test("senza nessuna dimensione nota non si inventa uno spazio", () => {
  for (const byte of [0, -1]) {
    const domanda = domandaDelloScarico({ brani: 300, byte, senzaDimensione: 300 }, FORMATI);
    assert.equal(domanda.descrizione.chiave, "devices.download.confirm.bodyUnknown");
    assert.deepEqual(domanda.descrizione.valori, {});
  }
  // E se l'unica dimensione nota è zero byte, è lo stesso: non c'è un numero da dire.
  assert.equal(
    domandaDelloScarico({ brani: 300, byte: 0, senzaDimensione: 0 }, FORMATI).descrizione.chiave,
    "devices.download.confirm.bodyUnknown",
  );
});

test("un elenco lungo parte a blocchi dei più grandi che il nucleo accetta", () => {
  const ids = Array.from({ length: 12_001 }, (_, i) => i);
  const blocchi = inBlocchi(ids, PER_VOLTA_SCARICO);
  assert.deepEqual(
    blocchi.map((b) => b.length),
    [5000, 5000, 2001],
  );
  assert.deepEqual(blocchi.flat(), ids, "né un id perso né uno ripetuto, nello stesso ordine");
  assert.deepEqual(inBlocchi([], PER_VOLTA_SCARICO), [], "niente da scaricare, nessuna chiamata");
  assert.deepEqual(inBlocchi([1, 2, 3], PER_VOLTA_SCARICO), [[1, 2, 3]]);
});

test("il numero di brani per volta è quello che il nucleo rifiuta di superare", () => {
  const nucleo = /const MASSIMI_PER_VOLTA: usize = (\d[\d_]*);/.exec(SCARICO_RS)?.[1];
  assert.ok(nucleo, "scarico.rs non ha più MASSIMI_PER_VOLTA");
  assert.equal(PER_VOLTA_SCARICO, Number(nucleo.replace(/_/g, "")));
});

test("«Scarica qui» chiede prima di accodare, e dopo aver tolto i dispositivi spenti", () => {
  const corpoDi = (nome: string) => {
    const inizio = APP.indexOf(`const ${nome} = useCallback(`);
    assert.ok(inizio > 0, `App non ha più ${nome}`);
    return APP.slice(inizio, APP.indexOf("\n  );\n", inizio));
  };
  // Il lavoro sta in due pezzi: `preparaLoScarico` toglie i dispositivi spenti dalle righe
  // intere e `preparaLoScaricoDaId` li ha già tolti il nucleo (S9-8, S9-9); poi tutti e due
  // passano da `confermaEAccoda`. `scaricaQui` lo mette dietro la guardia «una alla volta».
  const prepara = corpoDi("preparaLoScarico");
  assert.ok(
    prepara.includes("avvisaNonRaggiungibile(primoSpento(lontani, pannello))"),
    "le righe intere si filtrano dai dispositivi spenti prima di chiedere",
  );
  assert.ok(prepara.indexOf("confermaEAccoda(accesi.map") > prepara.indexOf("primoSpento"));
  const daId = corpoDi("preparaLoScaricoDaId");
  assert.ok(daId.includes("await confermaEAccoda(scelte.ids)"), "gli id del nucleo passano dalla stessa domanda");
  assert.ok(
    daId.indexOf("avvisaDellaRigaSpenta(scelte.primaSpenta)") < daId.indexOf("confermaEAccoda("),
    "i dispositivi spenti si dicono prima della domanda",
  );
  const corpo = corpoDi("confermaEAccoda");
  const chiede = corpo.indexOf("ipc.lontanoScaricaAnteprima(");
  const accoda = corpo.indexOf("await accodaRighe(ids)");
  assert.ok(chiede > 0, "la domanda ha il conto del nucleo");
  assert.ok(accoda > chiede, "e l'accodamento dopo la domanda");
  assert.ok(corpo.includes("chiedeConfermaPerLoScarico("), "la soglia sta in `lontano.ts`, non qui");
  assert.ok(
    corpo.includes("if (chiedeConfermaPerLoScarico(anteprima.brani))"),
    "si decide sul numero del nucleo, che non conta le righe che non scendono",
  );
  // Il «Scarica» della finestrella accoda e dice gli errori: non li lascia cadere.
  assert.match(corpo, /fai: \(\) => \{\s*accodaRighe\(ids\)\.catch\(segnalaErrore\);/);
  // E l'accodamento è a blocchi, ripartendo da dove si era fermato dopo il sì ai dati mobili.
  const accodamento = corpoDi("accodaRighe");
  assert.ok(accodamento.includes("inBlocchi(ids, PER_VOLTA_SCARICO)"));
  assert.ok(accodamento.includes("ipc.lontanoScaricaRighe("));
  assert.ok(
    accodamento.indexOf("let fatti = 0") < accodamento.indexOf("conIlSiDeiDatiMobili("),
    "`fatti` sta fuori dal gesto che si rifà",
  );
});

test("il comando del nucleo che dice quanto pesa è registrato e la finestra lo chiama", () => {
  const lib = leggi("../src-tauri/src/lib.rs");
  assert.match(lib, /lontano::scarico::lontano_scarica_anteprima,/);
  assert.match(leggi("../src/ipc.ts"), /invoke<AnteprimaScarico>\("lontano_scarica_anteprima", \{ righe \}\)/);
  // Non accoda niente: l'anteprima non nomina la coda.
  const inizio = SCARICO_RS.indexOf("pub async fn lontano_scarica_anteprima(");
  const corpo = SCARICO_RS.slice(inizio, SCARICO_RS.indexOf("\n}\n", inizio));
  assert.ok(corpo.includes("righe_da_scaricare"));
  assert.ok(!corpo.includes("accoda(") && !corpo.includes("CODA"), "l'anteprima non accoda");
});

test("le parole della domanda ci sono in tutte e due le lingue e dicono le stesse cose", () => {
  const chiavi = [
    "devices.download.confirm.title",
    "devices.download.confirm.body",
    "devices.download.confirm.bodyAtLeast",
    "devices.download.confirm.bodyUnknown",
    "devices.download.confirm.go",
  ];
  const segnaposto = (frase: string) => (frase.match(/\{\w+\}/g) ?? []).sort().join();
  for (const chiave of chiavi) {
    assert.ok(IT[chiave], `it.json non ha ${chiave}`);
    assert.ok(EN[chiave], `en.json non ha ${chiave}`);
    assert.equal(segnaposto(IT[chiave] ?? ""), segnaposto(EN[chiave] ?? ""), chiave);
  }
  assert.match(IT["devices.download.confirm.title"] ?? "", /\{brani\}/);
  assert.match(IT["devices.download.confirm.body"] ?? "", /\{spazio\}/);
  assert.match(IT["devices.download.confirm.bodyAtLeast"] ?? "", /\{spazio\}/);
  assert.doesNotMatch(IT["devices.download.confirm.bodyUnknown"] ?? "", /\{/);
  // «Scarica» e «Annulla», come la finestrella promette: l'«Annulla» è quello di sempre.
  assert.equal(IT["devices.download.confirm.go"], "Scarica");
  assert.equal(EN["devices.download.confirm.go"], "Download");
});
