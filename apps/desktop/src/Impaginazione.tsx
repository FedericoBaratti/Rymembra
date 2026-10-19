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
 * anche l'unico imbuto per lo stato di `App` che i widget leggono — quale sia,
 * e dove viva, sta nel commento gemello in `App.tsx`, e tenerlo in un posto
 * solo è il modo di non avere due elenchi che divergono — che stava passando
 * di mano in mano: `Lettore` prende sei prop, `Colonna` sei, `Coda` tre.
 *
 * # Lo stato del motore non passa dal contesto
 *
 * Ci passava, ed era la ragione per cui il contesto non stava mai fermo: ogni
 * passo del volume, ogni pausa, ogni banda dell'equalizzatore ne faceva uno
 * nuovo, e con lui si ridisegnava lo scafale intero — la navigazione, la barra
 * della selezione, la testata. Adesso lo stato sta in un archivio
 * (`riproduzione.ts`), e lo legge **la foglia** che lo disegna: i widget che
 * ne hanno bisogno sono segnati `conStato` nel registro, e solo loro si
 * iscrivono. Per decidere se mostrarsi, gli altri chiedono una cosa sola — «c'è
 * un brano?» — che cambia quando si comincia o si smette di ascoltare, non a
 * ogni passo del cursore.
 *
 * L'anteprima dello Studio lo stato lo porta ancora nel contesto, finto: quando
 * c'è, vince lui (`useStatoDelloScafale`), e la tela non segue il brano che si
 * sta ascoltando.
 *
 * # I widget sotto `memo`
 *
 * `Navigazione`, `Lettore`, `Colonna`, `Coda`, `Trasporto`, `Scrubber`,
 * `BarraSelezione`, `Ora` e `Giudizio` sono sotto `memo`. Lo scafale si
 * ridisegna quando cambia il contesto — una riga scelta, la colonna che si
 * apre — e senza `memo` ogni suo disegno rifaceva tutti i widget, anche quelli
 * a cui quel cambio non diceva niente: la colonna per una riga scelta, la
 * navigazione per un passo del volume. Le loro prop adesso sono stabili — lo
 * stato del motore cambia solo quando cambia davvero, le chiamate del contesto
 * hanno un'identità sola (`App.tsx`, e `useMani` qui sotto) — e `memo` serve.
 * Non sugli altri: `Copertina` e i segnaposto costano meno del confronto.
 *
 * La lingua non è una prop, e un componente sotto `memo` non si ridisegna
 * quando cambia: ciascuno di quei nove si iscrive con `useLingua`, come già
 * fanno le righe dell'elenco.
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
import {
  useCallback,
  useContext,
  useSyncExternalStore,
  type ReactNode,
} from "react";

import { NESSUNA_ISCRIZIONE } from "./archivio";
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
import { statoSuono } from "./segnali";
import { Scrubber } from "./parti/Scrubber";
import { Trasporto, type TagliaTrasporto } from "./parti/Trasporto";
import { Lettore } from "./Lettore";
import { titoloAlbum } from "./formato";
import { LettoreDellaFinestra } from "./riproduzione";

/**
 * Tutto quel che i widget sanno del mondo.
 *
 * Un oggetto solo, costruito una volta in `App.tsx`. Il prezzo è che cambiare
 * un widget può richiedere una voce in più qui; quel che si compra è che
 * aggiungere un widget all'albero non richiede di far scendere una prop
 * attraverso tre componenti che non la usano.
 *
 * È il contesto della finestra: lo stato del motore non c'è, lo leggono le
 * foglie dall'archivio (vedi «Lo stato del motore non passa dal contesto» in
 * testa). Quello dello Studio è `ContestoWidget`, che lo porta.
 */
