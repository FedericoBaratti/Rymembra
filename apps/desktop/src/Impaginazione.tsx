/**
 * Il renderer dello scafale: da albero a finestra.
 *
 * # Cosa decide chi
 *
 * **L'albero decide *dove* va la pagina; l'app decide *quale* pagina è.**
 * `testa()` e `corpo()` restano in `App.tsx` e arrivano qui come **slot**: sono
 * instradamento, guidato da stato dell'app — quale vista, quale album aperto, se
 * si sta cercando — e una skin non ha titolo a sceglierlo. Metterli nell'albero
 * vorrebbe dire condizionali nel formato, cioè un linguaggio di programmazione:
 * precisamente ciò che la decisione sui prefab rifiuta.
 *
 * Tutto il resto rende da `contesto`, un oggetto solo costruito in `App.tsx`. È
 * anche l'unico imbuto per gli `useState` di `App` — il conteggio sta nel
 * commento gemello in `App.tsx`, e tenerlo in un posto solo è il modo di non
 * avere due numeri che divergono — che stavano passando di mano in mano:
 * `Lettore` prende sei prop, `Colonna` sei, `Coda` tre.
 *
 * # Le sovrapposizioni non sono qui
 *
 * Modali, menù e notifiche restano fuori dall'albero, in `App.tsx`. Si
 * sovrappongono per definizione, e tenere gli unici elementi sovrapposti in mano
 * al motore è ciò che rende «una skin non può sovrapporre» **vero** dell'albero
 * invece che imposto su di esso.
 *
 * # Niente geometria da questo lato
 *
 * Nessuno `style={{…}}` calcolato: ogni misura è già nel foglio, scritta dal
 * compilatore. `parts.rs` registra cos'era successo l'ultima volta che il
 * sistema aveva due autori di CSS.
 */
import type { ReactNode } from "react";

import { Coda } from "./Coda";
import { Copertina } from "./Copertina";
import type {
  Brano,
  NodoScafale,
  Playlist,
  StatoRiproduzione,
  ValoreOpzione,
} from "./ipc";
import { BarraSelezione } from "./parti/BarraSelezione";
import { Colonna } from "./parti/Colonna";
import { Giudizio } from "./parti/Giudizio";
import { Navigazione, type Vista } from "./parti/Navigazione";
import { Ora } from "./parti/Ora";
import { Scrubber } from "./parti/Scrubber";
import { Trasporto, type TagliaTrasporto } from "./parti/Trasporto";
import { Lettore } from "./Lettore";
import { titoloAlbum } from "./formato";

/**
 * Tutto quel che i widget sanno del mondo.
 *
 * Un oggetto solo, costruito una volta in `App.tsx`. Il prezzo è che cambiare
 * un widget può richiedere una voce in più qui; quel che si compra è che
 * aggiungere un widget all'albero non richiede di far scendere una prop
 * attraverso tre componenti che non la usano.
 */
export interface ContestoWidget {
  stato: StatoRiproduzione;
  // La posizione **non** sta qui, ed è deliberato: cambia venti volte al
  // secondo, e questo oggetto è nelle dipendenze di tutto l'albero. La legge
  // `Scrubber` dal suo archivio — vedi `riproduzione.ts`.
  vista: Vista;
  playlistAperta: Playlist | null;
  inLibreria: boolean;
  conteggi: Partial<Record<Vista, number>>;
  playlist: Playlist[];
  colonnaAperta: boolean;
  codaAperta: boolean;
  /**
   * «In riproduzione» a schermo intero è aperto.
   *
   * I tre widget della riproduzione lo leggono per **togliersi di mezzo**.
   * Sono tre modi di mostrare la stessa cosa — la colonna, la barra, la coda —
   * e quella schermata è il quarto: tenerli accesi sotto voleva dire la stessa
   * copertina, lo stesso titolo, lo stesso trasporto e la stessa coda disegnati
   * due volte affiancati, con lo schermo intero schiacciato in mezza finestra.
   *
   * Lo legge anche `page-header`, e per una ragione diversa: non è un doppione
   * di quella schermata, è il titolo di **un'altra** pagina. Restando acceso
   * teneva la copertina grande sotto una fascia che diceva «Importazioni», e la
   * schermata intera cominciava un centimetro più in basso di dove finisce la
   * finestra. Andandosene, il posto del contenuto riempie la sua zona.
   */
  grande: boolean;
  selezionati: number[];
  tuttiSelezionati: boolean;
  onVista: (vista: Vista) => void;
  onPlaylist: (p: Playlist) => void;
  onMenuPlaylist: (e: React.MouseEvent, p: Playlist) => void;
  onNuovaPlaylist: () => void;
  /** Apre l'editor delle regole per una playlist che si aggiorna da sé. */
  onNuovaSmart: () => void;
  /** Sceglie un file M3U, PLS o XSPF da portare dentro. */
  onImportaFile: () => void;
  /** Apre la pagina delle donazioni nel browser di sistema. */
  onDona: () => void;
  onColonna: (aperta: boolean) => void;
  onCoda: (aperta: boolean) => void;
  onGrande: () => void;
  onPreferito: (brano: Brano) => void;
  onVoto: (brano: Brano, stelle: number) => void;
  onErrore: (e: unknown) => void;
  onSelezioneRiproduci: () => void;
  onSelezioneDopo: () => void;
  onSelezioneAccoda: () => void;
  onSelezionePlaylist: () => void;
  onSelezioneTuttiOAnnulla: () => void;
  onSelezioneChiudi: () => void;
}

