/**
 * La finestra.
 *
 * Non decide niente sulla libreria: chiede al nucleo e disegna quel che torna.
 * Ogni volta che qui comparisse una regola — quali file sono musica, quando due
 * brani sono lo stesso, cosa si può cancellare — sarebbe una regola che Android
 * dovrà riscrivere, ed è esattamente il modo in cui i due alberi sono divergiti.
 *
 * # L'impalcatura: tre colonne
 *
 * `240px 1fr 348px`. A sinistra **solo** navigazione; al centro la libreria; a
 * destra quel che sta suonando. La terza colonna prende il posto della barra in
 * fondo: la stessa area, girata di novanta gradi, mostra una copertina da
 * trecento pixel e la coda intera invece di una miniatura da quaranta.
 *
 * Chiudendo la colonna il lettore flottante torna identico a com'era, e sotto i
 * 1100 pixel si chiude da sé. È il patto che rende la scelta reversibile invece
 * che imposta.
 */
import { open, save } from "@tauri-apps/plugin-dialog";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";

import { AggiungiAPlaylist } from "./AggiungiAPlaylist";
import { Chiedi } from "./Chiedi";
import { Copertina } from "./Copertina";
import { Giro, type NomePasso } from "./Giro";
import { Impaginazione, type ContestoWidget } from "./Impaginazione";
import { Account } from "./Account";
import { Importa, ImportaLink } from "./Importa";
import { ImportaPlaylist } from "./ImportaPlaylist";
import { Regole } from "./Regole";
import { Menu, type Apertura } from "./Menu";
import { NuovoTema } from "./NuovoTema";
import { Primo } from "./Primo";
import { Ripristino } from "./Ripristino";
import { Stelle } from "./Stelle";
import { cambiandoVista } from "./transizione";
import {
  applicaAccento,
  applicaMovimento,
  applicaSkin,
  type MovimentoUtente,
} from "./aspetto";
import {
  brani_,
  durata,
  nomeArtista,
  nomeFonte,
  numero,
  titoloAlbum,
} from "./formato";
import { useFinestrella } from "./finestrella";
import { useFuoco } from "./fuoco";
import {
  eRitentabile,
  guastoDa,
  ipc,
  type Guasto,
  type Album,
  type Artista,
  type Casa,
  type Raccolta,
  type Avanzamento,
  type Avvio,
  type Brano,
  type Cancellazione,
  type EsitoScansione,
  type Ordine,
  type Playlist,
  type Skin,
  type VoceSkin,
} from "./ipc";
import { applicaLingua, scegli, t, useLingua } from "./lingue";
import { useNuvola } from "./nuvola";
import { useAscolto, usePagine, usePigro } from "./pagine";
import { AvvisoAggiornamento } from "./parti/Aggiornamenti";
import { AvvisoAudio } from "./parti/AvvisoAudio";
import { AvvisoCoda } from "./parti/AvvisoCoda";
import { Icona } from "./parti/Icone";
import { useImportazioni } from "./parti/Importazioni";
import { Intestazione } from "./parti/Intestazione";
import type { Vista } from "./parti/Navigazione";
import { ToastImportazioni } from "./parti/ToastImportazioni";
import { usePresaPerRiordino } from "./riordino";
import { useRiproduzione } from "./riproduzione";
import { Artisti } from "./schermate/Artisti";
import { Cartelle } from "./schermate/Cartelle";
import {
  Esplora,
  ESPLORA_INIZIALE,
  type StatoEsplora,
} from "./schermate/Esplora";
import { Home, TestaHome } from "./schermate/Home";
import {
  SchermataImportazioni,
  TestaImportazioni,
} from "./schermate/Importazioni";
import { Impostazioni, type Sezione } from "./schermate/Impostazioni";
import { InRiproduzione } from "./schermate/InRiproduzione";
import { Studio } from "./studio/Studio";
import { sorgenteNuova, type DatiTema } from "./studio/nuovo";
import {
  campoRicerca,
  leggiAssociazioni,
  scriviAssociazioni,
  useScorciatoie,
  type Associazioni,
} from "./tastiera";
import {
  applicaTema,
  dimenticaRipiego,
  seguiIlSistema,
  temaDiRipiego,
  type Tema,
} from "./tema";
import { useVirtuale } from "./virtuale";
import { Trans } from "./lingue/Trans";

/**
 * Come si legge un ordinamento, e in che ordine si sceglie.
 *
 * Una funzione perché porta testo: una costante di modulo si fisserebbe sulla
 * lingua che c'era al primo `import`.
 */
function ordinamenti(): readonly (readonly [Ordine, string])[] {
  return [
    ["scaffale", t("sort.shelf")],
    ["recenti", t("sort.recent")],
    ["ascoltati", t("sort.played")],
    ["titolo", t("sort.title")],
  ];
}

/**
 * Sotto questa larghezza la terza colonna si chiude da sé.
 *
 * Con la navigazione aperta, tre colonne su una finestra da 1100 lascerebbero
 * all'elenco meno spazio della sua intestazione: la griglia degli album
 * scenderebbe a due schede per riga, e le colonne «album» e «durata» delle
 * righe sparirebbero. Il minimo della finestra è 880, quindi questo caso
 * capita davvero.
 */
const LARGHEZZA_TRE_COLONNE = 1100;

/**
 * Una riga dell'elenco: una `row` di una `grid`, non un `div` qualunque.
 *
 * `memo` e non una funzione nuda. Da sola non bastava — finché la posizione
 * viveva in `App`, i gestori scendevano in identità nuove venti volte al
 * secondo e nessun confronto poteva riuscire. Ora che la posizione sta nel suo
 * archivio, `App` si ridisegna solo quando cambia qualcosa di vero, e le
 * duecento righe che non sono cambiate saltano il giro invece di riconciliarsi.
 *
 * # Perché `role="row"`, e perché il ruolo mancava
 *
 * Perché senza di esso `aria-selected` **non esiste**. L'attributo è valido solo
 * su pochi ruoli — `row`, `option`, `tab`, `gridcell` — e su un `div` generico
 * viene scartato in silenzio: la selezione multipla, cioè tutto ciò che accende
 * la barra di selezione e i suoi comandi, era visibile solo a chi guarda. Chi
 * ascolta lo schermo sentiva dodici righe identiche e nessuna scelta.
 *
 * # Perché le celle sono otto, e perché sono involucri
 *
 * In una `grid` i figli di una `row` devono essere celle, e per un po' qui lo
 * erano solo tre — titolo e artista, album, durata — mentre i quattro comandi e
 * la miniatura stavano nella riga senza ruolo. Non era una dimenticanza: un
 * `role` **sostituisce** quello nativo, quindi un `gridcell` scritto sul
 * `<button>` farebbe annunciare «cella» dove si deve sentire «bottone», e i
 * comandi di riga sono esattamente la ragione per cui questo elenco è una `grid`
 * e non una `listbox`.
 *
 * L'involucro è il modo di avere le due cose: la cella è un `<div>` attorno al
 * comando, il comando resta un bottone, e nessuno dei due ruoli paga per
 * l'altro. `display: contents` nel foglio toglie l'involucro dalla
 * disposizione — `--colonne-riga` continua a vedere otto figli come prima, non
 * sette più un contenitore — e in WebView2 **non** lo toglie dall'albero di
 * accessibilità: la cella compare col bottone dentro, verificato sull'albero vero
 * della versione in uso prima di scriverlo. È il punto su cui questa forma
 * stava o cadeva, perché una cella invisibile a chi ascolta sarebbe peggio di
 * nessuna cella.
 *
 * Il conto torna anche con l'intestazione, che ha sempre avuto sette
 * `columnheader` (otto dentro una playlist modificabile) contro le tre celle
 * delle righe: adesso ogni colonna ha la sua intestazione **e** la sua cella, e
 * «Durata, 3:41» si può dire di ogni riga e non solo di tre.
 */
const RigaBrano = memo(function RigaBrano({
  brano,
  indice,
  attivo,
  suonabile,
  colFuoco,
  onSpostaFuoco,
  onFuocoPreso,
  onSuona,
  onPreferito,
  onVoto,
  onMenu,
  onTogli,
  selezionato,
  onSeleziona,
  numeroTraccia,
  onRiordina,
  onPresaPuntatore,
  sopra,
}: {
  brano: Brano;
  indice: number;
  attivo: boolean;
  suonabile: boolean;
  /**
   * È la riga col fuoco: la **sola** dell'elenco con `tabIndex={0}`.
   *
   * Vedi `fuoco.ts`: sette comandi per riga su diciottomila righe erano oltre
   * centomila fermate di tabulazione, cioè un elenco da cui non si esce.
   */
  colFuoco: boolean;
  /** Chiede che il fuoco vada su un'altra riga. Da `useFuoco().vaiA`. */
  onSpostaFuoco: (indice: number) => void;
  /** Dice che il fuoco è arrivato qui. Da `useFuoco().segna`. */
  onFuocoPreso: (indice: number) => void;
  onSuona: (indice: number) => void;
  onPreferito: (b: Brano) => void;
  onVoto: (b: Brano, stelle: number) => void;
  onMenu: (e: React.MouseEvent, brani: number[]) => void;
  /**
   * Solo dentro una playlist modificabile: toglie questa riga da lì.
   *
   * Prende l'indice invece di essere già legata a questa riga: una lambda
   * scritta dentro il `map` avrebbe un'identità nuova a ogni disegno, e
   * basterebbe da sola ad annullare il `memo` di duecento righe.
   */
  onTogli?: ((indice: number) => void) | undefined;
  /** Fa parte della selezione multipla. */
  selezionato: boolean;
  /** Ctrl per aggiungere una riga, Maiusc per prendere un intervallo. */
  onSeleziona: (e: React.MouseEvent, indice: number) => void;
  /**
   * La colonna `#` porta il numero di traccia invece della posizione in
   * elenco.
   *
   * Vero **solo** dentro un album, dove quel numero è del disco e serve a
   * ritrovare il pezzo sulla custodia. Fuori di lì era rumore: in un elenco
   * piatto di duecentocinquanta brani la colonna leggeva `1, 39, 8, 1, 5, 1,
   * 9…`, cioè la posizione di ciascuno dentro il **suo** album — un numero che
   * in quella pagina non risponde a nessuna domanda.
   */
  numeroTraccia?: boolean | undefined;
  /**
   * Dentro una playlist a mano: sposta questa riga in un'altra posizione.
   *
   * La sua presenza è quel che accende il trascinamento. `playlistRiordina`
   * stava nell'IPC dal primo giorno e non lo chiamava nessuno: la coda aveva
   * sia il trascinamento sia `Alt+↑↓`, l'elenco di una playlist nessuno dei
   * due, quindi l'ordine di una playlist si poteva solo subire.
   */
  onRiordina?: ((da: number, a: number) => void) | undefined;
  /**
   * La pressione che può diventare un trascinamento: vedi `riordino.ts`.
   *
   * Come `onTogli`, prende l'indice invece di essere legata alla riga, e per
   * la stessa ragione: il `memo`.
   */
  onPresaPuntatore?:
    | ((e: React.PointerEvent<HTMLElement>, indice: number) => void)
    | undefined;
  /** Il rilascio cadrebbe **su questa riga**: disegna il segno. */
  sopra?: boolean | undefined;
}) {
  /*
   * L'unico `memo` dell'applicazione è anche l'unico posto che deve iscriversi
   * alla lingua da sé: `App` si ridisegna, ma il confronto delle prop farebbe
   * saltare il giro proprio a queste duecento righe, e i loro titoli — «Preferito»,
   * «Togli dalla playlist» — resterebbero nella lingua di prima fino al primo
   * cambio di elenco.
   */
  useLingua();

  /**
   * `Alt+↑↓` sposta la riga.
   *
   * Esiste perché il trascinamento HTML5 **non** è raggiungibile da tastiera:
   * `dragstart` nasce da un puntatore, e nessuna combinazione di tasti lo
   * produce. Senza, riordinare una playlist sarebbe una funzione che una parte
   * delle persone non ha — non «scomoda», assente.
   */
  const daTastiera = (e: React.KeyboardEvent) => {
    if (!onRiordina || !e.altKey) return;
    const passo = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (passo === 0) return;
    e.preventDefault();
    onRiordina(indice, indice + passo);
    // Il fuoco segue la riga spostata invece di restare sulla posizione: chi
    // tiene premuto Alt sta spostando **una** canzone, e lasciando il fuoco
    // fermo al colpo dopo scenderebbe quella che ha preso il suo posto.
    //
    // Si chiede la **riga**, non il nodo. Prima qui c'era una ricerca di
    // `.riga .indice` per posizione nel DOM, e con la finestra virtuale quella
    // posizione non vuol più dire niente: il nodo numero `indice + passo` è il
    // nodo numero `indice + passo` *della finestra*, e se la riga è appena
    // uscita dalla finestra quel nodo non esiste affatto. `useFuoco` sa
    // scorrere prima e focalizzare dopo, che è l'ordine giusto.
    onSpostaFuoco(indice + passo);
  };

  return (
    <div
      className="riga list-row"
      role="row"
      /* L'ancora del giro guidato sta su **tutte** le righe e non solo sulla
         prima: con l'elenco virtualizzato la riga numero zero non esiste nel
         DOM appena si scorre, e un'ancora su quella sola avrebbe fatto saltare
         il passo a chi ha la libreria a metà. `Giro` prende la prima
         disegnata, che è quella in cima allo schermo. */
      data-giro="riga-brano"
      /* `+2` perché la riga 1 è l'intestazione di colonna. È l'attributo che
         rende dicibile una finestra: «riga 12.004 di 18.535» si può dire anche
         se nel DOM ce ne sono trenta, e senza di esso chi ascolta lo schermo
         sentirebbe «riga 7 di 30» in mezzo a una libreria. */
      aria-rowindex={indice + 2}
      /* Una fermata di tabulazione per l'elenco intero: vedi `colFuoco`. */
      tabIndex={colFuoco ? 0 : -1}
      onFocus={() => onFuocoPreso(indice)}
      aria-current={attivo}
      aria-selected={selezionato}
      data-active={attivo || undefined}
      data-scelta={selezionato || undefined}
      data-sopra={sopra || undefined}
      /* Col puntatore e non col trascinamento HTML5, che su Windows non
         rilascia mai: vedi `riordino.ts`. L'attributo è anche il modo in cui
         il gesto riconosce la riga d'arrivo. */
      data-riordino={onRiordina !== undefined ? indice : undefined}
      onPointerDown={
        onRiordina && onPresaPuntatore && ((e) => onPresaPuntatore(e, indice))
      }
      /* Sul contenitore e non sul tasto dell'indice: così l'`Alt+↑↓` funziona
         da qualunque comando della riga abbia il fuoco — l'indice, le stelle,
         il cuore — invece che da uno solo, che per giunta è spento quando non
         c'è un dispositivo audio. */
      onKeyDown={daTastiera}
      /* Doppio clic oltre al tasto: è il gesto che chi arriva da un altro
         lettore prova per primo, e non costa niente averlo. */
      onDoubleClick={() => suonabile && onSuona(indice)}
      onClick={(e) => onSeleziona(e, indice)}
      onContextMenu={(e) => onMenu(e, [brano.id])}
    >
      {/* La cella attorno al comando, non sul comando: vedi la nota del
          componente. */}
      <div role="gridcell" className="cella-comando">
        <button
          type="button"
          className="indice"
          /* Da `t()` come tutto il resto. Era l'**unica** etichetta di
             accessibilità dell'albero scritta a mano in italiano: in interfaccia
             inglese lo screen reader diceva «Riproduci …» su ognuna delle
             diciottomila righe, e il difetto non si vedeva perché un'etichetta
             non si guarda. */
          aria-label={t("list.play", { titolo: brano.title })}
          /* Il fuoco arriva con ←→ dalla riga, non con Tab: vedi `fuoco.ts`. */
          tabIndex={-1}
          /* Senza dispositivo audio il comando fallirebbe a ogni clic: meglio un
             tasto spento e l'errore letto una volta, che un errore nuovo ogni
             volta che si prova. */
          disabled={!suonabile}
          onClick={() => onSuona(indice)}
        >
          <span className="numero">
            {numeroTraccia ? (brano.trackNumber ?? indice + 1) : indice + 1}
          </span>
          <span className="via" aria-hidden="true">
            <Icona nome="i-play" dim={13} />
          </span>
        </button>
      </div>
      {/* La copertina non è un comando ma è una colonna, e una colonna senza
          cella rompe il conteggio esattamente come lo rompeva un bottone. Ha una
          classe sua perché quel che avvolge non è un comando: il foglio le dà la
          stessa regola, il nome dice la verità. */}
      <div role="gridcell" className="cella-miniatura">
        <Copertina
          hash={brano.coverArtHash}
          titolo={titoloAlbum(brano.album)}
          classe="miniatura"
        />
      </div>
      {/* Titolo e artista impilati in una cella sola, l'album nella sua: sono
          due informazioni di peso diverso, e dare all'artista una colonna larga
          quanto il titolo lo farebbe leggere come se lo fosse. */}
      <div className="chi" role="gridcell">
        <div className="nome" title={brano.title}>
          {brano.title}
          {/* La pastiglia accanto al titolo e non in una colonna sua: dice una
              cosa su **questo** brano, e su una libreria di file sarebbe una
              colonna vuota per diciottomila righe. Chi ascolta con uno screen
              reader la sente nel titolo, che è dove serve — è la differenza fra
              un brano che c'è sul disco e uno che c'è finché c'è la rete. */}
          {brano.fonte !== null && (
            <span
              className="pastiglia flusso"
              data-livello="nota"
              title={t("track.stream.title", { fonte: nomeFonte(brano.fonte) })}
            >
              {t("track.stream.badge")}
            </span>
          )}
        </div>
        <div className="autore" title={nomeArtista(brano.artist)}>
          {nomeArtista(brano.artist)}
        </div>
      </div>
      <div className="disco" role="gridcell" title={titoloAlbum(brano.album)}>
        {titoloAlbum(brano.album)}
      </div>
      <div role="gridcell" className="cella-comando">
        <Stelle
          valore={brano.rating}
          onVoto={(stelle) => onVoto(brano, stelle)}
          /* Cinque stelle × 18.534 righe erano 92.670 fermate di tabulazione. */
          raggiungibile={false}
        />
      </div>
      <div className="durata" role="gridcell">
        {durata(brano.durationMs)}
      </div>
      <div role="gridcell" className="cella-comando">
        <button
          type="button"
          className="cuore icon-btn"
          tabIndex={-1}
          aria-pressed={brano.liked}
          aria-label={brano.liked ? t("track.unlike") : t("track.like")}
          onClick={() => onPreferito(brano)}
        >
          <Icona nome={brano.liked ? "i-heart-f" : "i-heart"} dim={15} />
        </button>
      </div>
      {onTogli && (
        <div role="gridcell" className="cella-comando">
          <button
            type="button"
            className="tasto icon-btn"
            tabIndex={-1}
            aria-label={t("list.removeFromPlaylist", { titolo: brano.title })}
            onClick={() => onTogli(indice)}
          >
            <Icona nome="i-x" dim={14} />
          </button>
        </div>
      )}
    </div>
  );
});

/**
 * L'intestazione di colonna dell'elenco: la stessa griglia delle righe.
 *
 * # Perché non è più `aria-hidden`
 *
 * Perché nascondeva i nomi delle colonne a chi ha più bisogno di sentirli.
 * Guardando l'elenco si vede che il numero a destra è una durata; ascoltandolo
 * si sente «3:41» e basta. Era nascosta perché senza un ruolo di griglia era
 * rumore — sei parole sciolte prima di un elenco di `div` — ma adesso le righe
 * sono `row` e queste sono le loro `columnheader`: l'intestazione è la prima
 * riga della griglia, e un lettore di schermo la usa per dire «Durata, 3:41»
 * invece di «3:41».
 *
 * I due `<span />` vuoti sono celle anche loro: una `row` può contenere solo
 * celle, e un figlio senza ruolo in mezzo a quelli che l'hanno romperebbe il
 * conteggio delle colonne.
 */
function TestaElenco({
  conTogli,
  muta,
}: {
  conTogli?: boolean | undefined;
  /**
   * Senza ruoli e fuori dall'albero di accessibilità.
   *
   * Serve al solo `ElencoFinto`: là la griglia non esiste ancora — è un
   * `role="status"` che dice «sto caricando» — e annunciare i nomi delle colonne
   * di un elenco che non c'è sarebbe peggio che tacere.
   */
  muta?: boolean | undefined;
}) {
  // Un ruolo calcolato e non due alberi uguali: la griglia di colonne è una cosa
  // sola, e copiarla per cambiarle gli attributi sarebbe tenerne allineate due.
  const cella = muta ? undefined : ("columnheader" as const);
  return (
    <div
      className="testa-elenco"
      aria-hidden={muta || undefined}
      role={muta ? undefined : "row"}
      aria-rowindex={muta ? undefined : 1}
    >
      <span className="indice" role={cella}>
        #
      </span>
      <span role={cella} />
      <span role={cella}>{t("list.title")}</span>
      {/* Le due colonne che si ritirano quando il contenuto si stringe portano un
          nome: nasconderle per posizione — `:nth-child(4)` — vorrebbe dire tenere
          allineati un numero qui e un numero nel foglio. */}
      <span className="disco" role={cella}>
        {t("list.album")}
      </span>
      <span className="voto" role={cella}>
        {t("list.rating")}
      </span>
      <span className="durata" role={cella}>
        {t("list.duration")}
      </span>
      <span role={cella} />
      {conTogli && <span role={cella} />}
    </div>
  );
}

/**
 * I segnaposto, mentre la vista chiede il suo contenuto.
 *
 * # Perché hanno la forma di quel che arriverà
 *
 * Perché il vuoto sia dichiarato invece che mascherato — prima restava in piedi
 * il contenuto della vista precedente, cioè le copertine di Album che fingevano
 * di essere Preferiti per mezzo secondo — e perché lo spazio sia già preso: se i
 * segnaposto avessero una misura diversa dalle righe vere, l'arrivo dei dati
 * farebbe saltare la pagina, che è il difetto che i segnaposto esistono per
 * evitare.
 *
 * `skeleton` è una parte del registro (`core/aether-skin/src/parts.rs`), e la
 * sua descrizione dice che l'animazione fa parte dell'identità della skin.
 * Fino a qui era emessa **solo** nell'anteprima dello Studio: l'applicazione
 * mostrava una parte che non usava.
 *
 * `role="status"` sul contenitore e `aria-hidden` sui riquadri: chi ascolta lo
 * schermo sente «sto caricando» una volta, non dodici rettangoli.
 */
function GrigliaFinta() {
  return (
    <div
      className="griglia track-grid"
      role="status"
      aria-label={t("list.loading")}
    >
      {Array.from({ length: 12 }, (_, i) => (
        <div className="scheda finta" key={i} aria-hidden="true">
          <div className="copertina skeleton" />
          <div className="riga-finta skeleton" />
          <div className="riga-finta corta skeleton" />
        </div>
      ))}
    </div>
  );
}

