/**
 * Il mondo finto dell'anteprima.
 *
 * # Perché non c'è più una scenetta
 *
 * Prima l'anteprima disegnava cinque scene scritte a mano: `<div>` con le classi
 * giuste, che somigliavano all'applicazione senza esserlo. Divergevano per
 * costruzione — nessuno le rifà quando l'app cambia — e la divergenza si scopre
 * scrivendo una skin, cioè dopo.
 *
 * Adesso l'anteprima usa **lo stesso renderer** della finestra vera, e questo
 * file è l'unica cosa che cambia: al posto della libreria dell'utente c'è un
 * brano inventato, al posto dei comandi che suonano ci sono funzioni che non
 * fanno niente. Le scene restano, ma sono *stati* dell'unico renderer invece di
 * altrettanti markup — e la classe di bug «il finto è andato alla deriva»
 * sparisce, perché non c'è più un finto da tenere allineato.
 *
 * # Perché una scena sola non bastava
 *
 * C'era un elenco solo, di nove voci, e mescolava due cose che nell'applicazione
 * sono indipendenti: **quale pagina** si sta guardando e **cosa c'è sopra**. La
 * scena «modale» era «la coda aperta *e* due brani selezionati», la scena
 * «avvisi» era menù, notifica e fumetto messi in fila dentro il contenuto. Chi
 * ridipingeva la barra della selezione poteva vederla solo sopra la libreria;
 * chi ridipingeva la notifica non poteva vederla mai sopra Impostazioni, che è
 * dove la notifica della scansione compare davvero.
 *
 * Adesso gli assi sono due, come nell'app: una **pagina**, che si sceglie, e un
 * insieme di **sovrapposizioni**, che si accendono e si spengono una per una.
 * Ogni combinazione che l'applicazione sa produrre si può guardare, e nessuna
 * che non sappia produrre — vedi `spentaPerche()`, che è la metà di questo che
 * dice *no*.
 */
import type { ContestoWidget } from "../Impaginazione";
import type { Brano, StatoRiproduzione } from "../ipc";
import { t } from "../lingue";

/**
 * Le pagine che possono stare nel buco del contenuto.
 *
 * Sono le destinazioni vere dell'applicazione: la griglia della libreria, una
 * pagina con l'hero, Impostazioni, Importazioni, l'account, «In riproduzione» a
 * schermo intero, i tre pannelli grandi, e i due stati in cui la libreria non
 * ha niente da mostrare.
 *
 * `importazioni` e `account` sono le due che mancavano, e non erano una svista
 * piccola: fra tutte e due montano diciotto delle cinquantadue parti del
 * registro, comprese quelle — `list-row` dentro un elenco fitto, `stat-number`
 * accanto a una barra — che si vedono per quel che sono solo in mezzo alle
 * altre.
 */
export type Pagina =
  | "libreria"
  | "pagina"
  | "impostazioni"
  | "importazioni"
  | "account"
  | "schermo"
  | "pannelli"
  | "vuoto"
  | "caricamento";

/**
 * Quel che sta **sopra** la pagina, e si accende a piacere.
 *
 * Le prime tre sono stato del mondo e le disegna lo scafale vero: la terza
 * colonna, il pannello della coda, la barra della selezione. Le altre tre sono
 * sovrapposizioni nel senso stretto — nell'applicazione stanno fuori
 * dall'albero, in `App.tsx`, perché si sovrappongono per definizione — e qui le
 * disegna `scene.tsx` con le classi vere.
 *
 * Tutte e sei insieme sono legali: nell'app un menù contestuale sopra la coda
 * aperta con tre brani selezionati è mercoledì pomeriggio.
 *
 * Il fumetto del giro guidato non c'è, e non è una dimenticanza: `tour-tooltip`
 * sta nel registro e nessuna schermata lo disegna — vedi `NON_ANCORA` in
 * `scene.tsx`. Un interruttore che accende una cosa che l'applicazione non ha
 * sarebbe la bugia peggiore che questa vista possa dire.
 */