/** I due buchi che l'app riempie: quale pagina, e la sua intestazione. */
export interface SlotScafale {
  intestazione: ReactNode;
  contenuto: ReactNode;
}

/** Quel che un widget riceve. */
interface Argomenti {
  nodo: NodoScafale;
  ctx: ContestoWidget;
  slot: SlotScafale;
}

/**
 * Una voce del registro di questo lato.
 *
 * `visibile` è dichiarato qui e non in Rust perché è **stato dell'app**, non
 * dato della skin: la terza colonna è aperta o chiusa, la barra della selezione
 * c'è solo con dei brani selezionati. Un widget nascosto semplicemente non è nel
 * flusso flex — ed è la ragione per cui le zone sono flex e non grid.
 */
interface ComponenteWidget {
  rende: (args: Argomenti) => ReactNode;
  visibile?: (ctx: ContestoWidget) => boolean;
}

/** Il valore di una manopola, col suo tipo e un ripiego se il tipo non torna. */
function bandiera(nodo: NodoScafale, nome: string, difetto: boolean): boolean {
  const valore: ValoreOpzione | undefined = nodo.options[nome];
  return typeof valore === "boolean" ? valore : difetto;
}

function parola<T extends string>(
  nodo: NodoScafale,
  nome: string,
  ammesse: readonly T[],
  difetto: T,
): T {
  const valore: ValoreOpzione | undefined = nodo.options[nome];
  return typeof valore === "string" && (ammesse as readonly string[]).includes(valore)
    ? (valore as T)
    : difetto;
}

const TAGLIE_TRASPORTO = ["bar", "column", "large"] as const;
const DA_TAGLIA: Record<(typeof TAGLIE_TRASPORTO)[number], TagliaTrasporto> = {
  bar: "barra",
  column: "colonna",
  large: "grande",
};

/**
 * Il registro dei widget di questo lato.
 *
 * Deve restare allineato con `WIDGETS` in `core/aether-skin/src/layout.rs`, e
 * **senza un runner di test in TypeScript** l'allineamento non si asserisce: si
 * rende visibile. Un nome dell'albero che non è qui produce un segnaposto che si
 * vede, non un buco silenzioso.
 */
