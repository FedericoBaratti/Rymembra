/**
 * Le lingue si caricano a richiesta (R21-4).
 *
 * `import.meta.glob("./*.json", { eager: true })` e l'import statico di `it.json`
 * mettevano i due dizionari nel pacchetto principale: ≈454 KB su 1 288 KB, il
 * 35 %, analizzati e costruiti a ogni avvio da WebView2 e dalla WebView di
 * Android — e ogni lingua aggiunta ne avrebbe allungato l'avvio. Se ne usa una
 * per volta: ora il pacchetto porta il motore, e la lingua in uso si carica
 * prima del primo disegno; l'altra quando serve (il cambio di lingua, o una
 * chiave che manca in una lingua incompleta).
 *
 * `lingue/index.ts` si porta dietro `import.meta.glob` e `node --test` non lo
 * carica: la parte che decide — caricare una volta sola, scordare un guasto, far
 * vincere l'ultima richiesta — sta in `lingue/caricamento.ts`, pura, e il resto
 * si legge dai sorgenti.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import * as caricamento from "../src/lingue/caricamento";

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

type Dizionario = Record<string, string>;

/** Un file di lingua finto: quante volte lo si è letto, e quando risponde. */
function lingua(testo: string) {
  let letture = 0;
  let risolvi: (d: Dizionario) => void = () => undefined;
  let rifiuta: (e: Error) => void = () => undefined;
  return {
    get letture() {
      return letture;
    },
    carica: () => {
      letture += 1;
      return new Promise<Dizionario>((ok, no) => {
        risolvi = ok;
        rifiuta = no;
      });
    },
    arriva: () => risolvi({ chiave: testo }),
    fallisce: () => rifiuta(new Error("manca il file")),
  };
}

test("una lingua si carica a richiesta, una volta sola, anche chiesta due volte (R21-4)", async () => {
  assert.equal(typeof caricamento.creaCaricatore, "function", "manca creaCaricatore");
  const it = lingua("italiano");
  const en = lingua("inglese");
  const caricatore = caricamento.creaCaricatore({ it: it.carica, en: en.carica });

  assert.equal(it.letture + en.letture, 0, "creare il caricatore non legge niente");
  assert.equal(caricatore.caricato("it"), undefined);

  const prima = caricatore.carica("it");
  const seconda = caricatore.carica("it");
  it.arriva();
  assert.deepEqual(await prima, { chiave: "italiano" });
  assert.equal(await seconda, await prima);
  assert.equal(it.letture, 1, "due richieste in volo, una lettura");
  assert.deepEqual(caricatore.caricato("it"), { chiave: "italiano" });

  await caricatore.carica("it");
  assert.equal(it.letture, 1, "già caricata: non si rilegge");
  assert.equal(en.letture, 0, "l'altra lingua non si è toccata: è quel che si risparmia all'avvio");
  assert.equal(caricatore.caricato("en"), undefined);
});

test("un guasto non si ricorda: la richiesta dopo riprova (R21-4)", async () => {
  const it = lingua("italiano");
  const caricatore = caricamento.creaCaricatore({ it: it.carica });
  const fallita = caricatore.carica("it");
  it.fallisce();
  await assert.rejects(fallita, /manca il file/);
  assert.equal(caricatore.caricato("it"), undefined, "niente dizionario a metà");
  const riprova = caricatore.carica("it");
  it.arriva();
  assert.deepEqual(await riprova, { chiave: "italiano" });
  assert.equal(it.letture, 2);
});

test("una lingua senza file si rifiuta, non si inventa (R21-4)", async () => {
  const caricatore = caricamento.creaCaricatore({});
  await assert.rejects(caricatore.carica("de"), /de/);
  // Nemmeno i nomi che un oggetto ha di suo.
  await assert.rejects(caricatore.carica("constructor"), /constructor/);
});

test("cambiando lingua a caldo, si carica e poi si applica; vince l'ultima richiesta (R21-4)", async () => {
  assert.equal(typeof caricamento.creaCambioLingua, "function", "manca creaCambioLingua");
  const it = lingua("italiano");
  const en = lingua("inglese");
  const caricatore = caricamento.creaCaricatore({ it: it.carica, en: en.carica });
  const applicate: string[] = [];
  const imposta = caricamento.creaCambioLingua(caricatore, (codice) => {
    // Quando si applica, il dizionario c'è già: il testo non passa mai per le chiavi nude.
    assert.ok(caricatore.caricato(codice) !== undefined, `«${codice}» applicata prima di essere caricata`);
    applicate.push(codice);
  });

  // L'inglese è lenta ad arrivare, l'italiano no: chi ha premuto per ultimo è l'italiano.
  const aInglese = imposta("en");
  const aItaliano = imposta("it");
  it.arriva();
  await aItaliano;
  en.arriva();
  await aInglese;
  assert.deepEqual(applicate, ["it"], "la risposta vecchia non rimette la lingua di prima");

});

