/**
 * L'elenco dei brani e i suoi segnaposto.
 *
 * Stava dentro `App.tsx`, e ne è uscito così com'era per una ragione sola: lo
 * Studio disegna le pagine vere con dati finti, e una pagina che vive dentro
 * il componente della finestra non si può disegnare da nessun'altra parte. Qui
 * non c'è niente che sappia della finestra: righe, selezione, fuoco e gesti
 * arrivano per prop, e quel che si legge da sé passa da `useDati`.
 */
import { memo, useCallback, useEffect, useRef } from "react";
import type { ReactNode } from "react";

import { Copertina } from "../Copertina";
import { durata, nomeArtista, nomeFonte, titoloAlbum } from "../formato";
import { useFuoco } from "../fuoco";
import type { Brano } from "../ipc";
import { t, useLingua } from "../lingue";
import { useVersione } from "../pagine";
import { Icona } from "../parti/Icone";
import type { SorgenteRada } from "../rada";
import { usePresaPerRiordino } from "../riordino";
import { conIndice, statoSuono } from "../segnali";
import { Stelle } from "../Stelle";
import { eLaLettera } from "../tasti";
import { useVirtuale } from "../virtuale";

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
  inPausa,
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
  /**
   * Il lettore è fermo. Vero solo sulla riga in ascolto: le altre lo ricevono
   * sempre falso, così la pausa ridisegna una riga e non duecento.
   */
  inPausa: boolean;
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

  const suono = statoSuono(attivo, inPausa);
  return (
    <div
      className="riga list-row"
      {...suono}
      style={conIndice(indice)}
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
      /* Per la finestra e per il fuoco, che non possono più contare sulla
         posizione nel DOM: la riga col fuoco resta montata anche fuori dalla
         finestra. Vedi `indiceDi` in `virtuale.ts`. */
      data-indice={indice}
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
          className="indice track-index"
          {...suono}
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
          /* L'album è scritto due celle più in là: vedi `Copertina`. */
          accanto
        />
      </div>
      {/* Titolo e artista impilati in una cella sola, l'album nella sua: sono
          due informazioni di peso diverso, e dare all'artista una colonna larga
          quanto il titolo lo farebbe leggere come se lo fosse.

          `dir="auto"` su tutti e tre: un titolo arabo o ebraico si scrive da
          destra, e la direzione la decide il testo, non la pagina. Senza, la
          punteggiatura di un titolo ebraico finiva dalla parte sbagliata. */}
      <div className="chi" role="gridcell">
        <div className="nome" title={brano.title} dir="auto">
          {brano.title}
          {/* La pastiglia accanto al titolo e non in una colonna sua: dice una
              cosa su **questo** brano, e su una libreria di file sarebbe una
              colonna vuota per diciottomila righe. Chi ascolta con uno screen
              reader la sente nel titolo, che è dove serve — è la differenza fra
              un brano che c'è sul disco e uno che c'è finché c'è la rete. */}
          {brano.fonte !== null && (
            <span
              className="pastiglia badge flusso"
              data-livello="nota"
              title={t("track.stream.title", { fonte: nomeFonte(brano.fonte) })}
            >
              {t("track.stream.badge")}
            </span>
          )}
        </div>
        <div className="autore" title={nomeArtista(brano.artist)} dir="auto">
          {nomeArtista(brano.artist)}
        </div>
      </div>
      <div className="disco" role="gridcell" title={titoloAlbum(brano.album)} dir="auto">
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
      className="testa-elenco list-header"
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
 * `skeleton` è una parte del registro (`core/rymembra-skin/src/parts.rs`), e la
 * sua descrizione dice che l'animazione fa parte dell'identità della skin.
 * Fino a qui era emessa **solo** nell'anteprima dello Studio: l'applicazione
 * mostrava una parte che non usava.
 *
 * `role="status"` sul contenitore e `aria-hidden` sui riquadri: chi ascolta lo
 * schermo sente «sto caricando» una volta, non dodici rettangoli.
 */
export function GrigliaFinta() {
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

export function ElencoFinto() {
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
export function Sentinella({
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
 * Una riga che non è ancora arrivata: lo scheletro di una `RigaBrano`.
 *
 * # Alta quanto una riga vera, e non quanto le pare
 *
 * Il segnaposto di `ElencoFinto` si ferma al pavimento di `.riga` — quaranta
 * pixel — e la riga vera si assesta a 41,9: fra i due, all'arrivo dei dati,
 * l'elenco si allungava di due pixel per riga. Dentro una lista rada quel salto
 * non sarebbe una volta sola ma a ogni pagina che arriva, e sotto il dito:
 * qui lo scheletro prende l'altezza che la finestra ha misurato sulle righe
 * vere (`altezza`), e quando i dati arrivano il posto è già quello.
 *
 * # Una riga della griglia, e una fermata se serve
 *
 * `role="row"` con le sue celle e il suo `aria-rowindex`, come la riga che
 * prenderà il posto: chi ascolta lo schermo sente «riga 12.004 di 18.535,
 * occupata» e non un buco nel conteggio. Può portare il fuoco — premendo Fine
 * su cinquantamila brani la riga d'arrivo è quasi sempre uno scheletro — e il
 * fuoco ci resta finché la riga vera non arriva.
 *
 * `memo` come la riga vera: scorrendo veloce sono questi i nodi che nascono
 * e muoiono di più.
 */
const RigaScheletro = memo(function RigaScheletro({
  indice,
  altezza,
  colFuoco,
  onFuocoPreso,
  conTogli,
}: {
  indice: number;
  /**
   * L'altezza misurata sulle righe vere: vedi la nota del componente. Finché
   * nessuna riga vera è stata misurata, lo scheletro tiene la sua.
   */
  altezza: number | undefined;
  colFuoco: boolean;
  onFuocoPreso: (indice: number) => void;
  conTogli?: boolean | undefined;
}) {
  return (
    <div
      className="riga finta"
      role="row"
      aria-rowindex={indice + 2}
      aria-busy="true"
      data-indice={indice}
      tabIndex={colFuoco ? 0 : -1}
      onFocus={() => onFuocoPreso(indice)}
      style={altezza === undefined ? undefined : { height: altezza }}
    >
      <span role="gridcell" />
      <div role="gridcell" className="miniatura skeleton" />
      <div className="chi" role="gridcell">
        <div className="riga-finta skeleton" />
        <div className="riga-finta corta skeleton" />
      </div>
      <div role="gridcell" className="riga-finta skeleton disco" />
      <span role="gridcell" />
      <div role="gridcell" className="riga-finta skeleton" />
      <span role="gridcell" />
      {conTogli && <span role="gridcell" />}
    </div>
  );
});

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
 * # Piena o rada
 *
 * Le righe arrivano in due modi. `righe` è un elenco intero già in mano — una
 * playlist, un album, lo Studio. `rada` è una lista rada (`rada.ts`): si sa
 * quanto è lunga e se ne hanno in mano solo le pagine guardate, che questo
 * componente chiede per la finestra che disegna. Lì la barra di scorrimento è
 * lunga quanto la vista dal primo istante, e le righe che mancano sono
 * scheletri della stessa altezza (`RigaScheletro`) finché la loro pagina non
 * arriva. Tirando la barra fino in fondo si chiede l'ultima pagina, e basta.
 *
 * Le pagine si chiedono in un effetto: dentro un `<Activity>` nascosto gli
 * effetti non girano, e una vista che non si vede non chiede niente.
 *
 * # La riga col fuoco resta montata
 *
 * Una riga che esce dalla finestra si smonta, e se aveva il fuoco il fuoco
 * cadeva sul `<body>`: un colpo di rotella e la tastiera non sapeva più dove
 * fosse. La riga attiva (`useFuoco().attivo`) quindi si disegna anche fuori
 * dalla finestra, **al suo posto**: la paglia di quel lato si spezza in due
 * attorno a lei, e le altezze tornano. È anche la fermata di tabulazione
 * dell'elenco, che così esiste sempre.
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
 * `aria-rowcount` è il totale **vero**: con la lista rada il numero c'è prima
 * delle righe, e non serve più contare quelle arrivate.
 */
export function ElencoBrani({
  righe,
  rada,
  chiave,
  memoria,
  scorrevole,
  inAscolto,
  inPausa,
  suonabile,
  selezione,
  onSuona,
  onPreferito,
  onVoto,
  onMenu,
  onSeleziona,
  onAlternaSelezione,
  onSelezionaTutto,
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
  /** Le righe, quando l'elenco è tutto in mano. Vedi «Piena o rada». */
  righe?: readonly Brano[] | undefined;
  /** Le righe, quando arrivano a pagine dove si guarda. Vince su `righe`. */
  rada?: SorgenteRada<Brano> | undefined;
  /**
   * L'identità di quel che si mostra: cambiandola si torna in cima. Serve a chi
   * cambia l'ordinamento di un elenco che resta montato.
   */
  chiave?: string | undefined;
  /**
   * Lo scorrimento si ricorda e si rimette tornando: solo per chi sta sotto un
   * `<Activity>`. Vedi «La memoria» in `virtuale.ts`.
   */
  memoria?: boolean | undefined;
  /** Lo scorrevole dentro cui l'elenco vive: `.dentro`. */
  scorrevole: React.RefObject<HTMLDivElement | null>;
  /** Quale brano sta suonando, per la riga in evidenza. */
  inAscolto: number | null;
  /** Il lettore è fermo: la riga in ascolto lo dice alla skin. */
  inPausa: boolean;
  suonabile: boolean;
  /**
   * Quali brani sono scelti. Basta che sappia rispondere a `has`: un `Set`, o la
   * mappa identificativo → posizione con cui `App` ricorda l'ordine della vista.
   */
  selezione: { has(id: number): boolean };
  onSuona: (indice: number) => void;
  onPreferito: (b: Brano) => void;
  onVoto: (b: Brano, stelle: number) => void;
  onMenu: (e: React.MouseEvent, brani: number[]) => void;
  onSeleziona: (e: React.MouseEvent, indice: number) => void;
  /** Spazio sulla riga col fuoco: il Ctrl+clic detto da tastiera. */
  onAlternaSelezione: (indice: number) => void;
  /**
   * `Ctrl+A` col fuoco nell'elenco: la stessa azione di «Seleziona tutto»
   * nella barra della selezione. Vedi `daTastiera` qui sotto.
   */
  onSelezionaTutto?: (() => void) | undefined;
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
  // Ridisegna questo elenco — e non chi lo possiede — a ogni pagina che arriva.
  const versione = useVersione(rada);
  const totale = rada !== undefined ? rada.totale : (righe?.length ?? 0);
  const finestra = useVirtuale({
    totale,
    contenitore: scorrevole,
    ancora: elenco,
    // Il metro sono le righe vere: gli scheletri prendono la loro altezza.
    selettoreMisura: ".riga:not(.finta)",
    memoria,
    chiave,
  });
  // Stabile per tutta la vita dell'elenco: le azioni si leggono al gesto.
  const presaPuntatore = usePresaPerRiordino({ onPresa, onMira, onLascia });

  /*
   * Le pagine che la finestra guarda. `versione` fra le dipendenze perché
   * un'invalidazione — il totale cambiato, una scansione finita — deve far
   * richiedere le pagine vecchie che si stanno guardando; per le pagine già
   * fresche la richiesta non fa niente.
   */
  useEffect(() => {
    rada?.richiedi(finestra.primo, finestra.ultimo);
  }, [rada, finestra.primo, finestra.ultimo, versione]);

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
    totale,
    ancora: elenco,
    finestra,
    scorriA: finestra.scorriA,
    onInvio: suona,
    onSpazio: onAlternaSelezione,
  });

  /*
   * `Ctrl+A` sceglie la vista intera, poi il resto a `fuoco.ts`.
   *
   * «Seleziona tutto» esisteva solo come tasto nella barra della selezione, che
   * compare dopo la prima riga scelta: da tastiera la strada era Spazio su una
   * riga, poi Tab fino alla barra. Il tasto di ogni elenco di Windows mancava,
   * e premuto qui faceva il suo mestiere di serie — selezionare il **testo**
   * della pagina, cioè colorare di blu titoli e intestazioni.
   *
   * Dentro un campo di testo non arriva mai: nell'elenco non ce ne sono, e il
   * gestore sta sull'elenco. La lettera si riconosce anche dalla posizione
   * (`eLaLettera`), così su una tastiera russa è `Ctrl+ф` a fare la stessa cosa.
   */
  const daTastiera = (e: React.KeyboardEvent<HTMLElement>) => {
    if (
      onSelezionaTutto !== undefined &&
      e.ctrlKey &&
      !e.altKey &&
      !e.shiftKey &&
      !e.metaKey &&
      eLaLettera(e, "a")
    ) {
      e.preventDefault();
      e.stopPropagation();
      onSelezionaTutto();
      return;
    }
    fuoco.daTastiera(e);
  };

  /** Una riga, vera o scheletro. */
  const disegna = (i: number): ReactNode => {
    const b = rada !== undefined ? rada.riga(i) : righe?.[i];
    if (b === undefined) {
      return (
        <RigaScheletro
          key={`r${i}`}
          indice={i}
          altezza={finestra.misurata ? finestra.altezza : undefined}
          colFuoco={i === fuoco.attivo}
          onFuocoPreso={fuoco.segna}
          conTogli={conTogli}
        />
      );
    }
    return (
      <RigaBrano
        /* In una lista rada la chiave è la posizione: lo scheletro e la riga
           che lo sostituisce sono lo stesso posto. Il nodo però non resta —
           sono due componenti, e React rifà il nodo quando cambia il tipo —
           e il fuoco che stava sullo scheletro lo rimette `useFuoco`. */
        key={rada !== undefined ? `r${i}` : chiaveConIndice ? `${i}-${b.id}` : b.id}
        brano={b}
        indice={i}
        attivo={b.id === inAscolto}
        inPausa={b.id === inAscolto && inPausa}
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
  };

  /** Il posto di `quante` righe che non si disegnano. */
  const paglia = (chiaveDiPaglia: string, quante: number): ReactNode => (
    <div
      key={chiaveDiPaglia}
      className="paglia"
      style={{ height: Math.max(quante, 0) * finestra.altezza }}
      /* `role="presentation"` perché non è una riga: è il posto che le righe
         assenti occuperebbero, e una `grid` con dentro due `div` sconosciuti
         direbbe di avere due righe in più di quante ne ha. */
      role="presentation"
    />
  );

  // La riga col fuoco fuori dalla finestra: vedi «La riga col fuoco resta
  // montata». Una sola, ed è la stessa chiave che ha dentro la finestra, così
  // React sposta il nodo invece di rifarlo — e il fuoco resta dov'è.
  const { primo, ultimo } = finestra;
  const fuori =
    totale > 0 && (fuoco.attivo < primo || fuoco.attivo >= ultimo) ? fuoco.attivo : null;

  const figli: ReactNode[] = [<TestaElenco key="testa" conTogli={conTogli} />];
  if (fuori !== null && fuori < primo) {
    figli.push(paglia("sopra", fuori), disegna(fuori), paglia("sopra-dopo", primo - fuori - 1));
  } else {
    figli.push(paglia("sopra", primo));
  }
  for (let i = primo; i < ultimo; i += 1) figli.push(disegna(i));
  if (fuori !== null && fuori >= ultimo) {
    figli.push(paglia("sotto", fuori - ultimo), disegna(fuori), paglia("sotto-dopo", totale - fuori - 1));
  } else {
    figli.push(paglia("sotto", totale - ultimo));
  }

  return (
    <div
      className="elenco track-grid"
      ref={elenco}
      role="grid"
      aria-multiselectable="true"
      aria-label={t("list.grid")}
      aria-rowcount={totale + 1}
      /* Un gestore solo per tutte le righe: appenderne uno a ciascuna
         darebbe a `RigaBrano` una prop con identità nuova a ogni disegno e
         annullerebbe il suo `memo`, che è l'altro conto che questa release
         paga. La riga da cui viene il tasto la dice il bersaglio dell'evento. */
      onKeyDown={daTastiera}
      /* L'attributo, non solo la prop: la colonna in più la deve conoscere
         anche la griglia del foglio, altrimenti l'ottavo figlio della riga
         finisce a capo invece che in fondo. */
      data-con-togli={conTogli || undefined}
      data-scorso={finestra.scorso || undefined}
    >
      {figli}
    </div>
  );
}
