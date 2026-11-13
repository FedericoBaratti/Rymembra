import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

/*
 * Un nome solo per ogni cosa (R17).
 *
 * Il catalogo delle lingue è scritto in tempi e moduli diversi, e ogni frase è
 * nata col nome che il suo modulo usava: «tema» per una skin, «portachiavi di
 * Windows» per Gestione credenziali, «il collegamento via internet attivo» per
 * l'interruttore «Raggiungibile da internet». Nessun compilatore vede un nome
 * che non coincide con quello dello schermo, e la persona che segue il rimando
 * di un errore cerca una parola che non trova. Le prove qui sotto tengono
 * fermi i nomi decisi (D-79, D-80, D-81, D-82) e i rimandi alle schede.
 */
const leggi = (percorso: string) => readFileSync(new URL(percorso, import.meta.url), "utf8");
type Dizionario = Record<string, string>;
const IT = JSON.parse(leggi("../src/lingue/it.json")) as Dizionario;
const EN = JSON.parse(leggi("../src/lingue/en.json")) as Dizionario;
const LINGUE: [string, Dizionario][] = [
  ["it", IT],
  ["en", EN],
];

/** Le chiavi che si leggono come frase: fuori dai sinonimi della ricerca, che tengono tutte le parole. */
const frasi = (d: Dizionario) => Object.entries(d).filter(([k]) => !k.endsWith(".syn") && !k.includes(".syn."));

test("«Crea una skin…» crea una skin: «tema» resta per chiaro e scuro (D-80, R17-11)", () => {
  for (const chiave of ["settings.skin.create", "newtheme.aria", "theme.new.unreadableBase", "tour.step.studio.body"]) {
    assert.doesNotMatch(IT[chiave] ?? "", /\btema\b/i, `it ${chiave}`);
    assert.doesNotMatch(EN[chiave] ?? "", /\btheme\b/i, `en ${chiave}`);
  }
  assert.equal(IT["settings.skin.create"], "Crea una skin…");
  assert.equal(EN["settings.skin.create"], "Create a skin…");
  // Il giro cita il tasto con le sue parole.
  assert.match(IT["tour.step.studio.body"] ?? "", /«Crea una skin…?»/);
  assert.match(EN["tour.step.studio.body"] ?? "", /“Create a skin…?”/);
});

test("il portachiavi del sistema ha un nome solo per piattaforma (R17-12)", () => {
  for (const [chiave, testo] of frasi(EN)) {
    assert.doesNotMatch(testo, /Windows keychain|system keychain|credential store|keyring/i, `en ${chiave}`);
  }
  for (const [chiave, testo] of frasi(IT)) {
    assert.doesNotMatch(testo, /portachiavi di Windows|portachiavi del sistema/i, `it ${chiave}`);
  }
  // Sul telefono è il portachiavi di Android, e le frasi del computer non lo dicono.
  for (const chiave of ["errors.insieme.identitaPersa", "errors.lontano.identitaPersa", "folders.add.mega.note"]) {
    assert.match(IT[chiave] ?? "", /Gestione credenziali di Windows/, `it ${chiave}`);
    assert.match(EN[chiave] ?? "", /Windows Credential Manager/, `en ${chiave}`);
    assert.match(IT[`${chiave}.telefono`] ?? "", /Android Keystore/, `it ${chiave}.telefono`);
    assert.match(EN[`${chiave}.telefono`] ?? "", /Android Keystore/, `en ${chiave}.telefono`);
  }
});

test("l'identità persa dice cosa fare, sul computer e sul telefono (R17-20)", () => {
  for (const chiave of ["errors.insieme.identitaPersa", "errors.lontano.identitaPersa"]) {
    assert.match(IT[chiave] ?? "", /riavvia Windows/, `it ${chiave}`);
    assert.match(EN[chiave] ?? "", /restart Windows/, `en ${chiave}`);
    assert.match(IT[`${chiave}.telefono`] ?? "", /riavvia il telefono/, `it ${chiave}.telefono`);
    assert.match(EN[`${chiave}.telefono`] ?? "", /restart the phone/, `en ${chiave}.telefono`);
  }
});

