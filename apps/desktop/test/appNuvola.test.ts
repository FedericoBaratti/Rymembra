import { test } from "node:test";
import assert from "node:assert/strict";

import type { AppUtente, ServizioOfferto } from "../src/ipc";
import {
  LUNGHEZZA_MASSIMA,
  NOME_MASSIMO,
  SUFFISSO_GOOGLE,
  appDellaPiattaforma,
  appPredefinita,
  cambiaIdentificativo,
  chiLaUsa,
  conDisponibili,
  difettoIdApp,
  difettoSegreto,
  idCanonico,
  nomeNuovo,
  nomePulito,
  piattaformaDi,
  rimedioDi,
  serveLApp,
} from "../src/appNuvola";

// Gli stessi di `rymembra_domain::nuvola_app::prove`: le due tabelle vanno
// tenute uguali, o una delle due parti rifiuta quel che l'altra accetta.
const GUID = "0f2c8a3e-5b7d-4c1a-9e8f-1a2b3c4d5e6f";
const GOOGLE = "123456789012-abcdefghijklmnopqrstuvwxyz012345.apps.googleusercontent.com";

test("gli identificativi buoni passano, anche con un a capo incollato", () => {
  const buoni = [
    ["microsoft", GUID],
    ["microsoft", "0F2C8A3E-5B7D-4C1A-9E8F-1A2B3C4D5E6F"],
    ["dropbox", "a1b2c3d4e5f6g7h"],
    ["google", GOOGLE],
  ] as const;
  for (const [piattaforma, id] of buoni) {
    assert.equal(difettoIdApp(piattaforma, id), null, id);
    assert.equal(difettoIdApp(piattaforma, ` ${id}\n`), null, id);
  }
});

test("gli identificativi sbagliati dicono perché", () => {
  const storti = [
    ["microsoft", "", "vuoto"],
    ["microsoft", "   ", "vuoto"],
    ["microsoft", "0f2c8a3e 5b7d", "spazi"],
    // Il valore del segreto di un client Entra, incollato al posto dell'ID.
    ["microsoft", "abc8Q~xyz", "forma"],
    // Un gruppo corto, i trattini mancanti, una lettera che non è esadecimale.
    ["microsoft", "0f2c8a3e-5b7d-4c1a-9e8f-1a2b3c4d5e6", "forma"],
    ["microsoft", "0f2c8a3e5b7d4c1a9e8f1a2b3c4d5e6f", "forma"],
    ["microsoft", "zf2c8a3e-5b7d-4c1a-9e8f-1a2b3c4d5e6f", "forma"],
    ["dropbox", "A1B2C3D4E5F6G7H", "forma"],
    ["dropbox", "corta", "forma"],
    ["dropbox", GUID, "forma"],
    ["google", "123456789012-abc", "forma"],
    // Il suffisso da solo, e il segreto incollato al posto dell'ID.
    ["google", ".apps.googleusercontent.com", "forma"],
    ["google", "GOCSPX-segreto", "forma"],
    ["google", GUID, "forma"],
  ] as const;
  for (const [piattaforma, id, atteso] of storti) {
    assert.equal(difettoIdApp(piattaforma, id), atteso, `${piattaforma} ${JSON.stringify(id)}`);
  }
  const lungo = `${"1".repeat(LUNGHEZZA_MASSIMA)}${SUFFISSO_GOOGLE}`;
  assert.equal(difettoIdApp("google", lungo), "lungo");
});