function ElencoFinto() {
  return (
    <div
      className="elenco track-grid"
      role="status"
      aria-label={t("list.loading")}
    >
      <TestaElenco muta />
      {Array.from({ length: 10 }, (_, i) => (
        <div className="riga finta" key={i} aria-hidden="true">
          <span />
          <div className="miniatura skeleton" />
          <div className="chi">
            <div className="riga-finta skeleton" />
            <div className="riga-finta corta skeleton" />
          </div>
          <div className="riga-finta skeleton disco" />
          <span />
          <div className="riga-finta skeleton" />
          <span />
        </div>
      ))}
    </div>
  );
}

/**
 * Il fondo di un elenco impaginato: quando si avvicina, arriva la pagina dopo.
 *
 * Non è invisibile, ed è una scelta. Un osservatore che chiede in silenzio
 * lascia chi scorre davanti a un elenco che si allunga da solo senza dire
 * perché — e nel mezzo secondo in cui la pagina è in volo, davanti a una fine
 * che non è la fine. Una barra che scintilla dice tutte e due le cose.
 *
 * `role="status"` sul contenitore e `aria-hidden` sulla barra: chi ascolta lo
 * schermo sente «altri in arrivo» una volta, non un rettangolo.
 */
function Sentinella({
  pagine,
}: {
  pagine: {
    altre: boolean;
    sentinella: (n: HTMLDivElement | null) => (() => void) | undefined;
  };
}) {
  if (!pagine.altre) return null;
  return (
    <div
      className="sentinella"
      ref={pagine.sentinella}
      role="status"
      aria-label={t("list.more")}
    >
      <span className="skeleton" aria-hidden="true" />
    </div>
  );
}

/**
 * L'elenco dei brani: una griglia vera, e una finestra sulle sue righe.
 *
 * # Perché esiste un componente invece di quattro `map`
 *
 * Perché i punti che montano un elenco di brani sono **quattro** — la ricerca,
 * la playlist, l'album aperto, la libreria — e finora ognuno ripeteva a mano lo
 * stesso `div.elenco` con dentro la stessa intestazione e lo stesso `map`. Una
 * finestra virtuale scritta quattro volte sarebbe quattro finestre da tenere
 * d'accordo, e la prima a scollarsi lo farebbe in silenzio: si vede solo
 * scorrendo fino in fondo *quella* vista.
 *
 * # Perché una finestra
 *
 * Il conto sta nel `//!` di `virtuale.ts`: 18.534 righe × una dozzina di nodi
 * ciascuna sono oltre duecentomila nodi DOM, ed è la voce più grossa dei 521 MB
 * che l'albero di processi occupava. Qui se ne disegnano una trentina, e sopra e
 * sotto stanno due `.paglia` alte quanto le righe che non ci sono.
 *
 * # Perché la `Sentinella` resta fuori
 *
 * Perché la `.paglia` inferiore conserva **l'altezza vera** delle righe non
 * disegnate: il fondo del documento è dove era prima della finestra, quindi
 * l'`IntersectionObserver` di `pagine.ts:143-160`, col suo `rootMargin: 600px`,
 * entra in vista allo stesso pixel di prima e chiede la pagina dopo nello stesso
 * momento. Portarla dentro l'elenco non servirebbe a niente e la esporrebbe alla
 * finestra; lasciarla fuori è ciò che rende la virtualizzazione invisibile alla
 * paginazione, che è il patto: `pagine.ts` non si tocca.
 *
 * # Perché `grid` e non `listbox`
 *
 * Due ragioni, e nessuna delle due è di gusto.
 *
 * La prima: una `option` **non deve contenere comandi focalizzabili**, e qui ce
 * ne sono sette per riga — l'indice, le cinque stelle, il cuore, più la × dentro
 * una playlist. Dichiarare `listbox` vorrebbe dire dichiarare il falso su ogni
 * riga, e i lettori di schermo che credono alla dichiarazione nascondono quel
 * che sta dentro l'opzione: i comandi sparirebbero invece di essere annunciati.
 *
 * La seconda: solo `aria-rowindex`/`aria-rowcount` rendono **dicibile una
 * finestra**. Con trenta righe nel DOM su diciottomila, una `listbox` direbbe
 * «elemento 7 di 30» — cioè una bugia — mentre una `grid` dice «riga 12.004 di
 * 18.535», che è la verità e per giunta l'unica informazione con cui ci si
 * orienta in una libreria.
 *
 * `aria-rowcount` conta le righe **arrivate**, non quelle che esistono: la
 * libreria è impaginata e nemmeno `usePagine` sa quante saranno. Un numero che
 * cresce è più onesto del `-1` che l'attributo prevede per «non si sa», che
 * cancellerebbe l'informazione proprio dove serve.
 */
function ElencoBrani({
  righe,
  scorrevole,
  inAscolto,
  suonabile,
  selezione,
  onSuona,
  onPreferito,
  onVoto,
  onMenu,
  onSeleziona,
  onAlternaSelezione,
  numeroTraccia,
  conTogli,
  chiaveConIndice,
  onTogli,
  onRiordina,
  onPresa,
  onMira,
  onLascia,
  mirata,
  trascinata,
}: {
  righe: Brano[];
  /** Lo scorrevole dentro cui l'elenco vive: `.dentro`. */
  scorrevole: React.RefObject<HTMLDivElement | null>;
  /** Quale brano sta suonando, per la riga in evidenza. */
  inAscolto: number | null;
  suonabile: boolean;
  selezione: Set<number>;
  onSuona: (indice: number) => void;
  onPreferito: (b: Brano) => void;
  onVoto: (b: Brano, stelle: number) => void;
  onMenu: (e: React.MouseEvent, brani: number[]) => void;
  onSeleziona: (e: React.MouseEvent, indice: number) => void;
  /** Spazio sulla riga col fuoco: il Ctrl+clic detto da tastiera. */
  onAlternaSelezione: (indice: number) => void;
  numeroTraccia?: boolean | undefined;
  /** La colonna in più della playlist a mano: il tasto che toglie la riga. */
  conTogli?: boolean | undefined;
  /**
   * La chiave di React porta anche la posizione.
   *
   * Serve **solo** dentro una playlist, dove lo stesso brano può comparire due
   * volte: l'identificativo da solo non sarebbe unico, e React accoppierebbe le
   * due righe.
   */
  chiaveConIndice?: boolean | undefined;
  onTogli?: ((indice: number) => void) | undefined;
  onRiordina?: ((da: number, a: number) => void) | undefined;
  onPresa?: ((indice: number | null) => void) | undefined;
  onMira?: ((indice: number | null) => void) | undefined;
  onLascia?: ((indice: number) => void) | undefined;
  /** Dove cadrebbe il rilascio, durante un trascinamento. */
  mirata?: number | null | undefined;
  /** Quale riga si sta trascinando. */
  trascinata?: number | null | undefined;
}) {
  const elenco = useRef<HTMLDivElement>(null);
  const finestra = useVirtuale({
    totale: righe.length,
    contenitore: scorrevole,
    ancora: elenco,
  });
  // Stabile per tutta la vita dell'elenco: le azioni si leggono al gesto.
  const presaPuntatore = usePresaPerRiordino({ onPresa, onMira, onLascia });

  /*
   * Invio suona, ma solo se c'è da suonare: è la stessa condizione che spegne il
   * tasto dell'indice, detta per l'altra strada. Senza, il tasto spento e il
   * tasto Invio farebbero due cose diverse sulla stessa riga.
   */
  const suona = useCallback(
    (indice: number) => {
      if (suonabile) onSuona(indice);
    },
    [suonabile, onSuona],
  );

  const fuoco = useFuoco({
    totale: righe.length,
    ancora: elenco,
    finestra,
    scorriA: finestra.scorriA,
    onInvio: suona,
    onSpazio: onAlternaSelezione,
  });

  return (
    <div
      className="elenco track-grid"
      ref={elenco}
      role="grid"
      aria-multiselectable="true"
      aria-label={t("list.grid")}
      aria-rowcount={righe.length + 1}
      /* Un gestore solo per tutte le righe: appenderne uno a ciascuna
         darebbe a `RigaBrano` una prop con identità nuova a ogni disegno e
         annullerebbe il suo `memo`, che è l'altro conto che questa release
         paga. La riga da cui viene il tasto la dice il bersaglio dell'evento. */
      onKeyDown={fuoco.daTastiera}
      /* L'attributo, non solo la prop: la colonna in più la deve conoscere
         anche la griglia del foglio, altrimenti l'ottavo figlio della riga
         finisce a capo invece che in fondo. */
      data-con-togli={conTogli || undefined}
    >
      <TestaElenco conTogli={conTogli} />
      {/* Le righe che stanno sopra la finestra, come altezza e basta.
          `role="presentation"` perché non sono una riga: sono il posto che le
          righe assenti occuperebbero, e una `grid` con dentro due `div`
          sconosciuti direbbe di avere due righe in più di quante ne ha. */}
      <div
        className="paglia"
        style={{ height: finestra.sopra * finestra.altezza }}
        role="presentation"
      />
      {righe.slice(finestra.primo, finestra.ultimo).map((b, k) => {
        const i = finestra.primo + k;
        return (
          <RigaBrano
            key={chiaveConIndice ? `${i}-${b.id}` : b.id}
            brano={b}
            indice={i}
            attivo={b.id === inAscolto}
            suonabile={suonabile}
            colFuoco={i === fuoco.attivo}
            onSpostaFuoco={fuoco.vaiA}
            onFuocoPreso={fuoco.segna}
            onSuona={onSuona}
            onPreferito={onPreferito}
            onVoto={onVoto}
            onMenu={onMenu}
            selezionato={selezione.has(b.id)}
            onSeleziona={onSeleziona}
            numeroTraccia={numeroTraccia}
            onTogli={onTogli}
            onRiordina={onRiordina}
            onPresaPuntatore={presaPuntatore}
            sopra={mirata === i && trascinata !== i}
          />
        );
      })}
      <div
        className="paglia"
        style={{ height: finestra.sotto * finestra.altezza }}
        role="presentation"
      />
    </div>
  );
}