test("l'interruttore e le schede degli errori si chiamano come sullo schermo (R17-13)", () => {
  for (const [lingua, d] of LINGUE) {
    const interruttore = d["settings.devices.reachable"];
    assert.ok(interruttore !== undefined, lingua);
    for (const chiave of ["errors.lontano.irraggiungibile", "errors.lontano.spento"]) {
      assert.ok((d[chiave] ?? "").includes(interruttore), `${lingua} ${chiave} nomina «${interruttore}»`);
    }
    // La scheda «Account cloud» e il bottone che la apre.
    assert.equal(
      (d["errors.action.cloudAccounts"] ?? "").toLowerCase().endsWith(d["settings.cloudAccounts.title"]?.toLowerCase() ?? "?"),
      true,
      `${lingua} errors.action.cloudAccounts`,
    );
    // Il diario: la frase cita il tasto che lo apre, sul computer e sul telefono.
    assert.ok((d["settings.together.unavailable"] ?? "").includes(d["settings.diary.open"] ?? "?"), lingua);
    assert.ok(
      (d["settings.together.unavailable.telefono"] ?? "").includes(d["settings.diary.open.telefono"] ?? "?"),
      `${lingua} .telefono`,
    );
    // E il gesto per i dispositivi della sincronia è «accetta», non «accoppia».
    assert.doesNotMatch(d["errors.sync.remoteCorrupt"] ?? "", /accoppia|pair it again/i, lingua);
  }
});

test("un PC associato si dimentica in Dispositivi (R17-8)", () => {
  for (const [lingua, d] of LINGUE) {
    for (const chiave of ["together.notice.forgottenBy", "together.notice.forgottenBy.telefono"]) {
      const testo = d[chiave] ?? "";
      assert.ok(testo.includes(` › ${d["settings.section.dispositivi"]}`), `${lingua} ${chiave}`);
      assert.doesNotMatch(testo, /Ascolta insieme ›|Listen together ›/, `${lingua} ${chiave}`);
    }
  }
});

/** Frasi che mandano alle impostazioni del sistema, non a quelle di Rymembra. */
const DI_ANDROID = ["errors.net.localNetwork"];

test("i percorsi «Impostazioni › …» cominciano da una sezione che c'è", () => {
  for (const [lingua, d] of LINGUE) {
    const radice = lingua === "it" ? "Impostazioni" : "Settings";
    const sezioni = Object.entries(d)
      .filter(([k]) => /^settings\.section\.[a-z]+$/.test(k))
      .map(([, v]) => v)
      .sort((a, b) => b.length - a.length);
    assert.ok(sezioni.length >= 10, `${lingua}: le sezioni non si trovano`);
    let visti = 0;
    for (const [chiave, testo] of frasi(d)) {
      // Le impostazioni di **Android** (Impostazioni › App › Rymembra › Autorizzazioni):
      // non sono quelle di Rymembra.
      if (DI_ANDROID.includes(chiave)) continue;
      for (const m of testo.matchAll(new RegExp(`${radice} › ([^.;:()“”«»—]+)`, "g"))) {
        visti += 1;
        const dopo = (m[1] ?? "").trim();
        assert.ok(
          sezioni.some((s) => dopo === s || dopo.startsWith(`${s} ›`) || dopo.startsWith(`${s},`) || dopo.startsWith(`${s} `)),
          `${lingua} ${chiave}: «${radice} › ${dopo}» non comincia da una sezione`,
        );
      }
    }
    assert.ok(visti > 15, `${lingua}: pochi percorsi trovati (${visti})`);
  }
});

test("una lista sola per i brani da comprare: «Da comprare» / «To buy» (D-79, R17-10)", () => {
  for (const [lingua, d] of LINGUE) {
    for (const [chiave, testo] of frasi(d)) {
      assert.doesNotMatch(testo, /lista della spesa|shopping list|lista acquisti/i, `${lingua} ${chiave}`);
    }
  }
  assert.equal(IT["explore.wanted"], IT["settings.import.how.p2.toBuy"]);
  assert.equal(EN["explore.wanted"], EN["settings.import.how.p2.toBuy"]);
});

