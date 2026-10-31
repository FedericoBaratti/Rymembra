/**
 * Il mondo finto della tela: una libreria, una coda, un testo, uno stato di
 * riproduzione — e le letture che le pagine vere fanno da sé.
 *
 * # Perché dati, e non disegni
 *
 * Lo Studio 2.x aveva `scene.tsx`: milletrecento righe di markup che
 * ricopiavano le pagine dell'app, e che andavano alla deriva ogni volta che
 * una pagina cambiava. Qui non c'è markup: la tela monta i componenti veri —
 * `Home`, `GrigliaAlbum`, `ElencoBrani`, `Cartelle`, `Esplora`,
 * `InRiproduzione` — e questo file dà loro da mangiare. Una pagina che cambia
 * nell'app cambia anche sulla tela, perché è la stessa.
 *
 * # Cosa non tocca
 *
 * Né il nucleo né il disco: le letture che le pagine fanno da sé passano dal
 * contesto `Dati` (`dati.tsx`), e qui ce n'è una versione che risponde da
 * questi elenchi. Nessuna scrittura: i gesti sulla tela li ferma la tela prima
 * che arrivino ai componenti.
 */
import type { Dati, LettureDati, Misura } from "../dati";
import type { ContestoWidget } from "../Impaginazione";
import type {
  Album,
  Artista,
  Brano,
  Casa,
  EsitoRicerca,
  FileCartella,
  NodoCartella,
  Raccolta,
  RigaTesto,
  StatoRiproduzione,
  TestoBrano,
} from "../ipc";
import type { ArchivioPosizione } from "../posizione";
import type { StatoEsplora } from "../schermate/Esplora";
import type { Larghezza, Scena } from "./scena";

// ── Le copertine ────────────────────────────────────────────────────────────

/** Un numero da una stringa: sempre lo stesso per la stessa impronta. */
function seme(testo: string): number {
  let h = 2166136261;
  for (const c of testo) h = Math.imul(h ^ c.charCodeAt(0), 16777619);
  return h >>> 0;
}

const copertineFatte = new Map<string, string>();

/**
 * Una copertina inventata, in SVG: due tinte, un disco o un sole, una riga.
 *
 * Deterministica — la stessa impronta dà sempre la stessa immagine — perché
 * una tela che a ogni ridisegno cambia le copertine sembra un'altra pagina.
 * Sono `data:` e non file: la politica dei contenuti le ammette già, e non c'è
 * niente da chiedere al disco.
 */
export function copertinaFinta(impronta: string): string {
  const fatta = copertineFatte.get(impronta);
  if (fatta !== undefined) return fatta;
  const n = seme(impronta);
  const tinta = n % 360;
  const altra = (tinta + 40 + ((n >> 9) % 140)) % 360;
  const forma =
    n % 3 === 0
      ? `<circle cx="${30 + (n % 40)}" cy="${40 + ((n >> 4) % 30)}" r="${18 + (n % 16)}" fill="hsl(${altra} 70% 62% / .85)"/>`
      : n % 3 === 1
        ? `<rect x="0" y="${55 + (n % 25)}" width="100" height="6" fill="hsl(${altra} 80% 70% / .8)"/>`
        : `<circle cx="50" cy="50" r="30" fill="none" stroke="hsl(${altra} 70% 70% / .7)" stroke-width="7"/>`;
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100">` +
    `<defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1">` +
    `<stop offset="0" stop-color="hsl(${tinta} 55% 38%)"/>` +
    `<stop offset="1" stop-color="hsl(${(tinta + 30) % 360} 60% 18%)"/>` +
    `</linearGradient></defs><rect width="100" height="100" fill="url(#g)"/>${forma}</svg>`;
  const url = `data:image/svg+xml,${encodeURIComponent(svg)}`;
  copertineFatte.set(impronta, url);
  return url;
}

// ── La libreria ─────────────────────────────────────────────────────────────

const DISCHI: readonly (readonly [string, string, number, string])[] = [
  ["Le stanze basse", "Anna Vestri", 2019, "ambient"],
  ["Nebbia sul porto", "I Maree", 2021, "folk"],
  ["Corrente alternata", "Delta Nove", 2018, "elettronica"],
  ["Ultimo treno", "Sara Bellone", 2022, "cantautorato"],
  ["Vetro soffiato", "Orchestra Lume", 2016, "classica"],
  ["Rumore bianco", "Pellicola", 2020, "rock"],
  ["Mezzogiorno", "Tiziano Arca", 2015, "jazz"],
  ["Linee d'ombra", "Nuvole Basse", 2023, "post-rock"],
  ["Fuochi a mare", "Lina Sorrenti", 2017, "pop"],
  ["Geometrie", "Quadrante", 2024, "elettronica"],
  ["Sale e rame", "I Maree", 2019, "folk"],
  ["Casa di vetro", "Anna Vestri", 2023, "ambient"],
];

