import { test } from "node:test";
import assert from "node:assert/strict";

import type {
  FidatoInsieme,
  InsiemeNelLettore,
  StatoInsieme,
  VicinoInsieme,
} from "../src/ipc";
import {
  MASSIMO_INDIRIZZI,
  MASSIMO_NOME,
  PORTA,
  aggiungiIndirizzo,
  ancoraValidi,
  branoDaMostrare,
  avvisoEGuasto,
  causeIrraggiungibile,
  chiaveAvviso,
  codiceDaLeggere,
  codiceLeggibile,
  daInvitare,
  fidatiCheOspitano,
  fraseNessunVicino,
  fraseProfiloRete,
  gestoPermesso,
  idDiLibreria,
  invitiAncoraAperti,
  leggiIndirizzo,
  mappaProprietari,
  nomePulito,
  ordinaFidati,
  ordinaVicini,
  ospitanti,
  pezziSegno,
  pezziVoce,
  pezzoSessione,
  proprietarioDellaRiga,
  righeDaChiedere,
  secondiRestanti,
} from "../src/insieme";

// ── i pezzi finti ─────────────────────────────────────────────────────────

const vicino = (campi: Partial<VicinoInsieme> = {}): VicinoInsieme => ({
  chiave: "aa",
  impronta: "aaaa aaaa",
  nome: "PC",
  pari: null,
  bloccato: false,
  ...campi,
});

const fidato = (campi: Partial<FidatoInsieme> = {}): FidatoInsieme => ({
  id: 1,
  nome: "PC",
  impronta: "aaaa aaaa",
  bloccato: false,
  associatoMs: 0,
  vistoMs: null,
  presente: false,
  ospita: false,
  inSessione: false,
  ...campi,
});

const stato = (campi: Partial<StatoInsieme> = {}): StatoInsieme => ({
  attivo: true,
  nome: "Io",
  fase: "libera",
  porta: null,
  firewallSpiegato: false,
  sessione: null,
  vicini: [],
  fidati: [],
  associazione: null,
  inviti: [],
  richieste: [],
  indirizzi: [],
  sincronia: null,
  indirizziDiQui: [],
  ...campi,
});

const nelLettore = (campi: Partial<InsiemeNelLettore> = {}): InsiemeNelLettore => ({
  ospita: false,
  host: "Marco",
  persone: 3,
  proprietario: null,
  locale: false,
  agganciato: true,
  diritti: "ascolta",
  calibrazione: false,
  revisione: 0,
  ...campi,
});

// ── id ─────────────────────────────────────────────────────────────────────

test("solo un id positivo è una riga della libreria di questo PC", () => {
  assert.equal(idDiLibreria(7), true);
  assert.equal(idDiLibreria(-7), false);
  assert.equal(idDiLibreria(0), false);
  // Un id che non è un intero non è niente: nemmeno un brano.
  assert.equal(idDiLibreria(1.5), false);
  assert.equal(idDiLibreria(Number.NaN), false);
});

// ── codice ─────────────────────────────────────────────────────────────────

test("il codice di quattro cifre si legge in due coppie", () => {
  assert.equal(codiceLeggibile("4821"), "48 21");
  assert.equal(codiceLeggibile(" 0007 "), "00 07");
});

test("quel che non è un codice di quattro cifre resta com'è", () => {
  assert.equal(codiceLeggibile("482"), "482");
  assert.equal(codiceLeggibile("48 21"), "48 21");
  assert.equal(codiceLeggibile("abcd"), "abcd");
});

test("chi ascolta lo schermo sente le cifre una per una", () => {
  assert.equal(codiceDaLeggere("4821"), "4 8 2 1");
  assert.equal(codiceDaLeggere("48 21"), "4 8 2 1");
});

// ── scadenze ───────────────────────────────────────────────────────────────

test("il conto alla rovescia arrotonda per eccesso e non scende sotto zero", () => {
  assert.equal(secondiRestanti(60_000, 0), 60);
  assert.equal(secondiRestanti(60_000, 800), 60);
  assert.equal(secondiRestanti(60_000, 59_001), 1);
  assert.equal(secondiRestanti(60_000, 60_000), 0);
  assert.equal(secondiRestanti(60_000, 75_000), 0);
});

