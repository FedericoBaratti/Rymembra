/**
 * La pagina dell'artista, intera: il nome in «In riproduzione», le sezioni
 * «Brani» e «Compare anche in», «Riproduci» e «Casuale», il filtro per
 * dispositivo. Sono prove del cablaggio — il test non ha un DOM — e quel che
 * hanno di giusto lo dicono i numeri delle query (`prove_artista.rs`) e le
 * funzioni pure (`artista.test.ts`).
 */
import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const sorgente = (file: string) => readFileSync(new URL(`../src/${file}`, import.meta.url), "utf8");
const app = sorgente("App.tsx");

/** Dal punto in cui comincia `da` al primo `fine` che lo segue. */
function brano(testo: string, da: string, fine: string): string {
  const inizio = testo.indexOf(da);
  assert.ok(inizio >= 0, `manca «${da}»`);
  const dopo = testo.indexOf(fine, inizio);
  assert.ok(dopo >= 0, `manca «${fine}» dopo «${da}»`);
  return testo.slice(inizio, dopo);
}

test("il nome in «In riproduzione» è un pulsante, sul telefono e sul computer", () => {
  const nome = sorgente("parti/NomeArtista.tsx");
  // Un pulsante vero, con un nome per chi ascolta lo schermo che dice dove porta.
  assert.match(nome, /<button\s+type="button"\s+className="artista-link"/);
  assert.match(nome, /aria-label=/);
  assert.match(nome, /onClick=\{\(\) => onApri\(brano\)\}/);
  // E testo dove il gesto non ha una riga su cui farlo: lo decide `paginaDellArtista`.
  assert.match(nome, /paginaDellArtista\(brano\)/);
  assert.match(nome, /apre === null/);
  for (const file of ["schermate/InRiproduzione.tsx", "schermate/InRiproduzioneTelefono.tsx"]) {
    const np = sorgente(file);
    assert.match(np, /<NomeArtista brano=\{brano\} onApri=\{onArtista\} \/>/, file);
  }
  // `App` passa lo stesso gesto a tutte e due.
  assert.equal([...app.matchAll(/onArtista=\{apriArtistaDalNome\}/g)].length, 2);
});

test("il tocco sul nome apre l'artista effettivo, e «In riproduzione» si toglie di mezzo", () => {
  const apre = brano(app, "const apriArtistaDelBrano = useStabile(", "\n  });");
  // L'artista effettivo, non l'interprete del brano.
  assert.match(apre, /artistaEffettivo\(brano\)/);
  assert.doesNotMatch(apre, /brano\.artist\b/);
  // «In riproduzione» aperta: si chiude per far vedere la pagina, e si ricorda
  // chi l'ha aperta.
  assert.match(apre, /const daInRiproduzione = grande;/);
  assert.match(apre, /if \(daInRiproduzione\) setGrande\(false\);/);
  assert.match(apre, /setArtistaDaInRiproduzione\(daInRiproduzione \? suo\.name : null\)/);
  // Il filtro che contiene la riga, e gli artisti letti sotto quello.
  assert.match(apre, /origineCheContiene\(origine, brano\.path\)/);
  assert.match(apre, /ipc\.artisti\(perIlNucleo\(nuova\)\)/);
  // «Vai all'artista» del menù passa dalla stessa strada.
  assert.match(app, /vaiAllArtista: conBrano\(\(brano\) => apriArtistaDelBrano\(brano\)\)/);
});

test("Indietro torna a «In riproduzione»: il tasto, Escape e la freccia del telefono", () => {
  const chiude = brano(app, "const apriArtista = useStabile(", "\n  });");
  assert.match(chiude, /tornaAInRiproduzione\(artistaDaInRiproduzione, artistaAperto\)/);
  assert.match(chiude, /if \(torna\) setGrande\(true\);/);
  assert.match(chiude, /setArtistaDaInRiproduzione\(null\);/);
  // Il tasto della pagina sul computer, e la scala di Escape e di Indietro (che
  // sul telefono è la freccia): la stessa funzione.
  assert.match(app, /onClick=\{\(\) => apriArtista\(null\)\}/);
  const scala = brano(app, "if (artistaAperto) {", "return true;");
  assert.match(scala, /apriArtista\(null\)/);
  // Il tasto lo dice: «In riproduzione» invece di «Artisti».
  assert.match(
    app,
    /tornaAInRiproduzione\(artistaDaInRiproduzione, artistaAperto\)\s*\?\s*t\("np\.nowPlaying"\)\s*:\s*t\("nav\.artists"\)/,
  );
  // Andare altrove la scorda.
  const vaiA = brano(app, "const vaiA = useCallback(", "}, []);");
  assert.match(vaiA, /setArtistaDaInRiproduzione\(null\);/);
});