const TITOLI = [
  "Corale in mi minore",
  "Il cortile",
  "Finestre aperte",
  "Scale mobili",
  "Onde corte",
  "Settembre",
  "La lampada",
  "Porto franco",
  "Ferro e seta",
  "Controluce",
  "Ninna nanna per un faro",
  "Tram 19",
];

const chiaveDi = (artista: string, titolo: string) =>
  `${artista.toLowerCase()}|${titolo.toLowerCase()}`;
const improntaDi = (artista: string, titolo: string) => `finta-${chiaveDi(artista, titolo)}`;

export const ALBUM: readonly Album[] = DISCHI.map(([title, artist, year, genre], i) => ({
  albumKey: chiaveDi(artist, title),
  title,
  artist,
  year,
  genre,
  totalTracks: 8 + (i % 5),
  coverArtHash: improntaDi(artist, title),
}));

export const BRANI: readonly Brano[] = Array.from({ length: 36 }, (_, i) => {
  const disco = DISCHI[i % DISCHI.length]!;
  const [album, artist, year] = disco;
  return {
    id: i + 1,
    path: `D:/Musica/${artist}/${album}/${String((i % 9) + 1).padStart(2, "0")}.flac`,
    title: TITOLI[i % TITOLI.length]! + (i >= TITOLI.length ? ` (${i >= 24 ? "live" : "reprise"})` : ""),
    artist,
    album,
    albumKey: chiaveDi(artist, album),
    trackNumber: (i % 9) + 1,
    discNumber: 1,
    durationMs: 150_000 + ((i * 37_000) % 190_000),
    year,
    coverArtHash: improntaDi(artist, album),
    playCount: (i * 7) % 23,
    liked: i % 4 === 0,
    rating: i % 6,
    // Uno su dieci viene da un catalogo: la pastiglia `badge` si vede.
    fonte: i % 10 === 3 ? "archive" : null,
    fontePagina: null,
    licenza: i % 10 === 3 ? "CC BY 4.0" : null,
  };
});

export const ARTISTI: readonly Artista[] = [...new Set(DISCHI.map((d) => d[1]))]
  .sort((a, b) => a.localeCompare(b))
  .map((name) => {
    const suoi = ALBUM.filter((a) => a.artist === name);
    return {
      name,
      sortName: name.toLowerCase(),
      albums: suoi.length,
      tracks: BRANI.filter((b) => b.artist === name).length,
      covers: suoi.map((a) => a.coverArtHash!).slice(0, 4),
    };
  });

export const CASA: Casa = {
  riprendi: BRANI[4]!,
  riprendiMs: 74_000,
  recenti: BRANI.slice(0, 8),
  aggiunti: ALBUM.slice(4, 10),
  trascurati: BRANI.slice(20, 28),
};

export const SETTIMANA: readonly Raccolta[] = [
  { id: 1, genere: "folk", ordine: 0, etichetta: "I Maree", etichettaTipo: "artista", aperta: false, brani: BRANI.slice(1, 9) },
  { id: 2, genere: "ambient", ordine: 1, etichetta: null, etichettaTipo: null, aperta: true, brani: BRANI.slice(12, 20) },
  { id: 3, genere: "jazz", ordine: 2, etichetta: null, etichettaTipo: null, aperta: false, brani: BRANI.slice(6, 14) },
];

/** Una ricerca già fatta: la pagina di Esplora coi suoi risultati. */
const ESITO_ESPLORA: EsitoRicerca = {
  annullata: false,
  muti: [],
  risultati: Array.from({ length: 7 }, (_, i) => ({
    url: `https://esempio.invalid/${i}`,
    titolo: ["Clair de lune", "Rêverie", "Arabesque n. 1", "La fille aux cheveux de lin", "Passepied", "Prélude", "Menuet"][i]!,
    autore: "Claude Debussy",
    album: i % 2 === 0 ? "Suite bergamasque" : null,
    durataSec: 240 + i * 31,
    fonte: i % 3 === 0 ? "archive" : "jamendo",
    fonteEtichetta: i % 3 === 0 ? "Internet Archive" : "Jamendo",
    licenza: i % 2 === 0 ? "pubblico-dominio" : "cc-by",
    disponibilita: i === 5 ? "soloAcquisto" : "ascolto",
    pagina: null,
    inLibreria: i === 2,
  })),
};