test("i difetti vengono nell'ordine del nucleo: vuoto, lungo, spazi, forma", () => {
  // Lungo **e** con spazi: il nucleo dice «lungo», e lo dice anche qui.
  assert.equal(difettoIdApp("microsoft", `${"a ".repeat(LUNGHEZZA_MASSIMA)}`), "lungo");
  // Un a capo **dentro** l'ID è uno spazio, non una forma storta.
  assert.equal(difettoIdApp("dropbox", "abcd\nefgh"), "spazi");
  // La lunghezza è in byte, come `str::len`: cento «è» sono duecento byte.
  assert.equal(difettoSegreto("è".repeat(100)), null);
  assert.equal(difettoSegreto("è".repeat(101)), "lungo");
  // Le virgolette, come nel nucleo: copiato da un JSON o da un messaggio.
  for (const incollato of ['"GOCSPX-AbC"', "'GOCSPX-AbC'", "`GOCSPX-AbC`", "“GOCSPX-AbC”", "«GOCSPX-AbC»"]) {
    assert.equal(difettoSegreto(incollato), "forma", incollato);
  }
  assert.equal(difettoSegreto("GOCSPX-AbC_dEf-123"), null);
  // Dropbox vuole proprio da 8 a 32 caratteri.
  assert.equal(difettoIdApp("dropbox", "a".repeat(8)), null);
  assert.equal(difettoIdApp("dropbox", "a".repeat(32)), null);
  assert.equal(difettoIdApp("dropbox", "a".repeat(33)), "forma");
});

test("il segreto vuole solo esserci", () => {
  assert.equal(difettoSegreto("GOCSPX-AbC_dEf-123"), null);
  assert.equal(difettoSegreto(" GOCSPX-AbC\n"), null);
  assert.equal(difettoSegreto(""), "vuoto");
  assert.equal(difettoSegreto("GOCSPX AbC"), "spazi");
  assert.equal(difettoSegreto("x".repeat(LUNGHEZZA_MASSIMA + 1)), "lungo");
});

test("il nome si ripulisce e ha un tetto in caratteri", () => {
  assert.equal(nomePulito("  Google di casa \n"), "Google di casa");
  assert.equal(nomePulito(""), null);
  assert.equal(nomePulito("   "), null);
  assert.equal(nomePulito("due\nrighe"), null);
  assert.equal(nomePulito("tab\tdentro"), null);
  const alTetto = "è".repeat(NOME_MASSIMO);
  assert.equal(nomePulito(alTetto), alTetto);
  assert.equal(nomePulito("è".repeat(NOME_MASSIMO + 1)), null);
});

/** Un'app di prova: le parti che contano nel test, il resto di serie. */
function app(id: string, piattaforma: AppUtente["piattaforma"], altro: Partial<AppUtente> = {}): AppUtente {
  return {
    id,
    piattaforma,
    nome: piattaforma,
    clientId: `${id}-client`,
    segretoSalvato: true,
    predefinita: false,
    usataDa: [],
    ...altro,
  };
}

test("la predefinita: quella che dice il nucleo, altrimenti la prima della piattaforma", () => {
  const elenco = [
    app("google-a", "google"),
    app("microsoft-a", "microsoft"),
    app("google-b", "google", { predefinita: true }),
    app("dropbox-a", "dropbox"),
    app("dropbox-b", "dropbox"),
  ];
  assert.equal(appPredefinita(elenco, "google")?.id, "google-b");
  assert.equal(appPredefinita(elenco, "dropbox")?.id, "dropbox-a");
  assert.equal(appPredefinita(elenco, "microsoft")?.id, "microsoft-a");
  assert.equal(appPredefinita([], "microsoft"), null);
  assert.equal(appPredefinita(elenco, null), null);
  assert.deepEqual(
    appDellaPiattaforma(elenco, "google").map((a) => a.id),
    ["google-a", "google-b"],
  );
  assert.deepEqual(appDellaPiattaforma(elenco, null), []);
});

test("ogni servizio col consenso nel browser ha la sua piattaforma", () => {
  assert.equal(piattaformaDi("onedrive"), "microsoft");
  assert.equal(piattaformaDi("gdrive"), "google");
  assert.equal(piattaformaDi("dropbox"), "dropbox");
  assert.equal(piattaformaDi("webdav"), null);
  assert.equal(piattaformaDi("mega"), null);
  assert.equal(piattaformaDi("megalink"), null);
});