test("«Riproduci» e «Casuale» suonano i brani dell'artista, non gli album delle raccolte", () => {
  const suona = brano(app, "const suonaArtista = useCallback(", "[suonaDa, segnalaErrore, braniDellArtista]");
  assert.match(suona, /braniDellArtista\(nome\)/);
  // Il vecchio cammino prendeva i dischi e i brani di ciascuno: la raccolta
  // entrava intera, con i brani di chiunque.
  assert.doesNotMatch(suona, /albumArtista|braniAlbum/);
  const prese = brano(app, "const braniDellArtista = useCallback(", "[ordineArtista, origineIpc]");
  assert.match(prese, /ipc\.braniArtista\(nome, ordineArtista, offset, limite, origineIpc\)/);
  assert.match(prese, /tutteLePagine\(/);
  // Con il filtro di adesso e nell'ordine che la pagina mostra: le stesse dipendenze.
  // E «Scarica qui» prende gli stessi brani.
  assert.match(app, /scaricaQui\(await braniDellArtista\(nome\)\)/);
});

test("la pagina dell'artista sul computer ha Riproduci, Casuale e Scarica qui, e il filtro", () => {
  // Tre bottoni veri (non voci di un menù), con un nome che dice di chi: raggiungibili
  // con la tastiera e dal lettore di schermo.
  for (const chiave of ["artist.play.aria", "artist.shuffle.aria", "artist.download.aria"]) {
    assert.match(app, new RegExp(`aria-label=\\{t\\("${chiave.replace(/\./g, "\\.")}", \\{ nome: nomeArtista\\(nome\\) \\}\\)\\}`), chiave);
  }
  assert.match(app, /onClick=\{\(\) => void suonaArtista\(nome, false\)\}/);
  assert.match(app, /onClick=\{\(\) => void suonaArtista\(nome, true\)\}/);
  assert.match(app, /onClick=\{\(\) => void scaricaArtista\(nome\)\}/);
  // Il filtro per dispositivo, sul computer nella testata e sul telefono sotto la barra.
  const pc = brano(app, "Il filtro per dispositivo vale anche qui", "</>");
  assert.match(pc, /<PilloleOrigine origine=\{origine\} onOrigine=\{setOrigine\} \/>/);
  const telefono = brano(app, 'case "artista":\n        return (\n          <BarraAlta', "/>\n        );");
  assert.match(telefono, /sotto=\{<PilloleOrigine origine=\{origine\} onOrigine=\{setOrigine\} \/>\}/);
});

test("il filtro per dispositivo vale per tutti gli elenchi della pagina dell'artista", () => {
  // Il filtro sta nella chiave di ogni lista rada e nella richiesta: un altro
  // dispositivo è un altro elenco.
  for (const nome of ["radaBraniArtista", "radaEsatte", "radaEuristiche"]) {
    const rada = brano(app, `const ${nome} = useRada<Brano>({`, "onErrore: segnalaErrore,");
    assert.match(rada, /chiaveArtista/, nome);
    assert.match(rada, /origineIpc/, nome);
  }
  assert.match(app, /const chiaveArtista = `\$\{nomeAperto\}:\$\{chiaveOrigine\}`;/);
  // E i numeri dell'artista si rileggono con il filtro nuovo: sono la lunghezza
  // delle liste, e un conto vecchio le lascerebbe corte o piene di scheletri.
  const rilegge = brano(app, "Promise.all([ipc.artisti(origineIpc), ipc.partecipazioniConta(nomeAperto, origineIpc)])", "return () => {");
  assert.match(rilegge, /albums: 0, tracks: 0/, "un artista senza niente col filtro nuovo resta aperto con zero");
  assert.match(app, /\[nomeAperto, chiaveArtista, giroArtista\]/);
  // La selezione dei brani dell'artista porta il filtro e l'ordinamento.
  assert.match(app, /pagina\.tipo === "artista" \? `\$\{ordineArtista\}:\$\{chiaveOrigine\}` : ordine/);
  // Il nucleo sa quale elenco è, per «Seleziona tutto» e per suonare da una riga.
  assert.match(app, /\{ tipo: "artista", nome: nomeArtistaCorrente, ordine: ordineArtista \}/);
});

test("le tre sezioni: «Brani» è l'elenco corrente, le partecipazioni hanno i loro gesti", () => {
  const pagina = brano(app, 'if (pagina.tipo === "artista") {\n      const nome = pagina.artista.name;', "return (\n        <>\n          {MOBILE && (\n            <TestataDettaglio\n              occhiello={t(\"page.artist\")}");
  assert.match(pagina, /origine\.tipo !== "tutti"/, "il vuoto del filtro si dice solo con un filtro");
  const corpo = brano(app, 'id="artista-brani"', 'id="artista-compare"');
  assert.match(corpo, /rada=\{radaBraniArtista\}/);
  assert.match(corpo, /onSuona=\{suonaQui\}/);
  assert.match(corpo, /onMenu=\{menuSuSelezione\}/);
  // Senza `chiave`: cambiare l'ordinamento non riporta in cima a tutta la pagina.
  assert.doesNotMatch(corpo, /chiave=\{/);
  for (const [id, rada, suona, menu] of [
    ["artista-compare", "radaEsatte", "suonaEsatte", "menuEsatte"],
    ["artista-forse", "radaEuristiche", "suonaEuristiche", "menuEuristiche"],
  ] as const) {
    const da = app.indexOf(`id="${id}"`);
    assert.ok(da >= 0, id);
    const sezione = app.slice(da, app.indexOf("</SezioneArtista>", da));
    // Elenchi **loro**: con `suonaQui` e `menuSuSelezione` un tocco andrebbe a una
    // riga di «Brani», e il menù non vedrebbe il percorso per togliere le voci che
    // toccano il file.
    assert.match(sezione, new RegExp(`rada=\\{${rada}\\}`), id);
    assert.match(sezione, new RegExp(`onSuona=\\{${suona}\\}`), id);
    assert.match(sezione, new RegExp(`onMenu=\\{${menu}\\}`), id);
    assert.match(sezione, /selezione=\{NESSUNA_SCELTA\}/, id);
    assert.doesNotMatch(sezione, /onSuona=\{suonaQui\}|onMenu=\{menuSuSelezione\}/, id);
  }
  // Il menù di una partecipazione trova il brano nella **sua** lista.
  assert.match(app, /apriMenu\(e, ids, \(id\) => radaEsatte\.cerca\(\(b\) => b\.id === id\)\)/);
  assert.match(app, /apriMenu\(e, ids, \(id\) => radaEuristiche\.cerca\(\(b\) => b\.id === id\)\)/);
});

test("le euristiche si dichiarano per quel che sono, e non si confondono con le esatte", () => {
  const esatte = brano(app, 'id="artista-compare"', 'id="artista-forse"');
  assert.match(esatte, /titolo=\{t\("artist\.appears\.title"\)\}/);
  assert.match(esatte, /nota=\{t\("artist\.appears\.sub"\)\}/);
  const forse = app.slice(app.indexOf('id="artista-forse"'), app.indexOf("</SezioneArtista>", app.indexOf('id="artista-forse"')));
  assert.match(forse, /titolo=\{t\("artist\.maybe\.title"\)\}/);
  assert.match(forse, /nota=\{t\("artist\.maybe\.sub"\)\}/);
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(sorgente(`lingue/${lingua}.json`)) as Record<string, string>;
    // «Forse»/«May also»: il titolo dice che è un'ipotesi, e la nota che è una ricerca.
    assert.match(testi["artist.maybe.title"] ?? "", /^(Forse|May)/, lingua);
    assert.notEqual(testi["artist.maybe.title"], testi["artist.appears.title"], lingua);
  }
});

test("un voto, un cuore o una rilettura toccano anche gli elenchi dell'artista", () => {
  const aggiorna = brano(app, "const aggiornaBrani = useCallback(", "[radaBrani, radaPreferiti");
  for (const nome of ["radaBraniArtista", "radaEsatte", "radaEuristiche"]) {
    assert.match(aggiorna, new RegExp(`${nome}\\.aggiorna\\(f\\)`), nome);
    assert.match(app, new RegExp(`${nome}\\.invalida\\(\\);`), nome);
  }
  // E i loro totali: una libreria che cambia rilegge l'artista aperto.
  const contorno = brano(app, "const ricaricaContorno = useCallback(", "}, [ricaricaAlbum");
  assert.match(contorno, /setGiroArtista\(/);
});

test("un brano di un altro dispositivo, dal nome o dal menù, apre l'artista con un filtro che lo contiene", () => {
  const origine = sorgente("origine.ts");
  assert.match(origine, /export function origineCheContiene\(/);
  // Il tipo di `Brano` porta l'artista del disco: il contratto con il nucleo.
  const tipi = sorgente("ipc/libreria.ts");
  assert.match(tipi, /albumArtist: string \| null;/);
  assert.match(tipi, /\{ tipo: "artista"; nome: string; ordine: OrdineArtista \}/);
  assert.match(tipi, /\{ tipo: "partecipazioni"; nome: string; specie: SpecieDiPartecipazione \}/);
});

test("le stringhe nuove hanno la loro frase in italiano e in inglese", () => {
  const usate = new Set<string>();
  for (const file of ["App.tsx", "parti/NomeArtista.tsx"]) {
    for (const m of sorgente(file).matchAll(/t\("(artist\.[A-Za-z.]+)"/g)) usate.add(m[1] ?? "");
  }
  assert.ok(usate.size >= 12, `poche chiavi artist.* usate: ${[...usate].join(", ")}`);
  for (const lingua of ["it", "en"]) {
    const testi = JSON.parse(sorgente(`lingue/${lingua}.json`)) as Record<string, string>;
    for (const chiave of usate) assert.ok(chiave in testi, `${lingua}: manca «${chiave}»`);
  }
});