test("un cambio di lingua che non si carica arriva a chi ha chiesto, e la lingua resta com'era (R21-4)", async () => {
  const tedesco = lingua("deutsch");
  const caricatore = caricamento.creaCaricatore({ de: tedesco.carica });
  const applicate: string[] = [];
  const imposta = caricamento.creaCambioLingua(caricatore, (codice) => {
    applicate.push(codice);
  });
  const fallita = imposta("de");
  tedesco.fallisce();
  await assert.rejects(fallita, /manca il file/);
  assert.deepEqual(applicate, [], "una lingua che non si carica non si applica");
});

test("il pacchetto principale non porta i dizionari: niente `eager`, `it.json` solo come tipo (cablaggio, R21-4)", () => {
  const indice = sorgente("lingue/index.ts");
  assert.doesNotMatch(indice, /eager:\s*true/, "i dizionari non si caricano in anticipo");
  assert.doesNotMatch(indice, /^import it from ["']\.\/it\.json["']/m, "`it.json` non si importa come valore");
  assert.match(indice, /^import type it from ["']\.\/it\.json["']/m, "il tipo `Chiave` resta quello di `it.json`");
  assert.match(indice, /import\.meta\.glob\(["']\.\/\*\.json["']/, "l'elenco resta il contenuto della cartella");
  // I nomi delle lingue non si leggono dai dizionari: li dà il modulo virtuale, fatto a build time.
  assert.match(indice, /from ["']virtual:lingue-nomi["']/);
  const vite = sorgente("../vite.config.ts");
  assert.match(vite, /virtual:lingue-nomi/);
  assert.match(vite, /_nome/);
});

test("la lingua si carica prima del primo disegno, e la finestra non si mostra prima della lingua salvata (cablaggio, R21-4)", () => {
  const principale = sorgente("main.tsx");
  const prima = principale.indexOf("await impostaLinguaIniziale(");
  const disegno = principale.indexOf("createRoot(");
  assert.ok(prima >= 0, "main.tsx aspetta la lingua di sistema");
  assert.ok(prima < disegno, "...prima di disegnare");

  const aspetto = sorgente("aspettoFinestra.ts");
  assert.match(aspetto, /impostaLingua\(scegli\(avvio\.lingua/, "la lingua salvata si carica e poi si applica");
  const mostrata = aspetto.slice(aspetto.indexOf("const mostrata = useRef"));
  const gate = mostrata.slice(0, mostrata.indexOf("ipc.pronto()"));
  assert.match(gate, /linguaPronta/, "la finestra aspetta anche la lingua salvata: niente fotogramma nella lingua sbagliata");
  assert.doesNotMatch(aspetto, /\bapplicaLingua\(/, "il cambio a caldo carica e poi applica: non si applica un dizionario che non c'è");
});

test("il primo disegno parte dall'ultima lingua applicata, non solo da quella del sistema (cablaggio, L2-1)", () => {
  // Sul telefono la WebView si vede da subito: un telefono in inglese con
  // Rymembra in italiano mostrava qualche decimo di secondo d'inglese prima che
  // `avvio()` portasse la lingua salvata (collaudo sugli emulatori, 16/11/2026).
  const principale = sorgente("main.tsx");
  assert.match(principale, /await impostaLinguaIniziale\(navigator\.language, linguaRicordata\(\)\)/);
  const indice = sorgente("lingue/index.ts");
  assert.match(indice, /export async function impostaLinguaIniziale\(\s*sistema: string,\s*ricordata: string \| null = null,?\s*\)/);
  assert.match(indice, /const scelta = scegli\(ricordata, sistema\);/, "una lingua ricordata senza file ricade sul sistema");
  // Si ricorda a ogni lingua applicata: quella del database, che `avvio()`
  // applica subito dopo, resta la vera.
  const applica = indice.slice(indice.indexOf("export function applicaLingua("), indice.indexOf("export const impostaLingua"));
  assert.match(applica, /ricordaLaLingua\(codice\);/);
  assert.match(indice, /globalThis\.localStorage\?\.setItem\(CHIAVE_RICORDATA, codice\)/);
});