test("un invito scaduto sparisce subito, anche se il nucleo non l'ha ancora tolto", () => {
  const inviti = [
    { id: 1, scadeMs: 1_000 },
    { id: 2, scadeMs: 5_000 },
  ];
  assert.deepEqual(
    ancoraValidi(inviti, 1_000).map((i) => i.id),
    [2],
  );
  assert.deepEqual(ancoraValidi(inviti, 0).map((i) => i.id), [1, 2]);
  assert.deepEqual(ancoraValidi(inviti, 9_000), []);
});

// ── ordinamenti ────────────────────────────────────────────────────────────

test("i vicini: prima gli associati, poi gli sconosciuti, i bloccati in fondo", () => {
  const ordinati = ordinaVicini([
    vicino({ chiave: "b", nome: "Bloccato", pari: 3, bloccato: true }),
    vicino({ chiave: "s", nome: "Sconosciuto" }),
    vicino({ chiave: "a", nome: "Zeta", pari: 1 }),
    vicino({ chiave: "c", nome: "Alfa", pari: 2 }),
  ]).map((v) => v.chiave);
  assert.deepEqual(ordinati, ["c", "a", "s", "b"]);
});

test("i vicini senza nome vanno dopo quelli col nome, e i nomi uguali per impronta", () => {
  const ordinati = ordinaVicini([
    vicino({ chiave: "x", nome: null, impronta: "0000" }),
    vicino({ chiave: "u2", nome: "Ufficio", impronta: "ffff" }),
    vicino({ chiave: "u1", nome: "ufficio", impronta: "1111" }),
  ]).map((v) => v.chiave);
  assert.deepEqual(ordinati, ["u1", "u2", "x"]);
});

test("i nomi con i numeri si ordinano come numeri", () => {
  const ordinati = ordinaVicini([
    vicino({ chiave: "10", nome: "PC 10" }),
    vicino({ chiave: "2", nome: "PC 2" }),
  ]).map((v) => v.chiave);
  assert.deepEqual(ordinati, ["2", "10"]);
});

test("i fidati: in sessione, poi chi ospita, poi i collegati, i bloccati in fondo", () => {
  const ordinati = ordinaFidati([
    fidato({ id: 1, nome: "Spento" }),
    fidato({ id: 2, nome: "Bloccato", bloccato: true, presente: true }),
    fidato({ id: 3, nome: "Collegato", presente: true }),
    fidato({ id: 4, nome: "Ospita", presente: true, ospita: true }),
    fidato({ id: 5, nome: "Dentro", presente: true, inSessione: true }),
  ]).map((f) => f.id);
  assert.deepEqual(ordinati, [5, 4, 3, 1, 2]);
});

test("l'elenco ordinato è una copia: quello del nucleo non si tocca", () => {
  const originale = [fidato({ id: 2, nome: "B" }), fidato({ id: 1, nome: "A" })];
  ordinaFidati(originale);
  assert.deepEqual(
    originale.map((f) => f.id),
    [2, 1],
  );
});

test("si invitano solo i collegati che non sono già dentro e non sono bloccati", () => {
  const s = stato({
    fase: "ospita",
    fidati: [
      fidato({ id: 1, presente: false }),
      fidato({ id: 2, presente: true }),
      fidato({ id: 3, presente: true, inSessione: true }),
      fidato({ id: 4, presente: true, bloccato: true }),
    ],
  });
  assert.deepEqual(
    daInvitare(s).map((f) => f.id),
    [2],
  );
});

test("gli ospitanti: i vicini, più gli associati che ospitano e la scoperta non vede", () => {
  const s = stato({
    vicini: [vicino({ chiave: "k1", nome: "Visto", pari: 1 })],
    fidati: [
      fidato({ id: 1, nome: "Visto", presente: true, ospita: true }),
      fidato({ id: 2, nome: "A mano", presente: true, ospita: true }),
      fidato({ id: 3, nome: "Non ospita", presente: true }),
      fidato({ id: 4, nome: "Spento", ospita: true }),
    ],
  });
  const elenco = ospitanti(s);
  assert.deepEqual(
    elenco.map((v) => v.pari),
    [2, 1],
  );
  // Quello aggiunto dagli associati non si associa: si chiede, e la chiave
  // resta vuota.
  assert.equal(elenco[0]?.chiave, "");
});

