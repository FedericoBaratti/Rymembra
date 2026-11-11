import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Come sono collegati i pezzi dello Studio, letto nei sorgenti.
 *
 * I componenti dello Studio portano con sé React e l'IPC e `node --test` non
 * li monta: quel che qui si controlla è il filo — chi chiama chi, con quale
 * guardia — nei punti in cui un filo sbagliato era un difetto. La logica che
 * si può separare sta nei moduli puri, provata nei loro test.
 */

const sorgente = (file: string) => readFileSync(new URL(`../src/studio/${file}`, import.meta.url), "utf8");

/** Il corpo di una funzione, da `inizio` alla graffa che chiude alla stessa rientranza. */
function corpo(testo: string, inizio: string): string {
  const da = testo.indexOf(inizio);
  assert.ok(da >= 0, `manca «${inizio}»`);
  const riga = testo.lastIndexOf("\n", da) + 1;
  const rientro = testo.slice(riga, da).match(/^ */)?.[0] ?? "";
  // La prima riga, alla stessa rientranza, che comincia con `}`: una funzione
  // dichiarata, una costante (`};`), un `useCallback` (`}, [dipendenze]);`).
  // Non `}: {` né `}) {`, che chiudono le props e i loro tipi e aprono altro.
  const chiusura = new RegExp(`\\n${rientro}\\}[^\\n{]*\\n`, "g");
  chiusura.lastIndex = da;
  const fine = chiusura.exec(testo)?.index ?? -1;
  return testo.slice(da, fine < 0 ? undefined : fine);
}

