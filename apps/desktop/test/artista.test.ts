/**
 * L'artista intero: chi è l'artista di un brano per la sua pagina, dove si torna
 * chiudendola, e con quale filtro la si apre.
 *
 * Funzioni pure di `artista.ts` e `origine.ts`: le prove del cablaggio — che il
 * pulsante di «In riproduzione», il menù e Indietro passino da qui — sono in
 * `artistaPagina.test.ts`.
 */
import { test } from "node:test";
import assert from "node:assert/strict";

import {
  artistaEffettivo,
  paginaDellArtista,
  tornaAInRiproduzione,
  trovaArtista,
  tutteLePagine,
} from "../src/artista";
import type { Artista, Brano } from "../src/ipc";
import { LOCALE, TUTTI, origineCheContiene, origineDi } from "../src/origine";

const brano = (artist: string, albumArtist: string | null) =>
  ({ artist, albumArtist }) as Pick<Brano, "artist" | "albumArtist">;
const artista = (name: string): Artista => ({ name, sortName: name, albums: 1, tracks: 1, covers: [] });

test("l'artista di un brano per la sua pagina è quello del disco, se c'è", () => {
  // La stessa regola di `ARTISTA_EFFETTIVO` in `library/aggregati.rs`: una
  // raccolta porta alla pagina di «Vari artisti», non a quella dell'interprete.
  assert.equal(artistaEffettivo(brano("Tale", "Vari Artisti")), "Vari Artisti");
  assert.equal(artistaEffettivo(brano("Tale", null)), "Tale");
  assert.equal(artistaEffettivo(brano("Tale", "")), "Tale");
  assert.equal(artistaEffettivo(brano("Tale", "   ")), "Tale", "un artista del disco di soli spazi non c'è");
});

test("gli spazi attorno al nome si tolgono come fa SQLite, e solo gli spazi", () => {
  // `TRIM(x)` di SQLite toglie gli spazi e basta: un nome con una tabulazione
  // in testa è un altro nome, per la griglia degli artisti come qui.
  assert.equal(artistaEffettivo(brano("  Tale  ", null)), "Tale");
  assert.equal(artistaEffettivo(brano("x", "\tTale")), "\tTale");
});

test("un brano senza artista né artista del disco non ha una pagina", () => {
  assert.equal(artistaEffettivo(brano("", null)), "");
});

test("la griglia degli artisti si cerca per nome esatto, poi senza badare alle maiuscole", () => {
  const elenco = [artista("Tale"), artista("tale"), artista("Altro")];
  assert.equal(trovaArtista(elenco, "tale")?.name, "tale", "l'esatto vince");
  assert.equal(trovaArtista(elenco, "TALE")?.name, "Tale", "poi il primo che somiglia");
  assert.equal(trovaArtista(elenco, "Nessuno"), undefined);
  assert.equal(trovaArtista([], "Tale"), undefined);
});

test("chiudere l'artista aperto da «In riproduzione» riapre «In riproduzione»", () => {
  const tale = artista("Tale");
  assert.equal(tornaAInRiproduzione("Tale", tale), true);
  // Un altro artista aperto, un artista chiuso, o un artista aperto da una
  // griglia: Indietro è quello di sempre.
  assert.equal(tornaAInRiproduzione("Tale", artista("Altro")), false);
  assert.equal(tornaAInRiproduzione("Tale", null), false);
  assert.equal(tornaAInRiproduzione(null, tale), false);
});

test("il filtro che non contiene la riga da cui si parte torna a «Tutti»", () => {
  const A = "a".repeat(64);
  const B = "b".repeat(64);
  const locale = "C:/Musica/uno.flac";
  const diA = `lontano://${A}/12`;
  // «Tutti» contiene ogni riga.
  assert.deepEqual(origineCheContiene(TUTTI, locale), TUTTI);
  assert.deepEqual(origineCheContiene(TUTTI, diA), TUTTI);
  // «Questo dispositivo» contiene solo le righe di qui: aprire l'artista di un
  // brano del PC con questo filtro darebbe una pagina vuota.
  assert.deepEqual(origineCheContiene(LOCALE, locale), LOCALE);
  assert.deepEqual(origineCheContiene(LOCALE, diA), TUTTI);
  // Un dispositivo contiene le sue righe e non quelle di un altro né quelle di qui.
  assert.deepEqual(origineCheContiene(origineDi(A), diA), origineDi(A));
  assert.deepEqual(origineCheContiene(origineDi(A), `lontano://${B}/3`), TUTTI);
  assert.deepEqual(origineCheContiene(origineDi(A), locale), TUTTI);
  // Una riga senza percorso — un brano di catalogo — è di qui.
  assert.deepEqual(origineCheContiene(LOCALE, null), LOCALE);
  assert.deepEqual(origineCheContiene(origineDi(A), null), TUTTI);
});

