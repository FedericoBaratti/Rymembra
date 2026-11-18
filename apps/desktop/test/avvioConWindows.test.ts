import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * «Avvia con Windows» e la proposta del sottofondo, letti come testo.
 *
 * L'interfaccia non ha un DOM nelle prove, e quel che qui conta è il cablaggio:
 * che l'interruttore si disegni da ciò che il sistema **rilegge**, che ogni
 * motivo di impedimento abbia le sue parole in tutte e due le lingue, che i nomi
 * dei comandi siano quelli che il nucleo registra, e che la proposta non
 * compaia durante `Primo` né durante il giro. La verità sul registro e sulla
 * chiusura sta nelle prove di Rust (`rymembra-barra/src/avvio.rs`,
 * `src-tauri/src/avvio_con_windows.rs`).
 */

const leggi = (relativo: string): string =>
  readFileSync(new URL(relativo, import.meta.url), "utf8");

const IMPOSTAZIONI = leggi("../src/schermate/Impostazioni.tsx");
const IPC = leggi("../src/ipc.ts");
const TIPI = leggi("../src/ipc/sottofondo.ts");
const PROPOSTA = leggi("../src/parti/PropostaSottofondo.tsx");
const APP = leggi("../src/App.tsx");
const LIB = leggi("../src-tauri/src/lib.rs");
const IT = JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>;
const EN = JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>;

/** Il corpo di una funzione di primo livello, dalla firma alla graffa in prima colonna. */
function corpo(sorgente: string, firma: string): string {
  const da = sorgente.indexOf(firma);
  assert.ok(da >= 0, `nel sorgente manca «${firma}»`);
  const resto = sorgente.slice(da);
  const fine = resto.indexOf("\n}\n");
  return fine < 0 ? resto : resto.slice(0, fine + 3);
}

