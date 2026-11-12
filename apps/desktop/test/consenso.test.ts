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
  // Gli inviti e le richieste d'ingresso, nelle due vesti: sopra la pagina e
  // dentro il pannello.
  const attesa = insieme.slice(insieme.indexOf("function InAttesa"), insieme.indexOf("function AvvisoDiPassaggio"));
  assert.equal([...attesa.matchAll(/data-consenso=""/g)].length, 2);
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

/** Il valore numerico di un `--strato-*` dichiarato in `stile.css`. */
function strato(nome: string): number {
  const trovato = FOGLIO.match(new RegExp(`--strato-${nome}:\\s*(\\d+);`));
  assert.ok(trovato, `manca --strato-${nome}`);
  return Number(trovato[1]);
}

test("il velo di un consenso sta sopra toast e menù, sotto la pastiglia e il giro", () => {
  // Il recinto dei consensi copre il loro sottoalbero, non lo schermo: un
  // toast (50) o un menù (41) che la skin gonfia stavano sopra il velo (40),
  // fuori dalle guardie del compilatore, e potevano coprire i tasti della
  // domanda (R04b-2, D-51).
  const regola = corpo(FOGLIO, ".velo:has(> [data-consenso]) {");
  assert.match(regola, /z-index:\s*var\(--strato-consenso\);/);
  const consenso = strato("consenso");
  for (const sotto of ["velo", "menu", "toast", "primo", "sopraprimo"]) {
    assert.ok(consenso > strato(sotto), `--strato-consenso non sta sopra --strato-${sotto}`);
  }
  for (const sopra of ["pastiglia", "giro", "studio", "finestra"]) {
    assert.ok(consenso < strato(sopra), `--strato-consenso non sta sotto --strato-${sopra}`);
  }
});

test("rimozioni, trasloco, ripristino e importazione del profilo sono consensi", () => {
  // Quattro domande che tolgono o riscrivono qualcosa: una skin le poteva
  // ancora ridisegnare, con un livello `::after` su `glass-modal` o i caratteri
  // del pacchetto (D-51).
  for (const file of ["ConfermaRimozioni.tsx", "ConfermaTrasloco.tsx"]) {
    const testo = sorgente(file);
    const dialogo = testo.slice(testo.indexOf("<Dialogo"), testo.indexOf(">", testo.indexOf("onChiudi=", testo.indexOf("<Dialogo"))));
    assert.match(dialogo, /^\s*consenso$/m, file);
  }
  const ripristino = sorgente("Ripristino.tsx");
  const finestrella = ripristino.slice(ripristino.indexOf('className="finestrella larga glass-modal"'), ripristino.indexOf("<h2>"));
  assert.match(finestrella, /data-consenso=""/);
  const profilo = sorgente("parti/Profilo.tsx");
  const piano = profilo.slice(profilo.indexOf("{daApplicare && piano && ("), profilo.indexOf("profile.writtenOn"));
  assert.match(piano, /data-consenso=""/);
});