test("il nome proposto per un'app nuova è il primo libero", () => {
  assert.equal(nomeNuovo([], "Google"), "Google");
  assert.equal(nomeNuovo([app("google-a", "google", { nome: "Google" })], "Google"), "Google 2");
  assert.equal(
    nomeNuovo(
      [
        app("google-a", "google", { nome: "google" }),
        app("google-c", "google", { nome: "Google 3" }),
      ],
      "Google",
    ),
    "Google 2",
  );
});

test("la disponibilità segue le app, e chi non ne vuole resta com'è", () => {
  const servizio = (s: ServizioOfferto["servizio"], disponibile: boolean): ServizioOfferto => ({
    servizio: s,
    nome: s,
    disponibile,
    modo: s === "webdav" || s === "mega" ? "modulo" : "browser",
    pronto: s !== "mega",
  });
  const prima = [
    servizio("onedrive", false),
    servizio("gdrive", true),
    servizio("dropbox", false),
    servizio("webdav", true),
    servizio("mega", false),
  ];
  const dopo = conDisponibili(prima, [app("microsoft-a", "microsoft")]);
  assert.deepEqual(
    dopo.map((s) => [s.servizio, s.disponibile]),
    [
      ["onedrive", true],
      ["gdrive", false],
      ["dropbox", false],
      ["webdav", true],
      ["mega", false],
    ],
  );
  // Chi non cambia resta lo stesso oggetto.
  assert.equal(dopo[3], prima[3]);
  assert.equal(dopo[2], prima[2]);
});

test("chi usa un'app, e quando un ID nuovo la cambia per loro", () => {
  const usata = app("google-a", "google", {
    clientId: GOOGLE,
    usataDa: [
      { conto: null, servizio: null, nome: "" },
      { conto: "gdrive-1", servizio: "gdrive", nome: "mario@gmail.com" },
      { conto: "gdrive-2", servizio: "gdrive", nome: "anna@gmail.com" },
    ],
  });
  assert.deepEqual(chiLaUsa(usata), {
    conti: ["mario@gmail.com", "anna@gmail.com"],
    backup: true,
  });
  assert.deepEqual(chiLaUsa(app("dropbox-a", "dropbox")), { conti: [], backup: false });
  assert.equal(cambiaIdentificativo(usata, ` ${GOOGLE}\n`), false);
  assert.equal(cambiaIdentificativo(usata, `9${GOOGLE}`), true);
  assert.equal(cambiaIdentificativo(null, GOOGLE), false);
  // Un GUID non distingue le maiuscole: riscriverlo così non cambia l'app,
  // come per `rymembra_domain::nuvola_app::forma_canonica`.
  const microsoft = app("microsoft-a", "microsoft", { clientId: GUID });
  assert.equal(cambiaIdentificativo(microsoft, ` ${GUID.toUpperCase()} `), false);
  assert.equal(idCanonico("microsoft", ` ${GUID.toUpperCase()}\n`), GUID);
  assert.equal(idCanonico("google", ` ${GOOGLE} `), GOOGLE);
  assert.equal(idCanonico("dropbox", "a1b2c3d4e5f6g7h"), "a1b2c3d4e5f6g7h");
});

test("il guasto di un conto dice il rimedio", () => {
  assert.equal(rimedioDi("cloud.clientMissing"), "scegliApp");
  assert.equal(rimedioDi("cloud.appRejected"), "correggiApp");
  assert.equal(rimedioDi("cloud.appSecretMissing"), "correggiApp");
  assert.equal(rimedioDi("cloud.appChanged"), "ricollega");
  assert.equal(rimedioDi("cloud.authExpired"), "ricollega");
  assert.equal(rimedioDi(null), "ricollega");
  assert.equal(rimedioDi(undefined), "ricollega");
  assert.equal(serveLApp("cloud.appRejected"), true);
  assert.equal(serveLApp("cloud.clientMissing"), true);
  assert.equal(serveLApp("cloud.appChanged"), false);
});
