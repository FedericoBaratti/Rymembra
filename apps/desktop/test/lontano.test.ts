/**
 * Il collegamento via internet nella finestra (`src/lontano.ts`): il QR
 * disegnato dalla matrice, il link pescato da un testo incollato, l'ordine dei
 * dispositivi, e il gesto che sui dati mobili aspetta un sì invece di fallire.
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

import type {
  AvanzamentoSpecchio,
  DispositivoLontano,
  DispositivoSincronia,
  FidatoInsieme,
  ScaricoLontano,
  StatoLontano,
} from "../src/ipc";
import {
  archivioDatiMobili,
  archivioLontano,
  archivioSpecchio,
  chiediLaFotografiaDelloSpecchio,
  chiedeIDatiMobili,
  conIlSiDeiDatiMobili,
  eventoDelloSpecchio,
  gestoLontano,
  annullatoDaQui,
  annunciDelloSpecchio,
  causeDiRilettura,
  chiaveDelTestoErrore,
  frasiDelDimentica,
  inErrore,
  linkDa,
  ordinaDispositivi,
  qrScaduto,
  qrSoloInVista,
  percentualeScarico,
  percorsoQr,
  NON_AUDIO,
  scaricoConNonAudio,
  scaricoFinito,
  serveAccendere,
  unaAllaVolta,
  unisciDispositivi,
} from "../src/lontano";

function dispositivo(nome: string, campi: Partial<DispositivoLontano> = {}): DispositivoLontano {
  return {
    chiave: nome.toLowerCase(),
    nome,
    piattaforma: "windows",
    dispositivo: nome,
    online: false,
    vistoMs: null,
    associatoMs: 0,
    revocato: false,
    versione: null,
    insiemeAcceso: false,
    insiemeOspita: false,
    insieme: null,
    via: null,
    ...campi,
  };
}

function stato(campi: Partial<StatoLontano> = {}): StatoLontano {
  return {
    attivo: true,
    acceso: true,
    chiave: null,
    impronta: null,
    relay: null,
    datiMobili: "chiedi",
    errore: null,
    dispositivi: [],
    invito: null,
    richieste: [],
    ...campi,
  };
}

test("collegarsi o mostrare il QR a collegamento spento chiede prima di accenderlo", () => {
  assert.equal(serveAccendere(stato({ attivo: false })), true);
  // Il pannello non c'è ancora: non si sa, e si chiede.
  assert.equal(serveAccendere(null), true);
  assert.equal(serveAccendere(stato({ attivo: true })), false);
  // Sul telefono può essere acceso l'interruttore e spento il nodo: lì basta
  // il sì di prima, il nucleo lo sveglia.
  assert.equal(serveAccendere(stato({ attivo: true, acceso: false })), false);
});

test("il QR: una riga di moduli scuri di fila è un rettangolo solo", () => {
  // 3×3: «110 / 000 / 101».
  assert.equal(percorsoQr("110000101", 3), "M0 0h2v1h-2zM0 2h1v1h-1zM2 2h1v1h-1z");
});

test("il QR: una matrice corta o un lato nullo non disegnano niente", () => {
  assert.equal(percorsoQr("1111", 3), "");
  assert.equal(percorsoQr("", 0), "");
  assert.equal(percorsoQr("000000000", 3), "");
});

test("il link si pesca dal testo incollato, con quel che c'è intorno", () => {
  assert.equal(linkDa("  rymembra://collega/AbC-_9 \n"), "rymembra://collega/AbC-_9");
  assert.equal(
    linkDa("Ecco il link: rymembra://collega/xyz. Ciao"),
    "rymembra://collega/xyz",
  );
  assert.equal(linkDa("https://example.com/collega/xyz"), null);
  assert.equal(linkDa("rymembra://collega/"), null);
});

test("i dispositivi: online prima, poi per nome, i revocati in fondo", () => {
  const ordinati = ordinaDispositivi([
    dispositivo("Zeta", { revocato: true, online: true }),
    dispositivo("Beta"),
    dispositivo("Gamma", { online: true }),
    dispositivo("Alfa"),
  ]);
  assert.deepEqual(
    ordinati.map((d) => d.nome),
    ["Gamma", "Alfa", "Beta", "Zeta"],
  );
});

test("la rete a consumo chiede solo con «Chiedi»", () => {
  const errore = { code: "lontano.reteAConsumo", serviceText: null };
  assert.equal(chiedeIDatiMobili(errore, stato()), true);
  assert.equal(chiedeIDatiMobili(errore, stato({ datiMobili: "no" })), false);
  assert.equal(chiedeIDatiMobili({ code: "lontano.spento" }, stato()), false);
  assert.equal(chiedeIDatiMobili("lontano.reteAConsumo", stato()), false);
  assert.equal(chiedeIDatiMobili(errore, null), false);
});

test("un gesto sui dati mobili si mette da parte invece di fallire", async () => {
  archivioLontano.scrivi(stato());
  archivioDatiMobili.scrivi(null);
  const errori: unknown[] = [];
  let tentativi = 0;
  const fai = () => {
    tentativi += 1;
    return Promise.reject({ code: "lontano.reteAConsumo", serviceText: null });
  };
  await gestoLontano(fai, (e) => errori.push(e));
  assert.equal(errori.length, 0);
  assert.equal(tentativi, 1);
  const messo = archivioDatiMobili.leggi();
  assert.ok(messo !== null, "il gesto aspetta la risposta");

  // Con «No» l'errore passa, e dice perché.
  archivioDatiMobili.scrivi(null);
  archivioLontano.scrivi(stato({ datiMobili: "no" }));
  await gestoLontano(fai, (e) => errori.push(e));
  assert.equal(errori.length, 1);
  assert.equal(archivioDatiMobili.leggi(), null);
  archivioLontano.scrivi(null);
});

function scarico(campi: Partial<ScaricoLontano> = {}): ScaricoLontano {
  return {
    titolo: null,
    byte: 0,
    totale: 0,
    inCoda: 0,
    fatti: 0,
    saltati: 0,
    falliti: 0,
    errore: null,
    ...campi,
  };
}

test("«Scarica qui» è finito quando il brano in corso se ne va e la coda è vuota", () => {
  const inCorso = scarico({ titolo: "X", inCoda: 0 });
  assert.equal(scaricoFinito(inCorso, scarico({ fatti: 1 })), true);
  // Il primo stato all'apertura non è una fine: prima non c'era niente.
  assert.equal(scaricoFinito(null, scarico()), false);
  assert.equal(scaricoFinito(scarico(), scarico()), false);
  // Fra un brano e l'altro c'è sempre un titolo.
  assert.equal(scaricoFinito(inCorso, scarico({ titolo: "Y" })), false);
});

test("un file che non è musica si dice alla fine della coda", () => {
  assert.equal(scaricoConNonAudio(scarico({ falliti: 1, errore: NON_AUDIO })), true);
  // Un altro guasto resta un conto e basta.
  assert.equal(scaricoConNonAudio(scarico({ falliti: 1, errore: "lontano.fileCambiato" })), false);
  assert.equal(scaricoConNonAudio(scarico({ falliti: 0, errore: NON_AUDIO })), false);
});

test("la percentuale di un brano che scende", () => {
  assert.equal(percentualeScarico(scarico({ byte: 0, totale: 0 })), 0);
  assert.equal(percentualeScarico(scarico({ byte: 1, totale: 3 })), 33);
  assert.equal(percentualeScarico(scarico({ byte: 9, totale: 3 })), 100);
});

function fidato(id: number, nome: string): FidatoInsieme {
  return {
    id,
    nome,
    impronta: "0000",
    bloccato: false,
    associatoMs: 0,
    vistoMs: null,
    presente: false,
    ospita: false,
    inSessione: false,
  };
}

function diSincronia(id: string, campi: Partial<DispositivoSincronia> = {}): DispositivoSincronia {
  return { id, nome: id, fidato: true, vistoMs: null, sonoIo: false, ...campi };
}

test("l'elenco unico: un dispositivo collegato si porta dietro le sue altre due voci", () => {
  const righe = unisciDispositivi(
    [dispositivo("Telefono", { insieme: 2, dispositivo: "tel" })],
    [fidato(1, "Ufficio"), fidato(2, "Telefono")],
    [diSincronia("tel"), diSincronia("io", { sonoIo: true }), diSincronia("nas")],
  );
  assert.deepEqual(
    righe.map((r) => r.tipo),
    ["lontano", "insieme", "sincronia"],
  );
  const primo = righe[0];
  assert.ok(primo?.tipo === "lontano");
  assert.equal(primo.insieme?.id, 2);
  assert.equal(primo.sincronia?.id, "tel");
  // Il PC fidato e il dispositivo della sincronia rimasti sono gli altri.
  assert.ok(righe[1]?.tipo === "insieme" && righe[1].insieme.id === 1);
  assert.ok(righe[2]?.tipo === "sincronia" && righe[2].sincronia.id === "nas");
});

test("l'elenco unico: questo dispositivo non c'è, e niente si unisce per caso", () => {
  const righe = unisciDispositivi(
    [dispositivo("Senza", { insieme: 99, dispositivo: "" })],
    [],
    [diSincronia("", { sonoIo: true })],
  );
  assert.equal(righe.length, 1);
  const unico = righe[0];
  assert.ok(unico?.tipo === "lontano");
  assert.equal(unico.insieme, null);
  assert.equal(unico.sincronia, null);
});

test("un QR scaduto non si mostra, uno valido sì", () => {
  assert.equal(qrScaduto(1_000, 999), false);
  assert.equal(qrScaduto(1_000, 1_000), true);
  assert.equal(qrScaduto(1_000, 5_000), true);
});

const UN_QR = { link: "rymembra://collega/x", lato: 1, moduli: "1", scadeMs: 0 };

test("il QR si annulla quando la scheda che lo mostra se ne va", async () => {
  // Reperto 02-12: la scheda si smontava (un'altra sezione, le Impostazioni
  // chiuse) e il QR restava valido nel nucleo per i suoi dieci minuti, con
  // `rymembra/associa/1` aperto e nessun QR a schermo.
  const annullati: number[] = [];
  const annulla = async () => {
    annullati.push(1);
    return stato({ invito: null });
  };
  archivioLontano.scrivi(stato({ invito: UN_QR }));
  const qr = qrSoloInVista(annulla);
  qr.monta();
  qr.smonta();
  assert.equal(annullati.length, 1, "smontata col QR in vista: si annulla");
  await Promise.resolve();
  assert.equal(archivioLontano.leggi()?.invito, null, "e il pannello lo sa subito");

  // Senza un QR in vista, smontare non chiede niente al nucleo.
  archivioLontano.scrivi(stato({ invito: null }));
  const senza = qrSoloInVista(annulla);
  senza.monta();
  senza.smonta();
  assert.equal(annullati.length, 1);
  archivioLontano.scrivi(null);
});

test("un QR chiesto prima e arrivato a scheda chiusa non resta valido", async () => {
  const annullati: number[] = [];
  const conta = async () => {
    annullati.push(1);
    return stato();
  };
  const qr = qrSoloInVista(conta);
  qr.monta();
  // In vista quando arriva: resta.
  assert.equal(await qr.crea(async () => UN_QR), UN_QR);
  assert.equal(annullati.length, 0);

  // Arriva dopo che la scheda se n'è andata: nessuno lo vede, e si annulla.
  const tardi = qrSoloInVista(conta);
  tardi.monta();
  let arriva: (invito: typeof UN_QR) => void = () => undefined;
  const inVolo = tardi.crea(() => new Promise<typeof UN_QR>((fatto) => (arriva = fatto)));
  tardi.smonta();
  arriva(UN_QR);
  await inVolo;
  assert.equal(annullati.length, 1);
  // Un'altra volta in vista (StrictMode rimonta): torna a valere.
  tardi.monta();
  await tardi.crea(async () => UN_QR);
  assert.equal(annullati.length, 1);
});

test("un QR arrivato in vista si annulla anche se il pannello non lo dice ancora", async () => {
  // Il pannello lo porta il `lontano:stato` che segue: una scheda che se ne
  // va prima non lo trova nell'archivio, ma il QR nel nucleo c'è.
  archivioLontano.scrivi(stato({ invito: null }));
  const annullati: number[] = [];
  const qr = qrSoloInVista(async () => {
    annullati.push(1);
    return stato();
  });
  qr.monta();
  await qr.crea(async () => UN_QR);
  qr.smonta();
  assert.equal(annullati.length, 1);
  archivioLontano.scrivi(null);
});

test("un collegamento annullato da qui non è un errore da dire", () => {
  assert.equal(annullatoDaQui({ code: "internal.aborted", params: {} }), true);
  assert.equal(annullatoDaQui({ code: "lontano.rifiutato", params: {} }), false);
  assert.equal(annullatoDaQui(new Error("x")), false);
  assert.equal(annullatoDaQui(null), false);
});


test("sui dati mobili con «Chiedi» il gesto aspetta il sì invece di fallire; gli altri errori salgono", async () => {
  const prima = archivioLontano.leggi();
  archivioLontano.scrivi(stato({ datiMobili: "chiedi" }));
  try {
    let chiamate = 0;
    const gesto = async () => {
      chiamate += 1;
      if (chiamate === 1) throw { code: "lontano.reteAConsumo", params: {} };
    };
    await conIlSiDeiDatiMobili(gesto);
    assert.equal(chiamate, 1);
    const rifai = archivioDatiMobili.leggi();
    assert.notEqual(rifai, null, "il gesto è in attesa del sì");
    await rifai?.();
    assert.equal(chiamate, 2, "dopo il sì si rifà lo stesso gesto");
    archivioDatiMobili.scrivi(null);

    // Un altro guasto non è una domanda: sale a chi ha chiamato.
    await assert.rejects(
      conIlSiDeiDatiMobili(async () => {
        throw { code: "lontano.irraggiungibile", params: {} };
      }),
    );
    assert.equal(archivioDatiMobili.leggi(), null);

    // Con «No» l'errore dei dati mobili sale e dice perché.
    archivioLontano.scrivi(stato({ datiMobili: "no" }));
    await assert.rejects(
      conIlSiDeiDatiMobili(async () => {
        throw { code: "lontano.reteAConsumo", params: {} };
      }),
    );
    assert.equal(archivioDatiMobili.leggi(), null);
  } finally {
    archivioLontano.scrivi(prima);
    archivioDatiMobili.scrivi(null);
  }
});

// ── S9-3: le cause di una raffica di riletture ─────────────────────────────

test("S9-3: le riletture accorpate dicono se qualcuna riguarda il filtro", () => {
  const cause = causeDiRilettura();
  // Senza cause dette (un modo di chiedere che non si è presentato) si rilegge
  // tutto: nel dubbio, i numeri dell'artista non restano vecchi.
  assert.equal(cause.preleva(), true);
  // Solo letture di dispositivi che il filtro non vede: niente da rifare.
  cause.aggiungi(false);
  cause.aggiungi(false);
  assert.equal(cause.preleva(), false);
  // Basta una causa che conta, in qualunque posto della raffica.
  cause.aggiungi(false);
  cause.aggiungi(true);
  cause.aggiungi(false);
  assert.equal(cause.preleva(), true);
  // Ogni raffica parte da zero: la causa di prima non si trascina.
  cause.aggiungi(false);
  assert.equal(cause.preleva(), false);
});

// ── S11-4: «Chiedi» si chiede da ogni gesto, e l'errore non mente ───────────

test("S11-4: dopo il sì si rifà quel che si dice, non per forza lo stesso gesto", async () => {
  // «Avanti» ha già spostato la coda sul brano che non si apre: rifare «avanti»
  // dopo il sì lo salterebbe. Chi chiama dice cosa rifare (`riprendi`).
  const prima = archivioLontano.leggi();
  archivioLontano.scrivi(stato({ datiMobili: "chiedi" }));
  try {
    const fatti: string[] = [];
    await conIlSiDeiDatiMobili(
      async () => {
        fatti.push("avanti");
        throw { code: "lontano.reteAConsumo", params: {} };
      },
      async () => {
        fatti.push("riprendi");
      },
    );
    assert.deepEqual(fatti, ["avanti"], "il gesto è in attesa del sì");
    await archivioDatiMobili.leggi()?.();
    assert.deepEqual(fatti, ["avanti", "riprendi"], "dopo il sì parte `riprendi`, non un secondo «avanti»");
  } finally {
    archivioLontano.scrivi(prima);
    archivioDatiMobili.scrivi(null);
  }
});

test("S11-4: con «Chiedi» la frase dell'errore non dice «Solo Wi-Fi è attivo»", () => {
  const chiave = "errors.lontano.reteAConsumo";
  // «Chiedi»: la frase che dice che serve il sì e dove darlo.
  assert.equal(chiaveDelTestoErrore(chiave, stato({ datiMobili: "chiedi" })), `${chiave}.chiedi`);
  // «No» è la frase di sempre; senza lo stato non si indovina.
  assert.equal(chiaveDelTestoErrore(chiave, stato({ datiMobili: "no" })), chiave);
  assert.equal(chiaveDelTestoErrore(chiave, null), chiave);
  // Gli altri errori restano com'erano.
  assert.equal(
    chiaveDelTestoErrore("errors.lontano.spento", stato({ datiMobili: "chiedi" })),
    "errors.lontano.spento",
  );
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(
      readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
    ) as Record<string, string>;
    const chiedi = testi[`${chiave}.chiedi`];
    assert.ok(chiedi, `${lingua}: manca la frase per «Chiedi»`);
    assert.doesNotMatch(chiedi, /Solo Wi-Fi|Wi-Fi only/, `${lingua}: dice un'impostazione che non è quella`);
    assert.match(testi[chiave] ?? "", /Solo Wi-Fi|Wi-Fi only/, `${lingua}: «No» dice ancora il suo nome`);
    // La frase si legge **dentro** Rymembra (fascia, avviso del brano saltato):
    // l'azione che dice è quella di qui — premere play, che adesso chiede il sì —
    // e non «apri Rymembra» o «da qui», che sono falsi dove si legge.
    assert.doesNotMatch(chiedi, /apri Rymembra|open Rymembra|da qui|from here/i, `${lingua}: manda fuori dall'app`);
    assert.match(chiedi, /play/i, `${lingua}: dice di premere play`);
  }
});

// ── S2-2, la parte della pagina: «Dimentica» dice che la cronologia si perde ──

test("S2-2: l'anteprima di «Dimentica» dice che la cronologia dei brani solo di quel dispositivo andrà persa", () => {
  // `play_history` ha la cascata sulle righe specchio, e `togli` non la sposta:
  // dimenticato il dispositivo, la cronologia dei suoi brani sparisce (quella dei
  // brani con una copia di qui passa alla copia: lo fa il nucleo). Il sottotitolo
  // diceva «Gli ascolti già mandati restano», che si leggeva come una
  // rassicurazione sulla cronologia.
  const chiavi = (a: Parameters<typeof frasiDelDimentica>[0]) => frasiDelDimentica(a).map((f) => f.chiave);
  // Un dispositivo con righe: la sua cronologia si perde.
  assert.deepEqual(chiavi({ righe: 120, playlistChePerdono: 0 }), [
    "devices.forget.sub",
    "devices.mirror.forget.history",
  ]);
  // E le playlist, se ce ne sono che perdono qualcosa, con il numero.
  assert.deepEqual(frasiDelDimentica({ righe: 120, playlistChePerdono: 3 }).slice(1), [
    { chiave: "devices.mirror.forget.loses", valori: { n: 3 } },
    { chiave: "devices.mirror.forget.history", valori: {} },
  ]);
  // Senza righe non c'è cronologia da perdere, e non si dice.
  assert.deepEqual(chiavi({ righe: 0, playlistChePerdono: 0 }), ["devices.forget.sub"]);
  // Anteprima che non è arrivata: tacere direbbe «non si perde niente».
  assert.deepEqual(chiavi(null), [
    "devices.forget.sub",
    "devices.mirror.forget.unknown",
    "devices.mirror.forget.history",
  ]);
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(
      readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
    ) as Record<string, string>;
    assert.ok(testi["devices.mirror.forget.history"], `${lingua}: manca la frase`);
    // Il sottotitolo non si legge più come una rassicurazione sulla cronologia.
    assert.doesNotMatch(testi["devices.forget.sub"] ?? "", /Gli ascolti già mandati restano|Listens already sent stay/, lingua);
  }
});

// ── S9-11: le strisce non sono regioni vive con un contatore dentro ─────────

function passo(dispositivo: string, campi: Partial<AvanzamentoSpecchio> = {}): AvanzamentoSpecchio {
  return {
    dispositivo,
    fase: "leggo",
    fatti: 0,
    totali: 50_000,
    righe: 0,
    inAttesa: 0,
    ultimoMs: null,
    errore: null,
    ...campi,
  };
}

test("S9-11: la lettura di un catalogo si annuncia all'inizio e alla fine, non a ogni passo", () => {
  // Il difetto: `role="status"` su un testo con «12.000/50.000», e un
  // `lontano:specchio` a ogni passo del catalogo: per i minuti della prima
  // lettura un lettore di schermo rileggeva il testo intero a ogni cambio. Si
  // annuncia il passaggio, non lo stato (`Annuncio.tsx`).
  const prima = { pc: passo("pc", { fatti: 0 }) };
  assert.deepEqual(annunciDelloSpecchio({}, prima), [{ tipo: "inizio", dispositivo: "pc" }]);
  // I passi in mezzo non dicono niente.
  assert.deepEqual(annunciDelloSpecchio(prima, { pc: passo("pc", { fatti: 12_000 }) }), []);
  // Finita: si dice, e una volta.
  const fine = { pc: passo("pc", { fase: "fine", fatti: 50_000, righe: 50_000 }) };
  assert.deepEqual(annunciDelloSpecchio(prima, fine), [{ tipo: "fine", dispositivo: "pc" }]);
  assert.deepEqual(annunciDelloSpecchio(fine, fine), []);
  // Interrotta: non «letta», e si dice che si è interrotta (S9-12).
  assert.deepEqual(annunciDelloSpecchio(prima, { pc: passo("pc", { fase: "errore", errore: "x" }) }), [
    { tipo: "errore", dispositivo: "pc" },
  ]);
  assert.deepEqual(
    annunciDelloSpecchio({ pc: passo("pc", { fase: "errore", errore: "x" }) }, { pc: passo("pc", { fase: "errore", errore: "x" }) }),
    [],
    "una volta sola, non a ogni evento",
  );
  // Due dispositivi: ognuno il suo.
  assert.deepEqual(
    annunciDelloSpecchio(
      { a: passo("a"), b: passo("b", { fase: "fine" }) },
      { a: passo("a", { fase: "fine" }), b: passo("b") },
    ),
    [
      { tipo: "fine", dispositivo: "a" },
      { tipo: "inizio", dispositivo: "b" },
    ],
  );
});

test("S9-11: nessun role=\"status\" sopra un testo che porta un conteggio", () => {
  const striscia = readFileSync(new URL("../src/parti/StrisciaSpecchio.tsx", import.meta.url), "utf8");
  // L'unica regione viva è quella montata sempre e vuota, col testo dell'annuncio.
  const regioni = [...striscia.matchAll(/role="status"/g)];
  assert.equal(regioni.length, 1, "una regione viva sola");
  const annuncio = striscia.slice(striscia.indexOf("function AnnuncioSpecchio"), striscia.indexOf("export function StrisciaSpecchio"));
  assert.match(annuncio, /role="status"/);
  assert.match(annuncio, /className="solo-lettori"/);
  // La striscia dello scarico (`percento`, ogni 2 MiB) e quella della lettura
  // (`fatti`/`totali`) non lo sono.
  const scarico = striscia.slice(striscia.indexOf("export function StrisciaScarico"));
  assert.doesNotMatch(scarico, /role="status"/);
  const lettura = striscia.slice(striscia.indexOf("export function StrisciaSpecchio"), striscia.indexOf("/**\n * La striscia di «Scarica qui»"));
  assert.doesNotMatch(lettura, /role="status"/);
  // Le due frasi dell'annuncio ci sono in italiano e in inglese, senza segnaposto di conteggio.
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(
      readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
    ) as Record<string, string>;
    for (const chiave of ["devices.mirror.announce.start", "devices.mirror.announce.end"]) {
      assert.ok(testi[chiave], `${lingua}: manca ${chiave}`);
      assert.doesNotMatch(testi[chiave] ?? "", /\{fatti\}|\{totali\}/, `${lingua}: ${chiave} ha un conteggio`);
    }
  }
});

// ── S9-12: la lettura del catalogo che si interrompe si dice ────────────────

test("S9-12: i dispositivi la cui lettura si è interrotta si riconoscono", () => {
  // `FaseSpecchio` ha `"errore"` da sempre, e nessun punto della finestra lo
  // guardava: la striscia spariva (non era più `leggo`), la libreria restava con
  // le righe già arrivate e nessuna spiegazione, e col filtro del dispositivo la
  // pagina diceva «non ha ancora mandato la sua musica, o non ne condivide»,
  // falso se c'era stato un errore.
  const avanzamenti = {
    a: passo("a", { fase: "errore", errore: "lontano.irraggiungibile" }),
    b: passo("b"),
    c: passo("c", { fase: "fine" }),
    d: passo("d", { fase: "nonSupportato" }),
  };
  assert.deepEqual(inErrore(avanzamenti).map((x) => x.dispositivo), ["a"]);
  assert.deepEqual(inErrore({}), []);
});

test("S9-12: la striscia e il vuoto del filtro dicono che la lettura si è interrotta, con «Riprova adesso»", () => {
  const striscia = readFileSync(new URL("../src/parti/StrisciaSpecchio.tsx", import.meta.url), "utf8");
  assert.match(striscia, /inErrore\(avanzamenti\)/);
  assert.match(striscia, /t\("devices\.mirror\.failed", \{ nome \}\)/);
  assert.match(striscia, /ipc\.lontanoSpecchioOra\(a\.dispositivo\)\.catch\(onErrore\)/);
  // Solo per un dispositivo acceso: se il PC si spegne a metà lettura la sua ultima
  // fase resta `errore` finché non torna, e la scheda «riprovo fra poco» resterebbe
  // nella pila per giorni. Le sue righe grigie dicono già che non risponde, come il
  // vuoto del filtro (che ha il ramo `online`).
  assert.match(striscia, /const inLinea = new Set\(/);
  assert.match(striscia, /inErrore\(avanzamenti\)\.filter\(\(a\) => inLinea\.has\(a\.dispositivo\)\)/);
  assert.match(striscia, /<AnnuncioSpecchio avanzamenti=\{avanzamenti\} nomi=\{nomi\} inLinea=\{inLinea\} \/>/);
  const vuoto = readFileSync(new URL("../src/parti/VuotoDelFiltro.tsx", import.meta.url), "utf8");
  const ramo = vuoto.slice(vuoto.indexOf('avanzamento?.fase === "errore"'));
  assert.ok(ramo.length > 0, "il vuoto ha un ramo per `errore`");
  assert.ok(
    vuoto.indexOf('avanzamento?.fase === "errore"') < vuoto.indexOf('t("library.origin.empty.device"'),
    "prima della frase «non ha ancora mandato la sua musica»",
  );
  assert.match(ramo.slice(0, 600), /t\("devices\.mirror\.failed", \{ nome \}\)/);
  assert.match(ramo.slice(0, 900), /ipc\.lontanoSpecchioOra\(origine\.chiave\)/);
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(
      readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
    ) as Record<string, string>;
    assert.ok(testi["devices.mirror.failed"], lingua);
    assert.match(testi["devices.mirror.failed"] ?? "", /\{nome\}/, lingua);
  }
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  assert.match(app, /<StrisciaSpecchio onErrore=\{segnalaErrore\} \/>/);
});

// ── S9-13: la fotografia dello specchio non ricopre un evento più nuovo ─────

test("S9-13: una fotografia chiesta prima di un evento e arrivata dopo non ricopre il dispositivo dell'evento", () => {
  // Il difetto: all'apertura della finestra la fotografia (`lontanoSpecchioStato`)
  // sostituiva l'intera mappa. Se l'evento `fine` di un dispositivo arrivava fra
  // la richiesta e la risposta — letta prima, con `leggo` — la risposta lo
  // ricopriva: la striscia «Leggo la libreria…» restava fino al prossimo evento
  // (cinque minuti) e `daConfermare` poteva non vedere mai un `inAttesa`.
  archivioSpecchio.scrivi({});
  const adotta = chiediLaFotografiaDelloSpecchio();
  eventoDelloSpecchio(passo("pc", { fase: "fine", fatti: 50_000, righe: 50_000, inAttesa: 3 }));
  adotta([
    passo("pc", { fase: "leggo", fatti: 100 }),
    passo("telefono", { fase: "fine", righe: 7 }),
  ]);
  const adesso = archivioSpecchio.leggi();
  assert.equal(adesso["pc"]?.fase, "fine", "l'evento più nuovo resta");
  assert.equal(adesso["pc"]?.inAttesa, 3);
  assert.equal(adesso["telefono"]?.righe, 7, "un dispositivo senza eventi si prende dalla fotografia");
  archivioSpecchio.scrivi({});
});

test("S9-13: senza eventi in mezzo la fotografia vale tutta, e `eventoDelloSpecchio` dice com'era", () => {
  archivioSpecchio.scrivi({});
  assert.equal(eventoDelloSpecchio(passo("pc", { fatti: 1 })), undefined);
  assert.equal(eventoDelloSpecchio(passo("pc", { fatti: 2 }))?.fatti, 1, "restituisce l'avanzamento di prima");
  const adotta = chiediLaFotografiaDelloSpecchio();
  adotta([passo("pc", { fase: "fine" })]);
  assert.equal(archivioSpecchio.leggi()["pc"]?.fase, "fine");
  archivioSpecchio.scrivi({});
});

test("S9-13: la fotografia dello specchio passa da un posto solo, e un guasto si dice", () => {
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  assert.match(app, /const prima = eventoDelloSpecchio\(nuovo\);/);
  assert.match(
    app,
    /const adotta = chiediLaFotografiaDelloSpecchio\(\);\s*ipc\.lontanoSpecchioStato\(\)\.then\(adotta\)\.catch\(segnalaErrore\);/,
  );
  assert.doesNotMatch(app, /daFotografia\(/);
  const conferma = readFileSync(new URL("../src/ConfermaSpecchio.tsx", import.meta.url), "utf8");
  assert.match(conferma, /chiediLaFotografiaDelloSpecchio\(\)/);
  assert.doesNotMatch(conferma, /daFotografia\(/);
});

// ── S9-9: «Scarica qui» una volta alla volta ────────────────────────────────

test("S9-9: un secondo «Scarica qui» mentre il primo prepara non parte", async () => {
  // Il difetto: fra il clic e la domanda di conferma passano 1-3 secondi o più
  // (le righe intere in pagina, poi l'anteprima del nucleo), senza un segno di
  // lavoro: un secondo clic apriva una seconda anteprima e, a conferma data, una
  // seconda coda.
  const una = unaAllaVolta();
  let fatti = 0;
  let finisci!: () => void;
  const lungo = una(async () => {
    fatti += 1;
    await new Promise<void>((r) => {
      finisci = r;
    });
  });
  // Mentre il primo lavora, il secondo si scarta e dice di essere stato scartato.
  assert.equal(await una(async () => void (fatti += 1)), false);
  assert.equal(fatti, 1);
  finisci();
  assert.equal(await lungo, true);
  // Finito il primo, si può rifare.
  assert.equal(await una(async () => void (fatti += 1)), true);
  assert.equal(fatti, 2);
  // Un lavoro che fallisce rilascia, e l'errore sale a chi ha chiamato.
  await assert.rejects(
    una(async () => {
      throw new Error("no");
    }),
  );
  assert.equal(await una(async () => void (fatti += 1)), true, "dopo un guasto si riparte");
});

test("S9-9: «Scarica qui» passa dalla guardia, dal menù e dagli altri punti, e dice che sta contando", () => {
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  assert.match(app, /const \[unoScarico\] = useState\(\(\) => unaAllaVolta\(\)\);/);
  // Il menù di una selezione grande: la guardia copre anche le righe intere in pagina.
  const menu = app.slice(app.indexOf("scaricaQui: (ids) => {"), app.indexOf("vaiAlDisco: conBrano"));
  assert.match(menu, /unoScarico\(/);
  assert.match(menu, /setNotizia\(t\("devices\.download\.counting"\)\)/);
  assert.match(menu, /ipc\.braniPerId\(ids\)/);
  // Gli altri punti passano da `scaricaQui`, che ha la stessa guardia.
  const scarica = app.slice(app.indexOf("const scaricaQui = useCallback("), app.indexOf("/**\n   * Fa partire **un** brano, da solo."));
  assert.match(scarica, /unoScarico\(\(\) => preparaLoScarico\(brani\)\)/);
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(
      readFileSync(new URL(`../src/lingue/${lingua}.json`, import.meta.url), "utf8"),
    ) as Record<string, string>;
    assert.ok(testi["devices.download.counting"], lingua);
  }
});

// ── S9-16: la domanda sulle righe tolte aspetta le Novità e il giro ─────────

test("S9-16: «Lasciali / Togli» non si apre sopra le Novità, il giro o il primo avvio", () => {
  // `Novita` e il giro guardavano `unModaleAperto()` prima di aprirsi, ma
  // `DomandaSpecchio` no: a lettura finita con righe in attesa apriva il suo
  // `alertdialog` sopra le Novità o sopra il giro, due modali impilati (il
  // fuoco rubato due volte, Escape alla cima e il fuoco che torna alla prima).
  // Aspetta la stessa condizione della proposta del sottofondo, che si decide
  // in un posto solo (`propostaPuoComparire`).
  const conferma = readFileSync(new URL("../src/ConfermaSpecchio.tsx", import.meta.url), "utf8");
  const dom = conferma.slice(conferma.indexOf("export function DomandaSpecchio"));
  assert.match(dom, /pronta,\s*onErrore,/);
  assert.match(dom, /pronta: boolean;/);
  assert.match(dom, /if \(domanda === undefined \|\| !pronta\) return null;/);
  const app = readFileSync(new URL("../src/App.tsx", import.meta.url), "utf8");
  assert.match(app, /<DomandaSpecchio\s+pronta=\{propostaPuoMostrarsi\}/);
});
