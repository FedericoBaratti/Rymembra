import { test } from "node:test";
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";

/*
 * Prove sui sorgenti delle finestrelle, dove la regola sta nel cablaggio e non
 * in una funzione: chi chiude e quando, dove va il fuoco, a chi arriva un
 * esito. Montarle vorrebbe dire un DOM e il nucleo finto; qui si legge il
 * sorgente e si cerca il punto in cui la regola è scritta, come fanno
 * `impostazioni.test.ts` e `insiemeGesti.test.ts`.
 */
const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

test("Account: mentre importa, Escape, il velo e «Annulla» non chiudono", () => {
  const account = sorgente("Account.tsx");
  assert.match(account, /useFinestrella<HTMLDivElement>\(scrivendo \? restaAperta : onChiudi\)/);
  assert.match(account, /gesto\.clic\(e\.target, e\.currentTarget\) && !scrivendo\) onChiudi\(\)/);
  assert.match(account, /onClick=\{onChiudi\}\s+disabled=\{scrivendo\}/);
  // Si alza solo per la scrittura, non per l'apertura dell'archivio o il piano.
  const importa = account.slice(account.indexOf("const importa = async"), account.indexOf("const dimentica = async"));
  assert.match(importa, /setScrivendo\(true\)/);
});

test("Importa e ImportaLink: mentre scrivono il dialogo non si chiude", () => {
  const importa = sorgente("Importa.tsx");
  const dialoghi = [...importa.matchAll(/<Dialogo\b[^>]*>/g)].map((m) => m[0]);
  assert.equal(dialoghi.length, 2);
  for (const dialogo of dialoghi) assert.match(dialogo, /chiudibile=\{!scrivendo\}/);
  assert.equal([...importa.matchAll(/onClick=\{onChiudi\} disabled=\{scrivendo\}/g)].length, 2);
});

