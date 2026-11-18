/**
 * Il collegamento via internet nella finestra (`src/lontano.ts`): il QR
 * disegnato dalla matrice, il link pescato da un testo incollato, l'ordine dei
 * dispositivi, e il gesto che sui dati mobili aspetta un sì invece di fallire.
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import type {
  DispositivoLontano,
  DispositivoSincronia,
  FidatoInsieme,
  ScaricoLontano,
  StatoLontano,
} from "../src/ipc";
import {
  archivioDatiMobili,
  archivioLontano,
  chiedeIDatiMobili,
  conIlSiDeiDatiMobili,
  gestoLontano,
  annullatoDaQui,
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