test("un vicino associato che non ospita più sparisce, anche se la scoperta lo ricorda", () => {
  const s = stato({
    vicini: [
      vicino({ chiave: "chiuso", nome: "Ha chiuso", pari: 1 }),
      vicino({ chiave: "scollegato", nome: "Scollegato", pari: 2 }),
      vicino({ chiave: "sparito", nome: "Dimenticato", pari: 9 }),
      vicino({ chiave: "vivo", nome: "Ospita", pari: 3 }),
      vicino({ chiave: "ignoto", nome: "Sconosciuto" }),
    ],
    fidati: [
      fidato({ id: 1, presente: true, ospita: false }),
      fidato({ id: 2, presente: false, ospita: true }),
      fidato({ id: 3, presente: true, ospita: true }),
    ],
  });
  // Lo sconosciuto resta: di lui si sa solo quel che dice la scoperta. Un
  // associato che il nucleo non elenca più (id 9) non si può chiedere, e va.
  assert.deepEqual(
    ospitanti(s).map((v) => v.chiave),
    ["vivo", "ignoto"],
  );
});

test("un vicino bloccato resta in fondo, senza tasto, anche se non è collegato", () => {
  const s = stato({
    vicini: [vicino({ chiave: "b", nome: "Bloccato", pari: 4, bloccato: true })],
    fidati: [fidato({ id: 4, bloccato: true, presente: false })],
  });
  assert.deepEqual(
    ospitanti(s).map((v) => v.chiave),
    ["b"],
  );
});

test("chi ospita già, fra gli associati: collegati, non bloccati, non già con me", () => {
  const s = stato({
    fidati: [
      fidato({ id: 1, nome: "Spento", ospita: true }),
      fidato({ id: 2, nome: "Bloccato", presente: true, ospita: true, bloccato: true }),
      fidato({ id: 3, nome: "Zeta", presente: true, ospita: true }),
      fidato({ id: 4, nome: "Alfa", presente: true, ospita: true }),
      fidato({ id: 5, nome: "Dentro", presente: true, ospita: true, inSessione: true }),
      fidato({ id: 6, nome: "Non ospita", presente: true }),
    ],
  });
  assert.deepEqual(
    fidatiCheOspitano(s).map((f) => f.id),
    [4, 3],
  );
});

test("gli inviti mandati: scadono, e cadono con chi non si può più invitare", () => {
  const s = stato({
    fase: "ospita",
    fidati: [
      fidato({ id: 1, presente: true }),
      fidato({ id: 2, presente: true, inSessione: true }),
      fidato({ id: 3, presente: false }),
      fidato({ id: 4, presente: true }),
    ],
  });
  const invitati = new Map([
    [1, 60_000],
    [2, 60_000],
    [3, 60_000],
    [4, 1_000],
  ]);
  assert.deepEqual([...invitiAncoraAperti(invitati, s, 5_000).keys()], [1]);
  // Finita la sessione di questo PC, nessun invito vale più.
  assert.equal(invitiAncoraAperti(invitati, stato({ fase: "libera" }), 0).size, 0);
  assert.equal(invitiAncoraAperti(invitati, null, 0).size, 0);
});

test("gli inviti mandati che valgono tutti restano la stessa mappa", () => {
  const s = stato({ fase: "ospita", fidati: [fidato({ id: 1, presente: true })] });
  const invitati = new Map([[1, 60_000]]);
  assert.equal(invitiAncoraAperti(invitati, s, 0), invitati);
});

// ── cause e rete ───────────────────────────────────────────────────────────

test("le cause: il firewall dell'host viene prima della rete", () => {
  const cause = causeIrraggiungibile(0);
  assert.ok(
    cause.indexOf("together.causes.hostFirewall") <
      cause.indexOf("together.causes.sameNetwork"),
  );
  assert.equal(cause.at(-1), "together.causes.version");
});

test("le cause: a chi ha già scritto degli indirizzi si dice di controllarli", () => {
  assert.ok(causeIrraggiungibile(0).includes("together.causes.addresses.none"));
  assert.ok(!causeIrraggiungibile(0).includes("together.causes.addresses.some"));
  assert.ok(causeIrraggiungibile(2).includes("together.causes.addresses.some"));
  assert.ok(!causeIrraggiungibile(2).includes("together.causes.addresses.none"));
});

