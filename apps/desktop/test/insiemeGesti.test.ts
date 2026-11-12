import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type { AssociazioneInsieme, SincroniaInsieme } from "../src/ipc";
import * as insieme from "../src/insieme";

/*
 * «Ascolta insieme»: i gesti che chiudono la sessione di tutti, gli indirizzi
 * che non portano a nessun PC, e la regione viva della sincronia.
 *
 * Le regole stanno in `insieme.ts` e si provano pure; dove la regola è «tutti
 * e tre i posti passano di lì», si legge il sorgente del pannello, come fa
 * `impostazioni.test.ts` con l'indice delle Impostazioni.
 */

const PANNELLO = readFileSync(new URL("../src/parti/Insieme.tsx", import.meta.url), "utf8");

/** Il corpo di una funzione di primo livello del pannello, fino alla prossima. */
function corpoDi(nome: string): string {
  const inizio = PANNELLO.search(new RegExp(`\\nfunction ${nome}\\(`));
  assert.ok(inizio >= 0, `manca la funzione ${nome}`);
  const dopo = PANNELLO.slice(inizio + 1).search(/\n(?:export )?function [A-Za-z]+\(/);
  return dopo < 0 ? PANNELLO.slice(inizio) : PANNELLO.slice(inizio, inizio + 1 + dopo);
}

// ── terminare per tutti ───────────────────────────────────────────────────

test("terminare chiede conferma solo se c'è qualcun altro nella sessione", () => {
  const { chiedeConfermaPerTerminare } = insieme;
  assert.equal(typeof chiedeConfermaPerTerminare, "function");
  assert.equal(chiedeConfermaPerTerminare(0), false);
  assert.equal(chiedeConfermaPerTerminare(1), false);
  assert.equal(chiedeConfermaPerTerminare(2), true);
  assert.equal(chiedeConfermaPerTerminare(3), true);
});

test("terminare per tutti passa sempre dalla domanda: fascia, pannello, Impostazioni, interruttore", () => {
  // Il pannello e le Impostazioni chiamavano `esci(true)` dritti, e
  // l'interruttore spegneva la funzione — cioè chiudeva la sessione di tutti —
  // senza chiedere niente.
  assert.ok(!PANNELLO.includes("esci(ospito,"), "il pannello termina senza domanda");
  assert.ok(
    !PANNELLO.includes('esci(fase === "ospita",'),
    "le Impostazioni terminano senza domanda",
  );
  assert.ok(
    !/onCambia=\{\(attivo\) => comanda\(ipc\.insiemeAttiva\(attivo\)/.test(PANNELLO),
    "l'interruttore spegne la sessione senza domanda",
  );
  // `insiemeTermina` e `insiemeAttiva(false)` partono da un posto solo: il
  // gesto confermato (o senza nessuno da lasciare a metà).
  const termina = [...PANNELLO.matchAll(/esci\(true,/g)].length;
  assert.equal(termina, 1, "un solo `esci(true, …)`");
  assert.ok(corpoDi("eseguiTermina").includes("esci(true,"));
  assert.ok(corpoDi("eseguiTermina").includes("ipc.insiemeAttiva(false)"));
  // E i tre tasti più l'interruttore passano dalla regola.
  assert.ok([...PANNELLO.matchAll(/terminaPerTutti\(/g)].length >= 5);
});

// ── la sincronia ──────────────────────────────────────────────────────────

const sincronia = (campi: Partial<SincroniaInsieme> = {}): SincroniaInsieme =>
  ({
    agganciato: true,
    erroreMs: 3.2,
    incertezzaMs: 0.8,
    reteLenta: false,
    ...campi,
  }) as SincroniaInsieme;

test("l'annuncio della sincronia non cambia con lo scarto", () => {
  const { annuncioSincronia } = insieme;
  assert.equal(typeof annuncioSincronia, "function");
  const a = annuncioSincronia(sincronia());
  const b = annuncioSincronia(sincronia({ erroreMs: -1.1, incertezzaMs: 0.3 }));
  assert.equal(a, b);
  assert.notEqual(a, annuncioSincronia(sincronia({ agganciato: false })));
});

test("con la rete troppo lenta per seguire lo si dice, invece di «aggancio…» per sempre", () => {
  // Decisione D-31: dopo trenta secondi con ogni scambio d'orologio oltre il
  // tetto, la frase è un'altra.
  const { annuncioSincronia } = insieme;
  assert.equal(
    annuncioSincronia(sincronia({ agganciato: false, reteLenta: true })),
    "together.sync.tooSlow",
  );
  assert.equal(annuncioSincronia(sincronia({ agganciato: false })), "together.sync.locking");
});

test("nella regione viva della sincronia non c'è lo scarto", () => {
  const corpo = corpoDi("Sincronia");
  const viva = /role="status"[^>]*>([\s\S]*?)<\//.exec(corpo);
  assert.ok(viva !== null, "la regione viva c'è ancora");
  assert.ok(!viva[1]!.includes("pezzi"), "la regione viva ridice scarto e incertezza");
  assert.ok(!viva[1]!.includes("together.sync.offset"));
});

// ── il fuoco della tastiera ───────────────────────────────────────────────

test("«Invita», una volta premuto, non si spegne con `disabled` e non perde il fuoco", () => {
  // R13-13: `disabled` sul tasto appena premuto da tastiera manda il fuoco
  // sul documento (come R12-6). `aria-disabled`, e il clic in più si ignora.
  const corpo = corpoDi("DaInvitare");
  assert.ok(!/\sdisabled=\{/.test(corpo), "«Invita» si spegne con `disabled`");
  assert.ok(corpo.includes("aria-disabled={giaInvitato"));
  assert.ok(corpo.includes("invitati.has(fidato.id)) return"), "il secondo clic si ignora");
});

// ── l'associazione chiesta da un altro PC (02-14) ─────────────────────────

const associazione = (campi: Partial<AssociazioneInsieme> = {}): AssociazioneInsieme => ({
  id: 7,
  nome: "Portatile di Anna",
  dichiarato: "DESKTOP-7GH2K",
  codice: null,
  confermato: false,
  chiesta: true,
  giaAssociato: false,
  omonimo: false,
  scadeMs: 0,
  ...campi,
});

test("la riga di una richiesta dice il nome dichiarato, o quello scelto qui per un associato", () => {
  const { domandaDiAssociazione, titoloDelCodice } = insieme;
  assert.deepEqual(domandaDiAssociazione(associazione()), {
    chiave: "together.pairing.request",
    nome: "DESKTOP-7GH2K",
  });
  assert.deepEqual(domandaDiAssociazione(associazione({ giaAssociato: true })), {
    chiave: "together.pairing.requestAgain",
    nome: "Portatile di Anna",
  });
  assert.equal(titoloDelCodice(associazione()), "together.code.title");
  assert.equal(titoloDelCodice(associazione({ giaAssociato: true })), "together.code.titleAgain");
});

test("un nome scelto da un altro PC si isola dal testo intorno", () => {
  // R26-7: nel titolo del dialogo e negli avvisi, dove c'è solo testo.
  assert.equal(insieme.isolato("PC di Marco"), "⁨PC di Marco⁩");
});

test("una richiesta d'associazione è una riga, non il dialogo, e il nome sta in un <bdi>", () => {
  // 02-14: il dialogo modale del codice si apriva da solo davanti a chi
  // ospitava. Adesso è montato solo quando la riga è già stata aperta.
  assert.ok(
    PANNELLO.includes("stato?.associazione != null && !stato.associazione.chiesta && ("),
    "il dialogo del codice si monta anche per una richiesta non aperta",
  );
  const riga = corpoDi("RichiestaDiAssociazione");
  assert.ok(riga.includes("<bdi>{nome}</bdi>"));
  assert.ok(riga.includes("ipc.insiemeCodice(associazione.id, si)"));
  assert.ok(corpoDi("InAttesa").includes("<RichiestaDiAssociazione"));
  // Il dialogo dice il nome isolato, e con un omonimo «No» è il primario.
  const dialogo = corpoDi("DialogoCodice");
  assert.ok(dialogo.includes("isolato("));
  assert.ok(dialogo.includes('daDecidere && omonimo ? "bottone primario" : "bottone"'));
});

test("chi ospita vede il nome scelto da lui e, tra parentesi, quello dichiarato", () => {
  // D-106 (R26-6): due membri che si presentavano entrambi come «Fisso di
  // Marco» erano due righe uguali con «Allontana» accanto.
  assert.equal(
    insieme.nomeDellaPersona({ nome: "Portatile di Anna", dichiarato: "Fisso di Marco" }),
    "Portatile di Anna (Fisso di Marco)",
  );
  assert.equal(insieme.nomeDellaPersona({ nome: "Sala", dichiarato: null }), "Sala");
  const persone = corpoDi("Persone");
  assert.ok(persone.includes("<bdi>{persona.dichiarato}</bdi>"));
});

// ── gli indirizzi scritti a mano ──────────────────────────────────────────

test("un indirizzo che non può essere di un PC ha la sua frase, non quella della forma", () => {
  for (const speciale of ["0.0.0.0", "224.0.0.251:47811", "255.255.255.255"]) {
    assert.deepEqual(
      insieme.aggiungiIndirizzo([], speciale),
      { elenco: null, errore: "together.addresses.notHost" },
      speciale,
    );
  }
  assert.deepEqual(insieme.aggiungiIndirizzo([], "192.168.1.300"), {
    elenco: null,
    errore: "together.addresses.invalid",
  });
});
