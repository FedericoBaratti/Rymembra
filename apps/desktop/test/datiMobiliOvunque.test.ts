/**
 * «Dati mobili: Chiedi» si chiede da **ogni** gesto della pagina che apre una
 * riga solo remota (S11-4).
 *
 * Il sì lo chiedeva la finestrella (`LontanoInApp`) solo quando il gesto passava
 * a mano da `conIlSiDeiDatiMobili`, e a farlo erano tre punti di `App.tsx`
 * (`suonaDa`, `suonaDaUnaLista`, `accodaRighe`). Gli altri — il play, avanti e
 * indietro dai controlli e dalla tastiera, la coda, la radio, la cronologia, le
 * Cartelle — lasciavano salire `lontano.reteAConsumo` a `segnalaErrore`, e la
 * frase diceva «Solo Wi-Fi è attivo» a chi aveva scelto «Chiedi». La regola sta
 * dove passano tutti, `ipc.ts`, non in venti chiamanti.
 *
 * Senza un DOM: si prova il cablaggio sul sorgente, e le regole pure stanno in
 * `lontano.test.ts`.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";

const sorgenti = new URL("../src/", import.meta.url);
const leggi = (percorso: string) =>
  readFileSync(new URL(percorso, sorgenti), "utf8").replace(/\r\n/g, "\n");

const IPC = leggi("ipc.ts");

/** La definizione di un comando nell'oggetto `ipc`, fino alla virgola che la chiude. */
function comando(nome: string): string {
  const inizio = IPC.indexOf(`\n  ${nome}: `);
  assert.ok(inizio >= 0, `manca ${nome}`);
  // La prossima riga a due spazi di rientro (un altro comando o un commento).
  const dopo = IPC.slice(inizio + 4).search(/\n {2}[^ \n]/);
  return IPC.slice(inizio, dopo < 0 ? undefined : inizio + 4 + dopo);
}

test("S11-4: ogni comando che può aprire una riga remota passa dal sì ai dati mobili", () => {
  for (const nome of ["suona", "radio", "riprendi", "alterna", "prossimo", "precedente", "codaVai"]) {
    assert.match(comando(nome), /conIlSiDeiDatiMobili\(/, `ipc.${nome} aspetta il sì invece di fallire`);
  }
  assert.match(IPC, /import \{ conIlSiDeiDatiMobili \} from "\.\/lontano";/);
});

test("S11-4: avanti, indietro e play/pausa, dopo il sì, riprendono il brano raggiunto", () => {
  // `prossimo` e `precedente` hanno già spostato la coda sul brano che non si
  // apre, e il motore è fermo (`avvio_fallito`): rifarli lo salterebbe. `alterna`
  // dopo un fallimento è ferma, e rifarla metterebbe in pausa. Si rifà `riprendi`.
  for (const nome of ["prossimo", "precedente", "alterna"]) {
    assert.match(
      comando(nome),
      /,\s*\(\) => invoke<void>\("riprendi"\)/,
      `ipc.${nome}: dopo il sì parte «riprendi», non lo stesso gesto`,
    );
  }
  // Gli altri sono ripetibili tali e quali: sostituiscono la coda, vanno a un
  // posto, o (`riprendi`) sono già quel gesto.
  for (const nome of ["suona", "radio", "codaVai"]) {
    assert.doesNotMatch(comando(nome), /riprendi/, nome);
  }
  assert.doesNotMatch(comando("riprendi"), /conIlSiDeiDatiMobili\([^)]*\)\s*,/);
});

/** Tutti i file `.ts` e `.tsx` sotto `src`, tranne `ipc.ts` e `src/ipc/`. */
function sorgentiDellaPagina(cartella = ""): string[] {
  const radice = new URL(cartella, sorgenti);
  const fuori: string[] = [];
  for (const nome of readdirSync(radice)) {
    const percorso = join(cartella, nome).replace(/\\/g, "/");
    if (statSync(new URL(percorso, sorgenti)).isDirectory()) {
      if (percorso === "ipc") continue;
      fuori.push(...sorgentiDellaPagina(percorso + "/"));
    } else if (/\.tsx?$/.test(nome) && percorso !== "ipc.ts") {
      fuori.push(percorso);
    }
  }
  return fuori;
}