test("l'elenco vuoto: «per ora» all'inizio, poi cosa controllare", () => {
  assert.equal(fraseNessunVicino(false, 0), "together.nearby.none");
  assert.equal(fraseNessunVicino(false, 3), "together.nearby.none");
  assert.equal(fraseNessunVicino(true, 0), "together.nearby.none.still");
  assert.equal(fraseNessunVicino(true, 3), "together.nearby.none.still.addresses");
});

test("la frase sulla rete parla del firewall solo a chi ospita", () => {
  assert.equal(fraseProfiloRete(true), "together.network.host");
  assert.equal(fraseProfiloRete(false), "together.network.member");
});

// ── il segno ───────────────────────────────────────────────────────────────

test("il segno della sessione: chi segue dice con chi, chi ospita dice che ospita", () => {
  assert.deepEqual(pezzoSessione(nelLettore()), {
    chiave: "together.sign.with",
    valori: { nome: "Marco", n: 3 },
  });
  assert.deepEqual(pezzoSessione(nelLettore({ ospita: true, persone: 1 })), {
    chiave: "together.sign.hosting",
    valori: { n: 1 },
  });
});

test("una voce di questo PC non aggiunge niente", () => {
  assert.deepEqual(pezziVoce(nelLettore({ ospita: true, diritti: "tutto" })), []);
});

test("una voce di un altro arriva in streaming da lui", () => {
  assert.deepEqual(
    pezziVoce(nelLettore({ proprietario: "Marco" })).map((p) => p.chiave),
    ["together.sign.streaming"],
  );
});

test("una copia trovata qui dice di chi è e che si legge da un file locale", () => {
  assert.deepEqual(
    pezziVoce(nelLettore({ proprietario: "Marco", locale: true })).map((p) => p.chiave),
    ["together.sign.owner", "together.sign.local"],
  );
});

test("chi segue e non è agganciato lo vede; chi ospita mai", () => {
  assert.deepEqual(
    pezziVoce(nelLettore({ agganciato: false })).map((p) => p.chiave),
    ["together.sign.locking"],
  );
  assert.deepEqual(pezziVoce(nelLettore({ ospita: true, agganciato: false })), []);
});

test("il segno intero mette l'aggancio in testa, prima del nome lungo dell'host", () => {
  assert.deepEqual(
    pezziSegno(nelLettore({ agganciato: false, proprietario: "Marco" })).map((p) => p.chiave),
    ["together.sign.locking", "together.sign.with", "together.sign.streaming"],
  );
  // Agganciato, l'ordine è quello di sempre: chi, poi di chi è la voce.
  assert.deepEqual(
    pezziSegno(nelLettore({ proprietario: "Marco", locale: true })).map((p) => p.chiave),
    ["together.sign.with", "together.sign.owner", "together.sign.local"],
  );
  assert.deepEqual(
    pezziSegno(nelLettore({ ospita: true, agganciato: false })).map((p) => p.chiave),
    ["together.sign.hosting"],
  );
});

// ── le righe della coda ────────────────────────────────────────────────────

test("si chiedono solo le righe che mancano", () => {
  const inMano = new Set([1, 2, -3]);
  assert.deepEqual(
    righeDaChiedere([1, 2, -3, 4, -5], (id) => inMano.has(id), false),
    [4, -5],
  );
});

test("con le voci cambiate si richiedono le voci della sessione, e solo loro", () => {
  const inMano = new Set([1, 2, -3, -4]);
  assert.deepEqual(
    righeDaChiedere([1, -3, 2, -4, -3, 7], (id) => inMano.has(id), true),
    [-3, -4, 7],
  );
});

// ── gesti ──────────────────────────────────────────────────────────────────

test("fuori sessione ogni gesto si può", () => {
  for (const gesto of [
    "trasporto",
    "coda",
    "svuota",
    "mescola",
    "ripristina",
    "radio",
    "autoplay",
    "timer",
  ] as const) {
    assert.equal(gestoPermesso(null, gesto), true, gesto);
  }
});

test("in sessione il casuale non si può, per nessuno", () => {
  assert.equal(gestoPermesso(nelLettore({ ospita: true, diritti: "tutto" }), "mescola"), false);
  assert.equal(gestoPermesso(nelLettore({ diritti: "tutto" }), "mescola"), false);
});

