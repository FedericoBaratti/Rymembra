import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import { FINESTRA_MAIUSC_MS, SECONDI_PER_TENERE, maiuscAllAvvio, secondiRimasti } from "../src/tieniSkin.ts";

/*
 * Le vie d'uscita da una skin che rende la finestra inservibile (D-48), e le
 * schede delle skin in Impostazioni (R12-4, R16-9).
 *
 * Le regole pure si provano chiamandole; il cablaggio — chi apre la domanda,
 * chi no, che la domanda sia un consenso — leggendo il sorgente, come fanno
 * `consenso.test.ts` e `sostituzioneSkin.test.ts`. La prova dal vivo sta nel
 * report del gruppo F10.
 */
const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

test("Maiusc vale solo tenuto, e solo nei primi secondi", () => {
  assert.equal(maiuscAllAvvio({ key: "Shift", repeat: true }, 200), true);
  assert.equal(maiuscAllAvvio({ key: "Shift", repeat: true }, FINESTRA_MAIUSC_MS), true);
  // Una pressione breve, per una scorciatoia, non ripete.
  assert.equal(maiuscAllAvvio({ key: "Shift", repeat: false }, 200), false);
  // Dopo la finestra d'avvio, Maiusc è Maiusc.
  assert.equal(maiuscAllAvvio({ key: "Shift", repeat: true }, FINESTRA_MAIUSC_MS + 1), false);
  assert.equal(maiuscAllAvvio({ key: "a", repeat: true }, 200), false);
});

test("il conto della domanda va da quindici a zero, e non sotto", () => {
  assert.equal(SECONDI_PER_TENERE, 15);
  const scadenza = 100_000;
  assert.equal(secondiRimasti(scadenza, scadenza - 15_000), 15);
  assert.equal(secondiRimasti(scadenza, scadenza - 13_500), 14);
  assert.equal(secondiRimasti(scadenza, scadenza - 1), 1);
  assert.equal(secondiRimasti(scadenza, scadenza), 0);
  assert.equal(secondiRimasti(scadenza, scadenza + 5_000), 0);
});

test("«Tieni questa skin?» è un consenso, parte da «Torna», ed Escape torna", () => {
  const conferma = sorgente("ConfermaSkin.tsx");
  const tieni = conferma.slice(conferma.indexOf("export function TieniSkin"), conferma.indexOf("export function AvvisoMaiusc"));
  const dialogo = tieni.slice(tieni.indexOf("<Dialogo"), tieni.indexOf("<div", tieni.indexOf("<Dialogo")));
  assert.match(dialogo, /^\s*consenso$/m);
  assert.match(dialogo, /onChiudi=\{torna\}/);
  const tornaBottone = tieni.slice(tieni.indexOf("onClick={torna}"), tieni.indexOf("</button>", tieni.indexOf("onClick={torna}")));
  assert.match(tornaBottone, /data-fuoco-iniziale/);
  // Il tempo scaduto torna indietro da sé.
  assert.match(tieni, /quanti === 0 && prima\(\)\) onTorna\(\)/);
  assert.match(tieni, /SECONDI_PER_TENERE \* 1000/);
});