test("l'interruttore dell'avvio si disegna da ciò che il sistema rilegge", () => {
  const scheda = corpo(IMPOSTAZIONI, "function SchedaChiusura(");
  // Lo stato arriva dal ritorno del comando, e il clic non lo cambia da sé.
  assert.match(scheda, /ipc\.avvioConWindowsImposta\(valore\)\.then\(setAvvio\)/);
  assert.match(scheda, /acceso=\{avvio\.acceso\}/);
  // Si disegna solo dopo aver letto: un interruttore che cambia dopo un attimo è un avvio che sembra cambiato.
  assert.match(scheda, /avvio !== null && \(/);
  // Quando non si può toccare lo dice, invece di spegnersi in silenzio.
  assert.match(scheda, /impedito=\{motivoDelloStartup\(avvio\.impedito\)\}/);
  // E l'altra metà della scheda, la X, resta com'era.
  assert.match(scheda, /ipc\.secondoPianoAttiva\(valore\)\.then\(setAcceso\)/);
});

test("ogni impedimento dell'avvio ha le sue parole, in italiano e in inglese", () => {
  const impedimenti = [...TIPI.matchAll(/"(\w+)"/g)].map((m) => m[1]!);
  assert.deepEqual(impedimenti.slice(0, 4), ["sistema", "criterio", "prova", "nonDisponibile"]);
  const funzione = corpo(IMPOSTAZIONI, "function motivoDelloStartup(");
  const chiaviPerImpedimento: Record<string, string> = {
    sistema: "settings.startup.blocked.system",
    criterio: "settings.startup.blocked.policy",
    prova: "settings.startup.blocked.test",
    nonDisponibile: "settings.startup.blocked.unavailable",
  };
  for (const impedimento of impedimenti.slice(0, 4)) {
    const chiave = chiaviPerImpedimento[impedimento];
    assert.ok(chiave, `l'impedimento «${impedimento}» non ha una chiave in questa prova`);
    assert.ok(
      funzione.includes(`case "${impedimento}":`) && funzione.includes(`t("${chiave}")`),
      `«${impedimento}» non arriva a «${chiave}»`,
    );
    assert.ok(IT[chiave] && EN[chiave], `«${chiave}» manca in una delle lingue`);
  }
  // `null` è «si può»: niente motivo.
  assert.match(funzione, /case null:\s+return undefined;/);
});

test("l'avvio con Windows è nell'indice delle Impostazioni, e non sul telefono", () => {
  assert.match(
    IMPOSTAZIONI,
    /\.\.\.\(MOBILE \? \[\] : \[v\("chiusura", "startup", "settings\.background\.title"\)\]\)/,
  );
  for (const lingua of [IT, EN]) {
    assert.ok(lingua["settings.entry.startup"], "manca il titolo della voce");
    assert.ok(lingua["settings.entry.startup.syn"], "mancano i sinonimi della voce");
    assert.ok(lingua["settings.startup.toggle"], "manca l'etichetta dell'interruttore");
  }
});

test("i comandi dell'avvio sono quelli che il nucleo registra", () => {
  for (const [metodo, comando] of [
    ["avvioConWindowsStato", "avvio_con_windows_stato"],
    ["avvioConWindowsImposta", "avvio_con_windows_imposta"],
    ["propostaSottofondoDaFare", "proposta_sottofondo_da_fare"],
    ["propostaSottofondoRispondi", "proposta_sottofondo_rispondi"],
  ] as const) {
    assert.ok(IPC.includes(`${metodo}:`), `ipc.ts non ha ${metodo}`);
    assert.ok(IPC.includes(`"${comando}"`), `ipc.ts non chiama «${comando}»`);
    assert.ok(LIB.includes(`avvio_con_windows::${comando},`), `lib.rs non registra «${comando}»`);
  }
});

test("le parole della X dicono che la finestra si chiude e il programma resta", () => {
  // Con il vassoio la X distrugge la finestra: «nasconde» non è più vero.
  for (const lingua of [IT, EN]) {
    assert.ok(!/nasconde|hides/i.test(lingua["settings.background.toggle"] ?? ""));
  }
});

test("la proposta non compare durante Primo né durante il giro", () => {
  const riga = APP.match(/const propostaPuoMostrarsi = ([^;]+);/)?.[1] ?? "";
  for (const motivo of ["primoVisibile", "giroDaFare", "giroAperto"]) {
    assert.ok(riga.includes(`!${motivo}`), `la proposta non aspetta «${motivo}»`);
  }
  // Una volta sola, sul computer e sul telefono: il componente ha le parole di
  // tutti e due (F7), e il punto di montaggio non li distingue.
  assert.match(APP, /<PropostaSottofondo\s+pronta=\{propostaPuoMostrarsi\}/);
  assert.doesNotMatch(APP, /\{!MOBILE && \(\s*<PropostaSottofondo/);
  // Il punto di montaggio sta nella pila delle notifiche, come le altre schede.
  const pila = APP.slice(APP.indexOf("<PilaToast"), APP.indexOf("</PilaToast>"));
  assert.ok(pila.includes("<PropostaSottofondo"));
});

test("la proposta ha due risposte e nessuna via d'uscita fra le due", () => {
  assert.match(PROPOSTA, /propostaSottofondoRispondi\(accetta\)/);
  assert.match(PROPOSTA, /risponde\(true\)/);
  assert.match(PROPOSTA, /risponde\(false\)/);
  // Niente tasto «chiudi»: lasciarla lì non è una risposta.
  assert.ok(!PROPOSTA.includes('"common.close"'));
  // Si decide quando mostrarla solo dopo `pronta`, e il nucleo dice se c'è da farla.
  assert.match(PROPOSTA, /if \(!pronta\) return;/);
  assert.match(PROPOSTA, /propostaSottofondoDaFare\(\)/);
  // I testi del computer hanno le loro chiavi; quelle del telefono, sotto
  // `background.proposal.phone.*`, le prova `sottofondoTelefono.test.ts`.
  for (const chiave of [
    "background.proposal.desktop.text",
    "background.proposal.desktop.yes",
    "background.proposal.desktop.no",
    "background.proposal.desktop.partial",
  ]) {
    assert.ok(PROPOSTA.includes(`"${chiave}"`), `il componente non usa «${chiave}»`);
    assert.ok(IT[chiave] && EN[chiave], `«${chiave}» manca in una delle lingue`);
  }
});

test("il testo della proposta è quello che la guida promette", () => {
  assert.equal(
    IT["background.proposal.desktop.text"],
    "Vuoi che Rymembra parta con Windows e resti nel vassoio quando la chiudi?",
  );
  assert.equal(IT["background.proposal.desktop.no"], "No, grazie");
});

test("le parole inglesi dell'avvio usano le virgolette e l'apostrofo tipografici", () => {
  for (const [chiave, testo] of Object.entries(EN)) {
    if (!chiave.startsWith("settings.startup.") && !chiave.startsWith("background.proposal.")) continue;
    assert.ok(!testo.includes("'"), `${chiave}: apostrofo dritto in inglese`);
  }
});