export type Sovrapposizione =
  | "colonna"
  | "coda"
  | "selezione"
  | "menu"
  | "avviso"
  | "dialogo";

/** Quali sovrapposizioni sono accese adesso. */
export type Accese = ReadonlySet<Sovrapposizione>;

/**
 * Nessuna, una volta sola.
 *
 * Condiviso e non costruito a ogni chiamata: finisce nelle dipendenze di un
 * `useMemo`, e un `Set` nuovo a ogni disegno rifarebbe il contesto — e con lui
 * tutto l'albero — sessanta volte al secondo.
 */
export const NESSUNA: Accese = new Set();

/** Le pagine, nell'ordine in cui si guardano. */
export function pagine(): readonly (readonly [Pagina, string])[] {
  return [
    ["libreria", t("studio.page.libreria")],
    ["pagina", t("studio.page.pagina")],
    ["impostazioni", t("studio.page.impostazioni")],
    ["importazioni", t("studio.page.importazioni")],
    ["account", t("studio.page.account")],
    ["schermo", t("studio.page.schermo")],
    ["pannelli", t("studio.page.pannelli")],
    ["vuoto", t("studio.page.vuoto")],
    ["caricamento", t("studio.page.caricamento")],
  ];
}

/** Le sovrapposizioni, nell'ordine in cui stanno nella testata. */
export function sovrapposizioni(): readonly (readonly [Sovrapposizione, string])[] {
  return [
    ["colonna", t("studio.over.colonna")],
    ["coda", t("studio.over.coda")],
    ["selezione", t("studio.over.selezione")],
    ["menu", t("studio.over.menu")],
    ["avviso", t("studio.over.avviso")],
    ["dialogo", t("studio.over.dialogo")],
  ];
}

/** Sulla pagina «vuoto» e «caricamento» non suona niente, e si vede. */
function suona(pagina: Pagina): boolean {
  return pagina !== "vuoto" && pagina !== "caricamento";
}

/**
 * Perché questo interruttore non si può accendere qui, se non si può.
 *
 * È la parte che tiene onesta la testata, ed è scritta contro i predicati
 * `visibile` di `Impaginazione.tsx` — non contro un'idea di come dovrebbero
 * essere. Senza, l'interruttore «coda» sulla pagina a schermo intero si
 * accendeva e non compariva niente: indistinguibile da una skin che nasconde la
 * coda per sbaglio, che è esattamente l'ambiguità che tutta questa vista esiste
 * per non avere.
 *
 * Ritorna `undefined` quando l'interruttore si può accendere.
 */
export function spentaPerche(
  quale: Sovrapposizione,
  pagina: Pagina,
  accese: Accese,
): string | undefined {
  // I tre widget dello scafale hanno bisogno di un brano: senza, `player`,
  // `column` e `queue` non rendono, e `selection-bar` resta senza il lettore di
  // cui prende il posto.
  const senzaBrano = !suona(pagina);
  const aSchermoIntero = pagina === "schermo";

  switch (quale) {
    case "colonna":
      if (senzaBrano) return t("studio.over.why.noTrack");
      if (aSchermoIntero) return t("studio.over.why.fullScreen");
      return undefined;
    case "coda":
      if (senzaBrano) return t("studio.over.why.noTrack");
      if (aSchermoIntero) return t("studio.over.why.fullScreen");
      // Nell'app la coda dentro la terza colonna ce l'ha già la colonna: il
      // pannello flottante non si monta, e il widget `queue` lo dice da sé.
      if (accese.has("colonna")) return t("studio.over.why.inColumn");
      return undefined;
    case "selezione":
      if (aSchermoIntero) return t("studio.over.why.fullScreen");
      return undefined;
    default:
      // Menù, notifica e finestrella galleggiano sopra qualunque cosa:
      // nell'app non c'è una pagina che li vieti.
      return undefined;
  }
}

/**
 * Le sovrapposizioni accese **e** possibili, su questa pagina.
 *
 * Un interruttore lasciato acceso e poi diventato impossibile — si accende la
 * coda, si passa a schermo intero — non deve arrivare al renderer: là
 * produrrebbe uno stato che l'applicazione non ha. Si spegne qui invece che nel
 * gesto, così tornando alla pagina di prima lo si ritrova acceso.
 */