export function App() {
  const [avvio, setAvvio] = useState<Avvio | null>(null);
  const [errore, setErrore] = useState<Guasto | null>(null);
  /**
   * Una notizia riuscita, non un guasto.
   *
   * Un canale suo e non quello degli errori: ci ho messo dentro «5 brani
   * scritti in …» e a schermo è comparso in rosso, con il triangolo d'avviso
   * accanto — un'esportazione andata bene che sembra fallita. Il colore di una
   * fascia è metà del suo messaggio, e riusare il canale sbagliato è il modo
   * più economico di dire la cosa opposta.
   */
  const [notizia, setNotizia] = useState<string | null>(null);
  /*
   * E se ne va da sola. Restava a schermo finché qualcuno non premeva la X —
   * cioè, per chi non la premeva, per sempre, in cima a ogni pagina, a dire
   * «5 brani scritti» di un'esportazione di un'ora prima. Otto secondi: una
   * notizia riuscita si legge e basta, e l'errore invece resta, perché quello
   * chiede di fare qualcosa.
   */
  useEffect(() => {
    if (notizia === null) return;
    const conto = window.setTimeout(() => setNotizia(null), 8_000);
    return () => window.clearTimeout(conto);
  }, [notizia]);
  const [vista, setVista] = useState<Vista>("home");
  /**
   * Quel che Esplora ha trovato l'ultima volta che era aperta.
   *
   * Sta qui e non dentro il componente perché il componente si smonta a ogni
   * cambio di schermata, e con lui sparivano risultati e frase cercata:
   * tornare su Esplora dopo aver guardato un album voleva dire rifare una
   * ricerca che costa da una a cinque richieste all'Internet Archive e una ad
   * Audius. Il nucleo i candidati non li aveva mai buttati — è la finestra
   * che dimenticava. Vedi `StatoEsplora` in `schermate/Esplora.tsx`.
   */
  const [esplora, setEsplora] = useState<StatoEsplora>(ESPLORA_INIZIALE);
  const [sezione, setSezione] = useState<Sezione>("cartelle");
  const [query, setQuery] = useState("");
  const [artisti, setArtisti] = useState<Artista[]>([]);
  // La Home arriva in una chiamata sola. `null` è «non ancora chiesta», che è
  // diverso da «vuota»: la schermata mostra i suoi segnaposto finché il nucleo
  // non ha risposto, invece dello stato vuoto per un fotogramma.
  const [casa, setCasa] = useState<Casa | null>(null);
  // Le raccolte del lunedì. Vuoto e non `null`: qui «non ancora chieste» e
  // «questa settimana non ce ne sono» si disegnano uguale — niente ripiano — e
  // distinguerle costerebbe uno stato in più per non mostrare niente in due modi.
  const [settimana, setSettimana] = useState<Raccolta[]>([]);
  /**
   * Di quale brano corrente parla la `casa` che si ha in mano.
   *
   * «Riprendi dov'eri» fa partire la coda del nucleo, quindi deve nominare il
   * brano che quella coda ha davvero in mano. Questo riferimento è il confronto
   * che dice se i due sono ancora d'accordo, e sta fuori dallo stato perché
   * serve a **decidere** una richiesta, non a disegnare qualcosa.
   */
  const casaPerBrano = useRef<number | null>(null);
  const [aperto, setAperto] = useState<Album | null>(null);
  const [artistaAperto, setArtistaAperto] = useState<Artista | null>(null);
  const [braniAperto, setBraniAperto] = useState<Brano[]>([]);
  const [ordine, setOrdine] = useState<Ordine>("scaffale");
  const [scansione, setScansione] = useState<Avanzamento | null>(null);
  /**
   * Il primo avvio è stato chiuso a mano.
   *
   * Non va nel database: la condizione vera è «non c'è nessuna cartella
   * sorvegliata», e quella il database ce l'ha già. Questo serve solo a chi ha
   * premuto «Lo faccio dopo» e non vuole rivedere la schermata finché la
   * finestra resta aperta — riaprendo il programma senza aver scelto niente, la
   * proposta è ancora la risposta giusta.
   */
  const [primoChiuso, setPrimoChiuso] = useState(false);
  /**
   * Il primo avvio si è aperto, in questa finestra.
   *
   * Serve a tenerlo **aperto**. La condizione per mostrarlo era soltanto
   * «nessuna cartella sorvegliata», e `primoConferma` scrive la cartella e
   * ricarica prima di scansionare: `Primo` spariva nell'istante in cui la
   * cartella c'era, cioè proprio prima di mostrare l'avanzamento e il tasto
   * «Ascolta» che esiste per quel momento. La condizione decide se si apre;
   * da lì in poi lo chiude soltanto chi ascolta, con «Ascolta» o «Lo faccio
   * dopo».
   */
  const [primoAperto, setPrimoAperto] = useState(false);
  const nessunaCartella = avvio !== null && avvio.cartelle.length === 0;
  useEffect(() => {
    if (nessunaCartella) setPrimoAperto(true);
  }, [nessunaCartella]);
  const primoVisibile = !primoChiuso && (primoAperto || nessunaCartella);
  /** Le cartelle trascinate sulla finestra mentre il primo avvio le chiede. */
  const [lasciateAlPrimo, setLasciateAlPrimo] = useState<string[]>([]);
  /**
   * Il giro guidato di questa versione del copione non è ancora stato fatto.
   *
   * Non è «il giro è aperto»: è il permesso di aprirlo. Le due cose sono
   * separate perché la seconda ha tre condizioni che il database non conosce —
   * il primo avvio chiuso, la libreria non vuota, e nessun giro già in corso —
   * e tenerle in un booleano solo vorrebbe dire ricalcolarle nel punto in cui
   * si disegna.
   */
  const [giroDaFare, setGiroDaFare] = useState(false);
  /** Il giro guidato è aperto adesso. */
  const [giroAperto, setGiroAperto] = useState(false);
  const [esito, setEsito] = useState<EsitoScansione | null>(null);
  const [colonnaAperta, setColonnaAperta] = useState(
    () => window.innerWidth >= LARGHEZZA_TRE_COLONNE,
  );
  const [codaAperta, setCodaAperta] = useState(false);
  const [daImportare, setDaImportare] = useState<string | null>(null);
  /**
   * La finestrella del link è aperta.
   *
   * Un booleano e non più il servizio da cui la si è aperta: c'era un
   * suggerimento — Spotify o YouTube — che decideva segnaposto e diagnosi di
   * là, e non c'è più niente da suggerire. I cataloghi si riconoscono dal link,
   * e chi incolla non deve sceglierne uno prima di sapere cosa ha negli
   * appunti.
   */
  const [importandoLink, setImportandoLink] = useState(false);
  const [importandoAccount, setImportandoAccount] = useState(false);
  const [playlist, setPlaylist] = useState<Playlist[]>([]);
  /** Il file di playlist da importare, o `null`. */
  const [filePlaylist, setFilePlaylist] = useState<string | null>(null);
  /**
   * L'editor delle regole: `null` chiuso, `{playlist: null}` per una nuova.
   *
   * Un oggetto e non due stati separati: «sto creando» e «sto modificando
   * questa» si escludono, e due booleani che si escludono sono due booleani che
   * prima o poi saranno veri insieme.
   */
  const [regoleAperte, setRegoleAperte] = useState<{
    playlist: Playlist | null;
  } | null>(null);
  const [playlistAperta, setPlaylistAperta] = useState<Playlist | null>(null);
  const [braniPlaylist, setBraniPlaylist] = useState<Brano[]>([]);
  /**
   * Di quale playlist sono i brani in `braniPlaylist`, quando sono arrivati.
   *
   * Serve a una cosa sola: distinguere «la playlist è vuota» da «i brani non
   * sono ancora arrivati». Senza, lo stato vuoto lampeggerebbe a ogni apertura.
   */
  const [playlistCaricata, setPlaylistCaricata] = useState<number | null>(null);
  const [daAggiungere, setDaAggiungere] = useState<number[] | null>(null);
  const [daRinominare, setDaRinominare] = useState<Playlist | null>(null);
  /**
   * I brani che il menù ha proposto di togliere, e in che modo.
   *
   * `dalDisco` sceglie fra i due comandi e fra i due testi della conferma: sono
   * due gesti diversi — «non lo voglio in elenco» e «non lo voglio più» — e la
   * differenza va letta prima di premere, non scoperta dopo.
   */
  const [daEliminare, setDaEliminare] = useState<{
    brani: number[];
    dalDisco: boolean;
  } | null>(null);
  const [creandoPlaylist, setCreandoPlaylist] = useState(false);
  /** La finestrella del ripristino è aperta. */
  const [ripristinando, setRipristinando] = useState(false);
  const [skin, setSkin] = useState<VoceSkin[]>([]);
  const [skinAttiva, setSkinAttiva] = useState<Skin | null>(null);
  /**
   * Il tema, che adesso arriva dal database.
   *
   * Si parte da «sistema» e non dal valore salvato perché quel valore non è
   * ancora arrivato: `avvio()` è una chiamata. Il fotogramma prima che
   * risponda segue il sistema operativo, che è il ripiego giusto — e la
   * finestra non si mostra finché skin e tema non sono sul documento (vedi
   * `pronto`), quindi nessuno lo vede.
   */
  const [tema, setTema] = useState<Tema>("sistema");
  /** Il tema è già stato deciso in questa sessione. Vedi l'effetto sotto. */
  const temaDeciso = useRef(false);
  /**
   * Quanto movimento vuole chi guarda, sotto quello che la skin dichiara.
   *
   * Si parte da «sistema» come per il tema, e per la stessa ragione: il valore
   * vero è nel database e arriva con una chiamata. Il fotogramma prima che
   * risponda è quello in cui vale la sola `prefers-reduced-motion`, che è il
   * ripiego giusto — chi ha una condizione dichiarata al sistema operativo non
   * vede movimento nemmeno lì.
   */
  const [movimentoUtente, setMovimentoUtente] =
    useState<MovimentoUtente>("sistema");
  /**
   * Di quanto è ingrandita l'interfaccia, o `null` finché non si sa.
   *
   * `null` e non `1`, al contrario del movimento, e la differenza qui non è
   * accademica: la finestra nasce nascosta e a mostrarla è `pronto`. Se lo
   * zoom non fosse fra le cose che si aspettano, chi ha scelto 1,5 vedrebbe il
   * primo fotogramma alla misura di serie e il secondo alla sua — lo stesso
   * difetto del fotogramma scuro e di quello in italiano, con le misure.
   *
   * Il valore non si applica di qua: lo applica `zoom_avvio` alla finestra,
   * perché è lo zoom della WebView e non una scala nel foglio. Qui serve solo
   * a disegnarlo nelle impostazioni.
   */
  const [zoom, setZoom] = useState<number | null>(null);
  /**
   * La lingua in uso.
   *
   * Non è uno stato di questo componente: vive nel modulo `lingue`, perché i
   * testi servono anche fuori da React — `formato.ts` e `ipc.ts` ne chiamano le
   * funzioni senza essere componenti. Qui ci si **iscrive** e basta, e
   * l'iscrizione è quel che ridisegna l'albero quando la lingua cambia.
   */
  const lingua = useLingua();
  /** La lingua è già stata decisa in questa sessione. Come il tema. */
  const linguaDecisa = useRef(false);
  /**
   * Le scorciatoie in uso.
   *
   * Derivate e non copiate in uno stato: la sorgente è `avvio.scorciatoie`, e
   * tenerne una seconda copia qui vorrebbe dire avere due verità che si
   * scostano appena una scrittura fallisce.
   */
  const scorciatoie = useMemo(
    () => leggiAssociazioni(avvio?.scorciatoie ?? null),
    [avvio?.scorciatoie],
  );
  const [menu, setMenu] = useState<Apertura | null>(null);
  const [grande, setGrande] = useState(false);
  /** La skin aperta nello Studio, o `null`. */
  const [studioAperto, setStudioAperto] = useState<string | null>(null);
  /** La finestrella che battezza un tema nuovo è aperta. */
  const [creandoTema, setCreandoTema] = useState(false);
  /** Da quale skin partire, quando si è arrivati da «Deriva…». */
  const [baseTema, setBaseTema] = useState<string | null>(null);
  /** Gli identificativi selezionati, e da dove partire per un intervallo. */
  const [selezione, setSelezione] = useState<Set<number>>(() => new Set());
  const [ancora, setAncora] = useState<number | null>(null);
  const contenuto = useRef<HTMLDivElement>(null);
  const riproduzione = useRiproduzione();
  /**
   * Le importazioni da Spotify e la coda che le scarica.
   *
   * Qui e non nella finestrella che le avvia: quella si chiude, la coda no.
   * Vedi `parti/Importazioni.tsx`.
   */
  const importazioni = useImportazioni();

  const segnalaErrore = useCallback((e: unknown) => setErrore(guastoDa(e)), []);

  const ricarica = useCallback(async () => {
    try {
      setAvvio(await ipc.avvio());
      setErrore(null);
    } catch (e) {
      segnalaErrore(e);
    }
  }, []);

  useEffect(() => {
    void ricarica();
  }, [ricarica]);

  // La skin, appena si può. Fino a quando non arriva valgono i token che
  // `stile.css` porta sotto `:root`, che sono gli stessi della skin di serie —
  // quindi non c'è un fotogramma del colore sbagliato, e se questa chiamata
  // fallisce l'applicazione resta usabile invece che illeggibile.
  useEffect(() => {
    ipc
      .skin()
      .then((s) => {
        applicaSkin(s);
        setSkinAttiva(s);
      })
      .catch((e: unknown) => segnalaErrore(e));
  }, []);

  // Il tema si riapplica quando cambia la scelta **o** quando cambia la skin:
  // una skin senza variante chiara costringe allo scuro, e passare da una con a
  // una senza deve togliere `data-theme` invece di lasciarlo lì a non fare
  // niente.
  /**
   * Il tema che si sta **davvero** mostrando.
   *
   * Non `tema === "chiaro"`: una skin senza variante chiara resta scura anche
   * con la preferenza sul chiaro, e chi calcola qualcosa contro le superfici
   * del tema in uso — l'accento che segue la copertina — deve sapere quale
   * delle due ha vinto.
   */
  const [chiaro, setChiaro] = useState(false);

  useEffect(() => {
    setChiaro(applicaTema(tema, skinAttiva?.light ?? false));
  }, [tema, skinAttiva]);

  /**
   * Il movimento chiesto da chi guarda, sul documento e nel database.
   *
   * Due effetti e non uno, come per il tema: qui si **applica** — cioè si
   * scrive l'attributo sulla radice a ogni cambio — e sotto si **legge** una
   * volta sola all'apertura. Tenerli insieme vorrebbe dire riscrivere il
   * documento anche quando non è cambiato niente.
   */
  useEffect(() => {
    applicaMovimento(movimentoUtente);
  }, [movimentoUtente]);

  /**
   * La preferenza, appena il database risponde.
   *
   * Non passa da `avvio`: è un comando suo, e la ragione sta scritta sopra
   * `movimento_ridotto` in `comandi.rs`. Una volta sola per apertura — questo
   * effetto non ha dipendenze — perché al contrario del tema non c'è nessuna
   * chiave vecchia da riconciliare: si legge, si applica, e da lì in poi
   * comanda il segmentato delle impostazioni.
   *
   * Un guasto qui non è un errore da mostrare: vorrebbe dire una finestra che
   * si apre con un avviso rosso per dire che il movimento è rimasto quello del
   * sistema, cioè per dire che non è successo niente.
   */
  useEffect(() => {
    ipc
      .movimentoRidotto()
      .then((ridotto) => setMovimentoUtente(ridotto ? "ridotto" : "sistema"))
      .catch(() => undefined);
  }, []);

  /**
   * Cambia quanto movimento si vuole, e lo scrive.
   *
   * Lo stato locale si muove prima della scrittura, come per il tema: è un
   * cambio che si vede: le transizioni si fermano, ed è quella la risposta al
   * gesto.
   */
  /**
   * Lo zoom, appena il database risponde — e applicato dall'altra parte.
   *
   * `zoomAvvio` legge **e** applica in un giro solo: lo zoom della WebView non
   * sopravvive alla chiusura, e il numero che torna di qua serve a disegnarlo
   * nelle impostazioni e a togliere l'attesa a `pronto`.
   *
   * Il `catch` scrive comunque il neutro invece di lasciare `null`, e non è
   * una formalità: senza, un database che non risponde lascerebbe la finestra
   * invisibile fino alla rete di sicurezza dei due secondi di `main.rs`, cioè
   * trasformerebbe una preferenza illeggibile in un'applicazione che non si
   * apre.
   */
  useEffect(() => {
    ipc
      .zoomAvvio()
      .then(setZoom)
      .catch(() => setZoom(1));
  }, []);

  /**
   * Un gradino in su o in giù.
   *
   * Al contrario del tema e del movimento, lo stato locale si muove **dopo**
   * la risposta e non prima: qui il valore che vale non lo decide questo
   * componente ma il nucleo, che ai due estremi della scala restituisce quello
   * di prima. Muoverlo prima vorrebbe dire mostrare per un fotogramma un
   * gradino che non esiste, ogni volta che si preme il tasto contro il fondo.
   * E non c'è niente da anticipare: quel che risponde al gesto è
   * l'ingrandimento, e a farlo è la finestra.
   */
  const cambiaZoom = useCallback(
    (su: boolean) => {
      ipc.zoomPasso(su).then(setZoom).catch(segnalaErrore);
    },
    [segnalaErrore],
  );

  /**
   * La misura di serie: `Ctrl+0`, e il bottone delle impostazioni.
   *
   * Nessun argomento, e non è una scorciatoia di scrittura: attraverso l'IPC
   * dello zoom non passa **nessun numero**. Da questa parte si sanno i tre
   * gesti, e la scala — quali gradini esistono e in che ordine — sta soltanto
   * in `preferenze::SCALA_ZOOM`. Un secondo elenco di numeri qui sarebbe la
   * cosa che si scosta senza dirlo.
   */
  const zoomNormale = useCallback(() => {
    ipc.zoomNormale().then(setZoom).catch(segnalaErrore);
  }, [segnalaErrore]);

  const cambiaMovimentoUtente = useCallback(
    (scelto: MovimentoUtente) => {
      setMovimentoUtente(scelto);
      ipc
        .impostaMovimentoRidotto(scelto === "ridotto")
        .catch(segnalaErrore);
    },
    [segnalaErrore],
  );

  /**
   * Da dove viene il tema: dal database, o dalla chiave vecchia.
   *
   * Una volta sola per apertura — il `ref` — perché `avvio` si rilegge dopo
   * ogni scansione e ogni cambio di cartella: senza la guardia, ogni ricarica
   * riscriverebbe il tema di chi non ne ha ancora scelto uno, e soprattutto
   * riporterebbe indietro quello appena scelto se la ricarica arrivasse fra la
   * scelta e la scrittura.
   */
  useEffect(() => {
    if (avvio === null || temaDeciso.current) return;
    temaDeciso.current = true;
    const salvato = avvio.tema;
    if (salvato === "scuro" || salvato === "chiaro" || salvato === "sistema") {
      setTema(salvato);
      // La chiave vecchia ha già fatto il suo mestiere: se restasse, un giorno
      // in cui il database non risponde tornerebbe a vincere lei.
      dimenticaRipiego();
      return;
    }
    const ripiego = temaDiRipiego();
    if (ripiego === null) return;
    setTema(ripiego);
    ipc.impostaTema(ripiego).then(dimenticaRipiego).catch(segnalaErrore);
  }, [avvio, segnalaErrore]);

  /**
   * Cambia il tema, e lo scrive.
   *
   * Qui lo stato locale si muove **prima** della scrittura, al contrario di
   * `cambiaAccentoDinamico`: il tema è un cambio che si vede: la finestra si
   * ridipinge, ed è quella la risposta al gesto. Aspettare il database
   * vorrebbe dire un ritardo visibile su ogni pressione per proteggersi da un
   * guasto che, se capita, si vede lo stesso — l'errore compare, e la prossima
   * apertura mostra la scelta di prima.
   */
  const cambiaTema = useCallback(
    (scelto: Tema) => {
      setTema(scelto);
      ipc.impostaTema(scelto).catch(segnalaErrore);
    },
    [segnalaErrore],
  );

  /**
   * Quale lingua parlare, appena il database risponde.
   *
   * Una volta sola per apertura — il `ref` — per la stessa ragione del tema:
   * `avvio` si rilegge dopo ogni scansione, e senza la guardia una ricarica
   * arrivata subito dopo un cambio di lingua rimetterebbe quella di prima.
   *
   * `scegli` fa il resto: la lingua salvata se il suo file c'è ancora, altrimenti
   * quella del sistema, altrimenti l'inglese. Il caso «c'era e non c'è più» non è
   * teorico — basta un profilo importato da un'installazione con più lingue — e
   * il ripiego lì è meglio di un'interfaccia di sole chiavi.
   */
  useEffect(() => {
    if (avvio === null || linguaDecisa.current) return;
    linguaDecisa.current = true;
    applicaLingua(scegli(avvio.lingua, navigator.language));
  }, [avvio]);

  /**
   * Le due voci del menù dell'icona nell'area di notifica.
   *
   * Scendono da qui perché i testi che si leggono stanno in `lingue/`, dove
   * `strumenti/lingue.js` controlla che ogni lingua le abbia tutte. Scritte in
   * Rust sarebbero le uniche due fuori da quel controllo, e il sintomo — un
   * menù metà in una lingua e metà nell'altra — somiglia troppo a una svista di
   * traduzione perché qualcuno lo segnali.
   *
   * Sull'iscrizione alla lingua e non su `avvio`: parte a ogni cambio, che è
   * esattamente quando il menù andrebbe altrimenti alla deriva. È anche il
   * momento in cui l'icona compare la prima volta — senza etichette non c'è un
   * menù da costruire, e `vassoio.rs` aspetta questa chiamata.
   */
  useEffect(() => {
    ipc.vassoioLingua(t("tray.show"), t("tray.quit")).catch(() => {
      /* Un menù che non si rietichetta non è una cosa da annunciare: chi non ha
         acceso il secondo piano non ha nemmeno un'icona, e chi l'ha acceso la
         ritrova nella lingua di prima. Segnalarlo sarebbe un errore rosso per
         una cosa che nessuno stava guardando. */
    });
  }, [lingua]);

  /**
   * Cambia la lingua, e la scrive.
   *
   * Come il tema: il documento si muove **prima** della scrittura, perché il
   * cambio si vede ed è quella la risposta al gesto. Se il database non prende
   * la modifica, l'errore compare e la prossima apertura mostra la lingua di
   * prima — cosa che si nota subito, al contrario di un'attesa su ogni click.
   */
  const cambiaLingua = useCallback(
    (scelta: string) => {
      applicaLingua(scelta);
      ipc.impostaLingua(scelta).catch(segnalaErrore);
    },
    [segnalaErrore],
  );

  /**
   * Riassegna le scorciatoie.
   *
   * Si scrive e si rilegge: `scorciatoie` è derivata da `avvio`, quindi
   * l'elenco a schermo si muove quando il database ha davvero preso la
   * modifica. Una scorciatoia che compare nella scheda e non funziona sarebbe
   * la peggiore delle due bugie possibili qui.
   */
  const cambiaScorciatoie = useCallback(
    (nuove: Associazioni) => {
      ipc
        .impostaScorciatoie(scriviAssociazioni(nuove))
        .then(ricarica)
        .catch(segnalaErrore);
    },
    [ricarica, segnalaErrore],
  );

  useEffect(
    () =>
      seguiIlSistema(
        () => ({ tema, chiara: skinAttiva?.light ?? false }),
        setChiaro,
      ),
    [tema, skinAttiva],
  );

  /**
   * L'accento segue la copertina del disco che sta suonando.
   *
   * Tre righe qui e nessuna decisione: il colore lo estrae `aether-app` dalla
   * miniatura, e se sia leggibile lo decide `aether-skin` in OKLCH, dove vive
   * `contrast_ratio`. La finestra scrive quel che le viene detto — è la stessa
   * regola per cui il foglio della skin arriva già compilato.
   *
   * `null` è la risposta normale, non un guasto: un disco senza copertina, una
   * copertina in bianco e nero, una skin che dichiara di non volerlo, o una
   * tonalità che a nessuna chiarezza regge 4,5:1 contro le superfici. In tutti
   * e quattro i casi vince l'accento che la skin ha scritto.
   */
  const [accentoDinamico, setAccentoDinamico] = useState(false);
  useEffect(() => {
    ipc.accentoDinamico().then(setAccentoDinamico).catch(segnalaErrore);
  }, [segnalaErrore]);

  /**
   * Accende o spegne la preferenza.
   *
   * Lo stato locale si muove **dopo** la scrittura e con quel che il nucleo
   * riporta, non prima: un interruttore che scatta e poi torna indietro perché
   * il database non ha risposto è peggio di uno che scatta un attimo dopo. Il
   * ricalcolo dell'accento lo fa l'effetto qui sotto, che ha `accentoDinamico`
   * fra le dipendenze — spegnerlo toglie le variabili, accenderlo le rimette.
   */
  const cambiaAccentoDinamico = useCallback(
    async (attivo: boolean) => {
      try {
        setAccentoDinamico(await ipc.accentoDinamicoAttiva(attivo));
      } catch (e) {
        segnalaErrore(e);
      }
    },
    [segnalaErrore],
  );

  /**
   * Un profilo è appena stato applicato: si rilegge tutto quel che ha toccato.
   *
   * `temaDeciso` torna falso apposta — è l'unico caso in cui il tema salvato
   * cambia sotto i piedi della finestra, e la guardia che impedisce di
   * rileggerlo a ogni ricarica qui va tolta di mano. Quel che il motore audio
   * legge alla sua apertura (volume, equalizzatore, normalizzazione) resta di
   * prima fino al riavvio, e la scheda lo dice invece di lasciarlo scoprire.
   */
  const dopoProfilo = useCallback(() => {
    temaDeciso.current = false;
    void ricarica();
    ipc
      .skin()
      .then((s) => {
        applicaSkin(s);
        setSkinAttiva(s);
      })
      .catch(segnalaErrore);
    ipc.accentoDinamico().then(setAccentoDinamico).catch(segnalaErrore);
  }, [ricarica, segnalaErrore]);

  /**
   * Quante volte un'anteprima ha rimesso a schermo la skin scelta.
   *
   * `applicaSkin` toglie l'accento della copertina, e deve farlo: uno ritagliato
   * sul contrasto di un'altra skin non vale niente. Ma provare una skin col
   * mouse non cambia né il brano, né il tema, né la skin scelta — cioè nessuna
   * delle dipendenze dell'effetto qui sotto — e senza questo contatore il
   * passaggio del puntatore su una scheda lascerebbe l'accento della skin fino
   * al brano dopo.
   *
   * Lo rimette **solo la revoca**, non l'anteprima: mentre si guarda un'altra
   * skin il colore giusto non si può nemmeno calcolare, perché il nucleo taglia
   * l'accento sulle superfici della skin *scelta* e a schermo c'è quella
   * provata. Un'anteprima senza accento dinamico è una risposta onesta; una con
   * l'accento sbagliato no.
   */
  const [accentoDaRimettere, setAccentoDaRimettere] = useState(0);

  const copertinaSuonata = riproduzione.stato.brano?.coverArtHash ?? null;
  useEffect(() => {
    // Il brano può cambiare mentre la risposta è in volo — succede saltando
    // avanti in fretta — e la risposta di prima dipingerebbe la finestra col
    // colore del disco sbagliato.
    let attuale = true;
    ipc
      .accentoCopertina(copertinaSuonata, chiaro)
      .then((variabili) => {
        if (attuale) applicaAccento(variabili);
      })
      .catch(segnalaErrore);
    return () => {
      attuale = false;
    };
  }, [
    copertinaSuonata,
    chiaro,
    skinAttiva,
    accentoDinamico,
    accentoDaRimettere,
    segnalaErrore,
  ]);

  /**
   * Mostra la finestra, una volta sola, quando c'è qualcosa di giusto da
   * vedere.
   *
   * La finestra nasce nascosta. Fino a qui `backgroundColor` di
   * `tauri.conf.json` era il fondo di `plain`, dipinto dal sistema operativo
   * **prima** che esistesse una pagina: chi sceglieva una skin chiara vedeva un
   * fotogramma scuro a ogni avvio, ed era il primo dei difetti noti.
   *
   * Due fotogrammi e non uno: il primo lascia che skin e tema arrivino al
   * documento, il secondo è quello in cui il motore li ha davvero disegnati.
   * Mostrarla nel mezzo rimetterebbe il difetto un fotogramma più in là.
   *
   * Si aspetta anche `avvio`, e non solo la skin, da quando l'interfaccia ha una
   * lingua: la lingua salvata arriva di lì, e mostrare la finestra prima
   * vorrebbe dire far leggere a chi ha scelto l'inglese un fotogramma di
   * italiano — lo stesso difetto del colore, con le parole.
   *
   * E si aspetta anche lo zoom, per la terza volta la stessa ragione: chi ha
   * scelto 1,5 non deve vedere un fotogramma alla misura di serie. Lo zoom
   * arriva sempre, riuscito o no — vedi il suo effetto — quindi questa
   * attesa non può diventare una finestra che non si apre.
   */
  const mostrata = useRef(false);
  useEffect(() => {
    if (
      mostrata.current ||
      skinAttiva === null ||
      avvio === null ||
      zoom === null
    )
      return;
    mostrata.current = true;
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        ipc.pronto().catch(segnalaErrore);
      }),
    );
  }, [skinAttiva, avvio, zoom, segnalaErrore]);

  // La colonna si chiude da sé quando la finestra si stringe, e si riapre da
  // sé quando torna larga **solo se a chiuderla è stata la finestra**. Prima
  // non si riapriva mai, con la ragione che riaprirla avrebbe annullato una
  // chiusura decisa a mano — ma così annullava anche l'apertura decisa a mano:
  // chi rimpiccioliva la finestra un momento per affiancarla a un'altra
  // ritrovava la colonna chiusa per sempre. La chiusura a mano ora si ricorda a
  // parte, e quella resta.
  const colonnaChiusaDallaFinestra = useRef(false);
  // Letta dal gestore del `resize`, che si registra una volta sola: senza il
  // riferimento vedrebbe per sempre il valore del primo disegno.
  const colonnaApertaAdesso = useRef(colonnaAperta);
  colonnaApertaAdesso.current = colonnaAperta;
  useEffect(() => {
    const guarda = () => {
      const larga = window.innerWidth >= LARGHEZZA_TRE_COLONNE;
      const aperta = colonnaApertaAdesso.current;
      if (!larga && aperta) {
        colonnaChiusaDallaFinestra.current = true;
        setColonnaAperta(false);
      } else if (larga && !aperta && colonnaChiusaDallaFinestra.current) {
        colonnaChiusaDallaFinestra.current = false;
        setColonnaAperta(true);
      }
    };
    window.addEventListener("resize", guarda);
    return () => window.removeEventListener("resize", guarda);
  }, []);
  /** Il gesto di chi apre o chiude la colonna a mano: vince sulla finestra. */
  const colonnaAMano = useCallback((aperta: boolean) => {
    colonnaChiusaDallaFinestra.current = false;
    setColonnaAperta(aperta);
  }, []);

  const ricaricaSkin = useCallback(async () => {
    try {
      setSkin(await ipc.skinElenco());
    } catch (e) {
      segnalaErrore(e);
    }
  }, []);

  useEffect(() => {
    void ricaricaSkin();
  }, [ricaricaSkin]);

  /**
   * Quale richiesta di foglio è ancora quella buona.
   *
   * Scegliere una skin e provarne una sono **due chiamate asincrone senza un
   * ordine fra loro**, e finiscono nello stesso posto: il testo di un unico
   * `<style>`. Quando il puntatore lascia una scheda parte
   * `anteprima(null)`, che rimette la skin attiva *di quel momento*; se quella
   * risposta arriva dopo la scelta appena fatta, riscrive il foglio con la skin
   * di prima. La finestra resta del colore vecchio mentre l'elenco dice già
   * «in uso» sulla nuova — e siccome dipende da chi risponde prima, capita una
   * volta sì e una no.
   *
   * Un contatore, e vince l'ultima partita. Non basta ignorare le anteprime
   * durante una scelta: un'anteprima cominciata **dopo** deve poter vincere,
   * altrimenti passare col mouse sulle schede subito dopo aver scelto non
   * mostrerebbe più niente.
   */
  const giroSkin = useRef(0);

  // `useCallback` e non una funzione qualunque: la registrazione del
  // trascinamento sulla finestra la tiene fra le dipendenze, e un'identità che
  // cambia a ogni disegno rifarebbe quella registrazione sessanta volte al
  // secondo mentre si trascina un file.
  const scegliSkin = useCallback(
    async (id: string) => {
      const giro = (giroSkin.current += 1);
      try {
        const scelta = await ipc.skinScegli(id);
        // Il foglio si scrive solo se nel frattempo non è partita un'anteprima
        // più recente; la scelta invece è già nel database, quindi l'elenco e
        // la skin attiva si aggiornano comunque.
        if (giro === giroSkin.current) applicaSkin(scelta);
        setSkinAttiva(scelta);
        await ricaricaSkin();
      } catch (e) {
        segnalaErrore(e);
      }
    },
    [ricaricaSkin],
  );

  /**
   * Toglie una skin installata.
   *
   * Il comando risponde con la skin che **resta attiva** — quella di serie, se
   * si è appena tolta quella indossata — quindi il foglio si riscrive con
   * quella e non c'è un istante in cui la finestra è dipinta da un pacchetto
   * che non esiste più. Passa dal contatore come la scelta e l'anteprima, per
   * la stessa ragione: se nel frattempo il puntatore è finito su un'altra
   * scheda, l'anteprima più recente vince.
   */
  const disinstallaSkin = useCallback(
    async (id: string) => {
      const giro = (giroSkin.current += 1);
      try {
        const resta = await ipc.skinDisinstalla(id);
        if (giro === giroSkin.current) applicaSkin(resta);
        setSkinAttiva(resta);
        await ricaricaSkin();
      } catch (e) {
        segnalaErrore(e);
      }
    },
    [ricaricaSkin],
  );

  /**
   * Prova una skin senza sceglierla.
   *
   * `skin(id)` compila e basta: la scelta persistente la scrive solo
   * `skin_scegli`. È un comando che esisteva già e che nessuno chiamava con un
   * argomento — cioè un'anteprima già pronta nel nucleo, aspettando che
   * l'interfaccia se ne accorgesse.
   *
   * Passa dal contatore come la scelta, e per la stessa ragione: passare veloci
   * su tre schede manda tre richieste, e senza un ordine la terza può arrivare
   * per prima e lasciare a schermo la seconda. Vedi `giroSkin`.
   */
  const anteprimaSkin = useCallback(
    (id: string | null) => {
      const quale = id ?? skin.find((s) => s.attiva)?.id;
      if (quale === undefined) return;
      const giro = (giroSkin.current += 1);
      ipc
        .skin(quale)
        .then((s) => {
          if (giro !== giroSkin.current) return;
          applicaSkin(s);
          // Tornati alla skin scelta, l'accento della copertina può tornare
          // anche lui. Vedi `accentoDaRimettere`.
          if (id === null) setAccentoDaRimettere((n) => n + 1);
        })
        .catch(segnalaErrore);
    },
    [skin, segnalaErrore],
  );

  const installaSkin = async () => {
    try {
      const scelta = await open({
        multiple: false,
        filters: [{ name: t("file.skin"), extensions: ["aeskin"] }],
      });
      if (typeof scelta !== "string") return;
      const installata = await ipc.skinInstalla(scelta);
      // Installare e non provare sarebbe metà del gesto: chi sceglie un file
      // di skin vuole vederla.
      await scegliSkin(installata.id);
    } catch (e) {
      segnalaErrore(e);
    }
  };

  /**
   * Un tema nuovo: manifest, installazione, scelta, Studio.
   *
   * Nasce **installato e attivo** e non come bozza, ed è la differenza che fa
   * funzionare tutto il resto. La seconda delle tre regole dello Studio dice che
   * l'anteprima è l'applicazione e non un riquadro: un tema che esiste solo
   * dentro l'editor non può mantenerla, e chi lo stesse facendo dovrebbe
   * esportarlo su disco e reinstallarlo per vedere davvero com'è.
   */
  const creaTema = async (dati: DatiTema) => {
    try {
      // La base può essere una bozza a metà — `studio_documento` le preferisce,
      // ed è giusto così. `sorgenteNuova` risponde `null` invece di restituire
      // un manifest che porta ancora l'id della base: installarlo
      // sovrascriverebbe proprio la skin da cui si voleva partire.
      const sorgente = sorgenteNuova(await ipc.studioDocumento(dati.base), dati);
      if (sorgente === null) {
        const quale = skin.find((s) => s.id === dati.base)?.nome ?? dati.base;
        setErrore({
          testo: t("theme.new.unreadableBase", { nome: quale }),
          dettaglio: null,
        });
        return;
      }
      await ipc.skinInstallaSorgente(sorgente);
      // La bozza parte allineata all'installato, così riaprendo lo Studio la
      // pillola in testa non dice «bozza» su un lavoro che nessuno ha ancora
      // toccato.
      await ipc.studioSalva(dati.id, sorgente);
      await scegliSkin(dati.id);
      setCreandoTema(false);
      setStudioAperto(dati.id);
    } catch (e) {
      segnalaErrore(e);
    }
  };

  /*
   * L'avanzamento della scansione, a non più di quattro disegni al secondo.
   *
   * `scansione:avanzamento` arriva ogni venticinque file, cioè decine di volte
   * al secondo su un disco veloce, e ogni arrivo ridisegnava l'applicazione
   * intera — elenchi, colonna, lettore — per spostare una barra di un pixel. Un
   * numero che si legge non cambia più spesso di così; l'ultimo valore arriva
   * sempre, e la fine del lavoro passa subito. Il timer in sospeso si butta
   * quando la scansione finisce, o un arrivo in ritardo rimetterebbe la barra
   * dopo che `scansiona` l'ha tolta.
   */
  const strozzaScansione = useRef<{
    ultimo: number;
    timer: number | undefined;
    valore: Avanzamento | null;
  }>({ ultimo: 0, timer: undefined, valore: null });
  const posaScansione = useCallback((valore: Avanzamento | null) => {
    const r = strozzaScansione.current;
    window.clearTimeout(r.timer);
    r.timer = undefined;
    r.ultimo = performance.now();
    r.valore = valore;
    setScansione(valore);
  }, []);
  useAscolto<Avanzamento>("scansione:avanzamento", (carico) => {
    const r = strozzaScansione.current;
    r.valore = carico;
    const passato = performance.now() - r.ultimo;
    const finito = carico.totale > 0 && carico.fatti >= carico.totale;
    if (finito || passato >= 250) {
      posaScansione(carico);
      return;
    }
    if (r.timer === undefined)
      r.timer = window.setTimeout(() => posaScansione(r.valore), 250 - passato);
  });
  useEffect(() => () => window.clearTimeout(strozzaScansione.current.timer), []);

  /**
   * Backup, sincronia e arricchimento, che adesso stanno in `nuvola.ts`.
   *
   * La chiamata è qui e non fra le dichiarazioni in cima perché è questa riga a
   * registrare le tre letture d'avvio e i cinque ascolti: li registra nel punto
   * in cui stavano — dopo `scansione:avanzamento`, prima di tutto quel che
   * viene sotto — e spostarla vorrebbe dire cambiare l'ordine in cui gli
   * effetti girano al primo disegno. Perché il grappolo viva di là, e perché il
   * tema e la selezione siano rimasti di qua, sta scritto in testa a quel file.
   */
  const {
    nuvola,
    sincronia,
    setSincronia,
    arricchimento,
    setArricchimento,
    avanzaArricchimento,
    esitoArricchimento,
    conNuvola,
    conSincronia,
  } = useNuvola(segnalaErrore);

  // La ricerca ha la precedenza su qualunque vista: quel che si sta cercando è
  // ciò che si vuole vedere. Il passaggio in modalità ricerca è **immediato**,
  // la richiesta no: quel che si chiede al nucleo segue con un ritardo.
  const cercando = query.trim().length > 0;
  const queryPigra = usePigro(query.trim(), 140);
  /** Il campo e la richiesta non dicono ancora la stessa cosa. */
  const inRitardo = cercando && queryPigra !== query.trim();

  /**
   * L'elenco dei brani, da qualunque parte venga.
   *
   * Tre sorgenti e una chiave sola: la ricerca, i preferiti e la vista Brani
   * col suo ordinamento. Erano tre rami dentro `caricaVista`, che chiedeva la
   * prima pagina e poi non ne chiedeva più — e i preferiti li tirava fuori
   * filtrando duemila brani nella finestra.
   */
  const elencoBrani = usePagine<Brano>(
    (offset, limite) => {
      if (cercando) {
        return queryPigra.length > 0
          ? ipc.cerca(queryPigra, offset, limite)
          : Promise.resolve([]);
      }
      if (vista === "preferiti") return ipc.preferiti(offset, limite);
      if (vista === "brani") return ipc.brani(ordine, offset, limite);
      // Le viste che non sono elenchi di brani non ne chiedono: la chiave
      // contiene `vista`, quindi tornandoci l'elenco si richiede da sé.
      return Promise.resolve([]);
    },
    cercando ? `cerca:${queryPigra}` : `${vista}:${ordine}`,
    segnalaErrore,
  );
  const brani = elencoBrani.righe;

  /**
   * Gli album: la griglia intera, o quelli di un artista.
   *
   * Anche questi una chiave sola. Prima la pagina di un artista **filtrava**
   * nella finestra i quattrocento album già scaricati: chi ne aveva di più
   * vedeva una griglia vuota sotto un titolo che diceva «tre album».
   */
  const elencoAlbum = usePagine<Album>(
    (offset, limite) =>
      artistaAperto
        ? ipc.albumArtista(artistaAperto.name, offset, limite)
        : ipc.album(offset, limite),
    artistaAperto ? `artista:${artistaAperto.name}` : "album",
    segnalaErrore,
  );
  const album = elencoAlbum.righe;

  /**
   * Quanti risultati ha la ricerca **in tutto**.
   *
   * Chiesto a parte e una volta per query: l'intestazione scriveva «60
   * risultati» per una ricerca che ne aveva trecento, perché il numero era la
   * lunghezza della prima pagina. Un conteggio che si ferma al limite non è un
   * conteggio, è il limite scritto in lettere.
   */
  const [risultati, setRisultati] = useState<number | null>(null);
  useEffect(() => {
    if (queryPigra.length === 0) {
      setRisultati(null);
      return;
    }
    let annullato = false;
    ipc
      .cercaConteggio(queryPigra)
      .then((quanti) => {
        if (!annullato) setRisultati(quanti);
      })
      .catch(segnalaErrore);
    return () => {
      annullato = true;
    };
  }, [queryPigra, segnalaErrore]);

  // Gli artisti si chiedono tutti: sono un ordine di grandezza meno dei brani,
  // e la vista li mostra con un indice alfabetico invece che a pagine — che è
  // la ragione per cui `artisti` non ha offset né limite nel nucleo.
  const [artistiInArrivo, setArtistiInArrivo] = useState(false);
  useEffect(() => {
    if (vista !== "artisti" || cercando) return;
    setArtistiInArrivo(true);
    let annullato = false;
    ipc
      .artisti()
      .then((arrivati) => {
        if (!annullato) setArtisti(arrivati);
      })
      .catch(segnalaErrore)
      .finally(() => {
        if (!annullato) setArtistiInArrivo(false);
      });
    return () => {
      annullato = true;
    };
  }, [vista, cercando, segnalaErrore]);

  /*
   * La Home, entrando.
   *
   * Fino a qui `ipc.casa()` stava **solo** dentro `caricaVista`, che parte sugli
   * eventi che cambiano la libreria — una scansione, dei brani scaricati. Non
   * all'apertura, e non entrando nella vista: quindi `casa` restava `null` per
   * sempre e la Home disegnava il vuoto sotto il proprio titolo. Era tutta
   * costruita — comando, stili, stringhe, indice — e non la chiedeva nessuno.
   *
   * I ripiani sono fatti di «di recente», che è vero solo nel momento in cui lo
   * si chiede: si rifà a ogni ritorno, come fa la vista degli artisti qui sopra.
   *
   * `casa` non si azzera prima della richiesta. Tornando alla Home i ripiani di
   * prima restano sullo schermo finché non arrivano i nuovi — che è anche il
   * motivo per cui il segnaposto di `CasaFinta` si vede una volta sola, la
   * prima.
   */
  useEffect(() => {
    if (vista !== "home" || cercando) return;
    // Subito, non nella risposta: questo effetto e quello qui sotto girano tutti
    // e due quando si entra nella vista, e senza questa riga il secondo vedrebbe
    // il segno di prima e chiederebbe la stessa cosa una seconda volta.
    casaPerBrano.current = riproduzione.stato.brano?.id ?? null;
    let annullato = false;
    ipc
      .casa()
      .then((arrivata) => {
        if (annullato) return;
        // E adesso il segno vero: di quale brano corrente parla la Home che si
        // ha in mano. All'avvio lo stato di riproduzione arriva per conto suo, e
        // senza questa riga il suo primo colpo sembrerebbe un cambio di brano —
        // cioè una seconda richiesta nel momento in cui la finestra ha già tutto
        // il resto da fare.
        casaPerBrano.current = arrivata.riprendi?.id ?? null;
        setCasa(arrivata);
      })
      .catch(segnalaErrore);
    return () => {
      annullato = true;
    };
  }, [vista, cercando, segnalaErrore]);

  /*
   * Le raccolte del lunedì, entrando nella Home.
   *
   * In una chiamata sua e non dentro `ipc.casa()`, che è il comando aggregato:
   * `casa` si rifà a ogni cambio di brano corrente — è la riga «riprendi dov'eri»
   * che deve restare vera — e la generazione delle raccolte apre una
   * transazione. Metterla lì dentro vorrebbe dire aprirne una a ogni cambio di
   * brano per riscoprire ogni volta che il lunedì c'è già.
   *
   * Non dipende dal brano corrente proprio per questo: entra nella vista, chiede
   * una volta, e per il resto della settimana la risposta è la stessa.
   *
   * «Una volta» però va fatto succedere, e le dipendenze da sole non bastano:
   * `vista` e `cercando` cambiano decine di volte in una sera — ogni giro fra
   * Home, Album e Artisti, ogni apertura e chiusura della ricerca — e ognuno
   * rifaceva la domanda per intero. Il lunedì chiesto si ricorda qui: finché è
   * lo stesso, la risposta che si ha in mano è già quella giusta.
   */
  const settimanaChiesta = useRef<number | null>(null);
  useEffect(() => {
    if (vista !== "home" || cercando) return;
    // Lo stesso lunedì che il nucleo calcolerebbe, e con lo stesso fuso: qui
    // serve solo a riconoscere che è cambiato, e a mezzanotte di domenica
    // cambia da sé senza che nessuno debba svegliare niente.
    const adesso = new Date();
    const giorno = (adesso.getDay() + 6) % 7;
    const lunedi = new Date(
      adesso.getFullYear(),
      adesso.getMonth(),
      adesso.getDate() - giorno,
    ).getTime();
    if (settimanaChiesta.current === lunedi) return;

    let annullato = false;
    ipc
      .settimana()
      .then((arrivate) => {
        if (annullato) return;
        // Si segna **dopo** la risposta, non prima della domanda: una chiamata
        // fallita deve poter essere rifatta entrando di nuovo nella Home.
        settimanaChiesta.current = lunedi;
        setSettimana(arrivate);
      })
      .catch(segnalaErrore);
    return () => {
      annullato = true;
    };
  }, [vista, cercando, segnalaErrore]);

  /*
   * E la Home, restando.
   *
   * «Riprendi dov'eri» fa partire **la coda del nucleo**, non il brano che ha
   * scritto sopra: se i due divergono la riga mente. E divergono appena si
   * preme qualcosa senza uscire dalla Home — la coda cambia brano corrente,
   * `casa` no.
   *
   * `trascurati` si conserva attraverso il rinfresco. Quel ripiano arriva da un
   * ordinamento casuale: rimescolarsi entrando nella vista va bene ed è il
   * senso di un ripiano di riscoperta, rimescolarsi mentre lo si sta guardando
   * vuol dire che la copertina sotto il dito non è più quella che si stava per
   * premere.
   */
  const branoCorrente = riproduzione.stato.brano?.id ?? null;
  useEffect(() => {
    if (vista !== "home" || cercando) return;
    // Il confronto sta in un riferimento e non fra le dipendenze: mettendoci
    // `casa` questo effetto girerebbe di nuovo a ogni sua risposta, e basterebbe
    // che la coda salvata sul database e quella del motore fossero d'accordo un
    // istante dopo invece che subito per farne un giro senza fine.
    if (casaPerBrano.current === branoCorrente) return;
    casaPerBrano.current = branoCorrente;
    let annullato = false;
    ipc
      .casa()
      .then((arrivata) => {
        if (annullato) return;
        setCasa((prima) =>
          prima ? { ...arrivata, trascurati: prima.trascurati } : arrivata,
        );
      })
      .catch(segnalaErrore);
    return () => {
      annullato = true;
    };
  }, [branoCorrente, vista, cercando, segnalaErrore]);

  // In cima a ogni cambio di elenco: la posizione di scorrimento di prima
  // appartiene a un contenuto che non c'è più.
  useEffect(() => {
    contenuto.current?.scrollTo({ top: 0 });
  }, [vista, ordine, queryPigra, aperto, playlistAperta, artistaAperto]);

  /**
   * Rifà quel che la vista aperta sta mostrando.
   *
   * Dipende dalle due `ricarica` e non dai due oggetti: quelle sono stabili,
   * gli oggetti cambiano identità a ogni riga che arriva — e `caricaVista` sta
   * a sua volta nelle dipendenze di due `useCallback`, che si riscriverebbero
   * di continuo.
   */
  const ricaricaBrani = elencoBrani.ricarica;
  const ricaricaAlbum = elencoAlbum.ricarica;

  /**
   * Tutto quel che sta **intorno** all'elenco dei brani: la griglia degli
   * album, quella degli artisti, i ripiani della Home.
   *
   * A parte, per chi l'elenco se l'è già sistemato da sé. `elimina` toglie le
   * righe a mano — è la parte che si vede, e richiederla al nucleo la
   * rimanderebbe alla prima pagina, buttando via lo scorrimento di chi era in
   * fondo alla libreria. Ma un brano che se ne va cambia anche il numero sotto
   * una copertina, e un disco rimasto senza tracce sparisce dalla griglia: quei
   * tre elenchi un «togli una riga» non ce l'hanno, perché quanti brani abbia
   * davvero un album lo sa solo il nucleo — la finestra ne vede una pagina.
   */
  const ricaricaContorno = useCallback(() => {
    ricaricaAlbum();
    if (vista === "artisti") {
      ipc.artisti().then(setArtisti).catch(segnalaErrore);
    }
    // La Home dopo una scansione o dei brani scaricati: i suoi ripiani sono
    // fatti di «di recente», e quel che è appena entrato in libreria li cambia
    // tutti e quattro. Entrandoci ci pensa invece l'effetto suo, che è quel che
    // fino a poco fa non esisteva e lasciava la schermata vuota.
    if (vista === "home") {
      ipc
        .casa()
        .then((arrivata) => {
          casaPerBrano.current = arrivata.riprendi?.id ?? null;
          setCasa(arrivata);
        })
        .catch(segnalaErrore);
    }
  }, [ricaricaAlbum, vista, segnalaErrore]);

  const caricaVista = useCallback(() => {
    ricaricaBrani();
    ricaricaContorno();
  }, [ricaricaBrani, ricaricaContorno]);

  /**
   * I brani scaricati sono entrati in libreria.
   *
   * La coda, quando finisce, rifà una scansione da sé: i file presi dai
   * cataloghi diventano brani veri senza che nessuno prema «Scansiona». Ma la vista
   * aperta continuerebbe a mostrare i conteggi di prima — una libreria a cui
   * sono appena arrivati quaranta brani che non compaiono finché non si cambia
   * schermata.
   */
  useAscolto("scarico:in_libreria", () => {
    void ricarica();
    void caricaVista();
  });

  /*
   * I brani dell'album aperto.
   *
   * Due difetti, e una riga ciascuno. Aprendo un album dopo un altro si vedevano
   * per un attimo i brani del **precedente** sotto la copertina del nuovo: si
   * azzera quando cambia l'album, e non quando cambia soltanto l'oggetto — lo
   * stesso album riletto dopo un voto non deve lampeggiare vuoto. E chi apriva
   * due album in fila veloce poteva ritrovarsi sotto il secondo i brani del
   * primo, se la prima risposta arrivava dopo: la risposta di una richiesta
   * superata si butta.
   */
  const albumDiPrima = useRef<string | null>(null);
  useEffect(() => {
    if (!aperto) {
      albumDiPrima.current = null;
      return;
    }
    if (albumDiPrima.current !== aperto.albumKey) setBraniAperto([]);
    albumDiPrima.current = aperto.albumKey;
    let superata = false;
    ipc
      .braniAlbum(aperto.albumKey)
      .then((brani) => {
        if (!superata) setBraniAperto(brani);
      })
      .catch((e: unknown) => {
        if (!superata) segnalaErrore(e);
      });
    return () => {
      superata = true;
    };
  }, [aperto]);

  /** Porta in una vista, azzerando tutto quel che le sta sopra. */
  const vaiA = useCallback((dove: Vista) => {
    cambiandoVista(() => {
      setQuery("");
      setAperto(null);
      setArtistaAperto(null);
      setPlaylistAperta(null);
      setVista(dove);
    });
  }, []);

  /*
   * ── il giro guidato ────────────────────────────────────────────────────────
   *
   * Il perché della forma sta in `Giro.tsx`; qui c'è soltanto **quando** parte
   * e **dove** si mette l'applicazione a ogni passo.
   */

  /**
   * Il permesso, chiesto una volta all'apertura.
   *
   * Un guasto non si mostra: sarebbe un avviso rosso per dire che non è
   * comparso un giro guidato, cioè per dire che non è successo niente. È la
   * stessa scelta della preferenza del movimento qui sopra.
   */
  useEffect(() => {
    ipc
      .giroDaFare()
      .then(setGiroDaFare)
      .catch(() => undefined);
  }, []);

  /**
   * Quando parte, e le due volte in cui **non** parte.
   *
   * **Dopo la chiusura del primo avvio, non dentro.** `Primo` chiede dove sta
   * la musica e fa una scansione: un fumetto sopra quella schermata spiegherebbe
   * una finestra che non si è ancora vista. Il giro aspetta che `Primo` se ne
   * sia andato — che si sia premuto «Ascolta» o «Lo faccio dopo» — e non
   * soltanto che una cartella ci sia: la cartella c'è già mentre `Primo` mostra
   * la scansione.
   *
   * **Libreria vuota: si rinvia, non si simula.** Illuminare una riga di brano
   * che non esiste vorrebbe dire disegnarne una finta, e insegnare una libreria
   * che non c'è; il finto di `studio/finto.ts` serve a chi disegna skin, non a
   * chi ascolta. Senza brani il giro non parte e basta — e siccome questa
   * condizione è uno stato e non un evento, riparte da sé alla prima scansione
   * che finisce con qualcosa dentro, senza che nessuno debba ricordarsene.
   *
   * «Lo faccio dopo» non è quindi un rifiuto del giro: chiude `Primo` e lo
   * rinvia, perché la libreria resta vuota.
   */
  useEffect(() => {
    if (!giroDaFare || giroAperto) return;
    if (avvio === null) return;
    if (primoVisibile) return;
    if (avvio.numeri.tracks === 0) return;
    setGiroAperto(true);
  }, [giroDaFare, giroAperto, avvio, primoVisibile]);

  /**
   * Quando il giro si apre senza niente in coda, ci si mette qualcosa — fermo.
   *
   * Quattro passi su undici parlano del lettore, e il lettore si vede solo con
   * un brano: al primo avvio si saltavano tutti e quattro. Il nucleo riempie
   * la coda **solo se è vuota** e non fa partire niente (`coda_prepara`), e i
   * primi passi — il benvenuto, la barra, la ricerca, la riga — lasciano il
   * tempo alla risposta di arrivare prima che serva.
   */
  const branoDelLettore = riproduzione.stato.brano;
  useEffect(() => {
    if (!giroAperto || branoDelLettore !== null) return;
    ipc
      .brani("recenti", 0, 50)
      .then((elenco) =>
        elenco.length === 0
          ? false
          : ipc.codaPrepara(elenco.map((b) => b.id)),
      )
      // Senza, il giro salta quei quattro passi come prima: è un ripiego, non
      // un guasto da mostrare in cima al giro.
      .catch(() => undefined);
    // Solo all'apertura: quando il brano arriva, l'effetto non deve rifarsi.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [giroAperto]);

  /**
   * Mette l'applicazione dove il passo vive.
   *
   * Non è una simulazione: sono gli stessi `setStato` che premono i tasti veri,
   * e quel che si illumina è quel che c'è. Dove la condizione non dipende da
   * qui — non suona niente, quindi non c'è né colonna né schermo intero — il
   * passo si salta da sé, ed è la regola dichiarata in `Giro.tsx`.
   *
   * `setGrande(false)` quasi ovunque perché la schermata a tutto schermo copre
   * la barra laterale: senza, il passo dopo «In riproduzione» illuminerebbe una
   * navigazione nascosta sotto una copertina.
   */
  const preparaGiro = useCallback(
    (passo: NomePasso) => {
      switch (passo) {
        case "ricerca":
        case "riga-brano":
          setGrande(false);
          vaiA("brani");
          break;
        case "giudizio":
        case "colonna":
          // La fascia del giudizio vive dentro la colonna e dentro lo schermo
          // intero: si sceglie la colonna, che è quella che si vede senza
          // coprire il resto.
          setGrande(false);
          setColonnaAperta(true);
          break;
        case "in-riproduzione":
          // Solo se c'è qualcosa da vedere: `InRiproduzione` non si monta senza
          // un brano, e accendere `grande` a vuoto lascerebbe la finestra in
          // uno stato che l'utente non ha chiesto e che il passo non usa.
          if (riproduzione.stato.brano !== null) setGrande(true);
          break;
        case "studio":
          setGrande(false);
          vaiA("impostazioni");
          setSezione("aspetto");
          break;
        case "impostazioni":
          setGrande(false);
          vaiA("impostazioni");
          break;
        default:
          setGrande(false);
          break;
      }
    },
    [vaiA, riproduzione.stato.brano],
  );

  /**
   * Il giro è finito, saltato, o chiuso con Escape.
   *
   * Tutti e tre segnano «visto»: vedi il `//!` di `src-tauri/src/giro.rs`.
   * `setGiroDaFare(false)` non aspetta la scrittura — se il database non
   * risponde, riaprire il giro sarebbe il rimedio peggiore del male.
   */
  const chiudiGiro = useCallback(() => {
    setGiroAperto(false);
    setGiroDaFare(false);
    ipc.giroFatto().catch(() => undefined);
  }, []);

  /** «Rifai il giro», dalle Impostazioni. */
  const rifaiGiro = useCallback(() => setGiroAperto(true), []);

  /**
   * Apre un album, o torna indietro se è `null`.
   *
   * Passa dalla transizione come `vaiA`: entrare in un disco e uscirne è un
   * cambio di pagina quanto passare da Album a Brani, e senza questo sarebbe
   * l'unico che avviene di scatto.
   */
  const apriAlbum = useCallback((quale: Album | null) => {
    cambiandoVista(() => setAperto(quale));
  }, []);

  const apriArtista = useCallback((quale: Artista | null) => {
    cambiandoVista(() => setArtistaAperto(quale));
  }, []);

  /**
   * Il primo avvio: scrive le cartelle scelte e comincia subito a leggere.
   *
   * Le due cose insieme e non due pulsanti: fra «ho detto dove sta la musica» e
   * «voglio che la legga» non c'è nessuna decisione in mezzo, e il secondo
   * pulsante esisteva solo perché il primo non sapeva cosa fare dopo.
   */
  const primoConferma = useCallback(
    async (cartelle: string[]) => {
      try {
        await ipc.impostaCartelle(cartelle);
        await ricarica();
        await scansiona();
      } catch (e) {
        segnalaErrore(e);
      }
    },
    // `scansiona` e `ricarica` sono definite in questo corpo e cambiano identità
    // a ogni disegno: entrarci nelle dipendenze farebbe rifare questa lambda
    // sessanta volte al secondo per niente. Chi la riceve non la confronta.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  /** Il dialogo di sistema, per chi la musica la tiene altrove. */
  const primoScegli = useCallback(async () => {
    try {
      const scelta = await open({ directory: true, multiple: false });
      return typeof scelta === "string" ? scelta : null;
    } catch (e) {
      segnalaErrore(e);
      return null;
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  /**
   * L'ultimo clic prima del suono.
   *
   * Prende la prima pagina della libreria e la suona dall'inizio. Non un brano
   * solo: mettere in coda un brano soltanto vorrebbe dire che tre minuti dopo
   * la coda finisce, che è il difetto che `suonaDa` documenta altrove.
   */
  const primoAscolta = useCallback(() => {
    void (async () => {
      try {
        const elenco = await ipc.brani("recenti", 0, 200);
        if (elenco.length === 0) return;
        await ipc.suona(
          elenco.map((b) => b.id),
          0,
        );
        setPrimoChiuso(true);
      } catch (e) {
        segnalaErrore(e);
      }
    })();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const scegliCartella = async () => {
    try {
      const scelta = await open({ directory: true, multiple: false });
      if (typeof scelta !== "string" || !avvio) return;
      const cartelle = avvio.cartelle.includes(scelta)
        ? avvio.cartelle
        : [...avvio.cartelle, scelta];
      await ipc.impostaCartelle(cartelle);
      await ricarica();
    } catch (e) {
      segnalaErrore(e);
    }
  };

  /**
   * Sceglie dove finiscono i brani scaricati.
   *
   * Il dialogo si apre sulla cartella già scelta, o sulla prima sorvegliata:
   * chi la cambia quasi sempre la sposta lì accanto, e ripartire dalla radice
   * del disco ogni volta è la differenza fra due clic e otto.
   */
  const scegliCartellaDownload = async () => {
    try {
      const partenza = avvio?.cartellaDownload ?? avvio?.cartelle[0];
      const scelta = await open({
        directory: true,
        multiple: false,
        ...(partenza === undefined ? {} : { defaultPath: partenza }),
      });
      if (typeof scelta !== "string") return;
      await ipc.impostaCartellaDownload(scelta);
      await ricarica();
    } catch (e) {
      segnalaErrore(e);
    }
  };

  /**
   * Sceglie un file di playlist da importare.
   *
   * I tre formati insieme in un filtro solo: chi ha un file di playlist non sa
   * necessariamente quale dei tre sia, e tre voci separate nel dialogo
   * sarebbero tre modi di non trovarlo.
   */
  const scegliFilePlaylist = async () => {
    try {
      const scelta = await open({
        multiple: false,
        filters: [
          {
            name: t("file.playlist"),
            extensions: ["m3u", "m3u8", "pls", "xspf"],
          },
        ],
      });
      if (typeof scelta === "string") setFilePlaylist(scelta);
    } catch (e) {
      segnalaErrore(e);
    }
  };

  /**
   * Scrive una playlist in un file.
   *
   * Il formato lo decide l'estensione che l'utente scrive nel dialogo, e il
   * nome proposto è quello della playlist: `defaultPath` con l'estensione
   * dentro è ciò che fa comparire «Serata.m3u8» già scritto, invece di un campo
   * vuoto in cui va indovinata anche l'estensione.
   */
  const esportaPlaylist = async (p: Playlist) => {
    try {
      const scelta = await save({
        defaultPath: `${p.name}.m3u8`,
        filters: [
          { name: "M3U", extensions: ["m3u8", "m3u"] },
          { name: "PLS", extensions: ["pls"] },
          { name: "XSPF", extensions: ["xspf"] },
        ],
      });
      if (typeof scelta !== "string") return;
      const quanti = await ipc.playlistEsporta(p.id, scelta);
      setNotizia(
        quanti === 1
          ? t("playlist.exported.one", { dove: scelta })
          : t("playlist.exported", { n: numero(quanti), dove: scelta }),
      );
    } catch (e) {
      segnalaErrore(e);
    }
  };

  const togliCartella = async (percorso: string) => {
    if (!avvio) return;
    try {
      await ipc.impostaCartelle(avvio.cartelle.filter((c) => c !== percorso));
      await ricarica();
    } catch (e) {
      segnalaErrore(e);
    }
  };

  /**
   * Sceglie il database della versione rilasciata.
   *
   * La cartella dati di questa applicazione è **diversa** da quella della 1.0.0
   * — `%APPDATA%\Aether` contro `%APPDATA%\io.github.federicobaratti.aether` — apposta perché
   * finché questa non è finita la vecchia deve restare apribile. Perciò il
   * dialogo si apre lì: è dove il file sta, e comporre il percorso a mano è il
   * modo di sbagliarlo su un profilo spostato.
   */
  const scegliDatabase = async () => {
    try {
      const roaming = avvio?.dataDir.replace(/[/\\][^/\\]+$/, "");
      const scelta = await open({
        multiple: false,
        // Sparso e non `defaultPath: … : undefined`: con
        // `exactOptionalPropertyTypes` una proprietà assente e una uguale a
        // `undefined` sono due cose diverse, e il dialogo vuole la prima.
        ...(roaming ? { defaultPath: `${roaming}\\Aether` } : {}),
        filters: [
          { name: t("file.database"), extensions: ["db", "sqlite", "sqlite3"] },
        ],
      });
      if (typeof scelta === "string") setDaImportare(scelta);
    } catch (e) {
      segnalaErrore(e);
    }
  };

  const scansiona = async () => {
    setEsito(null);
    posaScansione({ fatti: 0, totale: 0 });
    try {
      const risultato = await ipc.scansiona();
      setEsito(risultato);
      await ricarica();
      await caricaVista();
    } catch (e) {
      segnalaErrore(e);
    } finally {
      posaScansione(null);
    }
  };

  /**
   * Fa partire una lista a partire da un brano.
   *
   * La coda è **la lista che si sta guardando**, non il brano solo: chi clicca
   * la terza traccia di un album si aspetta che poi parta la quarta, e mandare
   * un brano solo trasformerebbe ogni clic in una fine coda tre minuti dopo.
   */
  const suonaDa = useCallback(async (elenco: Brano[], indice: number) => {
    try {
      await ipc.suona(
        elenco.map((b) => b.id),
        indice,
      );
    } catch (e) {
      segnalaErrore(e);
    }
  }, []);

  /**
   * Fa partire **un** brano, da solo.
   *
   * Non è `suonaDa` con un elenco di uno: quella prende righe di libreria e qui
   * si ha solo un identificativo, che è tutto quel che serve — la coda viaggia
   * per identificativi, e il motore rilegge la riga da sé.
   */
  const suonaSolo = useCallback(
    async (id: number) => {
      try {
        await ipc.suona([id], 0);
      } catch (e) {
        segnalaErrore(e);
      }
    },
    [segnalaErrore],
  );

  const ricaricaPlaylist = useCallback(async () => {
    try {
      setPlaylist(await ipc.playlistElenco());
    } catch (e) {
      segnalaErrore(e);
    }
  }, [segnalaErrore]);

  useEffect(() => {
    void ricaricaPlaylist();
  }, [ricaricaPlaylist]);

  // I brani della playlist aperta. `updatedAt` fa parte della dipendenza: ogni
  // comando restituisce la playlist aggiornata, e senza quel campo un'aggiunta
  // alla playlist che si sta già guardando non ricaricherebbe l'elenco.
  //
  // Come per l'album: si azzera solo cambiando playlist — un'aggiunta a quella
  // aperta non deve farla lampeggiare vuota — e la risposta di una richiesta
  // superata si butta.
  const playlistDiPrima = useRef<number | null>(null);
  useEffect(() => {
    if (!playlistAperta) {
      playlistDiPrima.current = null;
      return;
    }
    if (playlistDiPrima.current !== playlistAperta.id) setBraniPlaylist([]);
    playlistDiPrima.current = playlistAperta.id;
    let superata = false;
    const quale = playlistAperta.id;
    ipc
      .playlistBrani(quale)
      .then((brani) => {
        if (superata) return;
        setBraniPlaylist(brani);
        setPlaylistCaricata(quale);
      })
      .catch((e: unknown) => {
        if (!superata) segnalaErrore(e);
      });
    return () => {
      superata = true;
    };
  }, [playlistAperta, segnalaErrore]);

  /** Rimpiazza una playlist nell'elenco e, se è quella aperta, anche lì. */
  const aggiornaPlaylist = useCallback((cambiata: Playlist) => {
    setPlaylist((prima) =>
      prima.map((p) => (p.id === cambiata.id ? cambiata : p)),
    );
    setPlaylistAperta((prima) =>
      prima && prima.id === cambiata.id ? cambiata : prima,
    );
  }, []);

  /**
   * L'elenco che si sta guardando, leggibile da una chiusura.
   *
   * `elencoCorrente` è dichiarato **dopo** [`apriMenu`] — deriva da stati che
   * stanno più in basso — quindi il menù non può averlo fra le dipendenze:
   * nominarlo là sarebbe una lettura prima dell'inizializzazione. Un `ref` è il
   * modo di leggerlo al momento del clic invece che al momento del disegno, ed
   * è lo stesso motivo per cui `pagine.ts` tiene `chiedi` in un `ref`.
   *
   * Senza, la chiusura catturava l'elenco che c'era alla creazione del menù —
   * cioè quello vuoto del primo disegno — e ogni domanda fatta all'elenco
   * rispondeva «non lo so». Si vedeva su una cosa sola, la voce «mostra nella
   * cartella» offerta su un brano che una cartella non ce l'ha.
   */
  const elencoCorrenteRef = useRef<Brano[]>([]);

  /** Apre il menù contestuale su una selezione di brani. */
  const apriMenu = useCallback(
    (e: React.MouseEvent, elenco: number[]) => {
      e.preventDefault();
      // La radio ha **un** seme, e con più brani selezionati non si sa quale:
      // la voce sparisce invece di far finta di scegliere. È lo stesso motivo
      // per cui «rinomina» non compare su una selezione multipla.
      const primo = elenco.length === 1 ? elenco[0] : undefined;
      /*
       * Le tre voci che su un brano solo mancavano: portarsi al suo disco, al
       * suo artista, e al suo file. Il menù aveva «radio», «dopo», «in coda» e
       * «a una playlist» — cioè tutto quel che si fa **con** il brano e niente
       * di quel che si fa **da** il brano, che è la domanda più comune davanti a
       * una riga di un elenco lungo: «di che disco è questo?». Il brano si legge
       * al momento del gesto: l'elenco che ha aperto il menù ha solo i numeri.
       */
      const conBrano = (fai: (brano: Brano) => Promise<void> | void) => () => {
        if (primo === undefined) return;
        ipc
          .braniPerId([primo])
          .then(async ([brano]) => {
            if (brano !== undefined) await fai(brano);
          })
          .catch(segnalaErrore);
      };
      /*
       * Portarsi a un disco o a un artista vuol dire anche **uscire** da quel che
       * gli sta sopra. `corpo()` guarda prima la ricerca, poi la playlist, poi
       * l'album, poi l'artista: da una ricerca o da una playlist il disco si
       * apriva sotto e non si vedeva — compariva svuotando la ricerca, cioè
       * molto dopo il clic e senza nessun legame con lui — e l'artista, da un
       * album aperto, restava sotto l'album. Si esce da tutti in un colpo solo,
       * dentro la stessa transizione.
       */
      const daUnBrano =
        primo === undefined
          ? []
          : [
              {
                etichetta: t("menu.goToAlbum"),
                azione: conBrano(async (brano) => {
                  const chiave = brano.albumKey;
                  // Un brano senza disco — un flusso, un file sparso — ha la voce
                  // come tutti: il menù si apre prima di leggere il brano. Il
                  // clic almeno lo dice, invece di non fare niente.
                  if (chiave === null) {
                    setNotizia(t("menu.goToAlbum.none"));
                    return;
                  }
                  const tracce = await ipc.braniAlbum(chiave);
                  cambiandoVista(() => {
                    setQuery("");
                    setPlaylistAperta(null);
                    setAperto({
                      albumKey: chiave,
                      title: brano.album,
                      artist: brano.artist,
                      year: brano.year,
                      genre: null,
                      totalTracks: tracce.length,
                      coverArtHash: brano.coverArtHash,
                    });
                  });
                }),
              },
              {
                etichetta: t("menu.goToArtist"),
                azione: conBrano(async (brano) => {
                  const tutti = artisti.length > 0 ? artisti : await ipc.artisti();
                  const piegato = brano.artist.toLocaleLowerCase();
                  const suo =
                    tutti.find((a) => a.name === brano.artist) ??
                    tutti.find((a) => a.name.toLocaleLowerCase() === piegato);
                  // Un artista che la griglia non ha — una collaborazione
                  // scritta in un modo solo in quel brano — si cerca per nome
                  // invece di non fare niente.
                  if (suo === undefined) {
                    setQuery(brano.artist);
                    return;
                  }
                  cambiandoVista(() => {
                    setQuery("");
                    setPlaylistAperta(null);
                    setAperto(null);
                    setArtistaAperto(suo);
                  });
                }),
              },
              // «Mostra nella cartella» solo su un file. Un brano di catalogo
              // in nessuna cartella sta, e la voce che lo promettesse
              // risponderebbe con un errore a un clic perfettamente
              // ragionevole. Il comando si difende comunque da sé — un comando
              // è una porta — ma una porta che non si apre non va messa in
              // vista.
              // Il brano non trovato nell'elenco — il menù aperto da un
              // pannello che mostra un'altra lista — lascia la voce dov'è:
              // è quel che si faceva prima di distinguere i due casi, e il
              // comando si difende comunque da sé.
              ...(elencoCorrenteRef.current.find((b) => b.id === primo)?.fonte ==
              null
                ? [
                    {
                      etichetta: t("menu.showInFolder"),
                      azione: () => {
                        ipc.branoMostraNellaCartella(primo).catch(segnalaErrore);
                      },
                    },
                  ]
                : []),
              // E il contrario: la pagina pubblica esiste solo per un brano che
              // viene da un catalogo. È il gesto che «Mostra nella cartella»
              // è per un file — «fammi vedere da dove viene» — e per certe
              // licenze non è una curiosità ma una condizione d'uso, per cui
              // qui c'è e in «In riproduzione» c'è scritta per esteso.
              ...(elencoCorrenteRef.current.find((b) => b.id === primo)
                ?.fontePagina != null
                ? [
                    {
                      etichetta: t("track.openPage"),
                      azione: () => {
                        ipc.branoApriPagina(primo).catch(segnalaErrore);
                      },
                    },
                  ]
                : []),
            ];
      setMenu({
        x: e.clientX,
        y: e.clientY,
        voci: [
          ...(primo === undefined
            ? []
            : [
                {
                  etichetta: t("action.radio"),
                  azione: () => {
                    ipc.radio(primo).catch(segnalaErrore);
                  },
                },
              ]),
          {
            etichetta: t("action.playNext"),
            azione: () => {
              ipc.codaDopo(elenco).catch(segnalaErrore);
            },
          },
          {
            etichetta: t("action.enqueue"),
            azione: () => {
              ipc.codaAccoda(elenco).catch(segnalaErrore);
            },
          },
          {
            etichetta: t("action.addToPlaylist"),
            azione: () => setDaAggiungere(elenco),
          },
          ...daUnBrano,
          /*
           * Le due distruttive, in fondo e in quest'ordine.
           *
           * In fondo perché sono le uniche voci del menù che tolgono qualcosa, e
           * la distanza dal puntatore è l'unica protezione che un menù sa dare
           * contro il clic sbagliato. In quest'ordine perché la seconda contiene
           * la prima: chi elimina dal disco toglie anche dalla libreria, e
           * l'ordine inverso metterebbe la più grave sotto il dito per prima.
           *
           * Tutte e due aprono una conferma — i puntini lo dicono — e non perché
           * una domanda renda prudenti: un voto, un preferito e un conteggio
           * d'ascolto costruiti in anni se ne vanno con la riga, e di quello non
           * c'è annulla da offrire. Dove un annulla c'è, come per la coda
           * sostituita, si offre quello e non si chiede niente.
           */
          {
            etichetta: t("menu.removeFromLibrary"),
            azione: () => setDaEliminare({ brani: elenco, dalDisco: false }),
          },
          {
            etichetta: t("menu.deleteFromDisk"),
            azione: () => setDaEliminare({ brani: elenco, dalDisco: true }),
          },
        ],
      });
    },
    [segnalaErrore, artisti],
  );

  /** Il menù di una playlist nella barra di navigazione. */
  const menuPlaylist = useCallback(
    (e: React.MouseEvent, p: Playlist) => {
      e.preventDefault();
      const voci = [
        {
          etichetta: t("action.play"),
          azione: () => {
            ipc
              .playlistBrani(p.id)
              .then((tracce) =>
                tracce.length > 0
                  ? ipc.suona(
                      tracce.map((b) => b.id),
                      0,
                    )
                  : undefined,
              )
              .catch(segnalaErrore);
          },
        },
        {
          etichetta: t("common.delete"),
          azione: () => {
            ipc
              .playlistCancella(p.id)
              .then(() => {
                setPlaylistAperta((prima) =>
                  prima && prima.id === p.id ? null : prima,
                );
                return ricaricaPlaylist();
              })
              .catch(segnalaErrore);
          },
        },
      ];
      // Una playlist automatica non si rinomina: la chiave è il nome, e
      // cambiarlo ne farebbe una nuova che le regole del vecchio database non
      // conoscono più.
      if (!p.isSmart) {
        voci.splice(1, 0, {
          etichetta: t("menu.rename"),
          azione: () => setDaRinominare(p),
        });
      } else {
        voci.splice(1, 0, {
          etichetta: t("menu.editRules"),
          azione: () => setRegoleAperte({ playlist: p }),
        });
      }
      // L'esportazione vale per tutte e due: una playlist intelligente si
      // esporta con i brani che ha **adesso**, che è quel che un M3U può
      // rappresentare — le regole non attraversano il formato, e portarle via
      // fingendo di sì sarebbe peggio che non esportarla.
      voci.push({
        etichetta: t("menu.exportFile"),
        azione: () => void esportaPlaylist(p),
      });
      setMenu({ x: e.clientX, y: e.clientY, voci });
    },
    [ricaricaPlaylist, segnalaErrore],
  );

  /** Il menù di una scheda album: i brani si chiedono al momento del comando. */
  const menuAlbum = useCallback(
    (e: React.MouseEvent, chiave: string) => {
      e.preventDefault();
      const conBrani = (comando: (ids: number[]) => Promise<void>) => () => {
        ipc
          .braniAlbum(chiave)
          .then((tracce) => comando(tracce.map((b) => b.id)))
          .catch(segnalaErrore);
      };
      setMenu({
        x: e.clientX,
        y: e.clientY,
        voci: [
          // Il seme è la **prima traccia** e non un brano a caso del disco: è
          // quella che chi apre un album sente per prima, quindi è quella di
          // cui sta chiedendo «portami dove porta questo».
          {
            etichetta: t("action.radio"),
            azione: conBrani((ids) => {
              const primo = ids[0];
              return primo === undefined
                ? Promise.resolve()
                : ipc.radio(primo);
            }),
          },
          { etichetta: t("action.playNext"), azione: conBrani(ipc.codaDopo) },
          { etichetta: t("action.enqueue"), azione: conBrani(ipc.codaAccoda) },
        ],
      });
    },
    [segnalaErrore],
  );

  /** Il menù dell'ordinamento: quattro voci, dove sta il bottone. */
  const menuOrdine = useCallback((e: React.MouseEvent) => {
    const riquadro = e.currentTarget.getBoundingClientRect();
    setMenu({
      x: riquadro.left,
      y: riquadro.bottom + 4,
      voci: ordinamenti().map(([chiave, etichetta]) => ({
        etichetta,
        azione: () => setOrdine(chiave),
      })),
    });
  }, []);

  /**
   * Cambia la valutazione, ottimisticamente come il preferito.
   *
   * La riga si aggiorna prima della scrittura: cinque stelle che aspettano il
   * disco per accendersi sembrano un clic non registrato, e il rimedio — se la
   * scrittura fallisce — è ricaricare la vista, che rimette il valore vero.
   */
  const cambiaVoto = useCallback(
    async (brano: Brano, stelle: number) => {
      const aggiorna = (elenco: Brano[]) =>
        elenco.map((b) => (b.id === brano.id ? { ...b, rating: stelle } : b));
      elencoBrani.aggiorna(aggiorna);
      setBraniAperto(aggiorna);
      setBraniPlaylist(aggiorna);
      // E il brano che suona, che negli elenchi c'è ma è un'altra copia: quella
      // la tiene il nucleo, e `caricaVista` non la sfiora. Vedi `ritoccaBrano`.
      riproduzione.ritoccaBrano(brano.id, { rating: stelle });
      try {
        await ipc.valutazione(brano.id, stelle);
      } catch (e) {
        segnalaErrore(e);
        // La copia del nucleo si rimette a mano: `caricaVista` rimedia agli
        // elenchi, non a lei, e senza questa riga il valore sbagliato resterebbe
        // nella barra fino al prossimo evento della riproduzione.
        riproduzione.ritoccaBrano(brano.id, { rating: brano.rating });
        await caricaVista();
      }
    },
    [caricaVista, riproduzione.ritoccaBrano],
  );

  const cambiaPreferito = useCallback(
    async (brano: Brano) => {
      const valore = !brano.liked;
      const aggiorna = (elenco: Brano[]) =>
        elenco.map((b) => (b.id === brano.id ? { ...b, liked: valore } : b));
      // Ottimistico: il cuoricino deve rispondere al dito, non al disco. Se la
      // scrittura fallisce si ricarica, e la riga torna com'era.
      elencoBrani.aggiorna(aggiorna);
      setBraniAperto(aggiorna);
      setBraniPlaylist(aggiorna);
      // Il cuore della barra sta nel brano del nucleo, non negli elenchi: senza
      // questa riga restava fermo al valore di quando il brano era partito, e la
      // prima pausa — che è la prima occasione in cui il nucleo ricompone lo
      // stato — lo faceva saltare al valore vero.
      riproduzione.ritoccaBrano(brano.id, { liked: valore });
      try {
        await ipc.preferito(brano.id, valore);
        setAvvio((prima) =>
          prima
            ? {
                ...prima,
                numeri: {
                  ...prima.numeri,
                  liked: prima.numeri.liked + (valore ? 1 : -1),
                },
              }
            : prima,
        );
      } catch (e) {
        segnalaErrore(e);
        riproduzione.ritoccaBrano(brano.id, { liked: brano.liked });
        await caricaVista();
      }
    },
    [caricaVista, riproduzione.ritoccaBrano],
  );

  /**
   * Toglie dei brani dalla libreria, e — se lo si è chiesto — anche dal disco.
   *
   * # Ottimistico, con rimedio
   *
   * Le righe spariscono prima che il nucleo risponda, come il cuore e le
   * stelle: un elenco che resta fermo dopo un «Elimina» si legge come un clic
   * non registrato, e chi riprova cancella due volte. Se qualcosa va storto la
   * verità torna dal nucleo — e va **chiesta**, non dedotta: un'eliminazione che
   * si ferma a metà (due file nel Cestino, il terzo aperto da un altro
   * programma) lascia un risultato che da qui non si può ricostruire.
   *
   * # Tre elenchi e una selezione
   *
   * Gli stessi tre che `cambiaVoto` tiene allineati, più la selezione: lasciarci
   * dentro l'identificativo di un brano cancellato vorrebbe dire una barra che
   * dice «4 brani» sopra un elenco che ne mostra tre, e i comandi di quella
   * barra andrebbero a chiedere al nucleo una riga che non c'è.
   */
  const elimina = useCallback(
    async (brani: number[], dalDisco: boolean) => {
      const via = new Set(brani);
      const senza = (elenco: Brano[]) => elenco.filter((b) => !via.has(b.id));
      // L'album aperto resta senza niente? Si guarda **prima** di togliere:
      // dopo, l'elenco non sa più cosa conteneva.
      const svuotaLAlbum =
        braniAperto.length > 0 && braniAperto.every((b) => via.has(b.id));
      elencoBrani.aggiorna(senza);
      setBraniAperto(senza);
      setBraniPlaylist(senza);
      setSelezione((prima) => {
        const dopo = new Set(prima);
        for (const id of via) dopo.delete(id);
        return dopo;
      });
      try {
        if (dalDisco) {
          const quanti = await ipc.braniElimina(brani);
          setNotizia(t("delete.done.disk", { brani: brani_(quanti) }));
        } else {
          const esito: Cancellazione = await ipc.braniTogli(brani);
          setNotizia(
            esito.torneranno > 0
              ? t("delete.done.back", {
                  brani: brani_(esito.tolti),
                  n: esito.torneranno,
                })
              : t("delete.done", { brani: brani_(esito.tolti) }),
          );
        }
        // Un disco rimasto senza tracce non è una schermata: è una copertina
        // sopra il vuoto, con un titolo che promette qualcosa da ascoltare. Si
        // torna alla griglia, che è quel che c'è ancora da guardare. Dopo la
        // risposta e non prima: se l'eliminazione fallisce, l'album è ancora
        // pieno e uscirne sarebbe un movimento per niente.
        if (aperto && svuotaLAlbum) {
          cambiandoVista(() => setAperto(null));
        }
        // La griglia degli album, quella degli artisti, i ripiani della Home:
        // l'elenco dei brani se l'è già sistemato qui sopra, loro no.
        ricaricaContorno();
        // E il numero accanto a ogni playlist nella barra laterale: è di
        // `playlistElenco`, e un brano cancellato esce anche dalle playlist.
        // Qui e non dentro `ricaricaContorno` per una ragione stupida ma vera:
        // `ricaricaPlaylist` nasce trecento righe più in giù, e nominarla lassù
        // la leggerebbe prima che esista.
        void ricaricaPlaylist();
        // I contatori della barra: sono di `avvio`, e né `ricaricaContorno` né
        // `caricaVista` li sfiorano.
        await ricarica();
      } catch (e) {
        segnalaErrore(e);
        await caricaVista();
        // `caricaVista` rifà gli elenchi impaginati, non questi due: hanno
        // effetti loro, legati all'album e alla playlist aperti, che un errore
        // non fa scattare.
        if (aperto) {
          ipc.braniAlbum(aperto.albumKey).then(setBraniAperto).catch(segnalaErrore);
        }
        if (playlistAperta) {
          ipc
            .playlistBrani(playlistAperta.id)
            .then(setBraniPlaylist)
            .catch(segnalaErrore);
        }
      }
    },
    [
      elencoBrani.aggiorna,
      aperto,
      braniAperto,
      playlistAperta,
      ricarica,
      ricaricaContorno,
      ricaricaPlaylist,
      caricaVista,
      segnalaErrore,
    ],
  );

  /**
   * L'elenco che si sta guardando: quello su cui agiscono selezione e
   * riproduzione.
   *
   * Serve in tre posti — la selezione a intervallo, «seleziona tutto» e i
   * comandi della barra — e derivarlo tre volte vorrebbe dire tre occasioni di
   * dimenticare un ramo quando nascerà una vista nuova.
   */
  const elencoCorrente: Brano[] = playlistAperta
    ? braniPlaylist
    : aperto
      ? braniAperto
      : brani;
  // Durante il disegno e non in un effetto: chi legge questo `ref` lo fa a un
  // clic, cioè sempre dopo che il disegno è finito, e un effetto lo
  // aggiornerebbe un fotogramma più tardi senza guadagnarci niente.
  elencoCorrenteRef.current = elencoCorrente;

  /**
   * La selezione multipla.
   *
   * Ctrl aggiunge o toglie una riga, Maiusc prende l'intervallo dall'ultima
   * toccata. Un clic normale **non** seleziona: su una lista di brani il clic
   * singolo non ha un significato ovvio — chi lo usa per «apri» vede partire
   * una canzone che non voleva — e riservarlo alla selezione renderebbe
   * impossibile cliccare senza conseguenze.
   */
  const seleziona = useCallback(
    (e: React.MouseEvent, indice: number) => {
      const brano = elencoCorrente[indice];
      if (!brano) return;
      if (e.shiftKey && ancora !== null) {
        e.preventDefault();
        const da = Math.min(ancora, indice);
        const a = Math.max(ancora, indice);
        setSelezione(
          new Set(elencoCorrente.slice(da, a + 1).map((b) => b.id)),
        );
        return;
      }
      if (e.ctrlKey || e.metaKey) {
        e.preventDefault();
        setSelezione((prima) => {
          const dopo = new Set(prima);
          if (!dopo.delete(brano.id)) dopo.add(brano.id);
          return dopo;
        });
        setAncora(indice);
        return;
      }
      // Un clic senza modificatori con una selezione aperta la chiude: è il modo
      // di uscirne senza cercare un bottone.
      if (selezione.size > 0) setSelezione(new Set());
    },
    [elencoCorrente, ancora, selezione.size],
  );

  /**
   * Spazio sulla riga col fuoco: aggiunge o toglie quella riga dalla selezione.
   *
   * È il Ctrl+clic detto da tastiera, e non una seconda regola: `seleziona` ha
   * bisogno di un evento del puntatore per leggere i modificatori, mentre da
   * tastiera il modificatore **è** il tasto. Senza questo non c'era modo di
   * costruire una selezione multipla senza mouse, cioè la barra di selezione e
   * tutti i suoi comandi erano inaccessibili da tastiera.
   */
  const alternaSelezione = useCallback(
    (indice: number) => {
      const brano = elencoCorrente[indice];
      if (!brano) return;
      setSelezione((prima) => {
        const dopo = new Set(prima);
        if (!dopo.delete(brano.id)) dopo.add(brano.id);
        return dopo;
      });
      setAncora(indice);
    },
    [elencoCorrente],
  );

  // La selezione non sopravvive al cambio di elenco: cinquanta brani scelti fra
  // i Preferiti non significano niente dentro un album, e tenerli farebbe agire
  // i comandi su righe che non si vedono più.
  useEffect(() => {
    setSelezione(new Set());
    setAncora(null);
  }, [vista, query, aperto, playlistAperta, artistaAperto]);

  const selezionati = useMemo(
    () => elencoCorrente.filter((b) => selezione.has(b.id)).map((b) => b.id),
    [elencoCorrente, selezione],
  );

  /**
   * Fa partire l'elenco che si sta guardando, dal brano scelto.
   *
   * Un'identità sola per tutte le viste, e non una lambda dentro ogni `map`:
   * `elencoCorrente` **è** già la lista giusta in ognuna delle quattro — la
   * ricerca, la playlist, l'album, i brani — e una prop nuova per riga a ogni
   * disegno annullerebbe il `memo` di `RigaBrano` riga per riga.
   */
  const suonaQui = useCallback(
    (indice: number) => void suonaDa(elencoCorrente, indice),
    [suonaDa, elencoCorrente],
  );

  /** Toglie dalla playlist aperta la riga in quella posizione. */
  const togliDallaPlaylist = useCallback(
    (indice: number) => {
      if (!playlistAperta) return;
      ipc
        .playlistTogli(playlistAperta.id, indice)
        .then(aggiornaPlaylist)
        .catch(segnalaErrore);
    },
    [playlistAperta, aggiornaPlaylist, segnalaErrore],
  );

  /**
   * Il riordino dentro una playlist.
   *
   * Due stati e non uno: quale riga si sta portando in giro, e su quale
   * cadrebbe adesso. Il secondo esiste per **mostrarlo** — il pannello della
   * coda trascinava già senza dire dove sarebbe finita la riga, ed era il
   * difetto che il brief chiamava «nessun indicatore di rilascio».
   */
  const [trascinata, setTrascinata] = useState<number | null>(null);
  const [mirata, setMirata] = useState<number | null>(null);

  const spostaNellaPlaylist = useCallback(
    (da: number, a: number) => {
      if (!playlistAperta || da === a) return;
      // Il taglio qui e non nel nucleo: `Alt+↑` sulla prima riga e `Alt+↓`
      // sull'ultima sono gesti legittimi che non devono fare niente, non
      // errori da mostrare.
      if (a < 0 || a >= braniPlaylist.length) return;
      // Ottimistico come il cuore: la riga si muove sotto il dito, e se la
      // scrittura fallisce l'elenco si rilegge dalla playlist che torna.
      setBraniPlaylist((prima) => {
        const dopo = [...prima];
        const [presa] = dopo.splice(da, 1);
        if (presa) dopo.splice(a, 0, presa);
        return dopo;
      });
      ipc
        .playlistRiordina(playlistAperta.id, da, a)
        .then(aggiornaPlaylist)
        .catch((e: unknown) => {
          segnalaErrore(e);
          ipc
            .playlistBrani(playlistAperta.id)
            .then(setBraniPlaylist)
            .catch(segnalaErrore);
        });
    },
    [playlistAperta, braniPlaylist.length, aggiornaPlaylist, segnalaErrore],
  );

  const presa = useCallback((indice: number | null) => {
    setTrascinata(indice);
    if (indice === null) setMirata(null);
  }, []);

  const lascia = useCallback(
    (indice: number) => {
      if (trascinata !== null) spostaNellaPlaylist(trascinata, indice);
      setTrascinata(null);
      setMirata(null);
    },
    [trascinata, spostaNellaPlaylist],
  );

  /**
   * Il menù su una riga, che sa della selezione.
   *
   * Il tasto destro su una riga **che fa parte della selezione** agisce su tutta
   * la selezione; su una riga fuori dalla selezione agisce su quella sola, e non
   * la cambia. È la convenzione di ogni gestore di file, ed è l'unica che non
   * sorprende: il contrario — un menù che agisce sempre su una riga sola —
   * renderebbe la selezione multipla utile solo dalla barra.
   */
  const menuSuSelezione = useCallback(
    (e: React.MouseEvent, elenco: number[]) => {
      const dentro = elenco.some((id) => selezione.has(id));
      apriMenu(e, dentro && selezionati.length > 0 ? selezionati : elenco);
    },
    [apriMenu, selezione, selezionati],
  );

  /** Le scorciatoie. La mappa completa sta in `tastiera.ts`. */
  useScorciatoie(
    {
      durataMs: riproduzione.stato.durataMs,
      alterna: () => {
        ipc.alterna().catch(segnalaErrore);
      },
      cerca: () => {
        // Cercare da una pagina che non è una destinazione non ha un campo dove
        // atterrare: si torna prima in libreria, che è la cosa che chi preme «/»
        // sta chiedendo.
        if (vista === "impostazioni" || vista === "importazioni") vaiA("brani");
        // E si esce dallo schermo intero, per lo stesso motivo detto altrimenti:
        // là l'intestazione non è disegnata, quindi il campo dove atterrare non
        // esiste e il fuoco cadrebbe sul `<body>`.
        setGrande(false);
        window.requestAnimationFrame(() => campoRicerca()?.focus());
      },
      vaiA: (ms) => {
        ipc.vaiA(ms).catch(segnalaErrore);
      },
      inRiproduzione: () => {
        if (riproduzione.stato.brano) setGrande((prima) => !prima);
      },
      // L'altro schermo intero, quello della finestra: la riga qui sopra riempie
      // la finestra col brano, questa toglie di mezzo il resto del desktop. Lo
      // stato che torna non si tiene da questa parte — a disegnarlo è la barra
      // del titolo, che sta fuori da `App` e se lo richiede da sé quando la
      // pagina cambia misura.
      schermoIntero: () => {
        ipc.finestraSchermoIntero().catch(segnalaErrore);
      },
      // Lo zoom della finestra. Passa un verso e non un numero: la scala sta
      // nel nucleo, e ai due estremi non succede niente — vedi
      // `preferenze::zoom_al_gradino` sul perché non gira.
      zoom: cambiaZoom,
      // La via di ritorno. Non porta un numero nemmeno lei: vedi sopra.
      zoomNormale,
      // Alterna, come `inRiproduzione`: premuta due volte riporta dov'era, che è
      // quel che ci si aspetta da una scorciatoia che apre una pagina.
      importazioni: () => {
        vaiA(vista === "importazioni" ? "brani" : "importazioni");
      },
      // Da qualunque pagina, senza passare dalle impostazioni: è il gesto che
      // questo flusso esiste per rendere breve, e finora costava tre clic.
      // «qualunque»: la scorciatoia non viene da nessuna scheda, quindi non c'è
      // un servizio da suggerire e la finestrella li nomina tutti e due.
      incollaLink: () => setImportandoLink(true),
      chiudi: () => {
        if (grande) {
          setGrande(false);
          return true;
        }
        if (menu) {
          setMenu(null);
          return true;
        }
        if (selezione.size > 0) {
          setSelezione(new Set());
          return true;
        }
        if (query.length > 0) {
          setQuery("");
          return true;
        }
        // Un livello per pressione, nell'ordine in cui ci si è entrati. Prima
        // erano una riga sola che azzerava tutti e due, e da un album aperto
        // dentro un artista Escape saltava la pagina dell'artista — cioè
        // riportava due passi indietro chi ne aveva chiesto uno.
        if (aperto) {
          setAperto(null);
          return true;
        }
        if (artistaAperto) {
          setArtistaAperto(null);
          return true;
        }
        return false;
      },
    },
    scorciatoie,
  );

  /**
   * Il trascinamento sulla finestra.
   *
   * `dragDropEnabled` è acceso in `tauri.conf.json` da sempre e non aveva
   * nessun gestore: la finestra accettava i file e non ne faceva niente. Una
   * cartella diventa una cartella sorvegliata, un `.aeskin` una skin installata,
   * e in tutti e due i casi si atterra nella sezione di Impostazioni che
   * mostra il risultato — perché un'azione che avviene fuori dallo schermo è
   * un'azione che sembra non essere avvenuta.
   *
   * Quale percorso è cosa lo dice il nucleo (`trascinati.rs`). Qui prima si
   * provava come cartella tutto quel che non era una skin, e un `.m3u8` o un
   * MP3 lasciati cadere finivano fra le cartelle sorvegliate. Adesso una
   * playlist apre l'importazione, i brani che la libreria conosce vanno in
   * coda, e dei file che non conosce si dice cosa fare invece di tacere.
   */
  useAscolto<{ paths: string[] }>("tauri://drag-drop", ({ paths: arrivati }) => {
    if (arrivati.length === 0) return;
    ipc
      .smistaTrascinati(arrivati)
      .then((smistati) => {
        const skinLasciata = smistati.skin[0];
        if (skinLasciata !== undefined) {
          // `vaiA` e non `setVista`: quello nudo lasciava in piedi l'album, la
          // playlist e la ricerca di prima, cioè uno stato in cui l'intestazione
          // e il corpo rispondono a due domande diverse.
          vaiA("impostazioni");
          setSezione("aspetto");
          ipc
            .skinInstalla(skinLasciata)
            .then((installata) => scegliSkin(installata.id))
            .catch(segnalaErrore);
          return;
        }
        // Una playlist apre la sua finestrella: è un gesto con delle scelte
        // dentro, e non si fa a metà insieme ad altro.
        const playlistLasciata = smistati.playlist[0];
        if (playlistLasciata !== undefined) {
          setFilePlaylist(playlistLasciata);
          return;
        }
        if (smistati.brani.length > 0) {
          const quanti = smistati.brani.length;
          ipc
            .codaAccoda(smistati.brani)
            .then(() =>
              setNotizia(
                t("drop.queued", {
                  brani:
                    quanti === 1
                      ? t("format.tracks.uno")
                      : t("format.tracks", { n: quanti }),
                }),
              ),
            )
            .catch(segnalaErrore);
        }
        // Col primo avvio che chiede ancora dove sta la musica, le cartelle
        // vanno nel suo elenco: scriverle da qui voleva dire perderle al suo
        // «Continua», che scrive le spuntate e basta. A lettura partita o
        // finita con dei brani «Continua» non c'è più, e la strada di sempre —
        // che unisce — non perde niente.
        if (
          smistati.cartelle.length > 0 &&
          primoVisibile &&
          scansione === null &&
          (avvio?.numeri.tracks ?? 0) === 0
        ) {
          setLasciateAlPrimo((prima) => [
            ...new Set([...prima, ...smistati.cartelle]),
          ]);
          return;
        }
        if (smistati.cartelle.length > 0 && avvio) {
          vaiA("impostazioni");
          setSezione("cartelle");
          const unite = [...new Set([...avvio.cartelle, ...smistati.cartelle])];
          ipc.impostaCartelle(unite).then(ricarica).catch(segnalaErrore);
          return;
        }
        // Dopo i brani accodati, che hanno già la loro notizia: questa
        // la sostituirebbe, e i due casi insieme sono rari abbastanza da non
        // meritare una frase che li tenga tutti e due.
        if (smistati.brani.length > 0) return;
        if (smistati.fuoriLibreria > 0) setNotizia(t("drop.notInLibrary"));
        else if (smistati.ignorati > 0) setNotizia(t("drop.nothing"));
      })
      .catch(segnalaErrore);
  });

  const numeri = avvio?.numeri;
  const senzaCartelle = avvio !== null && avvio.cartelle.length === 0;
  const vuota = numeri !== undefined && numeri.tracks === 0;
  const inAscolto = riproduzione.stato.brano?.id ?? null;
  const guasto = errore ?? guastoDa(riproduzione.errore);
  const messaggio = guasto?.testo ?? null;
  /**
   * Il guasto in fascia si può riprovare.
   *
   * Solo quello che viene dal lettore: `errore` è già una stringa — di lui non
   * resta il codice, e senza codice non si sa se riprovare ha senso — mentre
   * `riproduzione.errore` arriva intero dal nucleo. È anche l'unico dei due che
   * abbia un gesto da offrire: la cartella di rete che sparisce a metà brano.
   */
  const ritentabile = errore === null && eRitentabile(riproduzione.errore);

  /**
   * Quale elenco sta aspettando la sua prima pagina.
   *
   * Diviso per vista e non più uno solo: `elencoBrani` ed `elencoAlbum` sono
   * due richieste indipendenti, e un flag condiviso metterebbe i segnaposto
   * della griglia sopra un elenco di brani già arrivato.
   */
  const caricandoBrani = elencoBrani.caricando || inRitardo;
  const caricandoAlbum = elencoAlbum.caricando;

  // Memoizzato: sta nelle dipendenze di `contesto`, e un oggetto nuovo a ogni
  // disegno rendeva quel `useMemo` una spesa senza effetto.
  const conteggi: Partial<Record<Vista, number>> = useMemo(
    () =>
      numeri
        ? {
            album: numeri.albums,
            artisti: numeri.artists,
            brani: numeri.tracks,
            // Le radici, non i brani: l'albero si costruisce alla prima
            // apertura della vista, quindi finché nessuno l'ha aperta un
            // conteggio di cartelle non esiste — e quello che esiste è pure
            // quello giusto, perché dice quante cartelle sorvegliate ci sono.
            cartelle: avvio?.cartelle.length ?? 0,
            preferiti: numeri.liked,
          }
        : {},
    [numeri, avvio],
  );

  const etichettaOrdine =
    ordinamenti().find(([c]) => c === ordine)?.[1] ?? t("sort.shelf");

  /**
   * Tutto quel che i widget sanno del mondo, in un oggetto solo.
   *
   * È l'unico imbuto per i quarantotto `useState` di questo componente. Prima
   * scendevano a mano: `Lettore` prende sei prop, `Colonna` sei, `Coda` tre, e
   * aggiungere un widget voleva dire farne passare un'altra attraverso tre
   * livelli che non la usavano. Qui la lista si scrive una volta.
   */
  const contesto: ContestoWidget = useMemo(
    () => ({
      stato: riproduzione.stato,
      vista,
      playlistAperta,
      inLibreria: !cercando && !aperto && !artistaAperto,
      conteggi,
      playlist,
      colonnaAperta,
      codaAperta,
      grande,
      selezionati,
      tuttiSelezionati: selezionati.length === elencoCorrente.length,
      onVista: vaiA,
      onPlaylist: (p) => {
        cambiandoVista(() => {
          setQuery("");
          setAperto(null);
          setArtistaAperto(null);
          // Da una pagina che non è una destinazione bisogna anche **uscire**:
          // `corpo()` guarda `vista` per prima, quindi senza questa riga la
          // playlist si accendeva nella barra e la pagina restava quella delle
          // impostazioni. Si atterra sui Brani perché è la vista che una
          // playlist somiglia di più, ed è quella che si ritrova chiudendola.
          if (vista === "impostazioni" || vista === "importazioni") {
            setVista("brani");
          }
          setPlaylistAperta(p);
        });
      },
      onMenuPlaylist: menuPlaylist,
      onNuovaPlaylist: () => setCreandoPlaylist(true),
      onNuovaSmart: () => setRegoleAperte({ playlist: null }),
      onImportaFile: () => void scegliFilePlaylist(),
      // Fuori, nel browser di sistema, e per nome: l'indirizzo lo conosce solo
      // il nucleo, che ne tiene un elenco chiuso — vedi `apri_documento`.
      onDona: () => {
        ipc.apriDocumento("donazioni").catch(segnalaErrore);
      },
      onColonna: colonnaAMano,
      onCoda: setCodaAperta,
      onGrande: () => setGrande(true),
      onPreferito: cambiaPreferito,
      onVoto: cambiaVoto,
      onErrore: segnalaErrore,
      onSelezioneRiproduci: () => {
        const scelti = elencoCorrente.filter((b) => selezione.has(b.id));
        void suonaDa(scelti, 0);
      },
      onSelezioneDopo: () => {
        ipc.codaDopo(selezionati).catch(segnalaErrore);
      },
      onSelezioneAccoda: () => {
        ipc.codaAccoda(selezionati).catch(segnalaErrore);
      },
      onSelezionePlaylist: () => setDaAggiungere(selezionati),
      onSelezioneTuttiOAnnulla: () =>
        setSelezione(
          selezionati.length === elencoCorrente.length
            ? new Set()
            : new Set(elencoCorrente.map((b) => b.id)),
        ),
      onSelezioneChiudi: () => setSelezione(new Set()),
    }),
    [
      riproduzione.stato,
      vista,
      playlistAperta,
      cercando,
      aperto,
      artistaAperto,
      conteggi,
      playlist,
      colonnaAperta,
      codaAperta,
      grande,
      selezionati,
      elencoCorrente,
      selezione,
      vaiA,
      menuPlaylist,
      cambiaPreferito,
      cambiaVoto,
      segnalaErrore,
      suonaDa,
    ],
  );

  /**
   * L'intestazione, che cambia con quel che si sta guardando.
   *
   * # L'ordine dei casi è quello di `corpo()`, e deve restarlo
   *
   * Quale pagina si sta guardando non è uno stato solo: è la precedenza fra
   * `vista`, `query`, `playlistAperta`, `aperto` e `artistaAperto`. Finché
   * quella precedenza era scritta due volte — qui e in `corpo()` — le due
   * potevano divergere, e divergevano: aprire un album dalla pagina di un
   * artista dava le tracce dell'album sotto il titolo dell'artista, senza
   * copertina, senza «Riproduci», e con un tasto «‹ Artisti» che portava
   * **avanti** nella pagina dell'album invece che indietro.
   *
   * Aggiungendo un caso, va aggiunto nello stesso punto di tutte e due.
   *
   * # Perché la Home sta in fondo e non in cima
   *
   * Era il primo caso di tutti e due, ed è la posizione sbagliata per una
   * destinazione. Sopra `cercando` voleva dire che dalla schermata su cui
   * l'applicazione apre la ricerca non funzionava — contro la regola scritta
   * sopra `elencoBrani`, «quel che si sta cercando è ciò che si vuole vedere».
   * E sopra `aperto` voleva dire che un disco aperto da un ripiano della Home
   * avrebbe rimesso la Home: `apriAlbum` scrive `aperto` e lascia `vista` dov'è.
   * Le pagine di servizio — importazioni, impostazioni — restano in cima perché
   * là non c'è niente da cercare e niente da aprire.
   */
  const testa = () => {
    if (vista === "importazioni") {
      return (
        <TestaImportazioni
          importazioni={importazioni}
          onIncollaLink={() => setImportandoLink(true)}
        />
      );
    }
    if (vista === "impostazioni") {
      return (
        <Intestazione
          titolo={t("page.settings.title")}
          sottotitolo={t("page.settings.sub")}
        />
      );
    }
    /* Senza `query`: la casella dell'intestazione cerca **in libreria**, e
       questa pagina cerca fuori. Due caselle vicine che cercano in due posti
       diversi sono la cosa che fa digitare nella sbagliata; quella di qui sta
       dentro la pagina, accanto al tasto che la fa partire. */
    if (vista === "esplora") {
      return (
        <Intestazione
          occhiello={t("page.explore.eyebrow")}
          titolo={t("page.explore.title")}
          sottotitolo={t("page.explore.sub")}
        />
      );
    }
    if (cercando) {
      return (
        <Intestazione
          occhiello={t("page.search.eyebrow")}
          titolo={`«${query.trim()}»`}
          /* Il conteggio del nucleo, non la lunghezza dell'elenco in mano:
             quello diceva «60 risultati» per una ricerca che ne aveva
             trecento, cioè il limite travestito da numero. Finché non è
             arrivato non si scrive niente — un numero provvisorio che poi
             cambia è peggio di nessun numero. */
          sottotitolo={
            risultati === null
              ? t("common.loading")
              : t("page.search.results", { n: risultati })
          }
          query={query}
          onQuery={setQuery}
        />
      );
    }
    if (playlistAperta) {
      return (
        <Intestazione
          occhiello={
            playlistAperta.isSmart
              ? t("page.playlist.smart")
              : t("page.playlist")
          }
          titolo={playlistAperta.name}
          sottotitolo={`${brani_(playlistAperta.tracks)}${
            playlistAperta.durationMs > 0
              ? ` · ${durata(playlistAperta.durationMs)}`
              : ""
          }`}
          query={query}
          onQuery={setQuery}
          azioni={
            <button
              type="button"
              className="pillola btn-accent"
              disabled={braniPlaylist.length === 0 || !riproduzione.disponibile}
              onClick={() => void suonaDa(braniPlaylist, 0)}
            >
              <Icona nome="i-play" dim={14} />
              {t("action.play")}
            </button>
          }
        />
      );
    }
    if (aperto) {
      return (
        <Intestazione
          occhiello={t("page.album")}
          titolo={titoloAlbum(aperto.title)}
          sottotitolo={`${nomeArtista(aperto.artist)}${aperto.year ? ` · ${aperto.year}` : ""} · ${brani_(
            aperto.totalTracks,
          )}${aperto.genre ? ` · ${aperto.genre}` : ""}`}
          /* L'unica pagina che ha un'immagine sua, e per questo la porta:
             «Album» senza la copertina è un titolo, con la copertina è un
             disco. Piena, non miniatura — a novantasei pixel su uno schermo a
             150% la miniatura da 160 è già al limite. */
          copertina={
            <Copertina
              hash={aperto.coverArtHash}
              titolo={titoloAlbum(aperto.title)}
              classe="hero-art"
              piena
            />
          }
          query={query}
          onQuery={setQuery}
          azioni={
            <>
              {/* Torna da dove si è entrati, e lo dice. Chiudere l'album lascia
                  in piedi quel che c'era sotto — è `apriAlbum(null)` in tutti e
                  tre i casi — quindi l'unica cosa che cambia è l'etichetta, che
                  deve cambiare: un tasto che dice «Album» e riporta alla pagina
                  di un artista è il tasto sbagliato, e da quando anche la Home
                  ha un ripiano di dischi i posti da cui si entra sono tre. */}
              <button
                type="button"
                className="pillola btn-ghost"
                onClick={() => apriAlbum(null)}
              >
                <Icona nome="i-chev-l" dim={14} />
                {artistaAperto
                  ? nomeArtista(artistaAperto.name)
                  : vista === "home"
                    ? t("nav.home")
                    : t("page.album")}
              </button>
              <button
                type="button"
                className="pillola btn-accent"
                disabled={braniAperto.length === 0 || !riproduzione.disponibile}
                onClick={() => void suonaDa(braniAperto, 0)}
              >
                <Icona nome="i-play" dim={14} />
                {t("action.play")}
              </button>
            </>
          }
        />
      );
    }
    if (artistaAperto) {
      return (
        <Intestazione
          occhiello={t("page.artist")}
          titolo={nomeArtista(artistaAperto.name)}
          sottotitolo={t("page.artist.sub", {
            brani: brani_(artistaAperto.tracks),
            album: t("artists.albums", { n: artistaAperto.albums }),
          })}
          query={query}
          onQuery={setQuery}
          azioni={
            <button
              type="button"
              className="pillola btn-ghost"
              onClick={() => apriArtista(null)}
            >
              <Icona nome="i-chev-l" dim={14} />
              {t("nav.artists")}
            </button>
          }
        />
      );
    }
    if (vista === "home") {
      return <TestaHome query={query} onQuery={setQuery} />;
    }
    const titoli: Record<string, [string, string]> = {
      album: [
        t("nav.albums"),
        t("page.library.inLibrary", { n: numeri?.albums ?? 0 }),
      ],
      artisti: [
        t("nav.artists"),
        t("page.library.artistsSub", { n: numeri?.artists ?? 0 }),
      ],
      brani: [
        t("nav.tracks"),
        t("page.library.inLibrary", { n: numeri?.tracks ?? 0 }),
      ],
      // Il sottotitolo conta le radici e non i brani, come il conteggio nella
      // barra: qui il numero dei brani lo saprebbe solo l'albero, che a questo
      // punto potrebbe non essere ancora stato costruito.
      cartelle: [
        t("nav.folders"),
        t("page.folders.sub", { n: avvio?.cartelle.length ?? 0 }),
      ],
      preferiti: [
        t("nav.favorites"),
        t("page.library.liked", { n: numeri?.liked ?? 0 }),
      ],
    };
    const [titolo, sotto] = titoli[vista] ?? [t("page.library"), ""];
    return (
      <Intestazione
        occhiello={t("page.library")}
        titolo={titolo}
        sottotitolo={sotto}
        query={query}
        onQuery={setQuery}
        {...(vista === "brani"
          ? { ordinamento: { etichetta: etichettaOrdine, onApri: menuOrdine } }
          : {})}
      />
    );
  };

  /** Il corpo, cioè quel che sta sotto l'intestazione. */
  const corpo = () => {
    if (vista === "importazioni") {
      return (
        <SchermataImportazioni
          importazioni={importazioni}
          onIncollaLink={() => setImportandoLink(true)}
        />
      );
    }
    if (vista === "esplora") {
      return (
        <Esplora
          stato={esplora}
          onStato={setEsplora}
          onErrore={segnalaErrore}
          onNotizia={setNotizia}
          onSuona={(id) => void suonaSolo(id)}
          onAccoda={(id) => {
            ipc.codaAccoda([id]).catch(segnalaErrore);
          }}
          onLibreriaCambiata={() => {
            /* La libreria è cambiata sotto gli elenchi aperti: un brano di
               catalogo entra in `tracks` e negli aggregati, quindi si affaccia
               in Brani e in Album. Senza questa riga comparirebbe solo al
               riavvio, che è il modo di far credere che il tasto non abbia
               funzionato. */
            elencoBrani.ricarica();
            elencoAlbum.ricarica();
          }}
        />
      );
    }
    if (vista === "impostazioni") {
      return (
        <Impostazioni
          sezione={sezione}
          onSezione={setSezione}
          onNotizia={setNotizia}
          avvio={avvio}
          scansione={scansione}
          esito={esito}
          skin={skin}
          dinamici={skinAttiva?.dynamicTokens.length ?? 0}
          accentoPermesso={skinAttiva?.dynamicAccent ?? false}
          accentoDinamico={accentoDinamico}
          onAccentoDinamico={(attivo) => void cambiaAccentoDinamico(attivo)}
          movimento={skinAttiva?.layout.motion ?? "full"}
          tema={tema}
          onTema={cambiaTema}
          movimentoUtente={movimentoUtente}
          onMovimentoUtente={cambiaMovimentoUtente}
          zoom={zoom ?? 1}
          onZoom={cambiaZoom}
          onZoomNormale={zoomNormale}
          lingua={lingua}
          onLingua={cambiaLingua}
          scorciatoie={scorciatoie}
          onScorciatoie={cambiaScorciatoie}
          onGiro={rifaiGiro}
          onProfiloImportato={dopoProfilo}
          eqAttivo={riproduzione.stato.eqAttivo}
          eqGuadagni={riproduzione.stato.eqGuadagni}
          replaygain={riproduzione.stato.replaygain}
          onReplaygain={(livello) => {
            // Nessun `setStato` qui: il comando manda `riproduzione:stato`, e
            // la linguetta si sposta quando il motore ha davvero cambiato
            // posizione. Anticiparlo mostrerebbe il livello nuovo anche se il
            // salvataggio fallisse.
            ipc.normalizzazione(livello).catch(segnalaErrore);
          }}
          spegnimentoMs={riproduzione.stato.spegnimentoMs}
          onSpegnimento={(minuti) => {
            ipc.spegnimento(minuti).catch(segnalaErrore);
          }}
          autoplay={riproduzione.stato.autoplay}
          onAutoplay={(attivo) => {
            ipc.autoplay(attivo).catch(segnalaErrore);
          }}
          dissolvenzaS={riproduzione.stato.dissolvenzaS}
          onDissolvenza={(secondi) => {
            ipc.dissolvenza(secondi).catch(segnalaErrore);
          }}
          onErrore={segnalaErrore}
          onAggiungiCartella={() => void scegliCartella()}
          onTogliCartella={(c) => void togliCartella(c)}
          onScegliCartellaDownload={() => void scegliCartellaDownload()}
          onCartellaDownloadDiSerie={() => {
            // Stringa vuota: il nucleo toglie la riga e torna alla prima
            // cartella sorvegliata.
            ipc
              .impostaCartellaDownload("")
              .then(ricarica)
              .catch(segnalaErrore);
          }}
          onScansiona={() => void scansiona()}
          onAnnullaScansione={() => {
            // Non si tocca `scansione`: la barra resta finché il nucleo non
            // risponde. Nascondere l'avanzamento qui direbbe «fermata» prima
            // che lo sia — la scansione si ferma alla fine del lotto in corso,
            // e quel lotto può durare ancora qualche secondo.
            ipc.annullaScansione().catch(segnalaErrore);
          }}
          onScegliSkin={(id) => void scegliSkin(id)}
          onAnteprimaSkin={anteprimaSkin}
          onInstallaSkin={() => void installaSkin()}
          onCreaTema={() => {
            setBaseTema(null);
            setCreandoTema(true);
          }}
          onDeriva={(id) => {
            setBaseTema(id);
            setCreandoTema(true);
          }}
          onDisinstallaSkin={(id) => void disinstallaSkin(id)}
          onApriStudio={setStudioAperto}
          onImporta={() => void scegliDatabase()}
          onImportaLink={() => setImportandoLink(true)}
          onImportaAccount={() => setImportandoAccount(true)}
          onVista={vaiA}
          importazioni={importazioni}
          arricchimento={arricchimento}
          avanzaArricchimento={avanzaArricchimento}
          esitoArricchimento={esitoArricchimento}
          onArricchimentoAttiva={(attivo) => {
            ipc
              .arricchimentoAttiva(attivo)
              .then(setArricchimento)
              .catch(segnalaErrore);
          }}
          onArricchimentoAnnulla={() => {
            // Ottimistico su `inCorso`: riscrivere i tag di centinaia di file
            // sono decine di secondi, e senza questo il pulsante resterebbe
            // premibile per tutto quel tempo — con il risultato che chi non
            // vede succedere niente clicca due volte e si prende un
            // `metadata.enrichBusy`.
            setArricchimento((prima) =>
              prima ? { ...prima, inCorso: true } : prima,
            );
            ipc
              .arricchimentoAnnulla()
              .then((esito) => setArricchimento(esito.stato))
              .catch(segnalaErrore);
          }}
          onArricchimentoRiportaNeiFile={() => {
            // Ottimistico come sopra, e qui serve ancora di più: questo riapre
            // in scrittura un file per ogni riga di `enrich_undo`, e su una
            // libreria vera sono decine di secondi in cui non si vede
            // succedere niente.
            setArricchimento((prima) =>
              prima ? { ...prima, inCorso: true } : prima,
            );
            ipc
              .arricchimentoRiportaNeiFile()
              .then((esito) => setArricchimento(esito.stato))
              .catch(segnalaErrore);
          }}
          nuvola={nuvola}
          onNuvolaCollega={() => conNuvola(() => ipc.nuvolaCollega())}
          onNuvolaScollega={() => conNuvola(() => ipc.nuvolaScollega())}
          onNuvolaAttiva={(attivo) =>
            conNuvola(() => ipc.nuvolaAttiva(attivo))
          }
          // Torna subito: quel che succede dopo arriva sull'evento.
          onNuvolaSalva={() => {
            ipc.nuvolaSalva().catch(segnalaErrore);
          }}
          onNuvolaRipristina={() => setRipristinando(true)}
          onNuvolaCredenziali={(id, segreto) =>
            conNuvola(() => ipc.nuvolaCredenziali(id, segreto))
          }
          sincronia={sincronia}
          onSincroniaAttiva={(accesa) =>
            conSincronia(() => ipc.sincroniaAttiva(accesa))
          }
          onSincroniaMagazzino={(dove, cartella) =>
            conSincronia(() => ipc.sincroniaMagazzino(dove, cartella))
          }
          // Aspetta davvero: una passata a vuoto è un'elencazione, e su una
          // cartella condivisa non tocca nemmeno la rete. Quel che torna lo
          // rimanda comunque l'evento, quindi qui basta non perdere l'errore.
          onSincroniaAdesso={() => {
            setSincronia((prima) =>
              prima ? { ...prima, inCorso: true } : prima,
            );
            ipc
              .sincroniaAdesso()
              .catch(segnalaErrore)
              .finally(() => {
                ipc.sincroniaStato().then(setSincronia).catch(segnalaErrore);
              });
          }}
          onSincroniaAccoppia={(id, nome) => {
            ipc
              .sincroniaAccoppia(id, nome)
              .then((dispositivi) =>
                setSincronia((prima) =>
                  prima ? { ...prima, dispositivi } : prima,
                ),
              )
              .catch(segnalaErrore);
          }}
          onSincroniaDimentica={(id) => {
            ipc
              .sincroniaDimentica(id)
              .then((dispositivi) =>
                setSincronia((prima) =>
                  prima ? { ...prima, dispositivi } : prima,
                ),
              )
              .catch(segnalaErrore);
          }}
        />
      );
    }

    if (senzaCartelle && !messaggio) {
      return (
        <div className="vuoto empty-state">
          <span className="empty-icon" aria-hidden="true">
            <Icona nome="i-folder" dim={30} />
          </span>
          <h2>{t("empty.noFolders.title")}</h2>
          <p>{t("empty.noFolders.body")}</p>
          <button
            type="button"
            className="bottone primario btn-accent"
            onClick={() => void scegliCartella()}
          >
            {t("empty.noFolders.cta")}
          </button>
        </div>
      );
    }

    if (vuota && !scansione && !cercando) {
      return (
        <div className="vuoto empty-state">
          <span className="empty-icon" aria-hidden="true">
            <Icona nome="i-scan" dim={30} />
          </span>
          <h2>{t("empty.library.title")}</h2>
          <p>{t("empty.library.body")}</p>
          <button
            type="button"
            className="bottone primario btn-accent"
            onClick={() => void scansiona()}
          >
            {t("empty.library.cta")}
          </button>
        </div>
      );
    }

    if (cercando) {
      // I segnaposto anche qui: la ricerca è impaginata come le altre viste, e
      // fra un tasto premuto e la risposta c'è il ritardo dei 140 ms.
      if (caricandoBrani) return <ElencoFinto />;
      return brani.length === 0 ? (
        <div className="vuoto empty-state">
          <span className="empty-icon" aria-hidden="true">
            <Icona nome="i-search" dim={30} />
          </span>
          <h2>{t("empty.search.title")}</h2>
          <p>{t("empty.search.body")}</p>
        </div>
      ) : (
        <>
          <ElencoBrani
            key="cerca"
            righe={brani}
            scorrevole={contenuto}
            inAscolto={inAscolto}
            suonabile={riproduzione.disponibile}
            selezione={selezione}
            onSuona={suonaQui}
            onPreferito={cambiaPreferito}
            onVoto={cambiaVoto}
            onMenu={menuSuSelezione}
            onSeleziona={seleziona}
            onAlternaSelezione={alternaSelezione}
          />
          <Sentinella pagine={elencoBrani} />
        </>
      );
    }

    if (
      playlistAperta &&
      playlistCaricata === playlistAperta.id &&
      braniPlaylist.length === 0
    ) {
      // Una playlist vuota mostrava l'intestazione di colonna dell'elenco e
      // sotto niente, cioè una tabella senza righe che sembra non caricata.
      // Qui si dice che è vuota, e come si riempie: le due strade sono diverse
      // per le due specie.
      return (
        <div className="vuoto empty-state">
          <span className="empty-icon" aria-hidden="true">
            <Icona nome={playlistAperta.isSmart ? "i-settings" : "i-list"} dim={30} />
          </span>
          <h2>{t("empty.playlist.title")}</h2>
          <p>
            {playlistAperta.isSmart
              ? t("empty.playlist.smart")
              : t("empty.playlist.body")}
          </p>
        </div>
      );
    }

    if (playlistAperta) {
      return (
        <>
          {playlistAperta.isSmart && (
            <p className="nota">
              <Trans
                k="page.playlist.smart.note"
                v={{ adesso: <strong>{t("rules.intro.now")}</strong> }}
              />
            </p>
          )}
          <ElencoBrani
            key="playlist"
            righe={braniPlaylist}
            scorrevole={contenuto}
            inAscolto={inAscolto}
            suonabile={riproduzione.disponibile}
            selezione={selezione}
            onSuona={suonaQui}
            onPreferito={cambiaPreferito}
            onVoto={cambiaVoto}
            onMenu={menuSuSelezione}
            onSeleziona={seleziona}
            onAlternaSelezione={alternaSelezione}
            /* Lo stesso brano può stare due volte in una playlist: la chiave
               porta anche la posizione, o React accoppierebbe le due righe. */
            chiaveConIndice
            conTogli={!playlistAperta.isSmart}
            /* Solo in una playlist a mano: l'appartenenza di una automatica la
               decidono le sue regole, e un ordine deciso qui sarebbe cancellato
               dal primo ricalcolo. */
            {...(playlistAperta.isSmart
              ? {}
              : {
                  onTogli: togliDallaPlaylist,
                  onRiordina: spostaNellaPlaylist,
                  onPresa: presa,
                  onMira: setMirata,
                  onLascia: lascia,
                  mirata,
                  trascinata,
                })}
          />
        </>
      );
    }

    if (aperto) {
      return (
        <ElencoBrani
          key="album-aperto"
          righe={braniAperto}
          scorrevole={contenuto}
          inAscolto={inAscolto}
          suonabile={riproduzione.disponibile}
          selezione={selezione}
          onSuona={suonaQui}
          onPreferito={cambiaPreferito}
          onVoto={cambiaVoto}
          onMenu={menuSuSelezione}
          onSeleziona={seleziona}
          onAlternaSelezione={alternaSelezione}
          /* L'unico posto in cui `#` è il numero del disco: qui la colonna dice
             dove sta il pezzo sulla custodia. */
          numeroTraccia
        />
      );
    }

    if (artistaAperto) {
      // `album` **è** già l'elenco dell'artista: la chiave di `elencoAlbum` lo
      // dice, e il nucleo lo filtra con la stessa chiave con cui raggruppa gli
      // artisti. Prima si filtrava qui la pagina già scaricata, ed era il
      // motivo per cui un artista oltre il quattrocentesimo album dava una
      // griglia vuota.
      if (caricandoAlbum) return <GrigliaFinta />;
      return (
        <>
          <div className="griglia track-grid">
            {album.map((a) => (
              <button
                key={a.albumKey}
                type="button"
                className="scheda list-row"
                onClick={() => apriAlbum(a)}
                onContextMenu={(e) => menuAlbum(e, a.albumKey)}
              >
                <Copertina
                  hash={a.coverArtHash}
                  titolo={titoloAlbum(a.title)}
                />
                <div className="titolo" title={titoloAlbum(a.title)}>
                  {titoloAlbum(a.title)}
                </div>
                <div className="sotto">
                  {a.year ?? ""}
                  {a.year && a.totalTracks > 1 ? " · " : ""}
                  {a.totalTracks > 1 ? brani_(a.totalTracks) : ""}
                </div>
              </button>
            ))}
          </div>
          <Sentinella pagine={elencoAlbum} />
        </>
      );
    }

    if (vista === "home") {
      return (
        <Home
          casa={casa}
          settimana={settimana}
          onSuona={(elenco, indice) => void suonaDa(elenco, indice)}
          onApriRaccolta={(raccolta) => {
            void (async () => {
              await suonaDa(raccolta.brani, 0);
              // Segnata aperta **dopo** che è partita: se il comando di
              // riproduzione fallisce, la raccolta è ancora nuova — e il
              // pallino che dice «non l'hai ancora sentita» dice il vero.
              await ipc.settimanaApri(raccolta.id).catch(segnalaErrore);
              setSettimana((prima) =>
                prima.map((r) =>
                  r.id === raccolta.id ? { ...r, aperta: true } : r,
                ),
              );
            })();
          }}
          onRiprendi={() => {
            // `riprendi` e non `suona`: la coda conservata è già in piedi — il
            // nucleo la rimette all'avvio senza far partire niente — e
            // `suona([brano], 0)` la buttava via per sostituirla con un brano
            // solo. Cioè «riprendi dov'eri» faceva calare il silenzio dove ieri
            // sera la serata continuava.
            //
            // E niente `vaiA` dopo: il salto al segno partiva anche su un brano
            // che stava già suonando, con il numero che la Home aveva letto —
            // cioè, per i primi secondi di un brano nuovo, quello del brano di
            // prima. Il punto da cui aprire lo sceglie il comando, che sa se il
            // motore ha le mani vuote.
            ipc.riprendi().catch(segnalaErrore);
          }}
          onMenu={(e, brano) => menuSuSelezione(e, [brano.id])}
          onApriAlbum={apriAlbum}
          onMenuAlbum={(e, album) => menuAlbum(e, album.albumKey)}
        />
      );
    }

    // Prima del segnaposto qui sotto, e non dopo: le Cartelle non passano da
    // `usePagine` — l'albero se lo chiede da sé, un livello alla volta — quindi
    // `caricandoBrani` parla di un elenco che qui non c'è, e uno scheletro di
    // righe di brani sopra un albero sarebbe l'attesa di qualcos'altro.
    if (vista === "cartelle") {
      return (
        <Cartelle
          radici={avvio?.cartelle ?? []}
          scorrevole={contenuto}
          onMenu={setMenu}
          onErrore={segnalaErrore}
        />
      );
    }

    // Da qui in giù comanda `vista`. Le tre schermate qui sopra — album aperto,
    // playlist, artista — hanno una richiesta propria e un contenuto che resta
    // valido mentre arriva.
    if (vista === "album" ? caricandoAlbum : vista === "artisti" ? artistiInArrivo : caricandoBrani) {
      return vista === "album" || vista === "artisti" ? <GrigliaFinta /> : <ElencoFinto />;
    }

    if (vista === "artisti") {
      return (
        <Artisti
          artisti={artisti}
          /* `apriArtista` e non `setArtistaAperto`: aprire un artista è un
             cambio di pagina come gli altri, e passando dal setter nudo era
             l'unico che avveniva di scatto — mentre la voce «Apri» del menù
             contestuale, che fa la stessa cosa, sfumava. */
          onApri={apriArtista}
          onMenu={(e, a) => {
            e.preventDefault();
            setMenu({
              x: e.clientX,
              y: e.clientY,
              voci: [
                {
                  etichetta: t("menu.open"),
                  azione: () => apriArtista(a),
                },
                {
                  etichetta: t("menu.searchName"),
                  azione: () => setQuery(a.name),
                },
              ],
            });
          }}
        />
      );
    }

    if (vista === "album") {
      return (
        <>
          <div className="griglia track-grid">
            {album.map((a) => (
              <button
                key={a.albumKey}
                type="button"
                className="scheda list-row"
                onClick={() => apriAlbum(a)}
                onContextMenu={(e) => menuAlbum(e, a.albumKey)}
              >
                <Copertina
                  hash={a.coverArtHash}
                  titolo={titoloAlbum(a.title)}
                />
                <div className="titolo" title={titoloAlbum(a.title)}>
                  {titoloAlbum(a.title)}
                </div>
                <div className="sotto" title={nomeArtista(a.artist)}>
                  {nomeArtista(a.artist)}
                  {a.totalTracks > 1 ? ` · ${a.totalTracks}` : ""}
                </div>
              </button>
            ))}
          </div>
          <Sentinella pagine={elencoAlbum} />
        </>
      );
    }

    return (
      <>
        <ElencoBrani
          key="brani"
          righe={brani}
          scorrevole={contenuto}
          inAscolto={inAscolto}
          suonabile={riproduzione.disponibile}
          selezione={selezione}
          onSuona={suonaQui}
          onPreferito={cambiaPreferito}
          onVoto={cambiaVoto}
          onMenu={menuSuSelezione}
          onSeleziona={seleziona}
          onAlternaSelezione={alternaSelezione}
        />
        <Sentinella pagine={elencoBrani} />
      </>
    );
  };

  // Lo Studio occupa la finestra intera, navigazione compresa: è un'altra
  // applicazione dentro la stessa finestra, e tenere la barra della libreria
  // accanto a un editor di skin darebbe due navigazioni che non c'entrano.
  if (studioAperto !== null) {
    return (
      <Studio
        id={studioAperto}
        onEsci={() => {
          setStudioAperto(null);
          // Le skin possono essere cambiate mentre si era là dentro.
          void ricaricaSkin();
        }}
        // «Salva e usa» ridipinge la finestra intera, non solo l'anteprima
        // dentro lo Studio: è la stessa strada di quando si sceglie una skin.
        onInstallata={(id) => void scegliSkin(id)}
        // La chat dello Studio, senza un modello configurato, non promette
        // niente e porta qui: la scheda sta in una finestra che lo Studio
        // nasconde per intero, e dire «configurane uno» senza portarci
        // lascerebbe da cercare.
        onModelli={() => {
          setStudioAperto(null);
          void ricaricaSkin();
          vaiA("impostazioni");
          setSezione("modelli");
        }}
        onErrore={segnalaErrore}
      />
    );
  }

  /*
   * La notizia e l'errore della fascia in cima alla pagina.
   *
   * Una costante e non un pezzo scritto dentro `.dentro`, perché servono in due
   * posti: là, e sopra lo schermo intero — che copre la pagina e la rende
   * `inert`, cioè proprio dove un brano che non si apre falliva senza dirlo.
   * Gli stessi due stati, quindi chiuderne uno lo chiude in tutti e due i posti.
   */
  const notizieDiFascia = (
    <>
      {notizia !== null && (
        <div className="notizia toast-card" role="status">
          <Icona nome="i-check" dim={16} />
          <span>{notizia}</span>
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("common.close")}
            onClick={() => setNotizia(null)}
          >
            <Icona nome="i-x" dim={14} />
          </button>
        </div>
      )}
      {messaggio && (
        <div className="errore toast-card" role="alert">
          <Icona nome="i-alert" dim={16} />
          <span>
            {messaggio}
            {/* Quel che il servizio ha scritto con parole sue: la
                frase del catalogo dice cosa è successo, questa dice
                cosa fare. Vedi `dettaglioErrore`. */}
            {guasto?.dettaglio != null && (
              <span className="dettaglio-errore">
                {guasto.dettaglio}
              </span>
            )}
          </span>
          {/* «Riprova» compare solo quando il catalogo dice che
              riprovare ha senso, e il caso per cui esiste è la
              cartella di rete che non risponde: lì il gesto non è
              «rifai quel che hai chiesto», è «rimetti la puntina
              dov'era», e il nucleo si è annotato il punto. La stessa
              forma della fascia dell'audio perso qui sopra: un
              `bottone minuto` in linea, senza CSS nuovo. */}
          {ritentabile && (
            <button
              type="button"
              className="bottone minuto btn-ghost"
              onClick={() => {
                riproduzione.scartaErrore();
                ipc.riprovaCorrente().catch(segnalaErrore);
              }}
            >
              {t("common.retry")}
            </button>
          )}
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("toast.dismiss")}
            onClick={() => {
              setErrore(null);
              riproduzione.scartaErrore();
            }}
          >
            <Icona nome="i-x" dim={14} />
          </button>
        </div>
      )}
    </>
  );

  return (
    <>
      {/* Prima di tutto il resto e sopra tutto il resto: al primo avvio non c'è
          una libreria da guardare dietro, e la schermata che chiede dove sta la
          musica è l'unica cosa che ha senso leggere. */}
      {primoVisibile && (
        <Primo
          onCartelle={primoConferma}
          onScegliCartella={primoScegli}
          scansione={scansione}
          brani={numeri?.tracks ?? 0}
          esito={esito}
          lasciate={lasciateAlPrimo}
          avvisi={notizieDiFascia}
          onAscolta={primoAscolta}
          onSalta={() => setPrimoChiuso(true)}
        />
      )}
      <Impaginazione
        albero={skinAttiva?.layout.shell ?? null}
        contesto={contesto}
        slot={{
          intestazione: testa(),
          contenuto: (
            <>
              {/* A schermo intero la pagina resta montata sotto: smontarla farebbe
                  perdere la posizione di scorrimento dell'elenco, e chiudere
                  riporterebbe in cima a una libreria che si stava guardando a metà. */}
              {grande && riproduzione.stato.brano ? (
                <InRiproduzione
                  stato={riproduzione.stato}
                  onChiudi={() => setGrande(false)}
                  onPreferito={cambiaPreferito}
                  onVoto={cambiaVoto}
                  onErrore={segnalaErrore}
                  avvisi={
                    <>
                      {/* Una seconda copia, montata solo qui: quella nella
                          pagina è sotto, `inert`. Vede gli avvisi che nascono a
                          schermo intero; quelli di prima erano già a schermo. */}
                      <AvvisoCoda onErrore={segnalaErrore} />
                      {notizieDiFascia}
                    </>
                  }
                />
              ) : null}
              {/* `inert` a schermo intero: la pagina resta montata ma è
                  coperta, e un Tab che ci entrasse porterebbe il fuoco su un
                  elenco che non si vede. Vedi «il fuoco» in `InRiproduzione`. */}
              <div
                className="dentro"
                ref={contenuto}
                inert={grande && riproduzione.stato.brano !== null}
              >
                {/* Qui e non solo nella terza colonna: la colonna si può
                    chiudere, e il lettore flottante sparisce quando non c'è un
                    brano — cioè proprio nei due casi in cui il dispositivo
                    manca da prima che si provasse a suonare qualcosa. Una
                    fascia che si può non vedere non è una fascia.

                    Il componente decide da sé se disegnarsi, e cosa: il rosso
                    di «non c'è audio», l'avviso che passa da sé quando il
                    suono si è spostato su un'altra uscita, o niente. */}
                <AvvisoAudio
                  stato={riproduzione.stato}
                  dove="fascia"
                  onErrore={segnalaErrore}
                />
                {/* Sopra la notizia e sotto il dispositivo audio perso, che è
                    l'ordine della fretta: l'audio che non si sente è adesso,
                    una versione nuova può aspettare che si finisca di
                    ascoltare. Il componente decide da sé se disegnarsi — non
                    c'è niente da mostrare quasi sempre — e non torna dopo un
                    «non ora», perché il rifiuto è scritto nel database. */}
                <AvvisoAggiornamento onErrore={segnalaErrore} />
                {/* «Coda sostituita — Annulla»: si disegna da sé quando il
                    nucleo manda `coda:sostituita`, e passa da sé. */}
                <AvvisoCoda onErrore={segnalaErrore} />
                {/* Col primo avvio aperto le notizie le mostra lui: qui
                    sarebbero sotto la sua schermata, e due copie della stessa
                    fascia d'errore si annuncerebbero due volte. */}
                {!primoVisibile && notizieDiFascia}
                {corpo()}
              </div>
            </>
          ),
        }}
      />

      {filePlaylist !== null && (
        <ImportaPlaylist
          percorso={filePlaylist}
          onChiudi={() => setFilePlaylist(null)}
          onImportato={() => {
            void ricaricaPlaylist();
          }}
          onErrore={segnalaErrore}
        />
      )}

      {regoleAperte !== null && (
        <Regole
          playlist={regoleAperte.playlist}
          onChiudi={() => setRegoleAperte(null)}
          onFatto={() => {
            setRegoleAperte(null);
            void ricaricaPlaylist();
          }}
          onErrore={segnalaErrore}
        />
      )}

      {daImportare && (
        <Importa
          percorso={daImportare}
          onChiudi={() => setDaImportare(null)}
          onImportato={() => {
            // Conteggi, preferiti e playlist sono cambiati sotto i piedi della
            // vista aperta: ricaricare è l'unico modo perché la navigazione non
            // continui a mostrare i numeri di prima.
            void ricarica();
            void caricaVista();
            void ricaricaPlaylist();
          }}
        />
      )}

      {importandoLink && (
        <ImportaLink
          onChiudi={() => setImportandoLink(false)}
          onImportato={(esito) => {
            // Il rapporto va all'elenco prima di tutto il resto: la finestrella
            // sta per chiudersi, e da lì in poi è quello l'unico posto che sa
            // che questa importazione esiste.
            importazioni.registra(esito);
            // Come sopra: può aver creato una playlist, riempito la sua, e
            // scritto identificativi che rifondono gli album.
            void ricarica();
            void caricaVista();
            void ricaricaPlaylist();
          }}
        />
      )}

      {importandoAccount && (
        <Account
          onChiudi={() => setImportandoAccount(false)}
          onImportato={(esito) => {
            // Un rapporto per playlist, e ognuno ha già la forma che l'elenco
            // delle importazioni conosce: registrarli tutti è quel che fa
            // comparire in coda le playlist appena importate, con il loro nome
            // invece di un identificativo.
            for (const elenco of [...esito.playlists, ...esito.albums, esito.liked]) {
              if (elenco.wantedRows > 0) importazioni.registra(elenco);
            }
            // Playlist nuove, preferiti segnati, conteggi d'ascolto riscritti,
            // album rifusi: è l'importazione che cambia più cose in una volta di
            // tutta l'applicazione, e nessuna vista aperta sa che è successo.
            void ricarica();
            void caricaVista();
            void ricaricaPlaylist();
          }}
        />
      )}

      {daEliminare && (
        <ConfermaEliminazione
          quanti={daEliminare.brani.length}
          dalDisco={daEliminare.dalDisco}
          onChiudi={() => setDaEliminare(null)}
          onConferma={() => {
            const cosa = daEliminare;
            setDaEliminare(null);
            void elimina(cosa.brani, cosa.dalDisco);
          }}
        />
      )}

      {daAggiungere && (
        <AggiungiAPlaylist
          brani={daAggiungere}
          playlist={playlist}
          onChiudi={() => setDaAggiungere(null)}
          onFatto={() => void ricaricaPlaylist()}
        />
      )}

      {creandoPlaylist && (
        <Chiedi
          titolo={t("dialog.newPlaylist")}
          etichetta={t("rules.name")}
          conferma={t("addTo.create")}
          onChiudi={() => setCreandoPlaylist(false)}
          onRispondi={(nome) => {
            setCreandoPlaylist(false);
            ipc
              .playlistCrea(nome)
              .then((creata) => {
                setPlaylistAperta(creata);
                return ricaricaPlaylist();
              })
              .catch(segnalaErrore);
          }}
        />
      )}

      {daRinominare && (
        <Chiedi
          titolo={t("dialog.renamePlaylist")}
          etichetta={t("rules.name")}
          iniziale={daRinominare.name}
          conferma={t("dialog.rename")}
          onChiudi={() => setDaRinominare(null)}
          onRispondi={(nome) => {
            const quale = daRinominare;
            setDaRinominare(null);
            ipc
              .playlistRinomina(quale.id, nome)
              .then((cambiata) => {
                aggiornaPlaylist(cambiata);
                return ricaricaPlaylist();
              })
              .catch(segnalaErrore);
          }}
        />
      )}

      {creandoTema && (
        <NuovoTema
          skin={skin}
          baseIniziale={baseTema}
          onChiudi={() => {
            setCreandoTema(false);
            setBaseTema(null);
          }}
          onCrea={(dati) => void creaTema(dati)}
        />
      )}

      {ripristinando && (
        <Ripristino
          onChiudi={() => setRipristinando(false)}
          onFatto={() => {
            // Ascolti, voti, preferiti, playlist e cartelle sorvegliate sono
            // cambiati sotto i piedi della vista aperta. Ricaricare è l'unico
            // modo perché la navigazione non continui a mostrare i numeri di
            // prima; la scansione la chiede la schermata stessa, perché è una
            // decisione dell'utente e dura venti secondi.
            void ricarica();
            void ricaricaSkin();
          }}
        />
      )}

      {/* Una pila e non due riquadri fissi.
          Finché il toast era uno, «in basso a destra» bastava a dire dove.
          Adesso sono due e possono stare in piedi insieme — una scansione parte
          da sé quando la coda ha finito di scaricare — e due elementi fissi allo
          stesso angolo si coprono a vicenda. La pila li impila; è anche l'unico
          posto in cui `view-transition-name` può stare, perché quel nome deve
          essere unico nella pagina e su `.toast` con due toast non lo era. */}
      <div className="pila-toast">
        {/* La scansione segue chi se ne va.
            Dura venti secondi la prima volta, e nessuno resta a guardare una
            barra per venti secondi: si torna alla libreria, e l'avanzamento va
            in basso a destra invece di sparire. Sparire farebbe credere che sia
            finita — o peggio, che sia stata annullata dal cambio di schermata. */}
        {scansione !== null && vista !== "impostazioni" && (
          <div className="toast toast-card" role="status">
            <Icona nome="i-scan" dim={16} />
            <div className="dentro">
              <div className="cosa">
                {scansione.totale > 0
                  ? t("toast.scan", {
                      fatti: numero(scansione.fatti),
                      totale: numero(scansione.totale),
                    })
                  : t("toast.scan.comparing")}
              </div>
              {/* L'avanzamento esisteva **solo a schermo**: una barra senza
                  ruolo è un rettangolo che si allunga, e a chi ascolta lo
                  schermo non diceva niente — né che c'è una scansione, né a che
                  punto è. `aria-valuenow` manca finché il totale non si sa
                  («confronto col disco…»), ed è il modo in cui si dichiara un
                  avanzamento indeterminato: un `0` lì vorrebbe dire «non è
                  ancora cominciato», che è un'altra cosa. */}
              <div
                className="toast-progress"
                role="progressbar"
                aria-label={t("toast.progress")}
                aria-valuemin={0}
                aria-valuemax={scansione.totale > 0 ? scansione.totale : undefined}
                aria-valuenow={scansione.totale > 0 ? scansione.fatti : undefined}
              >
                <span
                  style={{
                    width:
                      scansione.totale > 0
                        ? `${Math.round((scansione.fatti / scansione.totale) * 100)}%`
                        : "0%",
                  }}
                />
              </div>
            </div>
            <button
              type="button"
              className="bottone minuto btn-ghost"
              onClick={() => {
                vaiA("impostazioni");
                setSezione("cartelle");
              }}
            >
              {t("toast.open")}
            </button>
          </div>
        )}

        {/* La coda segue chi se ne va, per la ragione scritta sopra — e a
            maggior ragione: la scansione dura venti secondi, questa un'ora, e
            sopravvive alla chiusura dell'applicazione. Sulla pagina stessa il
            toast non compare: sarebbe la stessa barra due volte. */}
        <ToastImportazioni
          stato={importazioni.stato}
          rientro={importazioni.rientro}
          giaLì={vista === "importazioni"}
          onApri={() => vaiA("importazioni")}
          onChiudiRientro={importazioni.dimenticaRientro}
        />
      </div>

      {menu && <Menu apertura={menu} onChiudi={() => setMenu(null)} />}

      {/* Ultimo figlio, e non per ordine di importanza: il velo del giro copre
          la finestra intera, quindi deve stare **dopo** tutto quel che potrebbe
          disegnarsi sopra di lui a parità di strato — il menù contestuale, i
          toast. Lo strato vero lo dà il foglio, che è dove la scala vive; qui
          si dice soltanto che viene per ultimo. */}
      {giroAperto && <Giro onPrepara={preparaGiro} onChiudi={chiudiGiro} />}
    </>
  );
}

/**
 * «Sono tre brani, e uno lo stai ascoltando: sicuro?»
 *
 * Una finestrella qui dentro e non un componente condiviso, per la stessa
 * ragione per cui `schermate/Cartelle.tsx` tiene la sua: la domanda è di questo
 * gesto. Il comportamento — il fuoco che entra, la trappola del Tab, Escape, il
 * fuoco che torna a chi l'aveva — viene tutto da `useFinestrella`.
 *
 * # Due testi e non due finestrelle
 *
 * Perché la struttura è la stessa e a cambiare è solo quel che si sta per
 * perdere. Tenerle separate vorrebbe dire due volte la stessa impalcatura, e
 * due occasioni di correggerne una sola.
 *
 * Il tasto che conferma **non** è primario. Il primario è «Annulla»: in una
 * finestra che si apre sopra un gesto distruttivo, l'azione che il colore invita
 * a premere deve essere quella che non toglie niente.
 */
function ConfermaEliminazione({
  quanti,
  dalDisco,
  onConferma,
  onChiudi,
}: {
  quanti: number;
  dalDisco: boolean;
  onConferma: () => void;
  onChiudi: () => void;
}) {
  const finestrella = useFinestrella<HTMLDivElement>(onChiudi);
  const titolo = dalDisco
    ? t("delete.disk.title", { brani: brani_(quanti) })
    : t("delete.library.title", { brani: brani_(quanti) });
  return (
    <div className="velo scuro" onClick={onChiudi}>
      <div
        ref={finestrella}
        className="finestrella stretta glass-modal"
        role="dialog"
        aria-modal="true"
        aria-label={titolo}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{titolo}</h2>
        <p>{dalDisco ? t("delete.disk.body") : t("delete.library.body")}</p>
        <div className="tasti-finestrella">
          <button type="button" className="bottone primario" onClick={onChiudi}>
            {t("common.cancel")}
          </button>
          <button type="button" className="bottone" onClick={onConferma}>
            {dalDisco ? t("delete.disk.ok") : t("delete.library.ok")}
          </button>
        </div>
      </div>
    </div>
  );
}
