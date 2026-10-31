import { test } from "node:test";
import assert from "node:assert/strict";

import type { ContoNuvola, ServizioOfferto } from "../src/ipc";
import {
  NESSUN_ELENCO,
  apribile,
  arrivato,
  chiaveElenco,
  chiesto,
  codiceCompleto,
  codicePulito,
  contiDella,
  contoDaMostrare,
  daChiedere,
  daDoveSiRisale,
  dentro,
  difettoIndirizzo,
  entra,
  fallito,
  inCima,
  inVirtuale,
  indirizzoDaMandare,
  linguetteDi,
  linkMegaPlausibile,
  nomeDiQui,
  nomiDalla,
  riprova,
  risali,
  statoLinguetta,
  statoOfferta,
  tornaA,
  usabile,
  vista,
} from "../src/selettoreNuvola";

const musica = { id: "m1", nome: "Musica" };
const rock = { id: "r1", nome: "Rock" };
const anni70 = { id: "a7", nome: "Anni 70" };

test("le briciole: si entra, si risale, si torna a un gradino", () => {
  const cima = inCima("OneDrive");
  assert.equal(dentro(cima), null);
  assert.equal(usabile(cima), false);
  const giu = entra(entra(entra(cima, musica), rock), anni70);
  assert.deepEqual(
    giu.map((p) => p.nome),
    ["OneDrive", "Musica", "Rock", "Anni 70"],
  );
  assert.equal(dentro(giu), "a7");
  assert.equal(nomeDiQui(giu), "Anni 70");
  assert.equal(usabile(giu), true);
  assert.deepEqual(nomiDalla(giu), ["Musica", "Rock", "Anni 70"]);
  assert.deepEqual(
    risali(giu).map((p) => p.id),
    [null, "m1", "r1"],
  );
  assert.deepEqual(
    tornaA(giu, 1).map((p) => p.id),
    [null, "m1"],
  );
  // Dalla cima non si sale, e un gradino che non c'è non sposta niente.
  assert.equal(risali(cima), cima);
  assert.equal(tornaA(giu, 9), giu);
  assert.equal(tornaA(giu, -1), giu);
});

test("una cartella virtuale si attraversa ma non si sceglie", () => {
  const condivisi = { id: "~condivisi", nome: "Shared with me", virtuale: true };
  const dentroCondivisi = entra(inCima("Google Drive"), condivisi);
  assert.equal(inVirtuale(dentroCondivisi), true);
  assert.equal(usabile(dentroCondivisi), false);
  assert.equal(dentro(dentroCondivisi), "~condivisi");
  const diMarco = entra(dentroCondivisi, { id: "S", nome: "Musica di Marco", virtuale: false });
  assert.equal(inVirtuale(diMarco), false);
  assert.equal(usabile(diMarco), true);
  assert.deepEqual(nomiDalla(diMarco), ["Shared with me", "Musica di Marco"]);
  // Una cartella vera non porta il segno nelle briciole.
  assert.deepEqual(diMarco[2], { id: "S", nome: "Musica di Marco" });
  assert.equal(usabile(risali(diMarco)), false);
});

test("risalendo si sa da quale cartella si è usciti", () => {
  const giu = entra(entra(inCima("OneDrive"), musica), rock);
  assert.equal(daDoveSiRisale(giu, risali(giu)), "r1");
  assert.equal(daDoveSiRisale(giu, tornaA(giu, 0)), "m1");
  assert.equal(daDoveSiRisale(giu, entra(giu, anni70)), null);
});

test("una risposta in ritardo non scrive sopra quel che si guarda", () => {
  const conto = "onedrive-abcdefgh";
  const diMusica = chiaveElenco(conto, "m1");
  const diRock = chiaveElenco(conto, "r1");
  // Si entra in «Musica», la domanda parte; si entra in «Rock» prima che torni.
  let elenchi = chiesto(NESSUN_ELENCO, diMusica);
  elenchi = chiesto(elenchi, diRock);
  // Arriva la risposta di «Musica»: «Rock» resta in attesa.
  elenchi = arrivato(elenchi, diMusica, [rock, { id: "p1", nome: "Pop" }]);
  assert.deepEqual(vista(elenchi, diRock), { tipo: "attesa" });
  // E tornando su «Musica» è già pronta, senza un'altra domanda.
  assert.equal(vista(elenchi, diMusica).tipo, "elenco");
  assert.equal(daChiedere(elenchi, diMusica), false);
  elenchi = arrivato(elenchi, diRock, []);
  assert.deepEqual(vista(elenchi, diRock), { tipo: "vuota" });
});

