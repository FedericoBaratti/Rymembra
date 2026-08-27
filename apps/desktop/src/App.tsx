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
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";

import { AggiungiAPlaylist } from "./AggiungiAPlaylist";
import { Chiedi } from "./Chiedi";
import { Copertina } from "./Copertina";
import { Impaginazione, type ContestoWidget } from "./Impaginazione";
import { Account } from "./Account";
import { Importa, ImportaLink } from "./Importa";
import { ImportaPlaylist } from "./ImportaPlaylist";
import { Regole } from "./Regole";
import { Menu, type Apertura } from "./Menu";
import { NuovoTema } from "./NuovoTema";
import { Riordino } from "./Riordino";
import { Ripristino } from "./Ripristino";
import { Stelle } from "./Stelle";
import { cambiandoVista } from "./transizione";
import { brani_, durata, nomeArtista, numero, titoloAlbum } from "./formato";
import {
  applicaAccento,
  applicaSkin,
  ipc,
  testoErrore,
  type Album,
  type Artista,
  type Casa,
  type Avanzamento,
  type AvanzamentoArricchimento,
  type Avvio,
  type Brano,
  type EsitoArricchimento,
  type EsitoScansione,
  type Ordine,
  type Playlist,
  type Skin,
  type StatoArricchimento,
  type StatoNuvola,
  type StatoSincronia,
  type VoceSkin,
} from "./ipc";
import { applicaLingua, scegli, t, useLingua } from "./lingue";
import { usePagine, usePigro } from "./pagine";
import { AvvisoAggiornamento } from "./parti/Aggiornamenti";
import { Icona } from "./parti/Icone";
import { useImportazioni } from "./parti/Importazioni";
import { Intestazione } from "./parti/Intestazione";
import type { Vista } from "./parti/Navigazione";
import { ToastImportazioni } from "./parti/ToastImportazioni";
import { useRiproduzione } from "./riproduzione";
import { Artisti } from "./schermate/Artisti";
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
 * Una riga dell'elenco.
 *
 * `memo` e non una funzione nuda. Da sola non bastava — finché la posizione
 * viveva in `App`, i gestori scendevano in identità nuove venti volte al
 * secondo e nessun confronto poteva riuscire. Ora che la posizione sta nel suo
 * archivio, `App` si ridisegna solo quando cambia qualcosa di vero, e le
 * duecento righe che non sono cambiate saltano il giro invece di riconciliarsi.
 */
const RigaBrano = memo(function RigaBrano({
  brano,
  indice,
  attivo,
  suonabile,
  onSuona,
  onPreferito,
  onVoto,
  onMenu,
  onTogli,
  selezionato,
  onSeleziona,
  numeroTraccia,
  onRiordina,
  onPresa,
  onMira,
  onLascia,
  sopra,
}: {
  brano: Brano;
  indice: number;
  attivo: boolean;
  suonabile: boolean;
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
  /** Comincia (`indice`) o finisce (`null`) un trascinamento. */
  onPresa?: ((indice: number | null) => void) | undefined;
  /** Il rilascio cadrebbe qui (`indice`), o da nessuna parte (`null`). */
  onMira?: ((indice: number | null) => void) | undefined;
  /** Il rilascio è avvenuto su questa riga. */
  onLascia?: ((indice: number) => void) | undefined;
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
    const elenco = e.currentTarget.closest(".elenco");
    window.requestAnimationFrame(() =>
      elenco
        ?.querySelectorAll<HTMLElement>(".riga .indice")
        [indice + passo]?.focus(),
    );
  };

  return (
    <div
      className="riga list-row"
      aria-current={attivo}
      aria-selected={selezionato}
      data-active={attivo || undefined}
      data-scelta={selezionato || undefined}
      data-sopra={sopra || undefined}
      draggable={onRiordina !== undefined}
      onDragStart={onPresa && (() => onPresa(indice))}
      /* `preventDefault` è quel che dichiara la riga un bersaglio valido:
         senza, il puntatore mostra il divieto e `onDrop` non arriva mai. */
      onDragOver={
        onMira &&
        ((e) => {
          e.preventDefault();
          onMira(indice);
        })
      }
      onDragEnd={onPresa && (() => onPresa(null))}
      onDrop={onLascia && (() => onLascia(indice))}
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
      <button
        type="button"
        className="indice"
        aria-label={`Riproduci ${brano.title}`}
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
      <Copertina
        hash={brano.coverArtHash}
        titolo={titoloAlbum(brano.album)}
        classe="miniatura"
      />
      {/* Titolo e artista impilati in una cella sola, l'album nella sua: sono
          due informazioni di peso diverso, e dare all'artista una colonna larga
          quanto il titolo lo farebbe leggere come se lo fosse. */}
      <div className="chi">
        <div className="nome" title={brano.title}>
          {brano.title}
        </div>
        <div className="autore" title={nomeArtista(brano.artist)}>
          {nomeArtista(brano.artist)}
        </div>
      </div>
      <div className="disco" title={titoloAlbum(brano.album)}>
        {titoloAlbum(brano.album)}
      </div>
      <Stelle
        valore={brano.rating}
        onVoto={(stelle) => onVoto(brano, stelle)}
      />
      <div className="durata">{durata(brano.durationMs)}</div>
      <button
        type="button"
        className="cuore icon-btn"
        aria-pressed={brano.liked}
        aria-label={brano.liked ? t("track.unlike") : t("track.like")}
        onClick={() => onPreferito(brano)}
      >
        <Icona nome={brano.liked ? "i-heart-f" : "i-heart"} dim={15} />
      </button>
      {onTogli && (
        <button
          type="button"
          className="tasto icon-btn"
          aria-label={t("list.removeFromPlaylist", { titolo: brano.title })}
          onClick={() => onTogli(indice)}
        >
          <Icona nome="i-x" dim={14} />
        </button>
      )}
    </div>
  );
});