test("la domanda nasce dai file, e non da «Rimettila» né da Maiusc", () => {
  const skin = sorgente("skinAttiva.ts");
  // Una sola strada apre la domanda: `indossaDaFile`.
  assert.equal([...skin.matchAll(/setDaTenere\(\{/g)].length, 1);
  const daFile = skin.slice(skin.indexOf("const indossaDaFile = useCallback"), skin.indexOf("const dopoInstallazione = useCallback"));
  assert.match(daFile, /setDaTenere\(\{ nome: voce\.nome, ritorno \}\)/);
  // «Rimettila» e il ritorno da una copia indossano con `scegliSkin`, senza domanda.
  const copia = skin.slice(skin.indexOf("const rimettiCopia = useCallback"), skin.indexOf("const rimettiSostituita = useCallback"));
  assert.match(copia, /await scegliSkin\(risposta\.installata\.id\)/);
  assert.doesNotMatch(copia, /indossaDaFile|setDaTenere\(\{/);
  // Maiusc all'avvio sceglie Plain e lo dice; non chiede di tenerla.
  const maiusc = skin.slice(skin.indexOf("const sente = (e: KeyboardEvent)"), skin.indexOf("window.addEventListener(\"keydown\", sente, true)"));
  assert.match(maiusc, /maiuscAllAvvio\(e, trascorsi\)/);
  assert.match(maiusc, /scegliSkinRef\.current\(DI_SERIE\)/);
  assert.match(maiusc, /setTornataAPlain/);
});

test("«Non ora» su una skin che ha sostituito quella indossata rimette la versione di prima", () => {
  // R04b-3: la scelta puntava già all'id nuovo, e al riavvio si indossava la
  // versione mai accettata.
  const skin = sorgente("skinAttiva.ts");
  const nonOra = skin.slice(skin.indexOf("const nonOraEsaminata = useCallback"), skin.indexOf("const tieniSkin = useCallback"));
  assert.match(nonOra, /ritorno\?\.tipo !== "copia"/);
  assert.match(nonOra, /rimettiCopia\(ritorno\.percorso, ritorno\.impronta\)/);
  const conferma = skin.slice(skin.indexOf("const confermaSostituzione = useCallback"), skin.indexOf("const annullaSostituzione"));
  assert.match(conferma, /installata\.id === sceltaAdesso\.current && copia !== undefined/);
  // E `App` passa dall'esame a queste due, non a `scegliSkin`.
  const app = sorgente("App.tsx");
  const esame = app.slice(app.indexOf("<ConfermaSkin"), app.indexOf("/>", app.indexOf("<ConfermaSkin")));
  assert.match(esame, /onChiudi=\{skinFinestra\.nonOraEsaminata\}/);
  assert.match(esame, /onIndossa=\{skinFinestra\.indossaEsaminata\}/);
});

test("«Rimettila» risponde una volta sola, e la domanda di sostituzione ha una chiave sua", () => {
  const conferma = sorgente("ConfermaSkin.tsx");
  const avviso = conferma.slice(conferma.indexOf("export function AvvisoSostituita"), conferma.indexOf("export function TieniSkin"));
  assert.match(avviso, /const \[prima\] = useState\(unaVolta\)/);
  assert.match(avviso, /if \(prima\(\)\) onRimetti\(\)/);
  // R04b-7: col percorso come chiave, la stessa domanda rifatta in fretta
  // riusava la finestrella col cancello già consumato.
  const app = sorgente("App.tsx");
  const domanda = app.slice(app.indexOf("<ConfermaSostituzione"), app.indexOf("/>", app.indexOf("<ConfermaSostituzione")));
  assert.match(domanda, /key=\{chiaveDellApertura\(skinFinestra\.daSostituire\)\}/);
});

test("i nomi delle skin entrano nelle domande isolati col loro verso", () => {
  // R09-12: un nome scritto in un altro verso rovesciava le parole intorno.
  const conferma = sorgente("ConfermaSkin.tsx");
  assert.match(conferma, /t\("skin\.review\.title", \{ nome: isolato\(nome\) \}\)/);
  assert.match(conferma, /t\("skin\.replace\.title", \{ nome: isolato\(domanda\.presente\.nome\) \}\)/);
  assert.match(conferma, /<bdi>\{skin\.nome\}<\/bdi>/);
});

test("la scheda di una skin: anteprima solo col mouse, revocata allo smontaggio, e lo stato si legge", () => {
  const impostazioni = sorgente("schermate/Impostazioni.tsx");
  const scheda = impostazioni.slice(impostazioni.indexOf('className="scheda-skin section-card"'), impostazioni.indexOf('className="nome-skin"'));
  // R12-4: niente `mouseenter` di compatibilità del tocco.
  assert.doesNotMatch(scheda, /onMouseEnter|onMouseLeave/);
  assert.match(scheda, /onPointerEnter=\{\(e\) => \{\s*if \(e\.pointerType !== "mouse"/);
  assert.match(impostazioni, /if \(anteprimaAccesa\.current\) revocaAnteprima\.current\(null\)/);
  // R16-9: il nome viene dal contenuto, e la skin indossata è premuta.
  const bottone = scheda.slice(scheda.indexOf('className="prova"'));
  assert.doesNotMatch(bottone, /aria-label=/);
  assert.match(bottone, /aria-pressed=\{rotta \? undefined : s\.attiva\}/);
});