test("si chiede una volta sola, e un guasto aspetta «Riprova»", () => {
  const chiave = chiaveElenco("dropbox-abcdefgh", null);
  assert.equal(daChiedere(NESSUN_ELENCO, chiave), true);
  assert.deepEqual(vista(NESSUN_ELENCO, chiave), { tipo: "attesa" });
  let elenchi = chiesto(NESSUN_ELENCO, chiave);
  assert.equal(daChiedere(elenchi, chiave), false);
  const guasto = { code: "cloud.authExpired" };
  elenchi = fallito(elenchi, chiave, guasto);
  assert.equal(daChiedere(elenchi, chiave), false);
  assert.deepEqual(vista(elenchi, chiave), { tipo: "guasto", guasto });
  elenchi = riprova(elenchi, chiave);
  assert.equal(daChiedere(elenchi, chiave), true);
  // Riprovare dove non c'è un guasto non cambia niente.
  assert.equal(riprova(elenchi, chiave), elenchi);
});

test("la cima e una cartella con lo stesso conto hanno chiavi diverse", () => {
  assert.notEqual(chiaveElenco("c", null), chiaveElenco("c", "x"));
  assert.notEqual(chiaveElenco("c", "x"), chiaveElenco("d", "x"));
});

/** Un conto finto, coi campi che il selettore guarda. */
const conto = (
  id: string,
  servizio: ContoNuvola["servizio"],
  stato: ContoNuvola["stato"] = "collegato",
): ContoNuvola => ({
  conto: id,
  servizio,
  app: null,
  nome: id,
  stato,
  errore: null,
  radici: [],
  vociElencate: null,
  ultimoMs: null,
});

test("i conti di una linguetta, e quale mostrare", () => {
  const conti = [
    conto("onedrive-aaaaaaaa", "onedrive", "daRicollegare"),
    conto("onedrive-bbbbbbbb", "onedrive"),
    conto("mega-cccccccc", "mega"),
    conto("megalink-dddddddd", "megalink"),
  ];
  assert.deepEqual(
    contiDella(conti, "mega").map((c) => c.conto),
    ["mega-cccccccc", "megalink-dddddddd"],
  );
  const onedrive = contiDella(conti, "onedrive");
  // Il primo che si può sfogliare, se non se n'è scelto uno.
  assert.equal(contoDaMostrare(onedrive, null)?.conto, "onedrive-bbbbbbbb");
  // Quello scelto, anche se va ricollegato: è lui che spiega cosa manca.
  assert.equal(contoDaMostrare(onedrive, "onedrive-aaaaaaaa")?.conto, "onedrive-aaaaaaaa");
  // Uno scelto che non c'è più non conta.
  assert.equal(contoDaMostrare(onedrive, "sparito")?.conto, "onedrive-bbbbbbbb");
  assert.equal(contoDaMostrare([], null), null);
  const soloDaRicollegare = [conto("gdrive-eeeeeeee", "gdrive", "daRicollegare")];
  assert.equal(contoDaMostrare(soloDaRicollegare, null)?.conto, "gdrive-eeeeeeee");
});

test("WebDAV: l'utente entra nell'indirizzo, il server no", () => {
  const forma = "https://{server}/remote.php/dav/files/{utente}/";
  assert.equal(
    indirizzoDaMandare(forma, " mario rossi "),
    "https://{server}/remote.php/dav/files/mario%20rossi/",
  );
  assert.equal(indirizzoDaMandare(" https://x.it/dav/ ", ""), "https://x.it/dav/");
});

test("WebDAV: i difetti di un indirizzo si dicono prima di provarlo", () => {
  assert.equal(difettoIndirizzo(""), "vuoto");
  assert.equal(difettoIndirizzo("   "), "vuoto");
  assert.equal(difettoIndirizzo("https://{server}/remote.php/dav/"), "segnaposto");
  assert.equal(difettoIndirizzo("http://nas.casa/dav/"), "http");
  assert.equal(difettoIndirizzo("HTTP://nas.casa/dav/"), "http");
  assert.equal(difettoIndirizzo("nas.casa/dav"), "nonHttps");
  assert.equal(difettoIndirizzo("https://"), "nonHttps");
  assert.equal(difettoIndirizzo("https://app.koofr.net/dav/Koofr/"), null);
});