export interface ContestoScafale {
  /** Lo stato della riproduzione, solo se finto: vedi `ContestoWidget`. */
  stato?: StatoRiproduzione | undefined;
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

/**
 * Il contesto dell'anteprima dello Studio: quello della finestra, con lo stato.
 *
 * Lo Studio disegna un brano finto e non deve seguire quello che suona, quindi
 * lo stato lo porta qui dentro, e quando c'è vince sull'archivio
 * (`useStatoDelloScafale`). Il nome è quello di prima della separazione,
 * perché è quello che `studio/mondo.ts` e `studio/Tela.tsx` conoscono.
 */
export interface ContestoWidget extends ContestoScafale {
  stato: StatoRiproduzione;
}

/** I due buchi che l'app riempie: quale pagina, e la sua intestazione. */

export interface SlotScafale {
  intestazione: ReactNode;
  contenuto: ReactNode;
}

/** Quel che un widget riceve. */
interface Argomenti {
  nodo: NodoScafale;
  ctx: ContestoScafale;
  slot: SlotScafale;
}

/**
 * Le chiamate che i widget fanno col contesto, con un'identità che resta.
 *
 * Scritte sul posto — `() => ctx.onColonna(false)` — erano una funzione nuova a
 * ogni disegno dello scafale, e il `memo` di `Lettore`, `Colonna` e `Coda`
 * non sarebbe servito a niente: ogni cambio della selezione li avrebbe
 * ridisegnati per una prop che dice la stessa cosa. Vedi `useMani`.
 */
interface Mani {
  alternaCoda: () => void;
  chiudiCoda: () => void;
  apriColonna: () => void;
  chiudiColonna: () => void;
}

/**
 * Una voce del registro di questo lato.
 *
 * `visibile` è dichiarato qui e non in Rust perché è **stato dell'app**, non
 * dato della skin: la terza colonna è aperta o chiusa, la barra della selezione
 * c'è solo con dei brani selezionati. Un widget nascosto semplicemente non è nel
 * flusso flex — ed è la ragione per cui le zone sono flex e non grid.
 *
 * Del motore `visibile` sa una cosa sola, se c'è un brano (`conBrano`): è
 * tutto quel che serve per decidere, e chiederlo costa un booleano invece
 * dello stato intero. `conStato` segna chi lo stato lo disegna, e lo riceve.
 */
type ComponenteWidget =
  | {
      conStato?: false;
      rende: (args: Argomenti) => ReactNode;
      visibile?: (ctx: ContestoScafale, conBrano: boolean) => boolean;
    }
  | WidgetConStato;

/** Un widget che disegna lo stato del motore. */
interface WidgetConStato {
  conStato: true;
  rende: (args: Argomenti, stato: StatoRiproduzione, mani: Mani) => ReactNode;
  visibile?: (ctx: ContestoScafale, conBrano: boolean) => boolean;
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
 * Deve restare allineato con `WIDGETS` in `core/rymembra-skin/src/layout.rs`, e
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
    visibile: (ctx, conBrano) =>
      !ctx.colonnaAperta &&
      !ctx.grande &&
      ctx.selezionati.length === 0 &&
      conBrano,
    conStato: true,
    rende: ({ ctx }, stato, mani) => (
      <Lettore
        stato={stato}
        codaAperta={ctx.codaAperta}
        onCoda={mani.alternaCoda}
        onPreferito={ctx.onPreferito}
        onErrore={ctx.onErrore}
        onColonna={mani.apriColonna}
      />
    ),
  },

  column: {
    // Chiusa a schermo intero, e **non** chiusa davvero: `colonnaAperta` resta
    // com'era, quindi uscire dalla schermata la ritrova aperta. Spegnerla per
    // davvero avrebbe fatto pagare a chi apre la copertina grande la riapertura
    // della colonna ogni volta.
    visibile: (ctx) => ctx.colonnaAperta && !ctx.grande,
    conStato: true,
    rende: ({ ctx }, stato, mani) => (
      <Colonna
        stato={stato}
        onChiudi={mani.chiudiColonna}
        onEspandi={ctx.onGrande}
        onPreferito={ctx.onPreferito}
        onVoto={ctx.onVoto}
        onErrore={ctx.onErrore}
      />
    ),
  },

  transport: {
    visibile: (_ctx, conBrano) => conBrano,
    conStato: true,
    rende: ({ nodo, ctx }, stato) => (
      <Trasporto
        stato={stato}
        taglia={DA_TAGLIA[parola(nodo, "size", TAGLIE_TRASPORTO, "bar")]}
        conMescola={bandiera(nodo, "shuffle", true)}
        conRipeti={bandiera(nodo, "repeat", true)}
        onErrore={ctx.onErrore}
      />
    ),
  },

  scrubber: {
    visibile: (_ctx, conBrano) => conBrano,
    conStato: true,
    rende: ({ nodo, ctx }, stato) => (
      <Scrubber
        stato={stato}
        conTempi={bandiera(nodo, "times", true)}
        onErrore={ctx.onErrore}
      />
    ),
  },