test("tutte le pagine dell'artista si chiedono finché non ne arriva una vuota", async () => {
  // «Riproduci» e «Casuale» vogliono i brani **dell'artista**, tutti: la finestra
  // ne tiene in mano qualche pagina, e il resto lo chiede qui. Si ferma sulla
  // pagina vuota e non su una corta: un nucleo che un giorno limitasse la pagina
  // sotto la misura chiesta non farebbe suonare mezzo artista in silenzio.
  const righe = Array.from({ length: 450 }, (_, i) => i);
  const chiamate: [number, number][] = [];
  const chiedi = async (offset: number, limite: number) => {
    chiamate.push([offset, limite]);
    return righe.slice(offset, offset + Math.min(limite, 100));
  };
  assert.deepEqual(await tutteLePagine(chiedi, 200), righe);
  assert.deepEqual(chiamate, [
    [0, 200],
    [100, 200],
    [200, 200],
    [300, 200],
    [400, 200],
    [450, 200],
  ]);
  assert.deepEqual(await tutteLePagine(async () => [], 200), []);
});

test("un nucleo che ripete sempre la stessa pagina non fa girare la richiesta per sempre", async () => {
  await assert.rejects(
    tutteLePagine(async () => [1, 2, 3], 200, 5),
    /pagine/,
    "dopo il massimo di pagine è un errore, non un elenco tagliato",
  );
});

test("il nome dell'artista è un pulsante solo su una riga della libreria", () => {
  const riga = (id: number, artist: string, albumArtist: string | null) =>
    ({ id, artist, albumArtist }) as Pick<Brano, "id" | "artist" | "albumArtist">;
  // Una riga di qui, e una riga specchio di un altro dispositivo: id positivo.
  assert.equal(paginaDellArtista(riga(7, "Tale", null)), "Tale");
  assert.equal(paginaDellArtista(riga(4_000_001, "Tale", "Vari Artisti")), "Vari Artisti");
  // Una voce di «Ascolta insieme» non è nella libreria: il nome resta testo.
  assert.equal(paginaDellArtista(riga(-3, "Tale", null)), null);
  // Senza artista non c'è una pagina.
  assert.equal(paginaDellArtista(riga(7, "", null)), null);
  assert.equal(paginaDellArtista(riga(7, "", "  ")), null);
});

test("dal nome in «In riproduzione» Indietro torna lì; da una griglia no", () => {
  // La sequenza di `App`, con le sue stesse regole: il gesto parte da «In
  // riproduzione» (aperta), apre la pagina e la chiude; chiuderla riapre.
  type Stato = { grande: boolean; artista: Artista | null; da: string | null };
  const apri = (s: Stato, suo: Artista): Stato => ({
    grande: false,
    artista: suo,
    da: s.grande ? suo.name : null,
  });
  const chiudi = (s: Stato): Stato => ({
    grande: tornaAInRiproduzione(s.da, s.artista) ? true : s.grande,
    artista: null,
    da: null,
  });
  const vaiAltrove = (s: Stato): Stato => ({ ...s, artista: null, da: null });

  const dalNome = apri({ grande: true, artista: null, da: null }, artista("Tale"));
  assert.equal(dalNome.grande, false, "«In riproduzione» si chiude per far vedere la pagina");
  assert.equal(chiudi(dalNome).grande, true, "Indietro la riapre");
  assert.equal(chiudi(chiudi(dalNome)).grande, true, "e una seconda chiusura non cambia niente");

  const dallaGriglia = apri({ grande: false, artista: null, da: null }, artista("Tale"));
  assert.equal(chiudi(dallaGriglia).grande, false, "da una griglia Indietro è quello di sempre");

  // Andare altrove — la barra laterale — è un'altra strada: la pagina non c'è
  // più, e riaprire lo stesso artista da una griglia non riporta a «In riproduzione».
  const dopoLaBarra = vaiAltrove(dalNome);
  const riaperto = apri(dopoLaBarra, artista("Tale"));
  assert.equal(chiudi(riaperto).grande, false);
});