test("in sessione l'«Annulla» della coda non c'è, per nessuno", () => {
  assert.equal(gestoPermesso(nelLettore({ ospita: true, diritti: "tutto" }), "ripristina"), false);
  assert.equal(gestoPermesso(nelLettore({ diritti: "tutto" }), "ripristina"), false);
});

test("chi segue con `tutto` ha trasporto e coda come chi ospita", () => {
  const membro = nelLettore({ diritti: "tutto" });
  const host = nelLettore({ ospita: true, diritti: "tutto" });
  for (const gesto of ["trasporto", "coda"] as const) {
    assert.equal(gestoPermesso(membro, gesto), gestoPermesso(host, gesto), gesto);
    assert.equal(gestoPermesso(membro, gesto), true, gesto);
  }
  // E niente di più: svuotare, mescolare e annullare restano come prima.
  assert.equal(gestoPermesso(membro, "svuota"), false);
  assert.equal(gestoPermesso(membro, "mescola"), false);
  assert.equal(gestoPermesso(membro, "ripristina"), false);
});

test("in sessione radio, autoplay e timer non ci sono, per nessuno dei due ruoli", () => {
  for (const insieme of [nelLettore({ ospita: true, diritti: "tutto" }), nelLettore({ diritti: "tutto" })]) {
    for (const gesto of ["radio", "autoplay", "timer"] as const) {
      assert.equal(gestoPermesso(insieme, gesto), false, gesto);
    }
  }
});

test("svuotare la coda resta di chi ospita", () => {
  assert.equal(gestoPermesso(nelLettore({ ospita: true, diritti: "tutto" }), "svuota"), true);
  assert.equal(gestoPermesso(nelLettore({ diritti: "tutto" }), "svuota"), false);
});

test("trasporto e coda seguono i diritti che chi ospita ha concesso", () => {
  const attese = {
    ascolta: { trasporto: false, coda: false },
    comanda: { trasporto: true, coda: false },
    tutto: { trasporto: true, coda: true },
  } as const;
  for (const [diritti, attesa] of Object.entries(attese)) {
    const insieme = nelLettore({ diritti: diritti as keyof typeof attese });
    assert.equal(gestoPermesso(insieme, "trasporto"), attesa.trasporto, diritti);
    assert.equal(gestoPermesso(insieme, "coda"), attesa.coda, diritti);
    // Svuotare resta di chi ospita, a ogni livello.
    assert.equal(gestoPermesso(insieme, "svuota"), false, diritti);
  }
});

test("«di Marco» su ogni riga che il nucleo elenca, e mai su una riga della libreria", () => {
  const proprietari = mappaProprietari([
    { id: -4, nome: "Marco" },
    { id: -9, nome: "Giulia" },
  ]);
  assert.equal(proprietarioDellaRiga(-4, proprietari), "Marco");
  assert.equal(proprietarioDellaRiga(-9, proprietari), "Giulia");
  // Una voce di questo PC non è nell'elenco: niente etichetta.
  assert.equal(proprietarioDellaRiga(-5, proprietari), null);
  // Una riga della libreria non si cerca nemmeno, anche se il numero ci fosse.
  assert.equal(proprietarioDellaRiga(4, mappaProprietari([{ id: 4, nome: "X" }])), null);
  assert.equal(proprietarioDellaRiga(-4, mappaProprietari([])), null);
});

test("la voce di calibrazione si scrive nella lingua di chi guarda", () => {
  const brano = { id: -1, title: "Calibrazione", artist: "Rymembra", album: "" };
  const tr = (chiave: string) => `«${chiave}»`;
  const mostrato = branoDaMostrare(brano, nelLettore({ calibrazione: true }), tr);
  assert.deepEqual(mostrato, {
    id: -1,
    title: "«together.calibrate.voice.title»",
    artist: "«together.calibrate.voice.artist»",
    album: "«together.calibrate.voice.album»",
  });
});

test("fuori dalla calibrazione il brano è lo stesso, con la stessa identità", () => {
  const brano = { id: -1, title: "Titolo", artist: "Artista", album: "Album" };
  const tr = (chiave: string) => chiave;
  assert.equal(branoDaMostrare(brano, nelLettore(), tr), brano);
  assert.equal(branoDaMostrare(brano, null, tr), brano);
});