test("scafale: con Alt+frecce il fuoco segue il nodo spostato", () => {
  // Senza, il fuoco restava sulla via di partenza — che dopo la validazione
  // mostra il fratello salito — e il secondo Alt+Giù riportava indietro
  // l'altro nodo.
  const tastiera = corpo(sorgente("AlberoScafale.tsx"), "const daTastiera");
  const ramoAlt = tastiera.slice(tastiera.indexOf("e.altKey"), tastiera.indexOf('} else if (e.key === "ArrowDown")'));
  assert.match(ramoAlt, /sposta\(/);
  assert.match(ramoAlt, /vaiA\(/, "il ramo Alt non sposta il fuoco");
});

test("Ctrl+S e la chat in agent passano da daFotografare", () => {
  const guscio = sorgente("Guscio.tsx");
  const ctrlS = guscio.slice(guscio.indexOf('tasto === "s"'), guscio.indexOf('tasto === "k"'));
  assert.match(ctrlS, /istantaneaManuale/);
  assert.doesNotMatch(ctrlS, /istantanea\("manuale"\)/);
  assert.match(guscio, /onIstantanea=\{\(\) => void istantaneaManuale\(\)/);
  assert.match(corpo(guscio, "const istantaneaManuale"), /daFotografare\(/);
});

test("«Configura un modello» dalla chat esce come «Esci»: solo a salvataggio riuscito", () => {
  // onModelli smonta lo Studio: passato così com'è, quel che il salvataggio
  // automatico non aveva ancora scritto andava perso.
  const guscio = sorgente("Guscio.tsx");
  assert.doesNotMatch(guscio, /onVaiAImpostazioni=\{onModelli\}/);
  assert.match(guscio, /onVaiAImpostazioni=\{\(\) => \{\s*svuotaSospesi\(\);\s*void poiSeSalvato\(salva, onModelli, onErrore\);\s*\}\}/);
  assert.match(corpo(guscio, "const esci"), /poiSeSalvato\(salva, onEsci, onErrore\)/);
});

test("un documento che non si apre lascia un'uscita: Riprova e Chiudi lo Studio", () => {
  // La finestra sotto è `inert` e le scorciatoie dell'app sono spente: senza
  // tasti, «Caricamento…» restava per sempre e l'unica via era chiudere
  // Rymembra.
  const guscio = sorgente("Guscio.tsx");
  const attesa = guscio.slice(guscio.indexOf("if (azioni === null || !s.caricato)"), guscio.indexOf("<ContestoAzioniStudio"));
  assert.match(attesa, /guastoApertura/, "la schermata d'attesa non guarda il guasto");
  assert.match(attesa, /t\("crash\.studio\.close"\)/);
  assert.match(attesa, /t\("common\.retry"\)/);
  assert.match(guscio, /\.studioDocumento\(id\)[\s\S]{0,400}setGuastoApertura/);
  assert.match(guscio, /\.studioRegistro\(\)[\s\S]{0,400}setGuastoApertura/);
});

test("chat: il ciclo dell'agent guarda la sua bandiera e la modalità fissata alla domanda", () => {
  const chat = sorgente("Chat.tsx");
  const concludi = chat.slice(chat.indexOf("const concludi = useCallback("), chat.indexOf("const chiedi = useCallback("));
  assert.doesNotMatch(concludi, /\bmodalita\b/, "concludi legge ancora la modalità di adesso");
  assert.equal(concludi.match(/apreUnAltroGiro\(/g)?.length, 2);
  assert.match(corpo(chat, "const ferma = useCallback("), /cicloAgent\.current = false/);
  assert.match(corpo(chat, "const alGuasto"), /cicloAgent\.current = false/);
});

test("chat: il campo resta a fuoco durante il turno (readOnly, non disabled)", () => {
  // Un campo che diventa disabled perde il fuoco: finita la risposta, quel che
  // si scriveva finiva alle scorciatoie dello Studio («f», «i», Ctrl+Z).
  const chat = sorgente("Chat.tsx");
  const campo = chat.slice(chat.indexOf("<textarea"), chat.indexOf("/>", chat.indexOf("<textarea")));
  assert.doesNotMatch(campo, /\bdisabled=\{occupata\}/);
  assert.match(campo, /readOnly=\{occupata\}/);
  assert.match(campo, /aria-disabled=\{occupata \|\| undefined\}/);
});

test("ogni riga dell'ispettore è un gruppo che si chiama come il suo campo", () => {
  // I controlli del colore hanno nomi fissi («In che forma», «Quale token»):
  // senza il nome del campo intorno, cinque «In che forma» di fila non dicono
  // se si sta cambiando il testo o il bordo.
  const riga = corpo(sorgente("controlli.tsx"), "export function Riga(");
  assert.match(riga, /role="group"/);
  assert.match(riga, /aria-labelledby=\{/);
  assert.match(riga, /useId\(\)/);
});

test("i colori fuori da una Riga portano il nome del loro campo", () => {
  const conEtichetta = (file: string) => {
    const testo = sorgente(file);
    const usi = [...testo.matchAll(/<ValoreColore\b[\s\S]*?\/>/g)].map((m) => m[0]);
    assert.ok(usi.length > 0, file);
    return usi;
  };
  for (const uso of conEtichetta("Pila.tsx")) assert.match(uso, /etichetta=\{/, uso);
  for (const uso of conEtichetta("Timeline.tsx")) assert.match(uso, /etichetta=\{/, uso);
  const ombra = corpo(sorgente("controlli.tsx"), "export function ValoreOmbra(");
  assert.match(ombra.slice(ombra.indexOf("<ValoreColore")), /etichetta=\{/);
});

test("togliere un file del pacchetto chiede prima, dalle due strade, e non apre un passo", () => {
  for (const file of ["IspezioneVoci.tsx", "Variabili.tsx"]) {
    const testo = sorgente(file);
    assert.match(testo, /domandaTogliRisorsa\(/, file);
    assert.match(testo, /<ConfermaGesto domanda=\{domanda\}/, file);
    assert.match(testo, /manda\(\{ tipo: "risorsaTolta", nome \}\)/, file);
    assert.doesNotMatch(testo, /assetRemove/, file);
  }
});

test("i livelli dello sfondo si riordinano anche da tastiera (Alt+frecce)", () => {
  // Il riordino era solo col puntatore: da tastiera l'ordine — che cambia il
  // risultato — si cambiava solo togliendo e rimettendo i livelli.
  const pila = sorgente("Pila.tsx");
  const riga = pila.slice(pila.indexOf('className="livello"'), pila.indexOf('<span className="maniglia"'));
  assert.match(riga, /onKeyDown=/, "la riga del livello non ascolta la tastiera");
  assert.match(pila, /e\.altKey && \(e\.key === "ArrowUp" \|\| e\.key === "ArrowDown"\)/);
  assert.match(pila, /aria-keyshortcuts="Alt\+ArrowUp Alt\+ArrowDown"/);
});

test("Ctrl+K porta il fuoco nella ricerca dei Livelli anche partendo da Variabili", () => {
  // Il cambio di colonna si disegnava dopo il gestore: il campo era ancora
  // nascosto (`<Activity mode="hidden">`) e il focus non aveva effetto.
  const guscio = sorgente("Guscio.tsx");
  const ctrlK = guscio.slice(guscio.indexOf('tasto === "k"'), guscio.indexOf("if (mod || e.altKey"));
  assert.match(ctrlK, /flushSync\(\(\) =>[\s\S]*manda\(\{ tipo: "imposta", campi: \{ sinistra: "livelli" \} \}\)/);
  assert.match(ctrlK, /\.livelli-studio \.cerca-livelli input/);
});

test("ispettore del nodo: le scelte discrete hanno la guardia dello scafale, i cursori no", () => {
  const voci = sorgente("IspezioneVoci.tsx");
  const nodo = corpo(voci, "export function IspezioneNodo(");
  assert.match(nodo, /if \(scafaleScrivibile\(s\)\) cambia\(campo, valore\)/);
  assert.match(nodo, /onScegli=\{\(v\) => scegli\("size"/);
  assert.match(nodo, /onCambia=\{\(v\) => v !== null && cambia\("size"/, "il cursore della misura resta senza guardia");
});

test("le scritture dei controlli si calcolano nel riduttore, non su `attuale` (R14-1)", () => {
  // `attuale.current` resta indietro finché React non ridisegna: due scritture
  // nello stesso gestore partivano dallo stesso testo e la seconda disfaceva la
  // prima. Il «+» di un innesco ne faceva proprio due.
  const guscio = sorgente("Guscio.tsx");
  const tratti: readonly (readonly [string, string])[] = [
    ["const scrivi = useCallback(", "const scriviTesto = useCallback("],
    ["const scriviMolti = useCallback(", "// ── i file del pacchetto"],
  ];
  for (const [nome, fine] of tratti) {
    const qui = guscio.slice(guscio.indexOf(nome), guscio.indexOf(fine));
    assert.ok(qui.length > 0, `manca «${nome}»`);
    assert.doesNotMatch(qui, /attuale\.current/, `${nome} calcola ancora il testo fuori dal riduttore`);
    assert.match(qui, /tipo: "modifica"/);
  }
  // E il gesto è un passo solo: un Ctrl+Z toglie animazione e assegnazione insieme.
  const crea = corpo(sorgente("IspezioneParte.tsx"), "const crea = (innesco: string, ciclo: boolean)");
  assert.equal(crea.match(/scriviMolti\(/g)?.length, 1, "crea non scrive in un colpo solo");
  assert.doesNotMatch(crea, /\bscrivi\(|assegna\(/);
});

test("«Butta la bozza» chiede prima, rilegge i file e la conversazione (R14-2, D-66)", () => {
  // Partiva al primo clic, e dopo la finestra mostrava ancora i file che il
  // nucleo aveva tolto; la chat in memoria riscriveva la conversazione buttata.
  const variabili = sorgente("Variabili.tsx");
  const tasto = variabili.slice(variabili.indexOf('className="pillola btn-ghost scarta-bozza"'));
  const clic = tasto.slice(tasto.indexOf("onClick="), tasto.indexOf("</button>"));
  assert.doesNotMatch(clic, /onClick=\{\(\) => void scarta\(\)\}/, "il tasto butta la bozza senza chiedere");
  assert.match(clic, /setDomanda\(\{[\s\S]*fai: \(\) => void scarta\(\)/);
  const scarta = corpo(variabili, "const scarta = async");
  assert.match(scarta, /ipc\.studioScarta\(s\.id, testo\(s\)\)/, "l'istantanea dello scarto la prende il nucleo");
  assert.match(scarta, /rileggiFile\(\)/);
  assert.match(scarta, /tipo: "chatSulDisco"/);
  const ripristina = corpo(variabili, "const ripristina = async");
  assert.match(ripristina, /rileggiFile\(\)/);
  assert.match(ripristina, /tipo: "chatSulDisco"/);
  // La chat rilegge il disco quando lo Studio glielo dice.
  assert.match(sorgente("Chat.tsx"), /s\.chatSulDisco/);
});

const dellApp = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");

test("scegliere un'altra skin durante «Prova sull'app» chiude la prova, e lo dice (R14-4, D-67)", () => {
  // La finestra indossava la skin scelta con la pillola che diceva ancora
  // «Stai provando», e «Torna» rimetteva quella di prima della prova.
  const skin = dellApp("skinAttiva.ts");
  const scegli = skin.slice(skin.indexOf("const scegliSkin = useCallback("), skin.indexOf("const iniziaProva = useCallback("));
  assert.match(scegli, /chiudiProva\(\)/, "scegliere non chiude la prova");
  assert.match(scegli, /provaChiusa\.current\(/, "la prova si chiude senza dirlo");
  // «Tienila» sceglie anche lei, ma la prova l'ha già chiusa: niente avviso.
  assert.match(corpo(skin, "const tieniProva = useCallback("), /chiudiProva\(\)/);
  assert.match(corpo(skin, "const chiudiProva = useCallback("), /provaAdesso\.current = null/);

  const app = dellApp("App.tsx");
  // «Apri nello Studio» su un'altra skin durante la prova: prima si torna.
  const apri = app.slice(app.indexOf("onApriStudio: (id) => {"), app.indexOf("onImporta: importaDatabase"));
  assert.match(apri, /finisciProva\(\)/, "aprire un'altra skin nello Studio lascia la prova accesa");
  // E lo Studio apre davvero l'altra skin: il documento si legge una volta
  // per montaggio.
  assert.match(app, /<Studio\s+key=\{studioAperto\}/);
});

test("un doppio clic su «Prova» non fa della bozza la skin «di prima» (R14-8)", () => {
  const skin = dellApp("skinAttiva.ts");
  const inizia = corpo(skin, "const iniziaProva = useCallback(");
  assert.match(inizia, /provaAdesso\.current\?\.reale/, "una seconda prova prende la bozza come skin di prima");
});

test("lo Studio salva quel che ha in mano anche quando lo si smonta (R14-4)", () => {
  // Con `key` un'altra skin rimonta lo Studio: il respiro degli 800 ms non
  // arrivava, e l'ultima modifica restava indietro.
  const guscio = sorgente("Guscio.tsx");
  assert.match(guscio, /useEffect\(\s*\(\) => \(\) => \{\s*if \(daSalvare\(attuale\.current\)\) void salva\(\)/);
});

test("il codice di una parte non si perde cambiando parte, scheda o tema, né uscendo (R14-3)", () => {
  // Il testo viveva solo nello stato del componente, applicato da un timer
  // che lo smontaggio spegneva: un clic su un'altra parte lo buttava via.
  const parte = sorgente("IspezioneParte.tsx");
  const codice = parte.slice(parte.indexOf("function CodiceParte("));
  assert.match(codice, /registraSospeso\(/, "l'editor non si fa trovare da chi salva");
  assert.match(codice, /return \(\) => \{[\s\S]*?svuota\(\)/, "allo smontaggio il testo in mano si perde");
  assert.match(codice, /onBlur=/, "uscendo dal campo non si applica");
  assert.match(codice, /tipo: "codiceInSospeso"/);
  // Il tema fa parte della chiave: cambiarlo rimonta l'editor, che mette da parte.
  assert.match(parte, /<CodiceParte key=\{`\$\{s\.tema\}\/\$\{def\.name\}`\}/);

  const guscio = sorgente("Guscio.tsx");
  // Chi salva per un gesto svuota prima gli editor, dentro un flushSync.
  for (const inizio of ["const esci = useCallback(", "const salvaEUsa = useCallback(", "const esporta = useCallback(", "const prova = useCallback("]) {
    assert.match(corpo(guscio, inizio), /svuotaSospesi\(\)/, `${inizio} salva senza il codice in mano`);
  }
  const ctrlS = guscio.slice(guscio.indexOf('tasto === "s"'), guscio.indexOf('tasto === "k"'));
  assert.match(ctrlS, /svuotaSospesi\(\)/);
  assert.match(corpo(guscio, "const svuotaSospesi = useCallback("), /flushSync\(/);
  // Uscire con un codice che non è JSON chiede prima.
  assert.match(corpo(guscio, "const esci = useCallback("), /codiceInSospeso/);
});

test("«Prova», «Salva e usa» ed «Esporta» non partono due volte (R14-8)", () => {
  // Un doppio clic su «Prova» con la compilazione non ancora in memoria
  // arrivava due volte a `iniziaProva`; su «Salva e usa» faceva due
  // istantanee e due installazioni.
  const guscio = sorgente("Guscio.tsx");
  assert.match(guscio, /onProva=\{\(\) => void unoAllaVolta\(prova\)\}/);
  assert.match(guscio, /onSalvaEUsa=\{\(\) => void unoAllaVolta\(salvaEUsa\)\}/);
  assert.match(guscio, /onEsporta=\{\(\) => void unoAllaVolta\(esporta\)\}/);
  assert.match(corpo(guscio, "const unoAllaVolta = useCallback("), /gestoInVolo\.current\) return/);
  const barra = sorgente("BarraStudio.tsx");
  assert.equal(barra.match(/disabled=\{[^}]*inVolo/g)?.length, 3, "i tasti restano accesi mentre il gesto è in volo");
});