const WIDGET: Record<string, ComponenteWidget> = {
  ambient: {
    rende: () => <div className="ambiente ambient-backdrop" aria-hidden="true" />,
  },

  navigation: {
    rende: ({ nodo, ctx }) => (
      <Navigazione
        vista={ctx.vista}
        playlistAperta={ctx.playlistAperta?.id ?? null}
        inLibreria={ctx.inLibreria}
        conteggi={ctx.conteggi}
        playlist={ctx.playlist}
        larga={bandiera(nodo, "wide", false)}
        onVista={ctx.onVista}
        onPlaylist={ctx.onPlaylist}
        onMenuPlaylist={ctx.onMenuPlaylist}
        onNuovaPlaylist={ctx.onNuovaPlaylist}
        onNuovaSmart={ctx.onNuovaSmart}
        onImportaFile={ctx.onImportaFile}
        onDona={ctx.onDona}
      />
    ),
  },

  "bottom-nav": {
    rende: ({ ctx }) => (
      <Navigazione
        vista={ctx.vista}
        playlistAperta={ctx.playlistAperta?.id ?? null}
        inLibreria={ctx.inLibreria}
        conteggi={ctx.conteggi}
        playlist={ctx.playlist}
        inFondo
        onVista={ctx.onVista}
        onPlaylist={ctx.onPlaylist}
        onMenuPlaylist={ctx.onMenuPlaylist}
        onNuovaPlaylist={ctx.onNuovaPlaylist}
        onNuovaSmart={ctx.onNuovaSmart}
        onImportaFile={ctx.onImportaFile}
        onDona={ctx.onDona}
      />
    ),
  },

  "page-header": {
    // Via a schermo intero, per la stessa ragione degli altri quattro qui
    // sotto: quella schermata **è** la pagina, e un'intestazione che nomina
    // un'altra pagina — «Importazioni», con i suoi due tasti — sospesa sopra
    // una copertina dice che sotto ce ne sono due, che è quel che sembrava.
    //
    // Togliendola, `content` resta l'unico figlio a riempimento della sua
    // zona e la occupa tutta: la copertina grande arriva fin sotto la barra
    // del titolo senza che nessuna misura sia scritta due volte.
    visibile: (ctx) => !ctx.grande,
    rende: ({ slot }) => slot.intestazione,
  },
  content: { rende: ({ slot }) => slot.contenuto },

  player: {
    // La barra della selezione prende il posto del lettore: è la stessa domanda
    // — «cosa sto per fare adesso» — in due momenti diversi, e chi ha appena
    // scelto quaranta brani non sta cercando il tasto pausa.
    visibile: (ctx) =>
      !ctx.colonnaAperta &&
      !ctx.grande &&
      ctx.selezionati.length === 0 &&
      ctx.stato.brano !== null,
    rende: ({ ctx }) => (
      <Lettore
        stato={ctx.stato}
        codaAperta={ctx.codaAperta}
        onCoda={() => ctx.onCoda(!ctx.codaAperta)}
        onPreferito={ctx.onPreferito}
        onErrore={ctx.onErrore}
        onColonna={() => ctx.onColonna(true)}
      />
    ),
  },

  column: {
    // Chiusa a schermo intero, e **non** chiusa davvero: `colonnaAperta` resta
    // com'era, quindi uscire dalla schermata la ritrova aperta. Spegnerla per
    // davvero avrebbe fatto pagare a chi apre la copertina grande la riapertura
    // della colonna ogni volta.
    visibile: (ctx) => ctx.colonnaAperta && !ctx.grande,
    rende: ({ ctx }) => (
      <Colonna
        stato={ctx.stato}
        onChiudi={() => ctx.onColonna(false)}
        onEspandi={ctx.onGrande}
        onPreferito={ctx.onPreferito}
        onVoto={ctx.onVoto}
        onErrore={ctx.onErrore}
      />
    ),
  },

  transport: {
    visibile: (ctx) => ctx.stato.brano !== null,
    rende: ({ nodo, ctx }) => (
      <Trasporto
        stato={ctx.stato}
        taglia={DA_TAGLIA[parola(nodo, "size", TAGLIE_TRASPORTO, "bar")]}
        conMescola={bandiera(nodo, "shuffle", true)}
        conRipeti={bandiera(nodo, "repeat", true)}
        onErrore={ctx.onErrore}
      />
    ),
  },

  scrubber: {
    visibile: (ctx) => ctx.stato.brano !== null,
    rende: ({ nodo, ctx }) => (
      <Scrubber
        stato={ctx.stato}
        conTempi={bandiera(nodo, "times", true)}
        onErrore={ctx.onErrore}
      />
    ),
  },

  queue: {
    // A schermo intero la coda ce l'ha già quella schermata, nel suo pannello.
    visibile: (ctx) => ctx.codaAperta && !ctx.colonnaAperta && !ctx.grande,
    rende: ({ ctx }) => (
      <Coda stato={ctx.stato} onChiudi={() => ctx.onCoda(false)} onErrore={ctx.onErrore} />
    ),
  },

  "selection-bar": {
    // Non a schermo intero: galleggia più in alto della schermata (z-index 4
    // contro 3) e resterebbe sospesa sopra una copertina, a offrire comandi
    // sulle righe che quella copertina sta coprendo.
    visibile: (ctx) => ctx.selezionati.length > 0 && !ctx.grande,
    rende: ({ ctx }) => (
      <BarraSelezione
        quanti={ctx.selezionati.length}
        tuttiSelezionati={ctx.tuttiSelezionati}
        onRiproduci={ctx.onSelezioneRiproduci}
        onDopo={ctx.onSelezioneDopo}
        onAccoda={ctx.onSelezioneAccoda}
        onPlaylist={ctx.onSelezionePlaylist}
        onTuttiOAnnulla={ctx.onSelezioneTuttiOAnnulla}
        onChiudi={ctx.onSelezioneChiudi}
      />
    ),
  },

  "now-playing": {
    visibile: (ctx) => ctx.stato.brano !== null,
    rende: ({ nodo, ctx }) =>
      ctx.stato.brano && (
        <Ora
          brano={ctx.stato.brano}
          conCopertina={bandiera(nodo, "cover", true)}
          conCuore={bandiera(nodo, "heart", true)}
          onPreferito={ctx.onPreferito}
        />
      ),
  },

  "cover-large": {
    visibile: (ctx) => ctx.stato.brano !== null,
    rende: ({ ctx }) =>
      ctx.stato.brano && (
        <Copertina
          hash={ctx.stato.brano.coverArtHash}
          titolo={titoloAlbum(ctx.stato.brano.album)}
          classe="np-art"
          piena
        />
      ),
  },

  rating: {
    visibile: (ctx) => ctx.stato.brano !== null,
    rende: ({ nodo, ctx }) =>
      ctx.stato.brano && (
        <Giudizio
          stato={ctx.stato}
          brano={ctx.stato.brano}
          taglia="colonna"
          conStelle={bandiera(nodo, "stars", true)}
          conVolume={bandiera(nodo, "volume", true)}
          onPreferito={ctx.onPreferito}
          onVoto={ctx.onVoto}
          onErrore={ctx.onErrore}
        />
      ),
  },
};