/** L'intestazione di colonna dell'elenco: la stessa griglia delle righe. */
function TestaElenco({ conTogli }: { conTogli?: boolean | undefined }) {
  return (
    <div className="testa-elenco" aria-hidden="true">
      <span className="indice">#</span>
      <span />
      <span>{t("list.title")}</span>
      {/* Le due colonne che si ritirano quando il contenuto si stringe portano un
          nome: nasconderle per posizione — `:nth-child(4)` — vorrebbe dire tenere
          allineati un numero qui e un numero nel foglio. */}
      <span className="disco">{t("list.album")}</span>
      <span className="voto">{t("list.rating")}</span>
      <span className="durata">{t("list.duration")}</span>
      <span />
      {conTogli && <span />}
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
      <TestaElenco />
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

export function App() {
  const [avvio, setAvvio] = useState<Avvio | null>(null);
  const [errore, setErrore] = useState<string | null>(null);
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
  const [vista, setVista] = useState<Vista>("home");
  const [sezione, setSezione] = useState<Sezione>("cartelle");
  const [query, setQuery] = useState("");
  const [artisti, setArtisti] = useState<Artista[]>([]);
  // La Home arriva in una chiamata sola. `null` è «non ancora chiesta», che è
  // diverso da «vuota»: la schermata mostra i suoi segnaposto finché il nucleo
  // non ha risposto, invece dello stato vuoto per un fotogramma.
  const [casa, setCasa] = useState<Casa | null>(null);
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
  const [daAggiungere, setDaAggiungere] = useState<number[] | null>(null);
  const [daRinominare, setDaRinominare] = useState<Playlist | null>(null);
  const [creandoPlaylist, setCreandoPlaylist] = useState(false);
  const [daRiordinare, setDaRiordinare] = useState<string | null>(null);
  /** Lo stato del backup su Drive, o `null` finché non è stato chiesto. */
  const [nuvola, setNuvola] = useState<StatoNuvola | null>(null);
  const [sincronia, setSincronia] = useState<StatoSincronia | null>(null);
  /** La finestrella del ripristino è aperta. */
  const [ripristinando, setRipristinando] = useState(false);
  /** Lo stato dell'arricchimento, o `null` finché non è stato chiesto. */
  const [arricchimento, setArricchimento] =
    useState<StatoArricchimento | null>(null);
  /** A che punto è la passata in corso, o `null` quando non ne gira nessuna. */
  const [avanzaArricchimento, setAvanzaArricchimento] =
    useState<AvanzamentoArricchimento | null>(null);
  /** Cosa ha prodotto l'ultima passata di questa sessione. */
  const [esitoArricchimento, setEsitoArricchimento] =
    useState<EsitoArricchimento | null>(null);
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

  const segnalaErrore = useCallback((e: unknown) => setErrore(testoErrore(e)), []);

  const ricarica = useCallback(async () => {
    try {
      setAvvio(await ipc.avvio());
      setErrore(null);
    } catch (e) {
      setErrore(testoErrore(e));
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
      .catch((e: unknown) => setErrore(testoErrore(e)));
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
   */
  const mostrata = useRef(false);
  useEffect(() => {
    if (mostrata.current || skinAttiva === null || avvio === null) return;
    mostrata.current = true;
    requestAnimationFrame(() =>
      requestAnimationFrame(() => {
        ipc.pronto().catch(segnalaErrore);
      }),
    );
  }, [skinAttiva, avvio, segnalaErrore]);

  // La colonna si chiude da sé quando la finestra si stringe, e non si riapre
  // da sé quando torna larga: riaprirla annullerebbe una chiusura decisa a mano.
  useEffect(() => {
    const guarda = () => {
      if (window.innerWidth < LARGHEZZA_TRE_COLONNE) setColonnaAperta(false);
    };
    window.addEventListener("resize", guarda);
    return () => window.removeEventListener("resize", guarda);
  }, []);

  const ricaricaSkin = useCallback(async () => {
    try {
      setSkin(await ipc.skinElenco());
    } catch (e) {
      setErrore(testoErrore(e));
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
        setErrore(testoErrore(e));
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
        setErrore(testoErrore(e));
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
      setErrore(testoErrore(e));
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
        setErrore(`«${quale}» non è un documento leggibile: correggilo nello Studio prima di derivarne un tema.`);
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
      setErrore(testoErrore(e));
    }
  };

  useEffect(() => {
    const promessa = listen<Avanzamento>("scansione:avanzamento", (evento) =>
      setScansione(evento.payload),
    );
    return () => {
      void promessa.then((stop) => stop());
    };
  }, []);

  /**
   * Lo stato del backup: una sola sorgente, come per la riproduzione.
   *
   * Si chiede una volta all'avvio e poi si **ascolta**: il filo di sottofondo
   * salva per conto suo, e una schermata che si aggiornasse solo quando la si
   * apre mostrerebbe l'ora dell'ultimo salvataggio di quando l'hai guardata,
   * non di adesso.
   */
  useEffect(() => {
    ipc.nuvolaStato().then(setNuvola).catch(segnalaErrore);
    const promessa = listen<StatoNuvola>("nuvola:stato", (evento) =>
      setNuvola(evento.payload),
    );
    return () => {
      void promessa.then((stop) => stop());
    };
  }, [segnalaErrore]);

  /**
   * Lo stato della sincronia, con la stessa disciplina del backup.
   *
   * Si chiede una volta e poi si **ascolta**, e qui conta più che altrove: la
   * sincronia scrive nella libreria da sola, e la schermata deve poter dire
   * cosa è arrivato mentre la si guardava.
   */
  useEffect(() => {
    ipc.sincroniaStato().then(setSincronia).catch(segnalaErrore);
    const promessa = listen<StatoSincronia>("sincronia:stato", (evento) =>
      setSincronia(evento.payload),
    );
    return () => {
      void promessa.then((stop) => stop());
    };
  }, [segnalaErrore]);

  /**
   * Lo stato dell'arricchimento, con la stessa disciplina del backup.
   *
   * Si chiede una volta e poi si **ascolta**, per la stessa ragione: il filo
   * lavora per conto suo, e la sezione aperta mentre una passata gira deve
   * vedere i numeri salire invece di restare a quelli di quando l'hai aperta.
   */
  useEffect(() => {
    ipc.arricchimentoStato().then(setArricchimento).catch(segnalaErrore);
    const promesse = [
      listen<StatoArricchimento>("arricchimento:stato", (evento) => {
        setArricchimento(evento.payload);
        // Lo stato è l'ultima parola su «sta girando»: una passata caduta a
        // metà — il database che non risponde, la finestra che si chiude —
        // non manda l'ultimo passo, e senza questa riga la barra resterebbe
        // ferma a 3/12 per sempre.
        if (!evento.payload.inCorso) setAvanzaArricchimento(null);
      }),
      listen<AvanzamentoArricchimento>("arricchimento:avanzamento", (evento) =>
        // L'ultimo passo di una passata è `fatti === totale`, ed è anche il
        // segnale che è finita: tenerlo mostrato lascerebbe una barra piena
        // sotto uno stato che dice «ferma».
        setAvanzaArricchimento(
          evento.payload.fatti >= evento.payload.totale ? null : evento.payload,
        ),
      ),
      listen<EsitoArricchimento>("arricchimento:esito", (evento) => {
        setEsitoArricchimento(evento.payload);
        setAvanzaArricchimento(null);
      }),
    ];
    return () => {
      for (const promessa of promesse) void promessa.then((stop) => stop());
    };
  }, [segnalaErrore]);

  /** Un comando del backup: aggiorna lo stato, o mostra perché non ci riesce. */
  const conNuvola = useCallback(
    (azione: () => Promise<StatoNuvola>) => {
      // Ottimistico su `inCorso`: `nuvolaCollega` apre un browser e può metterci
      // tre minuti, e senza questo il tasto resterebbe premibile per tutto quel
      // tempo — con il risultato che chi non vede succedere niente clicca due
      // volte e si prende un `sync.busy`.
      setNuvola((prima) => (prima ? { ...prima, inCorso: true } : prima));
      azione()
        .then(setNuvola)
        .catch((e: unknown) => {
          setNuvola((prima) => (prima ? { ...prima, inCorso: false } : prima));
          segnalaErrore(e);
        });
    },
    [segnalaErrore],
  );

  /** Un comando della sincronia: aggiorna lo stato, o mostra perché non ci riesce. */
  const conSincronia = useCallback(
    (azione: () => Promise<StatoSincronia>) => {
      setSincronia((prima) => (prima ? { ...prima, inCorso: true } : prima));
      azione()
        .then(setSincronia)
        .catch((e: unknown) => {
          setSincronia((prima) =>
            prima ? { ...prima, inCorso: false } : prima,
          );
          segnalaErrore(e);
        });
    },
    [segnalaErrore],
  );

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
   * nelle dipendenze di un `listen`, che si riscriverebbe di continuo.
   */
  const ricaricaBrani = elencoBrani.ricarica;
  const ricaricaAlbum = elencoAlbum.ricarica;
  const caricaVista = useCallback(() => {
    ricaricaBrani();
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
  }, [ricaricaBrani, ricaricaAlbum, vista, segnalaErrore]);

  /**
   * I brani scaricati sono entrati in libreria.
   *
   * La coda, quando finisce, rifà una scansione da sé: i file presi dai
   * cataloghi diventano brani veri senza che nessuno prema «Scansiona». Ma la vista
   * aperta continuerebbe a mostrare i conteggi di prima — una libreria a cui
   * sono appena arrivati quaranta brani che non compaiono finché non si cambia
   * schermata.
   */
  useEffect(() => {
    const promessa = listen("scarico:in_libreria", () => {
      void ricarica();
      void caricaVista();
    });
    return () => {
      void promessa.then((stop) => stop());
    };
  }, [ricarica, caricaVista]);

  useEffect(() => {
    if (!aperto) return;
    ipc
      .braniAlbum(aperto.albumKey)
      .then(setBraniAperto)
      .catch((e: unknown) => setErrore(testoErrore(e)));
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
        `${numero(quanti)} ${quanti === 1 ? "brano scritto" : "brani scritti"} in ${scelta}`,
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
    setScansione({ fatti: 0, totale: 0 });
    try {
      const risultato = await ipc.scansiona();
      setEsito(risultato);
      await ricarica();
      await caricaVista();
    } catch (e) {
      setErrore(testoErrore(e));
    } finally {
      setScansione(null);
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
      setErrore(testoErrore(e));
    }
  }, []);

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
  useEffect(() => {
    if (!playlistAperta) return;
    ipc
      .playlistBrani(playlistAperta.id)
      .then(setBraniPlaylist)
      .catch(segnalaErrore);
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

  /** Apre il menù contestuale su una selezione di brani. */
  const apriMenu = useCallback(
    (e: React.MouseEvent, elenco: number[]) => {
      e.preventDefault();
      setMenu({
        x: e.clientX,
        y: e.clientY,
        voci: [
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
        ],
      });
    },
    [segnalaErrore],
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
        setErrore(testoErrore(e));
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
        setErrore(testoErrore(e));
        riproduzione.ritoccaBrano(brano.id, { liked: brano.liked });
        await caricaVista();
      }
    },
    [caricaVista, riproduzione.ritoccaBrano],
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
   */
  useEffect(() => {
    const promessa = listen<{ paths: string[] }>("tauri://drag-drop", (evento) => {
      const arrivati = evento.payload.paths;
      const skinLasciata = arrivati.find((p) => p.toLowerCase().endsWith(".aeskin"));
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
      // Tutto il resto si prova come cartella: `imposta_cartelle` accetta dei
      // percorsi, e la scansione salta da sé quel che non è musica. Distinguere
      // qui una cartella da un file vorrebbe dire chiedere al filesystem
      // dall'interfaccia, cioè mettere una regola dove non deve stare.
      if (arrivati.length === 0 || !avvio) return;
      vaiA("impostazioni");
      setSezione("cartelle");
      const unite = [...new Set([...avvio.cartelle, ...arrivati])];
      ipc.impostaCartelle(unite).then(ricarica).catch(segnalaErrore);
    });
    return () => {
      void promessa.then((stop) => stop());
    };
  }, [avvio, ricarica, segnalaErrore, scegliSkin, vaiA]);

  const numeri = avvio?.numeri;
  const senzaCartelle = avvio !== null && avvio.cartelle.length === 0;
  const vuota = numeri !== undefined && numeri.tracks === 0;
  const inAscolto = riproduzione.stato.brano?.id ?? null;
  const messaggio =
    errore ?? (riproduzione.errore ? testoErrore(riproduzione.errore) : null);

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
            preferiti: numeri.liked,
          }
        : {},
    [numeri],
  );

  const etichettaOrdine =
    ordinamenti().find(([c]) => c === ordine)?.[1] ?? t("sort.shelf");

  /**
   * Tutto quel che i widget sanno del mondo, in un oggetto solo.
   *
   * È l'unico imbuto per i ventisei `useState` di questo componente. Prima
   * scendevano a mano: `Lettore` prendeva sette prop, `Colonna` sette, `Coda`
   * tre, e aggiungere un widget voleva dire farne passare un'altra attraverso
   * tre livelli che non la usavano. Qui la lista si scrive una volta.
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
      onColonna: setColonnaAperta,
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
          lingua={lingua}
          onLingua={cambiaLingua}
          scorciatoie={scorciatoie}
          onScorciatoie={cambiaScorciatoie}
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
          onRiordina={setDaRiordinare}
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
          <div className="elenco track-grid">
            <TestaElenco />
            {brani.map((b, i) => (
              <RigaBrano
                key={b.id}
                brano={b}
                indice={i}
                attivo={b.id === inAscolto}
                suonabile={riproduzione.disponibile}
                onSuona={suonaQui}
                onPreferito={cambiaPreferito}
                onVoto={cambiaVoto}
                onMenu={menuSuSelezione}
                selezionato={selezione.has(b.id)}
                onSeleziona={seleziona}
              />
            ))}
          </div>
          <Sentinella pagine={elencoBrani} />
        </>
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
          {/* L'attributo, non solo la prop: la colonna in più la deve
              conoscere anche la griglia del foglio, altrimenti l'ottavo figlio
              della riga finisce a capo invece che in fondo. */}
          <div
            className="elenco track-grid"
            data-con-togli={!playlistAperta.isSmart || undefined}
          >
            <TestaElenco conTogli={!playlistAperta.isSmart} />
            {braniPlaylist.map((b, i) => (
              <RigaBrano
                key={`${i}-${b.id}`}
                brano={b}
                indice={i}
                attivo={b.id === inAscolto}
                suonabile={riproduzione.disponibile}
                onSuona={suonaQui}
                onPreferito={cambiaPreferito}
                onVoto={cambiaVoto}
                onMenu={menuSuSelezione}
                selezionato={selezione.has(b.id)}
                onSeleziona={seleziona}
                /* Solo in una playlist a mano: l'appartenenza di una
                   automatica la decidono le sue regole, e un ordine deciso
                   qui sarebbe cancellato dal primo ricalcolo. */
                {...(playlistAperta.isSmart
                  ? {}
                  : {
                      onTogli: togliDallaPlaylist,
                      onRiordina: spostaNellaPlaylist,
                      onPresa: presa,
                      onMira: setMirata,
                      onLascia: lascia,
                      sopra: mirata === i && trascinata !== i,
                    })}
              />
            ))}
          </div>
        </>
      );
    }

    if (aperto) {
      return (
        <div className="elenco track-grid">
          <TestaElenco />
          {braniAperto.map((b, i) => (
            <RigaBrano
              key={b.id}
              brano={b}
              indice={i}
              attivo={b.id === inAscolto}
              suonabile={riproduzione.disponibile}
              onSuona={suonaQui}
              onPreferito={cambiaPreferito}
              onVoto={cambiaVoto}
              onMenu={menuSuSelezione}
              selezionato={selezione.has(b.id)}
              onSeleziona={seleziona}
              /* L'unico posto in cui `#` è il numero del disco: qui la colonna
                 dice dove sta il pezzo sulla custodia. */
              numeroTraccia
            />
          ))}
        </div>
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
          onSuona={(elenco, indice) => void suonaDa(elenco, indice)}
          onRiprendi={(ms) => {
            void (async () => {
              // `riprendi` e non `suona`: la coda conservata è già in piedi —
              // il nucleo la rimette all'avvio senza far partire niente — e
              // `suona([brano], 0)` la buttava via per sostituirla con un brano
              // solo. Cioè «riprendi dov'eri» faceva calare il silenzio dove
              // ieri sera la serata continuava. Il comando sa già di dover
              // cominciare quando il motore ha le mani vuote.
              await ipc.riprendi().catch(segnalaErrore);
              // Il salto **dopo** l'avvio: `vai_a` su un motore che non ha
              // ancora aperto il file non ha un posto dove andare.
              if (ms > 0) await ipc.vaiA(ms).catch(segnalaErrore);
            })();
          }}
          onMenu={(e, brano) => menuSuSelezione(e, [brano.id])}
          onApriAlbum={apriAlbum}
          onMenuAlbum={(e, album) => menuAlbum(e, album.albumKey)}
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
        <div className="elenco track-grid">
          <TestaElenco />
          {brani.map((b, i) => (
            <RigaBrano
              key={b.id}
              brano={b}
              indice={i}
              attivo={b.id === inAscolto}
              suonabile={riproduzione.disponibile}
              onSuona={suonaQui}
              onPreferito={cambiaPreferito}
              onVoto={cambiaVoto}
              onMenu={menuSuSelezione}
              selezionato={selezione.has(b.id)}
              onSeleziona={seleziona}
            />
          ))}
        </div>
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
        onErrore={segnalaErrore}
      />
    );
  }

  return (
    <>
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
                />
              ) : null}
              <div className="dentro" ref={contenuto}>
                {/* Qui e non solo nella terza colonna: la colonna si può
                    chiudere, e il lettore flottante sparisce quando non c'è un
                    brano — cioè proprio nei due casi in cui il dispositivo
                    manca da prima che si provasse a suonare qualcosa. Una
                    fascia che si può non vedere non è una fascia. */}
                {riproduzione.stato.audio !== null && (
                  <div className="errore audio-perso toast-card" role="alert">
                    <Icona nome="i-alert" dim={16} />
                    <span>
                      <strong>{t("audio.lost.what")}</strong>{" "}
                      {riproduzione.stato.audio.causa}.
                    </span>
                    {riproduzione.stato.audio.riapribile && (
                      <button
                        type="button"
                        className="bottone minuto btn-ghost"
                        title={t("audio.lost.reopen.title")}
                        onClick={() => {
                          ipc.riapriAudio().catch(segnalaErrore);
                        }}
                      >
                        {t("audio.lost.reopen")}
                      </button>
                    )}
                  </div>
                )}
                {/* Sopra la notizia e sotto il dispositivo audio perso, che è
                    l'ordine della fretta: l'audio che non si sente è adesso,
                    una versione nuova può aspettare che si finisca di
                    ascoltare. Il componente decide da sé se disegnarsi — non
                    c'è niente da mostrare quasi sempre — e non torna dopo un
                    «non ora», perché il rifiuto è scritto nel database. */}
                <AvvisoAggiornamento onErrore={segnalaErrore} />
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
                    <span>{messaggio}</span>
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

      {daRiordinare && (
        <Riordino
          radice={daRiordinare}
          onChiudi={() => setDaRiordinare(null)}
          onFatto={() => {
            // I percorsi nel database sono vecchi finché non si riscansiona.
            // Ricaricare i numeri è quel che si può fare subito; la scansione
            // la chiede la schermata stessa, perché è una decisione dell'utente
            // e dura venti secondi.
            void ricarica();
          }}
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
              <div className="toast-progress">
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
    </>
  );
}