export const ESPLORA: StatoEsplora = {
  frase: "debussy clair de lune",
  fase: { tipo: "esito", frase: "debussy clair de lune", esito: ESITO_ESPLORA },
  tuttiInCorso: false,
};

/** Un testo sincronizzato, con la riga accesa a metà. */
function testoFinto(): TestoBrano {
  const versi = [
    "Si spegne la luce sul molo",
    "e il mare riprende la voce",
    "",
    "le barche contano i passi",
    "di chi torna tardi dal porto",
    "resta una lampada accesa",
    "per chi non ha fretta di dormire",
    "",
    "e domani sarà ancora qui",
  ];
  const righe: RigaTesto[] = versi.map((testo, i) => ({
    ms: 8_000 + i * 11_000,
    testo,
    parole: [],
    secondaria: null,
  }));
  return {
    righe,
    piatto: null,
    strumentale: false,
    fonte: "lrclib",
    offsetMs: 0,
    scartoMs: 0,
    aderenza: "buona",
    cercato: true,
    daChiedere: false,
  };
}

/** L'albero delle cartelle: una radice, gli artisti, i dischi. */
function figlie(percorso: string | null): NodoCartella[] {
  if (percorso === null) {
    return [{ percorso: "D:/Musica", nome: "Musica", brani: BRANI.length, sottocartelle: ARTISTI.length, radice: true }];
  }
  if (percorso === "D:/Musica") {
    return ARTISTI.map((a) => ({
      percorso: `D:/Musica/${a.name}`,
      nome: a.name,
      brani: a.tracks,
      sottocartelle: a.albums,
      radice: false,
    }));
  }
  const artista = percorso.slice("D:/Musica/".length);
  return ALBUM.filter((a) => a.artist === artista).map((a) => ({
    percorso: `${percorso}/${a.title}`,
    nome: a.title,
    brani: a.totalTracks,
    sottocartelle: 0,
    // I file finti di questo disco, perché aprendolo si vedano le foglie:
    // `folder-row` ha una forma per i file (`.nodo-file`), e chi disegna un
    // tema deve poterla vedere.
    file: fileDi(`${percorso}/${a.title}`).length,
    radice: false,
  }));
}

/** I file di una cartella finta: i brani il cui percorso sta lì dentro. */
function fileDi(percorso: string): FileCartella[] {
  return BRANI.flatMap((b) =>
    b.path !== null && b.path.slice(0, b.path.lastIndexOf("/")) === percorso
      ? [
          {
            brano: b.id,
            percorso: b.path,
            nome: b.path.slice(b.path.lastIndexOf("/") + 1),
            titolo: b.title,
            durataMs: b.durationMs,
          },
        ]
      : [],
  );
}

// ── La riproduzione ─────────────────────────────────────────────────────────

/** Il brano che suona sulla tela. */
export const IN_ASCOLTO: Brano = BRANI[0]!;

/** Lo stato del lettore finto. */
export function statoFinto(conBrano: boolean, inPausa: boolean): StatoRiproduzione {
  return {
    brano: conBrano ? IN_ASCOLTO : null,
    inPausa,
    posizioneMs: 96_000,
    durataMs: conBrano ? IN_ASCOLTO.durationMs : 0,
    shuffle: false,
    ripeti: "all",
    volume: 0.72,
    muto: false,
    coda: conBrano ? [1, 2, 3, 4, 5, 6, 7] : [],
    posizioneCoda: 0,
    // Piatto e spento: l'anteprima mostra come si disegna il lettore, non
    // come suona.
    eqAttivo: false,
    eqGuadagni: [],
    replaygain: "normale",
    spegnimentoMs: null,
    autoplay: false,
    dissolvenzaS: 0,
    latenzaMs: 0,
    anticipoMs: 0,
    audio: null,
    motivoProssimo: "suono",
    uscita: "Altoparlanti",
    // Pieni: `np-formato` si disegna solo se il dato c'è.
    formato: {
      codec: "FLAC",
      sampleRate: 44_100,
      channels: 2,
      bitrate: 1058,
      bitsPerSample: 24,
      outputRate: 48_000,
    },
    // Fuori sessione: l'anteprima di una skin non sta ascoltando con nessuno.
    insieme: null,
  };
}

// ── Le letture ──────────────────────────────────────────────────────────────

const subito = <T>(valore: T): Promise<T> => Promise.resolve(valore);