test("S11-4: nessuna pagina chiama a mano i comandi del trasporto, senza passare da `ipc`", () => {
  // Il confine con il nucleo è `ipc.ts`: un `invoke` scritto altrove non avrebbe
  // il sì. (`pausa`, `vai_a`, `volume` non aprono niente.)
  const cadute: string[] = [];
  for (const file of sorgentiDellaPagina()) {
    const testo = leggi(file);
    for (const m of testo.matchAll(/invoke(?:<[^>]*>)?\(\s*"(suona|radio|riprendi|alterna|prossimo|precedente|coda_vai)"/g)) {
      cadute.push(`${file}: invoke("${m[1] ?? ""}")`);
    }
  }
  assert.deepEqual(cadute, []);
});

test("S11-4: i punti di `App` che chiedevano il sì a mano non lo chiedono due volte", () => {
  // `suonaDa` e `suonaDaUnaLista` avvolgevano `ipc.suona`: adesso lo fa `ipc`.
  // Resta solo `accodaRighe`, che non passa dai comandi del trasporto ma da
  // `lontanoScaricaRighe`.
  const app = leggi("App.tsx");
  assert.doesNotMatch(app, /conIlSiDeiDatiMobili\(\(\) => ipc\.suona\(/);
  assert.equal([...app.matchAll(/conIlSiDeiDatiMobili\(/g)].length, 1, "solo `accodaRighe`");
});

test("S11-4: i punti che dicono l'errore dei dati mobili scelgono la frase per «Chiedi»", () => {
  // Il codice è lo stesso, la frase dipende da quel che ha scelto chi legge.
  // `testoErrore` è la strada di ogni errore che arriva a una fascia o a un
  // avviso; `riproduzione:saltato` costruisce la sua frase dal codice, e da lì
  // passano i gesti che la finestra non vede (la notifica, i tasti, l'auto).
  assert.match(leggi("ipc/errori.ts"), /tSe\(chiaveDelTestoErrore\(value\.i18nKey\)/);
  assert.match(leggi("App.tsx"), /tSe\(chiaveDelTestoErrore\(`errors\.\$\{codice\}`\), ""\)/);
});

/**
 * AVD-2: dopo «Solo Wi-Fi» la fascia diceva ancora «premi play e ti chiederà se usare
 * i dati mobili», e la scelta ormai era «no».
 *
 * La frase si sceglieva una volta, alla nascita del guasto (`guastoDa` ne teneva il
 * testo), e `App` non si ridisegnava quando cambiava l'impostazione: la sua
 * iscrizione a `archivioLontano` guarda solo le chiavi della libreria.
 */
test("AVD-2: la frase dei dati mobili si sceglie sull'impostazione di adesso, non su quella di quando è nata la fascia", async () => {
  const { chiaveSecondoIDatiMobili } = await import("../src/lontano");
  const chiave = "errors.lontano.reteAConsumo";
  // La fascia nasce con «Chiedi»; chi legge sceglie «Solo Wi-Fi» dalla finestrella;
  // la frase che si vede da lì in poi è quella di «Solo Wi-Fi».
  assert.equal(chiaveSecondoIDatiMobili(chiave, "chiedi"), `${chiave}.chiedi`);
  assert.equal(chiaveSecondoIDatiMobili(chiave, "no"), chiave);
  // E con «Sì», o senza sapere, la frase di sempre. Gli altri errori non cambiano mai.
  assert.equal(chiaveSecondoIDatiMobili(chiave, "si"), chiave);
  assert.equal(chiaveSecondoIDatiMobili(chiave, null), chiave);
  assert.equal(chiaveSecondoIDatiMobili("errors.lontano.spento", "chiedi"), "errors.lontano.spento");
});

test("AVD-2: la fascia rilegge la frase quando cambia l'impostazione, in tutti i posti che la mostrano", () => {
  const errori = leggi("ipc/errori.ts");
  // Il guasto si porta dietro la chiave e i dati, non solo il testo già composto.
  assert.match(errori, /chiave: eErroreIpc\(value\) \? value\.i18nKey : null/);
  assert.match(errori, /export function guastoAdesso\(/);
  const corpo = errori.slice(errori.indexOf("export function guastoAdesso("));
  assert.match(corpo.slice(0, corpo.indexOf("\n}\n")), /chiaveSecondoIDatiMobili\(/);
  // `App` si ridisegna quando cambia `datiMobili` e il guasto — quello della fascia
  // e quello del pannello di «Ascolta insieme», che è lo stesso — passa da lì.
  const app = leggi("App.tsx");
  assert.match(app, /useArchivio\(archivioLontano, \(stato\) => stato\?\.datiMobili \?\? null\)/);
  assert.match(app, /const guasto = guastoAdesso\(errore \?\? guastoDa\(riproduzione\.errore\), datiMobili\);/);
});