  queue: {
    // A schermo intero la coda ce l'ha già quella schermata, nel suo pannello.
    visibile: (ctx) => ctx.codaAperta && !ctx.colonnaAperta && !ctx.grande,
    conStato: true,
    rende: ({ ctx }, stato, mani) => (
      <Coda stato={stato} onChiudi={mani.chiudiCoda} onErrore={ctx.onErrore} />
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
    visibile: (_ctx, conBrano) => conBrano,
    conStato: true,
    rende: ({ nodo, ctx }, stato) =>
      stato.brano && (
        <Ora
          brano={stato.brano}
          inPausa={stato.inPausa}
          conCopertina={bandiera(nodo, "cover", true)}
          conCuore={bandiera(nodo, "heart", true)}
          onPreferito={ctx.onPreferito}
        />
      ),
  },

  "cover-large": {
    visibile: (_ctx, conBrano) => conBrano,
    conStato: true,
    rende: (_args, stato) =>
      stato.brano && (
        <Copertina
          hash={stato.brano.coverArtHash}
          titolo={titoloAlbum(stato.brano.album)}
          classe="np-art"
          piena
          suono={statoSuono(true, stato.inPausa)}
          cambio
        />
      ),
  },

  rating: {
    visibile: (_ctx, conBrano) => conBrano,
    conStato: true,
    rende: ({ nodo, ctx }, stato) =>
      stato.brano && (
        <Giudizio
          stato={stato}
          brano={stato.brano}
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

/**
 * Lo stato del motore per un widget dello scafale.
 *
 * Dall'archivio della finestra, iscrivendosi; dal contesto quando il contesto
 * lo porta, cioè nell'anteprima dello Studio, e allora senza iscriversi a
 * niente: la tela disegna un brano finto e non deve svegliarsi per quello vero.
 */
function useStatoDelloScafale(ctx: ContestoScafale): StatoRiproduzione {
  const archivio = useContext(LettoreDellaFinestra);
  const finto = ctx.stato;
  return useSyncExternalStore(
    finto === undefined ? archivio.iscrivi : NESSUNA_ISCRIZIONE,
    () => finto ?? archivio.leggi(),
  );
}

/**
 * C'è un brano, per chi deve soltanto decidere se mostrarsi.
 *
 * Un booleano e non lo stato: chi lo chiede si ridisegna quando si comincia o
 * si smette di ascoltare, non a ogni passo del volume.
 */
function useConBrano(ctx: ContestoScafale): boolean {
  const archivio = useContext(LettoreDellaFinestra);
  const finto = ctx.stato;
  return useSyncExternalStore(
    finto === undefined ? archivio.iscrivi : NESSUNA_ISCRIZIONE,
    () => (finto ?? archivio.leggi()).brano !== null,
  );
}

/** Le chiamate col contesto, stabili finché il contesto non le cambia: vedi `Mani`. */
function useMani(ctx: ContestoScafale): Mani {
  const { onCoda, onColonna, codaAperta } = ctx;
  return {
    alternaCoda: useCallback(() => onCoda(!codaAperta), [onCoda, codaAperta]),
    chiudiCoda: useCallback(() => onCoda(false), [onCoda]),
    apriColonna: useCallback(() => onColonna(true), [onColonna]),
    chiudiColonna: useCallback(() => onColonna(false), [onColonna]),
  };
}

function Foglia({ nodo, ctx, slot }: Argomenti) {
  const voce = WIDGET[nodo.name];
  if (!voce) return <SegnapostoWidget nome={nodo.name} />;
  if (voce.conStato) {
    return <FogliaConStato voce={voce} nodo={nodo} ctx={ctx} slot={slot} />;
  }
  return <>{voce.rende({ nodo, ctx, slot })}</>;
}

/**
 * Una foglia che disegna lo stato del motore: la sola che se lo legge.
 *
 * Un componente a parte e non un ramo di `Foglia`, perché gli hook non stanno
 * dietro un `if`: chi non è `conStato` non si iscrive a niente.
 */
function FogliaConStato({
  voce,
  nodo,
  ctx,
  slot,
}: Argomenti & { voce: WidgetConStato }) {
  const stato = useStatoDelloScafale(ctx);
  const mani = useMani(ctx);
  return <>{voce.rende({ nodo, ctx, slot }, stato, mani)}</>;
}

/**
 * Un widget al suo posto, se in questo momento va mostrato.
 *
 * La visibilità si chiedeva due volte, qui e in `Foglia`; adesso solo qui, che
 * è il punto in cui un widget nascosto esce dal flusso col suo involucro.
 */
function NodoWidget({ nodo, ctx, slot }: Argomenti) {
  const voce = WIDGET[nodo.name];
  const conBrano = useConBrano(ctx);
  if (voce?.visibile && !voce.visibile(ctx, conBrano)) return null;
  // L'involucro porta solo l'indirizzo, che aggancia le regole di misura del
  // foglio. La parte la porta il componente dentro: quando la portavano tutti
  // e due, ogni widget si dipingeva due volte — l'opacità si moltiplicava, i
  // bordi si raddoppiavano, un livello si disegnava sopra l'altro.
  return (
    <div className={classi(nodo)} data-nodo={nodo.at}>
      <Foglia nodo={nodo} ctx={ctx} slot={slot} />
    </div>
  );
}

function Nodo({ nodo, ctx, slot }: Argomenti) {
  if (nodo.kind === "widget") return <NodoWidget nodo={nodo} ctx={ctx} slot={slot} />;

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
 * del markup di tutta l'app. Solo le **zone** portano la loro parte: quella di
 * un widget è un metadato — dice allo Studio quale parte selezionare — e la
 * porta già il componente.
 */
function classi(nodo: NodoScafale): string {
  if (nodo.kind === "widget") return `posto posto-${nodo.name}`;
  const semantica = `zona zona-${nodo.name}`;
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
  contesto: ContestoScafale;
}) {
  const conBrano = useConBrano(contesto);
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
      data-lettore={WIDGET.player?.visibile?.(contesto, conBrano) ? "" : undefined}
    >
      {albero.children.map((figlio) => (
        <Nodo key={figlio.at} nodo={figlio} ctx={contesto} slot={slot} />
      ))}
    </div>
  );
}