export function effettive(pagina: Pagina, accese: Accese): Accese {
  const buone = new Set<Sovrapposizione>();
  for (const quale of accese) {
    if (spentaPerche(quale, pagina, accese) === undefined) buone.add(quale);
  }
  return buone;
}

const BRANO: Brano = {
  id: 1,
  path: "",
  title: "Corale in mi minore",
  artist: "Anna Vestri",
  album: "Le stanze basse",
  albumKey: "anna vestri|le stanze basse",
  trackNumber: 3,
  discNumber: 1,
  durationMs: 251_000,
  year: 2019,
  coverArtHash: null,
  playCount: 12,
  liked: true,
  rating: 4,
};

const STATO: StatoRiproduzione = {
  brano: BRANO,
  inPausa: false,
  posizioneMs: 96_000,
  durataMs: BRANO.durationMs,
  shuffle: false,
  ripeti: "all",
  volume: 0.72,
  muto: false,
  coda: [1, 2, 3],
  posizioneCoda: 0,
  // Piatto e spento: l'anteprima di una skin mostra come si disegna il lettore,
  // non come suona, e una curva finta darebbe a chi guarda l'impressione che
  // l'equalizzatore sia una decisione della skin.
  eqAttivo: false,
  eqGuadagni: [],
  replaygain: "normale",
  spegnimentoMs: null,
  autoplay: false,
  dissolvenzaS: 0,
  audio: null,
};

/** Un comando che non fa niente: nell'anteprima non c'è niente da comandare. */
const niente = () => {
  /* apposta */
};

/**
 * Il contesto di una scena.
 *
 * Le pagine si distinguono per quel che il **mondo** contiene, non per quello
 * che il renderer disegna: «vuoto» è una libreria senza brani, non un markup
 * diverso. È la differenza che rende impossibile alla scena di divergere
 * dall'applicazione.
 *
 * Le sovrapposizioni arrivano già filtrate da `effettive()`: qui si leggono e
 * basta, e non c'è un secondo posto in cui decidere cosa è possibile.
 */
export function contestoFinto(
  pagina: Pagina,
  accese: Accese = NESSUNA,
): ContestoWidget {
  const conBrano = suona(pagina);
  return {
    stato: conBrano ? STATO : { ...STATO, brano: null, coda: [], durataMs: 0 },
    // La voce accesa nella navigazione segue la pagina: era sempre «Album»,
    // e chi ridipingeva `nav-pill` vedeva lo stato attivo su una voce sola
    // qualunque schermata avesse davanti.
    vista:
      pagina === "impostazioni"
        ? "impostazioni"
        : pagina === "importazioni"
          ? "importazioni"
          : "album",
    playlistAperta: null,
    // Falso dove nell'app nessuna voce è «quella lì»: la pagina di un artista
    // si raggiunge *dentro* la libreria, e la barra non ha una voce per lei.
    inLibreria: pagina !== "pagina" && pagina !== "account",
    conteggi: conBrano
      ? { album: 135, artisti: 110, brani: 1592, preferiti: 41 }
      : {},
    playlist: [],
    colonnaAperta: accese.has("colonna"),
    codaAperta: accese.has("coda"),
    // A schermo intero i tre widget della riproduzione si tolgono di mezzo — e
    // con loro l'intestazione della pagina — perché quella schermata **è** la
    // pagina. È lo stesso predicato dell'app: qui si accende scegliendo la
    // pagina «schermo», non con un interruttore, proprio perché prende il posto
    // del contenuto invece di stargli sopra.
    grande: pagina === "schermo",
    selezionati: accese.has("selezione") ? [1, 2] : [],
    tuttiSelezionati: false,
    onVista: niente,
    onPlaylist: niente,
    onMenuPlaylist: niente,
    onNuovaPlaylist: niente,
    onNuovaSmart: niente,
    onImportaFile: niente,
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