/** Le domande che le pagine fanno da sé, con le risposte di questo mondo. */
const LETTURE: LettureDati = {
  cartelleFiglie: (_radici, percorso) => subito(figlie(percorso)),
  cartelleFile: (_radici, percorso) => subito(fileDi(percorso)),
  cartelleBrani: () => subito(BRANI.slice(0, 8).map((b) => b.id)),
  impostaCartelleUi: () => subito(undefined),
  esploraCerca: () => subito(ESITO_ESPLORA),
  // Già cercato e niente da chiedere: il pannello del testo, nell'anteprima,
  // non va in rete e non legge la libreria vera.
  testoBrano: () => subito({ ...testoFinto(), daChiedere: false, cercato: true }),
  testoCerca: () => subito(testoFinto()),
  testoCercaDiNuovo: () => subito(testoFinto()),
  testiPannello: () => subito(undefined),
  spettro: () => subito(undefined),
  spettroVisibile: () => subito(false),
  spettroBande: () => subito(64),
  spettroQualita: () => subito("auto"),
  braniPerId: (ids) => subito(BRANI.filter((b) => ids.includes(b.id))),
  eqPresetElenco: () => subito([]),
  cronologia: () =>
    subito(
      BRANI.slice(8, 14).map((brano, i) => ({
        id: i + 1,
        brano,
        quandoMs: Date.now() - (i + 1) * 3_600_000,
        msAscoltati: brano.durationMs,
        sorgente: "libreria",
      })),
    ),
  cronologiaConteggio: () => subito(6),
  cartelleCandidate: () =>
    subito([
      { percorso: "C:/Users/Tu/Musica", brani: 1464, troncato: false, parziale: false },
      { percorso: "D:/Dischi", brani: 312, troncato: false, parziale: false },
    ]),
};

/**
 * I dati della tela.
 *
 * `posizione` è l'archivio che il trasporto finto dello Studio muove, e
 * `misura` la finestra finta: la larghezza che si sceglie nella barra, non
 * quella dello Studio.
 */
export function datiFinti(posizione: ArchivioPosizione, misura: () => Misura): Dati {
  return {
    anteprima: true,
    ipc: LETTURE,
    copertina: (impronta) => (impronta === null ? null : copertinaFinta(impronta)),
    posizione,
    // Nessun evento del nucleo arriva sulla tela.
    ascolta: () => Promise.resolve(() => {}),
    misura,
  };
}

// ── Lo scafale ──────────────────────────────────────────────────────────────

/** Un comando che non fa niente: sulla tela non c'è niente da comandare. */
const niente = () => {
  /* apposta */
};

/**
 * Il contesto dei widget dello scafale, per una scena.
 *
 * Le pagine si distinguono per quel che il **mondo** contiene, non per quel
 * che il renderer disegna: «vuoto» è una libreria senza brani, non un markup
 * diverso.
 */
export function contestoFinto(
  scena: Scena,
  larghezza: Larghezza,
  stato: StatoRiproduzione,
): ContestoWidget {
  const sopra = new Set(scena.sovrapposizioni);
  const vuota = scena.pagina === "vuoto";
  return {
    stato,
    vista:
      scena.pagina === "impostazioni"
        ? "impostazioni"
        : scena.pagina === "esplora"
          ? "esplora"
          : scena.pagina === "home"
            ? "home"
            : scena.pagina === "artisti"
              ? "artisti"
              : scena.pagina === "cartelle"
                ? "cartelle"
                : scena.pagina === "brani"
                  ? "brani"
                  : "album",
    playlistAperta: null,
    inLibreria: scena.pagina !== "disco",
    conteggi: vuota ? {} : { album: ALBUM.length, artisti: ARTISTI.length, brani: BRANI.length, preferiti: 9, cartelle: 1 },
    playlist: [],
    // Come nell'app: sotto i 1100 pixel la colonna si chiude da sé.
    colonnaAperta: larghezza === "tre",
    codaAperta: sopra.has("coda"),
    grande: scena.pagina === "schermo" || scena.pagina === "testo",
    selezionati: sopra.has("selezione") ? [2, 3, 5] : [],
    tuttiSelezionati: false,
    onVista: niente,
    onPlaylist: niente,
    onMenuPlaylist: niente,
    onNuovaPlaylist: niente,
    onNuovaSmart: niente,
    onImportaFile: niente,
    onDona: niente,
    onColonna: niente,
    onCoda: niente,
    onGrande: niente,
    onPreferito: niente,
    onVoto: niente,
    onErrore: niente,
    onSelezioneRiproduci: niente,
    onSelezioneDopo: niente,
    onSelezioneAccoda: niente,
    onSelezionePlaylist: niente,
    onSelezioneTuttiOAnnulla: niente,
    onSelezioneChiudi: niente,
  };
}
