import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * I dialoghi di consenso, fuori dalle skin.
 *
 * Un dialogo che chiede un sì porta `data-consenso` sulla radice, e lì dentro
 * una skin non ridisegna niente: né livelli, né immagini, né caratteri. Le
 * metà sono due. Il compilatore non emette regole che raggiungano un consenso
 * (`rymembra-skin`, `una_skin_non_raggiunge_un_dialogo_di_consenso`); il
 * carattere invece arriva coi token, e lo ferma il foglio del motore con uno
 * strato che nessun foglio di una skin può battere. Qui si prova quella metà, e
 * che i dialoghi di «Ascolta insieme» e l'esame di una skin portino il segno.
 */
const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

// I fogli nell'ordine in cui `main.tsx` li importa, come in `stile.test.ts`.
const MAIN = sorgente("main.tsx");
const FOGLIO = [...MAIN.matchAll(/^import "\.\/(.+\.css)";\r?$/gm)]
  .map((m) => sorgente(m[1] ?? ""))
  .join("\n")
  .replace(/\r\n/g, "\n")
  // I commenti citano i selettori per spiegarli: una citazione non è una regola.
  .replace(/\/\*[\s\S]*?\*\//g, "");

/** Il corpo del blocco che si apre con `apertura`, graffe bilanciate. */
function corpo(testo: string, apertura: string): string {
  const inizio = testo.indexOf(apertura);
  assert.ok(inizio >= 0, `non trovo «${apertura}»`);
  let i = testo.indexOf("{", inizio + apertura.length - 1);
  const primo = i + 1;
  let profondita = 0;
  for (; i < testo.length; i++) {
    if (testo[i] === "{") profondita++;
    else if (testo[i] === "}") {
      profondita--;
      if (profondita === 0) return testo.slice(primo, i);
    }
  }
  throw new Error(`blocco non chiuso: «${apertura}»`);
}

test("il carattere dentro un consenso è quello di sistema, in uno strato che una skin non batte", () => {
  // Uno strato solo: un `!important` dentro uno strato vince su ogni
  // `!important` fuori, ma fra due strati vince il primo dichiarato.
  const strati = [...FOGLIO.matchAll(/@layer\s+([\w-]+)/g)].map((m) => m[1]);
  assert.deepEqual(strati, ["consenso"]);
  const strato = corpo(FOGLIO, "@layer consenso");
  const regola = corpo(strato, "[data-consenso] *");
  assert.match(strato, /\[data-consenso\],\s*\[data-consenso\] \*\s*\{/);
  assert.match(regola, /font-family:\s*system-ui, sans-serif !important;/);
  // Solo famiglie generiche: un nome proprio è un nome che un `@font-face`
  // potrebbe dichiarare, e nessun `var()`, che è il token della skin.
  for (const pila of strato.matchAll(/font-family:\s*([^;]+);/g)) {
    assert.doesNotMatch(pila[1] ?? "", /var\(|["']/, pila[0]);
  }
});

test("le cifre del codice non vengono dal carattere della skin", () => {
  const codice = corpo(FOGLIO, ".codice-insieme {");
  assert.doesNotMatch(codice, /font-family/);
  assert.match(codice, /font-variant-numeric:\s*tabular-nums/);
});

test("i dialoghi di «Ascolta insieme» sono consensi", () => {
  const insieme = sorgente("parti/Insieme.tsx");
  // Il codice a quattro cifre.
  const codice = insieme.slice(insieme.indexOf("function DialogoCodice"), insieme.indexOf("function SceltaOspita"));
  const dialogo = codice.slice(codice.indexOf("<Dialogo"), codice.indexOf("<Annuncio"));
  assert.match(dialogo, /^\s*consenso$/m);
  // Gli inviti e le richieste d'ingresso, e la richiesta d'associazione con
  // «Mostra il codice» (02-14), nelle due vesti: sopra la pagina e dentro il
  // pannello.
  const attesa = insieme.slice(insieme.indexOf("function InAttesa"), insieme.indexOf("function AvvisoDiPassaggio"));
  assert.equal([...attesa.matchAll(/data-consenso=""/g)].length, 4);
  // E la porta che si apre a chi sta sulla stessa rete.
  const firewall = insieme.slice(insieme.indexOf('t("together.firewall.title")'), insieme.indexOf("ospitaDopoIlFirewall(onErrore)"));
  assert.match(firewall, /consenso: true/);
});

test("l'esame di una skin si legge com'è, anche sotto una skin che non si legge", () => {
  const conferma = sorgente("ConfermaSkin.tsx");
  const esame = conferma.slice(conferma.indexOf("export function ConfermaSkin"), conferma.indexOf("function Versione"));
  const dialogo = esame.slice(esame.indexOf("<Dialogo"), esame.indexOf("<ul"));
  assert.match(dialogo, /^\s*consenso$/m);
});

test("i consensi che arrivano da soli stanno fuori da .finestra-app, e si aprono sopra lo Studio", () => {
  // R13-1 e D-29. «X vuole collegarsi» e il codice di «Ascolta insieme»
  // arrivano anche con lo Studio aperto: montati dentro `.finestra-app`, che
  // lo Studio rende inerte, non si vedevano e congelavano la finestra.
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  const blocco = (apertura: string) => {
    const da = app.indexOf(apertura);
    assert.ok(da >= 0, `non trovo «${apertura}»`);
    // I due contenitori sono figli di `Fornitori`, rientrati di sei spazi:
    // il primo `</div>` allo stesso rientro li chiude.
    return app.slice(da, app.indexOf("\n      </div>", da));
  };
  const finestraApp = blocco('<div className="finestra-app" inert={studioVisibile}>');
  const domande = blocco('<div className="strato-domande">');
  for (const ospite of ["<InsiemeInApp", "<LontanoInApp"]) {
    assert.ok(!finestraApp.includes(ospite), `${ospite} è ancora sotto .finestra-app`);
    assert.ok(domande.includes(ospite), `${ospite} non è nel loro strato`);
  }
  // Lo strato sta sopra lo Studio e sotto i tasti della finestra.
  const valore = (nome: string) => {
    const trovato = new RegExp(String.raw`--strato-${nome}:\s*(\d+);`).exec(FOGLIO);
    assert.ok(trovato, `manca --strato-${nome}`);
    return Number(trovato[1]);
  };
  assert.ok(valore("domande") > valore("studio"), "sotto lo Studio");
  assert.ok(valore("domande") < valore("finestra"), "sopra i tasti della finestra");
  const regola = corpo(FOGLIO, ".strato-domande {");
  assert.match(regola, /position:\s*fixed;/);
  assert.match(regola, /z-index:\s*var\(--strato-domande\);/);
  // Niente `inset`: un contenitore grande quanto lo schermo si mangerebbe i
  // clic; vuoto e fisso, è largo zero e i veli dentro coprono loro.
  assert.doesNotMatch(regola, /inset|width|height/);
});
