/**
 * L'APK che Android non installerebbe: la fascia non lo scarica, e dice come
 * passare alla versione pubblica.
 *
 * Le copie di Rymembra per telefono uscite finora sono firmate con la chiave di
 * prova, e l'APK pubblico con quella di rilascio: Android non installa l'uno
 * sopra l'altro. «Aggiorna» scaricava decine di megabyte, l'installatore li
 * rifiutava con «Riavvia Rymembra», e ogni tocco successivo li riscaricava
 * (R03-2, R11-2). Il nucleo adesso lo sa prima — o lo ricorda dopo il primo
 * rifiuto — e lo dice con `altraChiave` (D-5); qui si controlla che la fascia
 * ne tenga conto e che il testo dica il rimedio vero.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
const it = JSON.parse(leggi("../src/lingue/it.json")) as Record<string, string>;
const en = JSON.parse(leggi("../src/lingue/en.json")) as Record<string, string>;

test("con un APK firmato con un'altra chiave la fascia non offre «Aggiorna»", () => {
  const fascia = leggi("../src/parti/Aggiornamenti.tsx");
  const tasto = fascia.indexOf('{t("update.install")}');
  assert.ok(tasto >= 0, "il tasto «Aggiorna» non c'è più");
  // Il tasto sta dentro il ramo che lo esclude per l'altra chiave.
  const prima = fascia.slice(Math.max(0, tasto - 300), tasto);
  assert.match(prima, /\{!altraChiave && \(/, "«Aggiorna» resta anche per un APK che Android rifiuterebbe");
  // E al suo posto c'è la spiegazione.
  assert.match(fascia, /t\("update\.otherKey\.steps"/);
});

test("il testo dell'altra chiave dice come passare alla versione pubblica, non «Riavvia»", () => {
  for (const [lingua, testi, esporta, leggiProfilo] of [
    ["it", it, it["profile.export"], it["profile.read"]],
    ["en", en, en["profile.export"], en["profile.read"]],
  ] as const) {
    const passi = testi["update.otherKey.steps"];
    assert.ok(passi, `${lingua}: manca update.otherKey.steps`);
    assert.ok(testi["update.otherKey"], `${lingua}: manca update.otherKey`);
    // I nomi dei due tasti, così come sono scritti nell'interfaccia.
    assert.ok(esporta && passi.includes(esporta), `${lingua}: non nomina «${esporta}»`);
    assert.ok(leggiProfilo && passi.includes(leggiProfilo), `${lingua}: non nomina «${leggiProfilo}»`);
    assert.doesNotMatch(passi, /riavvia|restart/i, `${lingua}: promette che un riavvio basti`);
    assert.match(testi["update.otherKey"], /\{v\}/, `${lingua}: non dice quale versione`);
  }
});

// ── la notifica «Pronto da installare» (§11.2.4) ────────────────────────────
//
// Il Kotlin non ha prove unitarie, e `mobile/aggiornamenti.rs` si compila solo
// per Android: si guarda il cablaggio nei sorgenti. La prova vera — la
// notifica sull'emulatore, con un debug — sta nella procedura del report.

const KOTLIN = "../src-tauri/gen/android/app/src/main/java/io/github/federicobaratti/rymembra/";
const RES = "../src-tauri/gen/android/app/src/main/res/";
/** Un sorgente senza commenti: le ragioni citano parole che il codice non deve usare. */
const senzaCommenti = (percorso: string) => leggi(percorso).replace(/\/\/.*$|\/\*[\s\S]*?\*\//gm, "");
/** Il corpo della funzione Rust `nome`, dalla firma alla graffa che la chiude. */
function corpoRust(sorgente: string, nome: string): string {
  const inizio = sorgente.indexOf(`fn ${nome}(`);
  assert.ok(inizio >= 0, `manca fn ${nome}`);
  let profondita = 0;
  for (let i = sorgente.indexOf("{", inizio); i < sorgente.length; i++) {
    if (sorgente[i] === "{") profondita++;
    else if (sorgente[i] === "}" && --profondita === 0) return sorgente.slice(inizio, i + 1);
  }
  return sorgente.slice(inizio);
}

test("la notifica «pronto da installare» ha il suo canale a bassa importanza e riapre l'app", () => {
  const notifica = senzaCommenti(`${KOTLIN}AggiornamentoPronto.kt`);
  assert.match(notifica, /NotificationChannel\(\s*CANALE,[\s\S]*?NotificationManager\.IMPORTANCE_LOW/);
  assert.match(notifica, /private const val CANALE = "aggiornamento"/);
  // Il tocco e il tasto aprono la stessa attività, con un `PendingIntent` non modificabile.
  assert.match(notifica, /PendingIntent\.getActivity\(\s*contesto,\s*CODICE,\s*Intent\(contesto, MainActivity::class\.java\)/);
  assert.match(notifica, /PendingIntent\.FLAG_IMMUTABLE/);
  assert.match(notifica, /\.setContentIntent\(apri\)/);
  assert.match(notifica, /\.addAction\(/);
  assert.match(notifica, /\.setAutoCancel\(true\)/);
  // Il canale e il codice non sono quelli delle altre notifiche.
  for (const altro of ["ConsensoInAttesa.kt", "ScaricoInCorso.kt", "Presenza.kt"]) {
    const sorgente = senzaCommenti(`${KOTLIN}${altro}`);
    assert.doesNotMatch(sorgente, /private const val ID = 7434/, `${altro}: stesso id`);
    assert.doesNotMatch(sorgente, /"aggiornamento"/, `${altro}: stesso canale`);
  }
  assert.match(notifica, /private const val ID = 7434/);
});

test("la notifica non chiede le notifiche, rispetta chi le ha spente, e sulla distribuzione play non c'è", () => {
  const notifica = senzaCommenti(`${KOTLIN}AggiornamentoPronto.kt`);
  // Niente richiesta nuova: «Scarica qui» e il sottofondo chiedono, questa no.
  assert.doesNotMatch(notifica, /notificheAlBisogno|requestPermissions|POST_NOTIFICATIONS/);
  assert.match(notifica, /NotificationManagerCompat\.from\(contesto\)\.areNotificationsEnabled\(\)/);
  // Solo l'aggiornatore github: lo stesso controllo dell'installatore.
  assert.match(notifica, /Installatore\.soloFuoriDalPlay\(\)/);
  assert.match(senzaCommenti(`${KOTLIN}Installatore.kt`), /internal fun soloFuoriDalPlay\(\)/);
  // Con la pagina in vista la scheda è già lì: niente notifica.
  assert.match(notifica, /if \(Ponte\.inVista\) return/);
  // E il permesso è già dichiarato, una volta sola.
  const manifesto = leggi(`../src-tauri/gen/android/app/src/main/AndroidManifest.xml`);
  assert.equal(manifesto.match(/android\.permission\.POST_NOTIFICATIONS/g)?.length, 1);
});

test("Kotlin accende la notifica dal ponte, e la toglie quando l'app torna in vista", () => {
  const ponte = senzaCommenti(`${KOTLIN}Ponte.kt`);
  assert.match(
    ponte,
    /"sistema\.aggiornamentoPronto" ->\s*AggiornamentoPronto\.imposta\(contesto\(\), a\.getBoolean\("acceso"\), a\.optString\("versione"\)\)/,
  );
  const attivita = senzaCommenti(`${KOTLIN}MainActivity.kt`);
  const onStart = attivita.slice(attivita.indexOf("override fun onStart()"));
  assert.match(onStart.slice(0, 300), /AggiornamentoPronto\.togli\(this\)/);
});

test("Rust accende la notifica col pacchetto verificato, e la spegne ovunque lo butta", () => {
  const sistema = leggi("../rymembra-android/src/sistema.rs");
  assert.match(sistema, /pub fn aggiornamento_pronto\(acceso: bool, versione: &str\) -> Result<\(\), Errore>/);
  assert.ok(sistema.includes('"sistema.aggiornamentoPronto"'));
  const mobile = leggi("../src-tauri/src/mobile/aggiornamenti.rs");
  // Accesa dopo `tieni_per_installa`, solo col certificato buono.
  const lavora = corpoRust(mobile, "lavora");
  const dopoTieni = lavora.slice(lavora.indexOf("tieni_per_installa("));
  assert.match(dopoTieni.slice(0, 400), /notifica_pronto\(true, &trovato\.versione\)/);
  // Spenta da `scarta_pronto_se`, l'unico posto che butta il pacchetto: la strada di
  // «Non installare», «Non ora», dell'altra chiave e dell'errore…
  assert.match(corpoRust(mobile, "scarta_pronto_se"), /notifica_pronto\(false, ""\)/);
  assert.match(corpoRust(mobile, "scarta_pronto"), /scarta_pronto_se\(aggiornamenti, \|_\| true\)/);
  assert.equal(mobile.match(/\*pronto = None;/g)?.length, 1, "un altro punto azzera il pacchetto pronto");
  // …compresa la passata che non annuncia più la versione: prima lo azzerava da sola.
  const passata = corpoRust(mobile, "passata");
  assert.doesNotMatch(passata, /\*pronto = None;/, "la passata butta il pacchetto senza spegnere la notifica");
  assert.match(passata, /scarta_pronto_se\(&aggiornamenti,/);
  // Un guasto della notifica si scrive, e non ferma l'aggiornamento.
  const notifica = corpoRust(mobile, "notifica_pronto");
  assert.match(notifica, /nota!\(/);
  assert.match(notifica, /aggiornamento_pronto\(acceso, versione\)/);
});

test("la notifica ha le sue frasi in inglese e in italiano, con la versione", () => {
  const inglese = leggi(`${RES}values/strings.xml`);
  const italiano = leggi(`${RES}values-it/strings.xml`);
  for (const [lingua, testo] of [["en", inglese], ["it", italiano]] as const) {
    for (const nome of ["aggiornamento_canale", "aggiornamento_titolo", "aggiornamento_testo", "aggiornamento_apri"]) {
      assert.match(testo, new RegExp(`<string name="${nome}">[^<]+</string>`), `${lingua}: manca ${nome}`);
    }
    assert.match(testo, /<string name="aggiornamento_testo">[^<]*%1\$s[^<]*<\/string>/, `${lingua}: non dice la versione`);
    // Un apostrofo dritto non scappato rompe aapt; in inglese si scrive ’.
    for (const riga of testo.split("\n").filter((r) => r.includes('name="aggiornamento_'))) {
      assert.doesNotMatch(riga.split("\\'").join(""), /'/, `${lingua}: apostrofo dritto in «${riga.trim()}»`);
    }
  }
  // In inglese nemmeno quello scappato: ci va l'apostrofo tipografico.
  const righeInglesi = inglese.split("\n").filter((r) => r.includes('name="aggiornamento_'));
  assert.ok(righeInglesi.every((riga) => !riga.includes("\\'")));
});