test("«scafale» è un nome del formato delle skin: nell'interfaccia si dice «struttura» (D-81)", () => {
  for (const [chiave, testo] of frasi(IT)) {
    // Il nome del campo del formato (`layout.shell`) e quelli dei registri restano; la prosa no.
    assert.doesNotMatch(testo.replace(/\{\w+\}/g, ""), /\bscafale\b/i, `it ${chiave}`);
  }
  assert.equal(IT["studio3.shell.section"], "Struttura");
});

test("l'app parla di sé in terza persona e dà del tu (D-82, R17-17)", () => {
  // Nessun «Ho guardato», «non l'ho trovato», «ti chiediamo», «glielo diciamo», «vostra».
  const PRIMA_PERSONA = /\b(ho (guardato|letto|aperto|trovato|chiesto|potuto)|non (l'|le |li )?ho\b|ti chiediamo|glielo diciamo|indicami|vostr[aeio])\b/i;
  for (const [chiave, testo] of frasi(IT)) {
    // «Ho autorizzato, completa» e «Ho un link» sono quel che dice chi preme il tasto.
    if (["scrobble.lfm.done", "folders.add.mega.link", "explore.down.sub"].includes(chiave)) continue;
    assert.doesNotMatch(testo, PRIMA_PERSONA, `it ${chiave}`);
  }
  for (const chiave of ["primo.body", "primo.nessuna.title", "primo.vuota.title", "primo.vuota.body", "primo.saltate"]) {
    assert.match(IT[chiave] ?? "", /Rymembra/, `it ${chiave}`);
    assert.match(EN[chiave] ?? "", /Rymembra/, `en ${chiave}`);
    assert.doesNotMatch(EN[chiave] ?? "", /\b(I|me)\b/, `en ${chiave}`);
  }
});

test("lettore, spettro e brano: i termini italiani dell'interfaccia (R17-17)", () => {
  for (const [chiave, testo] of frasi(IT)) {
    assert.doesNotMatch(testo, /\b(player|visualizer)\b/i, `it ${chiave}`);
    // «traccia» è il numero di traccia di un CD (`folders.imageTrack`) o la traccia di una griglia
    // CSS (le frasi del validatore, `skin.issue.*`): in nessun'altra frase.
    if (chiave !== "folders.imageTrack" && !chiave.startsWith("skin.issue.")) {
      assert.doesNotMatch(testo, /\btracc(ia|e)\b/i, `it ${chiave}`);
    }
  }
  assert.match(IT["settings.update.store"] ?? "", /è lo Store/);
  assert.doesNotMatch(IT["skin.token.canvas.viz.attack"] ?? "", /vúmetro/);
  assert.doesNotMatch(IT["drift.title"] ?? "", /video/);
});

test("le frasi con più numeri non scrivono «1 righe» (R17-18)", () => {
  // Le tre che più si vedono: la chat dello Studio, il codice, la variante.
  for (const chiave of ["studio.chat.proposed", "studio.chat.proposed.uno", "studio3.code.lines", "studio3.vars.variant.some"]) {
    assert.doesNotMatch(IT[chiave] ?? "", /\{(piu|meno|token|colori|motivi|parti)\} (righe|token|colori|motivi|parti)/, `it ${chiave}`);
    assert.doesNotMatch(EN[chiave] ?? "", /\{(piu|meno|token|colori|motivi|parti)\} (lines|tokens|colors|patterns|parts)/, `en ${chiave}`);
  }
});

test("le frasi da computer che il telefono disegna hanno la loro variante (R17-19)", () => {
  // Le frasi che un componente disegna anche sul telefono e che dicono «PC», «clic»
  // o mandano a una cartella di rete: sul telefono la variante `.telefono` dice il vero.
  const DA_VARIARE = [
    "queue.empty",
    "settings.together.trusted.moved",
    "settings.together.trusted.moved.uno",
    "errors.cloud.insecureServer",
    "errors.db.ioFailed",
    "errors.db.readOnly",
    "errors.fs.permissionDenied",
    "db.damaged.how",
    // Le stringhe dell'associazione dei dispositivi (F02): «PC» non è il telefono.
    "more.devices.link.sub",
    "settings.devices.reachable.sub",
    "devices.reachable.sub",
    "devices.kind.together",
    "together.nearby.sameName",
    "together.code.title",
    "together.code.computing",
    "together.code.trust",
    "together.code.sameName",
    "together.pairing.request",
  ];
  for (const [lingua, d] of LINGUE) {
    for (const chiave of DA_VARIARE) {
      // Il plurale di una variante sta in `<chiave>.telefono.uno`, non in `<chiave>.uno.telefono`.
      const variante = chiave.endsWith(".uno")
        ? `${chiave.slice(0, -4)}.telefono.uno`
        : `${chiave}.telefono`;
      const testo = d[variante];
      assert.ok(testo !== undefined, `${lingua}: manca «${variante}»`);
      assert.doesNotMatch(testo, /\bPCs?\b|\bclic\b|\bclick|trascina|drag|rete\b.*cartella|network folder/i, `${lingua} ${variante}`);
    }
  }
  // La strada dei documenti: i tre file del database si spostano insieme, e così la libreria
  // più nuova resta recuperabile (R06b-2).
  for (const [lingua, d] of LINGUE) {
    const how = d["db.damaged.how"] ?? "";
    assert.match(how, /rymembra\.db-wal/, lingua);
    assert.match(how, /rymembra\.db-shm/, lingua);
    assert.match(how, lingua === "it" ? /sposta insieme/ : /together to another folder/, lingua);
    assert.doesNotMatch(how, lingua === "it" ? /\btogli\b/ : /\bdelete\b/, lingua);
  }
});

test("l'avviso dell'aggiornamento non promette una copia che può essere saltata (R06b-2)", () => {
  // `Istantanea::Saltata`: senza spazio la copia di prima della migrazione non si fa, e
  // l'aggiornamento procede. La fase che la finestra riceve («aggiornamento») è annunciata
  // **prima** della copia: non sa ancora com'è andata, e la frase non lo dice per lei.
  for (const [lingua, d] of LINGUE) {
    for (const chiave of ["startup.updating.detail", "startup.updating.detail.telefono"]) {
      const testo = d[chiave] ?? "";
      assert.match(testo, lingua === "it" ? /prova prima a farne una copia/ : /first tries to make a copy/, `${lingua} ${chiave}`);
      assert.match(testo, lingua === "it" ? /se non c'è abbastanza spazio libero la salta/ : /if there isn’t enough free space it skips/, `${lingua} ${chiave}`);
    }
  }
  // Il nome della copia, che è un dato, sta solo nella frase del computer.
  assert.match(IT["startup.updating.detail"] ?? "", /\{copia\}/);
  assert.doesNotMatch(IT["startup.updating.detail.telefono"] ?? "", /\{copia\}/);
});

test("lo stato dell'apertura è la voce di Rymembra, non una prima persona (D-82)", () => {
  for (const chiave of ["startup.opening", "startup.updating", "primo.cercando", "primo.scansione", "together.code.waiting", "devices.link.waiting", "folders.add.waiting", "settings.cloud.waiting", "together.code.computing"]) {
    assert.match(IT[chiave] ?? "", /^(Fatto\. )?Rymembra /, `it ${chiave}`);
  }
});

test("«nessun modello» nomina i fornitori come le impostazioni, e dice dove sceglierli (R17-20)", () => {
  for (const [lingua, d] of LINGUE) {
    const testo = d["studio.chat.noModel"] ?? "";
    for (const nome of ["Ollama", "LM Studio", "OpenRouter"]) assert.ok(testo.includes(nome), `${lingua}: ${nome}`);
    // Il fornitore si chiama «Bionic / LM Studio» nel selettore: la frase non lo spezza in due nomi.
    assert.ok((d["ia.provider.bionic"] ?? "").includes("LM Studio"), lingua);
    assert.ok(testo.includes(d["settings.models.title"] ?? "?"), `${lingua}: la scheda dei modelli`);
  }
});