test("Ripristino: chiuso a metà, l'esito arriva alla finestra", () => {
  const ripristino = sorgente("Ripristino.tsx");
  assert.match(ripristino, /if \(aperta\.current\) setErrore\(testoErrore\(e\)\);\s+else onErrore\(e\);/);
  assert.match(ripristino, /if \(!aperta\.current\) \{\s+onNotizia\(/);
  const app = sorgente("App.tsx");
  const montato = app.slice(app.indexOf("<Ripristino"), app.indexOf("/>", app.indexOf("onNotizia=", app.indexOf("<Ripristino"))));
  assert.match(montato, /onErrore=\{segnalaErrore\}/);
  assert.match(montato, /onNotizia=\{setNotizia\}/);
});

test("Ripristino: l'avanzamento del backup automatico non si mostra come ripristino", () => {
  const ripristino = sorgente("Ripristino.tsx");
  assert.doesNotMatch(ripristino, /useAscolto<AvanzamentoNuvola>\("nuvola:avanzamento", setAvanzamento\)/);
  assert.match(ripristino, /if \(eseguendo\.current\) setAvanzamento\(p\)/);
});

test("NuovoTema: le frecce spostano il fuoco, non solo la scelta", () => {
  const nuovo = sorgente("NuovoTema.tsx");
  const daTastiera = nuovo.slice(nuovo.indexOf("const daTastiera"), nuovo.indexOf("};", nuovo.indexOf("const daTastiera")));
  // Il fuoco prima della scelta, sul bottone d'arrivo: come `Segmentato`.
  assert.match(daTastiera, /bottoni\.current\[dove\]\?\.focus\(\);\s+setBase\(prossima\.id\);/);
  assert.match(nuovo, /bottoni\.current\[indice\] = nodo;/);
});

test("ConfermaGesto: il fuoco aprendo va su «Annulla», anche con la casella", () => {
  const conferma = sorgente("ConfermaGesto.tsx");
  const annulla = conferma.slice(conferma.lastIndexOf("<button", conferma.indexOf('t("common.cancel")')), conferma.indexOf('t("common.cancel")'));
  assert.match(annulla, /data-fuoco-iniziale/);
});

test("Account: «Tolte N righe» resta a schermo quando la scheda degli importati sparisce", () => {
  const account = sorgente("Account.tsx");
  const inizio = account.indexOf("stato.ascoltiImportati > 0 && (");
  const scheda = account.slice(inizio, account.indexOf("</section>", inizio));
  // Dimenticati gli ascolti, `ascoltiImportati` torna a zero e la scheda si
  // smonta: il numero dentro di lei durava un giro di IPC.
  assert.doesNotMatch(scheda, /account\.forgotten/);
  assert.match(account, /t\("account\.forgotten", \{ n: dimenticati \}\)/);
});

test("Elimina dal disco: un doppio clic, o Escape e poi «Elimina», non rispondono due volte", () => {
  const app = sorgente("App.tsx");
  // La conferma si chiude con `useStatoPresente`, che passa da una transizione:
  // finché non è applicata la finestrella resta montata e il suo tasto
  // risponde. Ogni uscita passa dal cancello della domanda (`unaVolta`).
  const inizio = app.indexOf("<ConfermaEliminazione");
  const montata = app.slice(inizio, app.indexOf("/>", inizio));
  assert.match(montata, /onConferma=\{\(definitivi\) => \{\s+if \(!daEliminare\.risposta\(\)\) return;/);
  assert.match(montata, /onChiudi=\{\(\) => \{\s+if \(daEliminare\.risposta\(\)\) setDaEliminare\(null\);/);
  // E ogni apertura porta il suo cancello: la domanda rifatta è un'altra
  // domanda, con la sua risposta.
  const aperture = [...app.matchAll(/setDaEliminare\(\{[^}]*\}\)/g)].map((m) => m[0]);
  assert.equal(aperture.length, 3, aperture.join("\n"));
  for (const apertura of aperture) assert.match(apertura, /risposta: unaVolta\(\)/);
});

test("i tasti della finestra non diventano mai inerti, e la × chiude prima la domanda", () => {
  // R13d-1 e D-41. Il marchio sta sul contenitore dei tre tasti, quello che
  // `main.tsx` monta accanto all'applicazione.
  const barra = sorgente("parti/BarraTitolo.tsx");
  assert.match(barra, /<div className="controlli-finestra window-controls"[^>]*data-sempre-vivo/);
  // La × chiede prima alla finestrella in cima, e chiude la finestra solo se
  // lei non c'era o non era una domanda da chiudere.
  const chiudi = barra.slice(barra.indexOf("onChiudi={() =>"));
  const corpo = chiudi.slice(0, chiudi.indexOf("\n      />"));
  assert.ok(
    corpo.indexOf("rispondiNoAllaDomandaInCima()") >= 0 &&
      corpo.indexOf("rispondiNoAllaDomandaInCima()") < corpo.indexOf("finestraChiudi()"),
    corpo,
  );
  // E la regola è quella di `modale.ts`, letta sulla finestrella in cima.
  const finestrella = sorgente("finestrella.ts");
  const risposta = finestrella.slice(finestrella.indexOf("export function rispondiNoAllaDomandaInCima"));
  assert.match(risposta.slice(0, risposta.indexOf("\n}\n")), /cosaFaLaX\(/);
  // Di serie ogni finestrella è una domanda che si chiude come con Escape;
  // `Dialogo` dice se adesso si può, e il primo avvio non è una domanda.
  assert.match(finestrella, /domanda: \{ chiudibile: boolean \} \| "nonEUnaDomanda" = \{ chiudibile: true \}/);
  assert.match(sorgente("Dialogo.tsx"), /useFinestrella<HTMLElement>\([^;]*\{ chiudibile \}\)/);
  assert.match(sorgente("Primo.tsx"), /useFinestrella<HTMLDivElement>\(onSalta, "nonEUnaDomanda"\)/);
});

test("le regioni d'annuncio portano il loro marchio, e restano vive sotto un velo", () => {
  // R16-4: `daRendereInerti` salta `data-annuncio`.
  for (const file of ["parti/Annuncio.tsx", "parti/AnnuncioScansione.tsx"]) {
    assert.match(sorgente(file), /className="solo-lettori" role="status" data-annuncio/, file);
  }
});

test("ogni superficie aria-modal rende inerte il resto della finestra", () => {
  // R16-3: `aria-modal` è un suggerimento, e WebView2 e la WebView di Android
  // lasciano leggibile e attivabile la pagina dietro. La regola sta in
  // `useVeloInerte`, o in `Dialogo` che la chiama.
  const DA_FARE: Record<string, string> = {
    // Di altri gruppi della revisione: la correzione è la stessa riga.
    "Giro.tsx": "F08 (il giro)",
    "parti/Equalizzatore.tsx": "F13",
    "parti/Insieme.tsx": "F02 (il pannello della barra)",
    // Rende inerte la pagina da sé, con `inerte` di `Impaginazione`.
    "schermate/InRiproduzioneTelefono.tsx": "inerte di suo",
  };
  const file = [
    "Account.tsx",
    "Dialogo.tsx",
    "Primo.tsx",
    "Regole.tsx",
    "Ripristino.tsx",
    "Sincronizza.tsx",
    "parti/Foglio.tsx",
    "parti/SceltaTesto.tsx",
    ...Object.keys(DA_FARE),
  ];
  // L'elenco è completo: ogni sorgente fuori dallo Studio che dichiara un
  // modale ci sta (lo Studio disegna finestrelle finte sulla tela).
  const tutti = readdirSync(new URL("../src/", import.meta.url), { recursive: true })
    .map((p) => String(p).replace(/\\/g, "/"))
    .filter((p) => p.endsWith(".tsx") && !p.startsWith("studio/"))
    .filter((p) => sorgente(p).includes('aria-modal="true"'))
    .sort();
  assert.deepEqual(tutti, [...file].sort());
  for (const nome of file) {
    const testo = sorgente(nome);
    assert.match(testo, /aria-modal="true"/, nome);
    if (nome in DA_FARE) continue;
    assert.match(testo, /useVeloInerte\(/, nome);
  }
  // Il primo avvio: la radice fa da velo, dopo `useFinestrella`.
  const primo = sorgente("Primo.tsx");
  assert.ok(
    primo.indexOf("useVeloInerte(finestrella)") > primo.indexOf("useFinestrella<HTMLDivElement>("),
    "Primo: useVeloInerte dopo useFinestrella",
  );
  // Il foglio: il pannello resta vivo accanto al suo velo, e chi aveva il
  // fuoco si legge prima che la pagina diventi inerte.
  const foglio = sorgente("parti/Foglio.tsx");
  assert.match(foglio, /useVeloInerte\(velo, pannello\)/);
  assert.ok(foglio.indexOf("chiAveva.current =") < foglio.indexOf("useVeloInerte(velo, pannello)"));
});

test("il menù del tasto destro si chiude quando si apre una finestrella", () => {
  // R13d-4. Una finestrella arrivata da sola con un menù aperto lo rendeva
  // inerte ma lo lasciava sopra il velo, col suo ascolto in cattura vivo:
  // frecce, Inizio/Fine, Tab ed Escape della finestrella andavano al menù.
  const menu = sorgente("Menu.tsx");
  const popup = menu.slice(menu.indexOf("function MenuPopup("));
  assert.match(popup, /quandoSiApreUnModale\(\(\) => chiudi\.current\(\)\)/);
  // Non sulla tela dello Studio, dove il menù è un disegno.
  const effetto = popup.slice(popup.lastIndexOf("useEffect(", popup.indexOf("quandoSiApreUnModale(")));
  assert.match(effetto.slice(0, effetto.indexOf("quandoSiApreUnModale(")), /if \(anteprima\) return;/);
});

test("gli avvisi che nascono pieni parlano da una regione d'annuncio", () => {
  // R13d-6. `role="status"` sul riquadro che compare insieme alla frase:
  // NVDA e l'Assistente vocale spesso non lo dicono (vedi `Annuncio.tsx`).
  const avvisi = sorgente("parti/Avvisi.tsx");
  const avviso = avvisi.slice(avvisi.indexOf("export function Avviso("), avvisi.indexOf("export function AvvisoErrore("));
  assert.doesNotMatch(avviso, /"status"/);
  // Il blocco resta un `alert`, che si annuncia anche nascendo pieno.
  assert.match(avviso, /role=\{livello === "blocco" \? "alert" : undefined\}/);
  assert.match(avviso, /<Annuncio testo=\{detta\} \/>/);
  // «Il suono si è spostato»: una regione sempre montata, e una copia sola
  // che parla — la fascia; la terza colonna la mostra soltanto.
  const audio = sorgente("parti/AvvisoAudio.tsx");
  assert.doesNotMatch(audio, /role="status"/);
  assert.match(audio, /dove === "fascia" && <Annuncio testo=\{daDire\} \/>/);
  // Montata in ogni forma del riquadro, e anche quando non ce n'è nessuno.
  assert.match(audio, /if \(spostato === null\) return <>\{annuncio\}<\/>;/);
  // E l'allarme «non c'è audio» lo dice una copia sola anche lui.
  assert.equal([...audio.matchAll(/role="alert"/g)].length, 1);
});

// ── R18-16: un'eliminazione che non si può fare non si chiede ──────────────

test("Elimina dal disco rifiutato (un brano d'immagine CD da solo): il titolo dice il fatto", () => {
  const conferma = readFileSync(new URL("../src/ConfermaEliminazione.tsx", import.meta.url), "utf8");
  const titolo = conferma.slice(conferma.indexOf("const titolo ="), conferma.indexOf("return (", conferma.indexOf("const titolo =")));
  assert.match(titolo, /rifiuto !== null/);
  assert.match(titolo, /delete\.disk\.refused\.title/);
  // E il testo del Cestino non si dice sopra un'eliminazione che non avverrà.
  const descrizione = conferma.slice(conferma.indexOf("descrizione={"), conferma.indexOf("ruolo="));
  assert.match(descrizione, /rifiuto/);
});