// ── avvisi ─────────────────────────────────────────────────────────────────

test("si mostrano solo gli avvisi delle due famiglie del contratto", () => {
  assert.equal(chiaveAvviso("together.notice.joined"), "together.notice.joined");
  assert.equal(
    chiaveAvviso("errors.insieme.connessioneCaduta"),
    "errors.insieme.connessioneCaduta",
  );
  assert.equal(chiaveAvviso("errors.fs.notFound"), null);
  assert.equal(chiaveAvviso("settings.title"), null);
});

test("una chiave che non è fatta come una chiave non passa", () => {
  assert.equal(chiaveAvviso("together.notice.Marco ha detto ciao"), null);
  assert.equal(chiaveAvviso(""), null);
  assert.equal(chiaveAvviso("together.notice.{x}"), null);
});

test("gli errori sono guasti, le notizie no", () => {
  assert.equal(avvisoEGuasto("errors.insieme.connessioneCaduta"), true);
  assert.equal(avvisoEGuasto("together.notice.joined"), false);
});

test("i codici nuovi del nucleo passano, e sono guasti: nessuna lista chiusa li scarta", () => {
  for (const codice of [
    "estraneoScaduto",
    "associazioneScaduta",
    "associazioneOccupata",
    "superato",
  ]) {
    const chiave = `errors.insieme.${codice}`;
    assert.equal(chiaveAvviso(chiave), chiave, codice);
    assert.equal(avvisoEGuasto(chiave), true, codice);
  }
});

// ── nomi e indirizzi ───────────────────────────────────────────────────────

test("il nome perde i caratteri di controllo e gli spazi di troppo", () => {
  assert.equal(nomePulito("  PC\tdi\n\nMarco  "), "PC di Marco");
  assert.equal(nomePulito("\u0007Ufficio"), "Ufficio");
  assert.equal(nomePulito("   "), "");
});

test("il nome si taglia a sessantaquattro caratteri, non a metà di un'emoji", () => {
  const lungo = "🎵".repeat(MASSIMO_NOME + 10);
  const tagliato = nomePulito(lungo);
  assert.equal([...tagliato].length, MASSIMO_NOME);
  assert.ok(tagliato.endsWith("🎵"));
});

test("un indirizzo senza porta prende quella di serie", () => {
  assert.equal(leggiIndirizzo("192.168.1.20"), `192.168.1.20:${PORTA}`);
  assert.equal(leggiIndirizzo(" 10.0.0.5:47812 "), "10.0.0.5:47812");
  assert.equal(leggiIndirizzo("127.0.0.1:1"), "127.0.0.1:1");
  assert.equal(leggiIndirizzo("0.0.0.0:65535"), "0.0.0.0:65535");
});

test("un indirizzo che non si legge si rifiuta", () => {
  for (const cattivo of [
    "",
    "192.168.1",
    "192.168.1.300",
    "192.168.01.2",
    "192.168.1.2:0",
    "192.168.1.2:65536",
    "192.168.1.2:047811",
    "192.168.1.2:",
    "pc-di-marco:47811",
    "[::1]:47811",
    "1.2.3.4.5",
  ]) {
    assert.equal(leggiIndirizzo(cattivo), null, cattivo);
  }
});

test("aggiungere: la forma del nucleo, il doppione riconosciuto, il tetto", () => {
  const primo = aggiungiIndirizzo([], "10.0.0.5");
  assert.deepEqual(primo, { elenco: [`10.0.0.5:${PORTA}`], errore: null });

  const doppione = aggiungiIndirizzo([`10.0.0.5:${PORTA}`], `10.0.0.5:${PORTA}`);
  assert.deepEqual(doppione, { elenco: null, errore: "together.addresses.duplicate" });

  const pieno = Array.from({ length: MASSIMO_INDIRIZZI }, (_, i) => `10.0.0.${i + 1}:${PORTA}`);
  assert.deepEqual(aggiungiIndirizzo(pieno, "10.0.1.1"), {
    elenco: null,
    errore: "together.addresses.full",
  });

  assert.deepEqual(aggiungiIndirizzo([], "boh"), {
    elenco: null,
    errore: "together.addresses.invalid",
  });
});