/**
 * Un widget che il nucleo conosce e questo lato no.
 *
 * Si **vede**, e non è un ripiego elegante: senza un runner di test in
 * TypeScript, l'unico modo onesto di tenere allineati due elenchi è rendere
 * rumorosa la divergenza. È la stessa mossa che l'anteprima dello Studio fa già
 * per le parti.
 */
function SegnapostoWidget({ nome }: { nome: string }) {
  return (
    <div className="widget-mancante" role="note">
      Widget «{nome}» non montato
    </div>
  );
}

function Foglia({ nodo, ctx, slot }: Argomenti) {
  const voce = WIDGET[nodo.name];
  if (!voce) return <SegnapostoWidget nome={nodo.name} />;
  if (voce.visibile && !voce.visibile(ctx)) return null;
  return <>{voce.rende({ nodo, ctx, slot })}</>;
}

function Nodo({ nodo, ctx, slot }: Argomenti) {
  if (nodo.kind === "widget") {
    const voce = WIDGET[nodo.name];
    if (voce?.visibile && !voce.visibile(ctx)) return null;
    // L'involucro porta l'indirizzo e la classe di parte: il primo aggancia le
    // regole di misura del foglio, la seconda quel che la skin ridipinge. Il
    // componente dentro resta ignaro di entrambe le cose.
    return (
      <div className={classi(nodo)} data-nodo={nodo.at}>
        <Foglia nodo={nodo} ctx={ctx} slot={slot} />
      </div>
    );
  }

  return (
    <div className={classi(nodo)} data-nodo={nodo.at}>
      {nodo.children.map((figlio) => (
        <Nodo key={figlio.at} nodo={figlio} ctx={ctx} slot={slot} />
      ))}
    </div>
  );
}

/**
 * Le classi di un nodo.
 *
 * Semantica prima, parte del registro dopo, sempre additive: è la convenzione
 * del markup di tutta l'app.
 */
function classi(nodo: NodoScafale): string {
  const semantica = nodo.kind === "zone" ? `zona zona-${nodo.name}` : `posto posto-${nodo.name}`;
  return nodo.part ? `${semantica} ${nodo.part}` : semantica;
}

/**
 * Rende lo scafale di una skin.
 *
 * `null` finché la skin non è arrivata: il fondo giusto lo dà già `stile.css`,
 * che porta il blocco `:root` copiato dal compilatore, quindi il fotogramma
 * vuoto non lampeggia del colore sbagliato.
 */
export function Impaginazione({
  albero,
  slot,
  contesto,
}: {
  albero: NodoScafale | null;
  slot: SlotScafale;
  contesto: ContestoWidget;
}) {
  if (!albero) return null;
  return (
    <div
      className={classi(albero)}
      data-nodo={albero.at}
      data-scafale=""
      /* Due attributi sulla radice, e non uno stato in più: sono derivati dagli
         **stessi** predicati di visibilità che decidono se i widget rendono,
         quindi non possono contraddirli. Servono a chi galleggia `fixed` sopra
         il contenuto — la barra del lettore, la notifica — e deve sapere se
         sotto c'è spazio suo o della terza colonna. */
      data-colonna={contesto.colonnaAperta || undefined}
      data-lettore={WIDGET.player?.visibile?.(contesto) ? "" : undefined}
    >
      {albero.children.map((figlio) => (
        <Nodo key={figlio.at} nodo={figlio} ctx={contesto} slot={slot} />
      ))}
    </div>
  );
}
