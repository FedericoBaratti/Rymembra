import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { erroriDopo, etichettaImporta, fuocoSulPrimario, NESSUN_ERRORE, type ErroriLink } from "../src/statoImportaLink";

const AUTOMATICA = { code: "library.playlistIsSmart", domain: "library", severity: "error" };
const RETE = { code: "net.timeout", domain: "net", severity: "error" };

function dopo(...eventi: Parameters<typeof erroriDopo>[1][]): ErroriLink {
  return eventi.reduce(erroriDopo, NESSUN_ERRORE);
}

test("un piano riuscito toglie l'errore del piano precedente", () => {
  // Playlist automatica rifiutata, poi «Importa i brani senza creare la
  // playlist»: il piano rifatto riesce, e il blocco rosso deve sparire.
  const stato = dopo({ tipo: "pianoFallito", errore: AUTOMATICA }, { tipo: "pianoArrivato" });
  assert.equal(stato.piano, null);
});

test("un piano riuscito non cancella l'errore di un «Riprova» fallito", () => {
  const stato = dopo(
    { tipo: "letturaFallita", errore: RETE },
    { tipo: "pianoFallito", errore: AUTOMATICA },
    { tipo: "pianoArrivato" },
  );
  assert.equal(stato.azione, RETE);
  assert.equal(stato.piano, null);
});

test("una lettura nuova riparte pulita, un'importazione tiene il piano", () => {
  assert.deepEqual(
    dopo({ tipo: "letturaFallita", errore: RETE }, { tipo: "pianoFallito", errore: AUTOMATICA }, { tipo: "letturaPartita" }),
    NESSUN_ERRORE,
  );
  const fallita = dopo({ tipo: "esecuzionePartita" }, { tipo: "esecuzioneFallita", errore: RETE });
  assert.equal(fallita.azione, RETE);
  assert.equal(dopo({ tipo: "esecuzioneFallita", errore: RETE }, { tipo: "esecuzionePartita" }).azione, null);
});

test("il fuoco va su «Importa» quando il tasto si accende, una volta per anteprima", () => {
  const anteprima = { titolo: "Rock" };
  // Il disegno in cui arriva l'anteprima: il piano non c'è ancora e il tasto è spento.
  assert.equal(fuocoSulPrimario({ anteprima, servita: null, acceso: false }), false);
  // Arriva il piano: il tasto si accende, ed è adesso che il fuoco ci va.
  assert.equal(fuocoSulPrimario({ anteprima, servita: null, acceso: true }), true);
  // Dato una volta: un piano rifatto dalla casella non lo riprende.
  assert.equal(fuocoSulPrimario({ anteprima, servita: anteprima, acceso: true }), false);
  // «Riprova» porta un'anteprima nuova, e il fuoco torna.
  assert.equal(fuocoSulPrimario({ anteprima: { titolo: "Rock" }, servita: anteprima, acceso: true }), true);
  assert.equal(fuocoSulPrimario({ anteprima: null, servita: null, acceso: true }), false);
});

test("mentre si rifà il piano il tasto dice «Calcolo…», non «Importazione…» (R13c-14)", () => {
  // La casella «Crea la playlist» rifà il piano, e `inCorso` vale anche
  // allora: il tasto diceva «Importazione…» per un'importazione che nessuno
  // aveva chiesto.
  assert.equal(etichettaImporta({ inCorso: true, scrivendo: false, leggendo: false }), "link.planning");
  assert.equal(etichettaImporta({ inCorso: true, scrivendo: true, leggendo: false }), "legacy.importing");
  // «Riprova» con l'anteprima già a schermo rilegge il link.
  assert.equal(etichettaImporta({ inCorso: true, scrivendo: false, leggendo: true }), "link.retrying");
  assert.equal(etichettaImporta({ inCorso: false, scrivendo: false, leggendo: false }), "legacy.import");
});

test("Importa.tsx scrive l'etichetta del tasto con etichettaImporta", () => {
  const sorgente = readFileSync(new URL("../src/Importa.tsx", import.meta.url), "utf8");
  const link = sorgente.slice(sorgente.indexOf("export function ImportaLink"));
  assert.match(link, /\{t\(etichettaImporta\(\{ inCorso, scrivendo, leggendo \}\)\)\}/);
});