test("WebDAV: utente e password nell'indirizzo si dicono prima di provarlo", () => {
  // La forma che molte guide mostrano: tenuta, la password finirebbe in chiaro
  // nel database e nel diario. Il nucleo la rifiuta (`cloud.credentialsInAddress`).
  assert.equal(difettoIndirizzo("https://mario:segreta@cloud.it/dav"), "credenziali");
  assert.equal(difettoIndirizzo("https://mario@cloud.it/dav"), "credenziali");
  assert.equal(difettoIndirizzo("https://mario:segreta@cloud.it"), "credenziali");
  assert.equal(difettoIndirizzo(" HTTPS://mario:s@cloud.it/dav "), "credenziali");
  assert.equal(difettoIndirizzo("http://mario:segreta@nas.casa/dav/"), "credenziali");
  // Solo l'autorità: una chiocciola nel percorso è un nome utente fatto da
  // un'email, e va bene.
  assert.equal(
    difettoIndirizzo("https://cloud.it/remote.php/dav/files/mario@esempio.it/"),
    null,
  );
  // Senza schema resta quel che era.
  assert.equal(difettoIndirizzo("mario@nas.casa/dav"), "nonHttps");
});

test("MEGA: la forma del link e il codice a sei cifre", () => {
  assert.equal(linkMegaPlausibile("https://mega.nz/folder/AbC123#chiave-lunga_x"), true);
  assert.equal(linkMegaPlausibile(" https://mega.nz/#F!AbC123!chiave "), true);
  // Senza la chiave dopo il `#` il link non apre niente.
  assert.equal(linkMegaPlausibile("https://mega.nz/folder/AbC123"), false);
  assert.equal(linkMegaPlausibile("https://mega.nz/file/AbC123#chiave"), false);
  assert.equal(linkMegaPlausibile("https://esempio.it/folder/a#b"), false);
  assert.equal(codicePulito(" 123 456 "), "123456");
  assert.equal(codiceCompleto("123 456"), true);
  assert.equal(codiceCompleto("12345"), false);
  assert.equal(codiceCompleto("12345a"), false);
});

/** Un servizio offerto finto. */
const offerto = (
  servizio: ServizioOfferto["servizio"],
  pronto = true,
  disponibile = true,
): ServizioOfferto => ({
  servizio,
  nome: servizio,
  disponibile,
  modo: servizio === "mega" || servizio === "megalink" || servizio === "webdav" ? "modulo" : "browser",
  pronto,
});

test("le linguette: nell'ordine del nucleo, e il link di MEGA dentro MEGA", () => {
  const linguette = linguetteDi([
    offerto("onedrive"),
    offerto("dropbox", false),
    offerto("mega"),
    offerto("megalink"),
    offerto("webdav"),
  ]);
  assert.deepEqual(
    linguette.map((l) => l.scheda),
    ["onedrive", "dropbox", "mega", "webdav"],
  );
  const mega = linguette.find((l) => l.scheda === "mega");
  assert.equal(mega?.offerto.servizio, "mega");
  assert.equal(mega?.link?.servizio, "megalink");
  // Il link prima di MEGA: la linguetta resta una, e il nome lo dà MEGA.
  const alRovescio = linguetteDi([offerto("megalink"), offerto("mega")]);
  assert.equal(alRovescio.length, 1);
  assert.equal(alRovescio[0]?.offerto.servizio, "mega");
  assert.equal(alRovescio[0]?.link?.servizio, "megalink");
  // Solo il link: la linguetta è sua.
  const soloLink = linguetteDi([offerto("megalink")]);
  assert.equal(soloLink[0]?.scheda, "mega");
  assert.equal(soloLink[0]?.offerto.servizio, "megalink");
});

test("lo stato di una linguetta: in arrivo, da configurare, pronta", () => {
  assert.equal(statoOfferta(offerto("gdrive", false, false)), "inArrivo");
  assert.equal(statoOfferta(offerto("gdrive", true, false)), "daConfigurare");
  assert.equal(statoOfferta(offerto("gdrive")), "pronto");
  // MEGA è pronta se lo è una delle due strade.
  const [mega] = linguetteDi([offerto("mega", false), offerto("megalink")]);
  assert.ok(mega !== undefined);
  assert.equal(statoLinguetta(mega), "pronto");
  const [daFare] = linguetteDi([offerto("mega", false), offerto("megalink", true, false)]);
  assert.ok(daFare !== undefined);
  assert.equal(statoLinguetta(daFare), "daConfigurare");
});

test("una linguetta da configurare si apre; una in arrivo no", () => {
  // Dalla 3.2 l'app è dell'utente: senza, la linguetta si apre sul modulo che
  // la configura, invece di restare spenta con una ragione.
  const [onedrive, gdrive, mega] = linguetteDi([
    offerto("onedrive", true, false),
    offerto("gdrive"),
    offerto("mega", false, false),
  ]);
  assert.ok(onedrive !== undefined && gdrive !== undefined && mega !== undefined);
  assert.equal(apribile(onedrive), true);
  assert.equal(apribile(gdrive), true);
  assert.equal(apribile(mega), false);
});
