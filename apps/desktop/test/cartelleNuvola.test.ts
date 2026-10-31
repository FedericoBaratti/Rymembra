import { test } from "node:test";
import assert from "node:assert/strict";

import {
  contoDi,
  decodificaSegmento,
  eNuvola,
  iconaServizio,
  nomeRadice,
  nomeServizioDi,
  primaLocale,
  schedaDi,
  segmentiDi,
  servizioDelConto,
  servizioDi,
  soloLocali,
} from "../src/cartelleNuvola";

test("eNuvola: solo lo schema, e niente per un percorso che manca", () => {
  assert.equal(eNuvola("nuvola://onedrive-k3x7q2ab/Musica"), true);
  assert.equal(eNuvola("nuvola://onedrive-k3x7q2ab"), true);
  assert.equal(eNuvola("C:\\Musica\\nuvola://finto"), false);
  assert.equal(eNuvola("D:\\Musica"), false);
  assert.equal(eNuvola("\\\\nas\\musica"), false);
  assert.equal(eNuvola(null), false);
  assert.equal(eNuvola(undefined), false);
});

test("il conto e il servizio vengono dal primo segmento", () => {
  const p = "nuvola://gdrive-abcdefgh/Musica/Rock/01.flac";
  assert.equal(contoDi(p), "gdrive-abcdefgh");
  assert.equal(servizioDi(p), "gdrive");
  assert.equal(nomeServizioDi(p), "Google Drive");
  assert.equal(contoDi("nuvola://dropbox-abcdefgh"), "dropbox-abcdefgh");
  assert.equal(contoDi("D:\\Musica"), null);
  assert.equal(servizioDi("D:\\Musica"), null);
  assert.equal(servizioDi(null), null);
});

test("un servizio sconosciuto non è un servizio", () => {
  assert.equal(servizioDelConto("icloud-abcdefgh"), null);
  assert.equal(servizioDelConto("senzatrattino"), null);
  assert.equal(servizioDelConto("-abcdefgh"), null);
  assert.equal(servizioDelConto("megalink-abcdefgh"), "megalink");
});

test("la decodifica scioglie solo le fughe del nucleo", () => {
  assert.equal(decodificaSegmento("AC%2FDC"), "AC/DC");
  assert.equal(decodificaSegmento("a%7Cb%5Cc"), "a|b\\c");
  assert.equal(decodificaSegmento("100%25 Hits"), "100% Hits");
  assert.equal(decodificaSegmento("%2E%2E"), "..");
  // Un `%` che non apre una fuga del nucleo resta com'è.
  assert.equal(decodificaSegmento("100% Hits"), "100% Hits");
  assert.equal(decodificaSegmento("%41"), "%41");
  assert.equal(decodificaSegmento("fine%"), "fine%");
  assert.equal(decodificaSegmento("fine%2"), "fine%2");
});

test("i segmenti dopo il conto, decodificati, senza vuoti", () => {
  assert.deepEqual(segmentiDi("nuvola://onedrive-abcdefgh/Musica/AC%2FDC/"), [
    "Musica",
    "AC/DC",
  ]);
  assert.deepEqual(segmentiDi("nuvola://onedrive-abcdefgh"), []);
  assert.deepEqual(segmentiDi("C:\\Musica"), []);
});

test("nomeRadice: il disco resta com'è, il cloud si scrive per una persona", () => {
  assert.equal(nomeRadice("D:\\Musica"), "D:\\Musica");
  assert.equal(
    nomeRadice("nuvola://onedrive-abcdefgh/Musica/Rock"),
    "OneDrive · Musica › Rock",
  );
  assert.equal(nomeRadice("nuvola://dropbox-abcdefgh"), "Dropbox");
  assert.equal(nomeRadice("nuvola://megalink-abcdefgh/Dischi"), "MEGA · Dischi");
  // Un servizio che questa finestra non conosce: il conto, non lo schema.
  assert.equal(nomeRadice("nuvola://icloud-abcdefgh/Musica"), "icloud-abcdefgh · Musica");
});

test("la prima cartella del disco salta quelle del cloud", () => {
  const cartelle = ["nuvola://onedrive-abcdefgh/Musica", "D:\\Musica", "E:\\Altro"];
  assert.equal(primaLocale(cartelle), "D:\\Musica");
  assert.equal(primaLocale(["nuvola://onedrive-abcdefgh"]), undefined);
  assert.deepEqual(soloLocali(cartelle), ["D:\\Musica", "E:\\Altro"]);
});

test("le icone sono generiche, e MEGA col link sta sotto MEGA", () => {
  assert.equal(iconaServizio("webdav"), "i-server");
  assert.equal(iconaServizio("onedrive"), "i-nuvola");
  assert.equal(iconaServizio(null), "i-folder");
  assert.equal(schedaDi("megalink"), "mega");
  assert.equal(schedaDi("dropbox"), "dropbox");
});
