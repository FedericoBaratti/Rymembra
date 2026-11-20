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
 *
 * # Cosa resta qui, e dove sta il resto
 *
 * Qui c'è l'instradamento — quale pagina (`pagina.ts`), la sua testata
 * (`testa()`) e il suo corpo (`corpo()`) — le finestrelle e i menù che si
 * sovrappongono, il giro guidato, e i gesti che toccano tutto questo insieme.
 * Il resto sta accanto, ciascuno con le sue ragioni scritte in testa:
 *
 * - lo stato che cambia spesso, fuori da React: la riproduzione
 *   (`riproduzione.ts`), la libreria e la scansione (`libreria.ts`,
 *   `scansione.ts`), letti da chi li disegna (`collegati.tsx`,
 *   `ImpostazioniFinestra.tsx`, i widget di `Impaginazione.tsx`);
 * - i fornitori della finestra, con le importazioni e il backup
 *   (`Fornitori.tsx`, `parti/Importazioni.tsx`, `nuvola.ts`);
 * - gli hook di questo componente: l'aspetto (`aspettoFinestra.ts`), la skin
 *   (`skinAttiva.ts`), la selezione (`selezioneVista.ts`), la ricerca
 *   (`ricerca.ts`), la Home (`casa.ts`), gli artisti (`artisti.ts`), la
 *   playlist aperta (`playlistAperta.ts`), le cartelle sorvegliate
 *   (`cartelleSorvegliate.ts`), la terza colonna (`terzaColonna.ts`), i file
 *   trascinati (`trascinamento.ts`);
 * - le regole che si provano da sole: le voci dei menù (`menuBrani.ts`), cosa
 *   fare dei file trascinati (`trascinati.ts`), la selezione (`selezione.ts`).
 *
 * Il giro, `testa()` e `corpo()`, i menù contestuali e i gesti sulle playlist
 * (crea, rinomina, cancella con «Annulla») restano qui perché leggono e
 * scrivono una dozzina di stati di questo componente: tirati fuori, si
 * porterebbero dietro tutti i loro argomenti.
 */
import { open, salvaCon } from "./dialoghi";
import { MOBILE } from "./piattaforma";
import { SCAFALE_TELEFONO } from "./scafaleTelefono";
import {
  Activity,
  lazy,
  Suspense,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type RefObject,
} from "react";

import { accorpa } from "./accorpa";
import { Account } from "./Account";
import { AggiungiAPlaylist, fraseDellAggiunta } from "./AggiungiAPlaylist";
import { AggiungiCartella } from "./AggiungiCartella";
import { ANNULLI, annullaATempo } from "./annullaATempo";
import { ugualiInSuperficie, useArchivio } from "./archivio";
import {
  PER_VOLTA_SCARICO,
  archivioLontano,
  archivioScarico,
  archivioSpecchio,
  causeDiRilettura,
  chiaveDelTestoErrore,
  chiedeConfermaPerLoScarico,
  conAvanzamento,
  conIlSiDeiDatiMobili,
  copertinaArrivata,
  daFotografia,
  domandaDelloScarico,
  inBlocchi,
  rileggereDopo,
} from "./lontano";
import { useArtisti } from "./artisti";
import {
  ORDINI_ARTISTA,
  artistaEffettivo,
  tornaAInRiproduzione,
  trovaArtista,
  tutteLePagine,
} from "./artista";
import { useAspetto } from "./aspettoFinestra";
import { useCartelleSorvegliate } from "./cartelleSorvegliate";
import { useCasa } from "./casa";
import { Chiedi } from "./Chiedi";
import {
  AvvisoAudioCollegato,
  CartelleCollegate,
  ConScansione,
  ElencoBraniCollegato,
  InRiproduzioneCollegata,
  InRiproduzioneTelefonoCollegata,
} from "./collegati";
import { MenuDellaCoda } from "./Coda";
import { ConfermaEliminazione } from "./ConfermaEliminazione";
import { ConfermaGesto, type Domanda as DomandaDiConferma } from "./ConfermaGesto";
import { ConfermaRimozioni } from "./ConfermaRimozioni";
import {
  AvvisoMaiusc,
  AvvisoSostituita,
  ConfermaSkin,
  ConfermaSostituzione,
  TieniSkin,
} from "./ConfermaSkin";
import { ConfermaTrasloco } from "./ConfermaTrasloco";
import { Copertina } from "./Copertina";
import { CARTELLA_DELLA_1_0 } from "./eredita";
import { chiaveDellApertura, chiusuraDi } from "./foglio";
import { brani_, durata, nomeArtista, numero, pesoDi, titoloAlbum } from "./formato";
import { SCHEDA_FERMA, fuocoDiRitorno, fuocoSullaTestata } from "./fuocoPagina";
import { Fornitori } from "./Fornitori";
import {
  Giro,
  apriIlMenuDiUnaRiga,
  unModaleAperto,
  type NomePasso,
} from "./Giro";
import { codaDaSvuotare, comeParteIlGiro, propostaPuoComparire } from "./guida";
import { Impaginazione, type ContestoScafale } from "./Impaginazione";
import { Importa, ImportaLink } from "./Importa";
import { ImportaPlaylist } from "./ImportaPlaylist";
import {
  ImpostazioniDellaFinestra,
  type AzioniImpostazioni,
} from "./ImpostazioniFinestra";
import { gestoPermesso, idDiLibreria } from "./insieme";
import {
  eErroreIpc,
  eRitentabile,
  guastoDa,
  ipc,
  type Guasto,
  type Album,
  type Artista,
  type Brano,
  type ContoPartecipazioni,
  type OrdineArtista,
  type AvanzamentoSpecchio,
  type OrigineLibreria,
  type Cancellazione,
  type EsitoScansione,
  type Ordine,
  type RimozioniDaConfermare,
  type Playlist,
  type VistaBrani,
  type VoceSkin,
} from "./ipc";
import { conteggiDelFiltro, conteggiDi, creaLibreria, dopoLaLettura } from "./libreria";
import { nomeDiFile } from "./nomeDiFile";
import { t, tSe } from "./lingue";
import { Trans } from "./lingue/Trans";
import { Menu, type Apertura } from "./Menu";
import {
  vociAlbum,
  vociArtista,
  vociBrani,
  vociPlaylist,
  type AzioniBrani,
} from "./menuBrani";
import { modaliAperti, unaVolta } from "./modale";
import { NuovoTema } from "./NuovoTema";
import { chiaveDiPagina, paginaCorrente, type VistaLibreria } from "./pagina";
import { sportelloAscolti, useAscolto, useRada } from "./pagine";
import { Annuncio, AnnuncioDelBrano } from "./parti/Annuncio";
import { AnnuncioScansione } from "./parti/AnnuncioScansione";
import { AvvisoAggiornamento } from "./parti/Aggiornamenti";
import { notaDelPermesso, useAccessoTelefono } from "./parti/AccessoTelefono";
import { AvvisoCoda } from "./parti/AvvisoCoda";
import { AvvisoDatabase } from "./parti/Avvisi";
import { LibreriaDiPrima } from "./parti/LibreriaDiPrima";
import { NovitaAllAvvio } from "./parti/Novita";
import { PropostaSottofondo } from "./parti/PropostaSottofondo";
import { AvvisoConti } from "./parti/AvvisoConti";
import { Fascia, type AvvisoNeutro } from "./parti/Fascia";
import { Icona } from "./parti/Icone";
import { ConImportazioni } from "./parti/Importazioni";
import { AvvisiInsieme, InsiemeInApp, InvitiInsieme, apriFinestra } from "./parti/Insieme";
import { LontanoInApp } from "./parti/Dispositivi";
import { Intestazione } from "./parti/Intestazione";
import { BarraAlta, TastoBarra } from "./parti/BarraAlta";
import { SezioneArtista } from "./parti/SezioneArtista";
import { TestataDettaglio } from "./parti/TestataDettaglio";
import { mescolato } from "./mescolato";
import { dispositivoDellaRiga } from "./ipc/libreria";
import { BarraCerca } from "./parti/BarraCerca";
import { PilloleLibreria } from "./parti/PilloleLibreria";
import { PilloleOrigine } from "./parti/PilloleOrigine";
import { StrisciaScarico, StrisciaSpecchio } from "./parti/StrisciaSpecchio";
import { VuotoDelFiltro } from "./parti/VuotoDelFiltro";
import { DomandaSpecchio } from "./ConfermaSpecchio";
import { useNumeriDelFiltro } from "./numeriDelFiltro";
import {
  TUTTI,
  chiaviDiLibreria,
  eFiltrabile,
  origineCheContiene,
  origineConChiavi,
  origineDi,
  origineVedeIlDispositivo,
  ugualiOrigini,
  perIlNucleo,
  primoSpento,
  rigaSuonabile,
  statoDellaRiga,
  suonabili,
  vistaPerIlFiltro,
  type StatoDellaRiga,
} from "./origine";
import type { Vista } from "./parti/Navigazione";
import { PilaToast } from "./parti/PilaToast";
import { SaltaAlContenuto } from "./parti/SaltaAlContenuto";
import { ToastImportazioni } from "./parti/ToastImportazioni";
import { usePlaylistAperta } from "./playlistAperta";
import {
  COPERTINA_IN_VOLO,
  Presenza,
  PresenzaPagina,
  mappaPresenze,
  useSpecchioPresente,
  useStatoPresente,
} from "./presenza";
import { Primo } from "./Primo";
import { MISURA_PAGINA, elencoPieno, type ArchivioRado, type Elenco } from "./rada";
import { fasciaDaRitirare, quandoRispondonoLeRadici } from "./radiciVive";
import { RecintoErrori } from "./Recinto";
import { indietro, indietroSulComputer } from "./indietro";
import { Regole } from "./Regole";
import { useRicerca } from "./ricerca";
import { Ripristino } from "./Ripristino";
import { useRiproduzione } from "./riproduzione";
import { useAvanzamentoScansione } from "./scansione";
import { nienteInCorso } from "./schermoIntero";
import { Altro } from "./schermate/Altro";
import { Artisti } from "./schermate/Artisti";
import { ElencoPlaylist } from "./schermate/ElencoPlaylist";
import { CartelleFinte } from "./schermate/Cartelle";
import { ElencoFinto, GrigliaFinta } from "./schermate/Elenco";
import {
  Esplora,
  ESPLORA_INIZIALE,
  type StatoEsplora,
} from "./schermate/Esplora";
import { Home, testaHome } from "./schermate/Home";
import {
  SchermataImportazioni,
  TestaImportazioni,
} from "./schermate/Importazioni";
import { etichettaSezione, type Sezione } from "./schermate/Impostazioni";
import { GrigliaAlbum, StatoVuoto, testaLibreria } from "./schermate/Libreria";
import {
  eDellaLibreria,
  passoIndietroTelefono,
  radiceDellaScheda,
  schedaDi,
  type SchedaTelefono,
  type SegmentoCerca,
} from "./schedeTelefono";
import { chiaveDellaSelezione } from "./selezione";
import { useSelezione } from "./selezioneVista";
import { useSkinAttiva } from "./skinAttiva";
import { useStabile } from "./stabile";
import { vinceLUltima } from "./ultima";
import { PillolaProva } from "./studio/PillolaProva";
import { sorgenteNuova, type DatiTema } from "./studio/nuovo";
import { campoRicerca, useScorciatoie } from "./tastiera";
import { useTerzaColonna } from "./terzaColonna";
import { useTrascinati } from "./trascinamento";
import { cambiandoVista, comeScorrere } from "./transizione";

/**
 * Lo Studio, caricato quando si apre.
 *
 * È la parte più pesante dell'applicazione — l'editor di codice, la timeline,
 * i controlli di ogni proprietà — e chi ascolta musica non la apre quasi mai:
 * tenerla nel pacchetto principale voleva dire leggerla a ogni avvio. Da file
 * locale arriva in un attimo, e nel frattempo si vede lo Studio che carica.
 *
 * Sul telefono non c'è proprio, nemmeno come pacchetto da caricare. Disegnare
 * una skin vuol dire una tela, un ispettore, un editor di codice e una chat
 * accanto: in quattrocento pixel erano tre pannelli da alternare con le
 * linguette, e ognuno era troppo stretto per il suo mestiere. Una skin fatta sul
 * computer arriva sul telefono col profilo, o si installa dal file. `null` e non
 * un `lazy` mai chiamato: un `import()` scritto resta un pezzo del pacchetto
 * anche se nessuno lo apre, e questo con l'editor dentro pesa più di mezzo
 * megabyte nell'APK.
 */
const Studio = MOBILE
  ? null
  : lazy(() => import("./studio/Guscio").then((m) => ({ default: m.Studio })));

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
 * Le viste della libreria che restano montate quando se ne esce.
 *
 * Sotto `<Activity>`: nascoste, i loro effetti si smontano — niente ascolti,
 * niente richieste — ma lo stato resta, e tornandoci lo scorrimento, il fuoco e
 * la selezione sono dove erano. Prima ogni ritorno ricominciava dall'alto e
 * richiedeva tutto: guardare un album e tornare alla griglia voleva dire
 * ritrovare la griglia in cima, cento dischi prima di quello appena aperto.
 *
 * Solo queste quattro: sono le destinazioni della libreria, quelle a cui si
 * torna. La ricerca, un album, una playlist cambiano contenuto a ogni apertura,
 * e tenerne una montata vorrebbe dire tenere quella di prima.
 */
const VISTE_TENUTE = ["brani", "preferiti", "album", "artisti"] as const;
type VistaTenuta = (typeof VISTE_TENUTE)[number];

/** Chi ha il fuoco adesso, se non è il `<body>`: lì torna chiudendo. */
function daChi(): HTMLElement | null {
  const adesso = document.activeElement;
  return adesso instanceof HTMLElement && adesso !== document.body ? adesso : null;
}

/** Una playlist appena eliminata, finché il suo «Annulla» è a schermo. */
interface PlaylistCancellata {
  /** Il gettone della cancellazione: ripassato a `playlistRipristina`. */
  gettone: number;
  nome: string;
}

/**
 * «Ci ho ripensato»: il «no» al dialogo del Cestino di Android. Lo si lancia
 * dentro `elimina` per fare la strada del rimedio — le righe tolte in anticipo
 * tornano — senza la fascia d'errore, che un ripensamento non è.
 */
const RIPENSATO = Symbol("ripensato");

/** Nessun brano: un elenco vuoto con un'identità sola, per le dipendenze. */
const NESSUN_BRANO: Brano[] = [];

/** Nessuna scelta: la selezione delle viste nascoste, con un'identità sola. */
const NESSUNA_SCELTA: ReadonlySet<number> = new Set();

/**
 * Il gesto di selezione che non fa niente, per gli elenchi in cui non si sceglie:
 * le partecipazioni di un artista, sotto i suoi brani. Un'identità sola, perché
 * le righe di un elenco la ricevono come prop.
 */
const senzaSelezione = (): void => undefined;

/**
 * Quanto si aspetta il secondo evento di una raffica prima di rileggere la
 * libreria: vedi `ricaricaAccorpata`.
 *
 * Il nucleo li manda uno dopo l'altro dallo stesso filo, e arrivano a pochi
 * millisecondi; cinquanta li prendono anche con la finestra impegnata a
 * disegnare, e dopo una scansione di secondi tre fotogrammi non si vedono.
 */
const ACCORPA_RICARICHE_MS = 50;

/**
 * Di quanto si muove il volume con la tastiera: un ventesimo della corsa.
 *
 * Venti pressioni da muto a pieno: abbastanza fine da trovare il punto, e
 * abbastanza grosso da non dover tenere giù il tasto. La curva percettiva sta
 * nel nucleo, quindi i passi si sentono uguali anche se i numeri non lo sono.
 */
const PASSO_VOLUME = 0.05;

/**
 * Com'è andata una scansione, in una riga per la fascia.
 *
 * Le stesse frasi della scheda in Impostazioni › Cartelle e scansione, che
 * resta il posto dei dettagli — i percorsi delle cartelle mute, i file
 * illeggibili —: qui i numeri, e le due cose che non devono passare
 * inosservate, una cartella che non ha risposto e le rimozioni trattenute.
 */
function riassuntoScansione(esito: EsitoScansione): string {
  const parti = [
    t("settings.scan.result", {
      inseriti: esito.inseriti,
      aggiornati: esito.aggiornati,
      spostati: esito.spostati,
      tolti: esito.tolti,
      secondi: numero(esito.durataMs / 1000, 1),
    }),
  ];
  if (esito.annullata) parti.push(t("settings.scan.stopped.strong"));
  if (esito.radiciSaltate.length > 0) {
    parti.push(t("settings.scan.skipped.strong", { n: esito.radiciSaltate.length }));
  }
  if (esito.rimozioniRinviate > 0) {
    parti.push(t("settings.scan.held.strong", { n: esito.rimozioniRinviate }));
  }
  // I segnaposto di OneDrive & C. non letti, per non farli scaricare (D-101).
  if (esito.soloInRete > 0) {
    parti.push(t("settings.scan.onlineOnly.strong", { n: esito.soloInRete }));
  }
  return parti.join(" · ");
}

export function App() {
  const [errore, setErrore] = useStatoPresente<Guasto | null>(null);
  const segnalaErrore = useCallback((e: unknown) => setErrore(guastoDa(e)), []);
  // Un ascolto del nucleo che non si apre finisce nella stessa fascia: vedi
  // `sportelloAscolti` in `pagine.ts`.
  useEffect(() => sportelloAscolti(segnalaErrore), [segnalaErrore]);
  /**
   * `avvio`, le playlist, la scansione e il suo esito: vedi `libreria.ts`, e lì
   * perché sono quattro archivi e non quattro stati di questo componente.
   * `App` si iscrive ad `avvio` e alle playlist, che decidono la pagina; della
   * scansione vuole sapere soltanto **se** ce n'è una (`scansionando`), e il
   * suo avanzamento lo leggono i componenti che lo disegnano (`ConScansione`).
   */
  /**
   * Il guasto che la libreria stessa ha messo nella fascia, per identità: una
   * rilettura nata da un evento toglie questo e nessun altro. Vedi
   * `dopoLaLettura` in `libreria.ts`.
   */
  const guastoLibreria = useRef<Guasto | null>(null);
  const [libreria] = useState(() =>
    creaLibreria(
      { avvio: () => ipc.avvio(), playlistElenco: () => ipc.playlistElenco() },
      {
        onErrore: (e) => {
          const guasto = guastoDa(e);
          guastoLibreria.current = guasto;
          setErrore(guasto);
        },
        onLetta: (tutto) =>
          setErrore((ora) => dopoLaLettura(ora, guastoLibreria.current, tutto)),
      },
    ),
  );
  const { ricarica, rileggi, ricaricaPlaylist } = libreria;
  const avvio = useArchivio(libreria.avvio);
  const playlist = useArchivio(libreria.playlist);
  const esito = useArchivio(libreria.esito);
  const scansionando = useSpecchioPresente(libreria.scansione, (s) => s !== null);
  /**
   * Una notizia riuscita, non un guasto.
   *
   * Un canale suo e non quello degli errori: ci ho messo dentro «5 brani
   * scritti in …» e a schermo è comparso in rosso, con il triangolo d'avviso
   * accanto — un'esportazione andata bene che sembra fallita. Il colore di una
   * fascia è metà del suo messaggio, e riusare il canale sbagliato è il modo
   * più economico di dire la cosa opposta.
   */
  const [notizia, setNotizia] = useStatoPresente<string | null>(null);
  /**
   * Un avviso neutro: una cosa da sapere che non è né un successo né un guasto —
   * un dispositivo che non risponde. Un canale suo, con la sua icona: nella
   * notizia sarebbe comparso con la spunta del successo, e nella fascia rossa
   * sarebbe sembrato un errore. Si legge e se ne va, come la notizia.
   */
  const [avviso, setAvviso] = useStatoPresente<AvvisoNeutro | null>(null);
  useEffect(() => {
    if (avviso === null) return;
    const conto = window.setTimeout(() => setAvviso(null), 8_000);
    return () => window.clearTimeout(conto);
  }, [avviso]);
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
  /**
   * L'ultima playlist cancellata, finché il suo «Annulla» è a schermo.
   *
   * Il gettone è quello che il nucleo ha dato alla cancellazione: ripassato a
   * `playlistRipristina`, rimette la playlist com'era. Dieci secondi come
   * l'«Annulla» della coda (`parti/AvvisoCoda.tsx`), e per la stessa ragione:
   * chi si accorge di aver sbagliato voce del menù se ne accorge un attimo dopo.
   * Una cancellazione nuova prende il posto della precedente, e ricomincia il
   * conto.
   *
   * Il conto si ferma col fuoco o il puntatore sul riquadro, e `Ctrl+Z` fa
   * l'ultimo «Annulla» a schermo: chi usa la tastiera trovava il riquadro
   * lontano dal fuoco, e spesso spariva mentre ci arrivava (`annullaATempo.ts`).
   * Lo stato è uno per le due copie del riquadro. Il gesto che rimette la
   * playlist sta in `rimettiPlaylist`, più giù, perché usa la rilettura delle
   * playlist e la notizia, che nascono dopo.
   */
  const rimettiPlaylist = useRef<(quale: PlaylistCancellata) => void>(() => undefined);
  const [annulloPlaylist] = useState(() =>
    annullaATempo<PlaylistCancellata>((quale) => rimettiPlaylist.current(quale)),
  );
  const cancellata = useArchivio(annulloPlaylist.avviso);
  /**
   * La playlist da eliminare, finché la domanda è aperta (D-40).
   *
   * L'«Annulla» dopo resta, ma da solo non bastava: per chi usa la tastiera o
   * un lettore di schermo era in pratica irraggiungibile, e l'eliminazione
   * diventava irreversibile. `risposta` è il cancello della domanda
   * (`unaVolta`), come per `daEliminare`: un doppio clic su «Elimina» non
   * cancella due volte.
   */
  const [daCancellare, setDaCancellare] = useStatoPresente<{
    playlist: Playlist;
    risposta: () => boolean;
  } | null>(null);
  /**
   * «Scarica qui» su un insieme grande, in attesa del sì: la domanda con il
   * numero di brani e lo spazio (`scaricaQui`). `null` finché non c'è.
   */
  const [confermaScarico, setConfermaScarico] = useStatoPresente<DomandaDiConferma | null>(null);
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
  const [aperto, setAperto] = useState<Album | null>(null);
  /** L'album la cui copertina vola verso la sua pagina: vedi `presenza.tsx`. */
  const [inVolo, setInVolo] = useState<string | null>(null);
  const [artistaAperto, setArtistaAperto] = useState<Artista | null>(null);
  /**
   * L'artista aperto dal nome in «In riproduzione», se è lui quello aperto:
   * chiudere la sua pagina riapre «In riproduzione» (`tornaAInRiproduzione`).
   * Un nome e non un segno: aperta un'altra pagina, o un altro artista, non
   * corrisponde più e Indietro è quello di sempre.
   */
  const [artistaDaInRiproduzione, setArtistaDaInRiproduzione] = useState<string | null>(null);
  /**
   * Da dove si guarda la libreria: il filtro per dispositivo («Tutti», «Questo
   * dispositivo», un dispositivo collegato). Vedi `origine.ts`.
   *
   * Uno solo per tutta la finestra — Album, Artisti, Brani, Preferiti e la
   * ricerca —, e non si ricorda fra un'apertura e l'altra: salvarlo vorrebbe
   * dire una chiave di impostazioni per un gesto che costa un tocco, e un
   * dispositivo che domani non c'è più.
   */
  const [origineScelta, setOrigine] = useState<OrigineLibreria>(TUTTI);
  /** I dispositivi collegati, per sapere se quello scelto c'è ancora. */
  const chiaviCollegate = useArchivio(archivioLontano, chiaviDiLibreria);
  const origine = useMemo(
    () => origineConChiavi(origineScelta, chiaviCollegate),
    [origineScelta, chiaviCollegate],
  );
  const [braniAperto, setBraniAperto] = useState<Brano[]>([]);
  const [ordine, setOrdine] = useState<Ordine>("scaffale");
  /**
   * Il primo avvio è stato chiuso a mano.
   *
   * Non va nel database: la condizione vera è «non c'è nessuna cartella
   * sorvegliata», e quella il database ce l'ha già. Questo serve solo a chi ha
   * premuto «Lo faccio dopo» e non vuole rivedere la schermata finché la
   * finestra resta aperta — riaprendo il programma senza aver scelto niente, la
   * proposta è ancora la risposta giusta.
   */
  const [primoChiuso, setPrimoChiuso] = useStatoPresente(false);
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
  /**
   * La libreria è da cominciare: nessuna cartella e nessun brano.
   *
   * Il brano conta quanto la cartella. Chi ha scelto di ascoltare soltanto
   * quel che trova in Esplora non ha cartelle e ha una libreria, e con la sola
   * condizione sulle cartelle si ritrovava davanti la proposta del primo
   * avvio — e dietro, il giro guidato — a ogni apertura del programma.
   */
  const nessunaCartella =
    avvio !== null &&
    avvio.cartelle.length === 0 &&
    avvio.numeri.tracks === 0;
  useEffect(() => {
    if (nessunaCartella) setPrimoAperto(true);
  }, [nessunaCartella]);
  const primoVisibile = !primoChiuso && (primoAperto || nessunaCartella);
  /*
   * La testata della Home, finché non si sa se questo è il primo avvio.
   *
   * Il primo avvio si apre quando arriva `avvio` e dice che non ci sono né
   * cartelle né brani; prima di allora si disegnava la Home, e sul telefono —
   * dove `avvio` arriva dopo qualche secondo — si leggeva «Riprendi da dove eri
   * rimasto» sopra lo scheletro, prima del benvenuto (R18-14). Finché `avvio`
   * non c'è, una testata neutra che dice soltanto che sta arrivando.
   */
  const testaDellaHome = avvio === null ? { titolo: t("list.loading") } : testaHome();
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
  const [giroAperto, setGiroAperto] = useStatoPresente(false);
  /**
   * Le Novità dopo un aggiornamento sono ancora da mostrare: il nucleo non ha
   * ancora risposto, o la finestra deve ancora aprirsi, o è aperta. Parte vero e
   * lo abbassa `NovitaAllAvvio`: quando il nucleo dice di no (installazione
   * nuova, versione senza novità, già viste), quando la domanda fallisce, o
   * quando la finestra si chiude. Vedi `parti/Novita.tsx`.
   */
  const [novitaInAttesa, setNovitaInAttesa] = useState(true);
  /**
   * La proposta «parti con Windows e resta nel vassoio» può comparire adesso.
   *
   * Mai durante `Primo`, né durante il giro — né quando il giro sta per
   * partire: chi comincia ha altro da guardare — e **dopo** la finestra delle
   * Novità: non solo «non aperta», ma «già risposta e chiusa», o la proposta
   * comparirebbe per il tempo che il nucleo impiega a rispondere. Il giro conta
   * **dove esiste**: sul telefono non parte mai, e `giroDaFare` resta vero per
   * sempre (vedi `propostaPuoComparire`). **È il solo posto** in cui si decide
   * quando aspettare: `PropostaSottofondo` non conosce né il giro né le Novità,
   * e non deve.
   */
  const propostaPuoMostrarsi = propostaPuoComparire({
    primoVisibile,
    giroDaFare,
    giroAperto,
    novitaInAttesa,
    telefono: MOBILE,
  });
  const [codaAperta, setCodaAperta] = useStatoPresente(false);
  const [daImportare, setDaImportare] = useStatoPresente<string | null>(null);
  /**
   * La finestrella del link è aperta.
   *
   * Un booleano e non più il servizio da cui la si è aperta: c'era un
   * suggerimento — Spotify o YouTube — che decideva segnaposto e diagnosi di
   * là, e non c'è più niente da suggerire. I cataloghi si riconoscono dal link,
   * e chi incolla non deve sceglierne uno prima di sapere cosa ha negli
   * appunti.
   */
  const [importandoLink, setImportandoLink] = useStatoPresente(false);
  const [importandoAccount, setImportandoAccount] = useStatoPresente(false);
  /** Il file di playlist da importare, o `null`. */
  const [filePlaylist, setFilePlaylist] = useStatoPresente<string | null>(null);
  /**
   * L'editor delle regole: `null` chiuso, `{playlist: null}` per una nuova.
   *
   * Un oggetto e non due stati separati: «sto creando» e «sto modificando
   * questa» si escludono, e due booleani che si escludono sono due booleani che
   * prima o poi saranno veri insieme.
   */
  const [regoleAperte, setRegoleAperte] = useStatoPresente<{
    playlist: Playlist | null;
  } | null>(null);
  const [playlistAperta, setPlaylistAperta] = useState<Playlist | null>(null);
  const [daAggiungere, setDaAggiungere] = useStatoPresente<number[] | null>(null);
  const [daRinominare, setDaRinominare] = useStatoPresente<Playlist | null>(null);
  /**
   * I brani che il menù ha proposto di togliere, e in che modo.
   *
   * `dalDisco` sceglie fra i due comandi e fra i due testi della conferma: sono
   * due gesti diversi — «non lo voglio in elenco» e «non lo voglio più» — e la
   * differenza va letta prima di premere, non scoperta dopo.
   *
   * `cambiata` quando la si riapre perché il nucleo ha rifiutato la conferma di
   * prima: vedi `elimina`.
   *
   * `risposta` è il cancello della domanda (`unaVolta` in `modale.ts`), creato
   * con lei: la finestrella si chiude con una transizione, e fino ad allora il
   * suo tasto risponde. Un doppio clic su «Elimina» mandava due cancellazioni
   * degli stessi file; Escape seguito subito da «Elimina» cancellava quel che
   * si era appena deciso di tenere. La domanda rifatta ne ha uno nuovo.
   */
  const [daEliminare, setDaEliminare] = useStatoPresente<{
    brani: number[];
    dalDisco: boolean;
    cambiata?: boolean;
    risposta: () => boolean;
  } | null>(null);
  const [creandoPlaylist, setCreandoPlaylist] = useStatoPresente(false);
  /** La finestrella del ripristino è aperta. */
  const [ripristinando, setRipristinando] = useStatoPresente(false);
  /**
   * Le rimozioni che l'ultima scansione a mano ha trattenuto, e che aspettano
   * un sì: vedi `ConfermaRimozioni.tsx`. L'elenco vero sta nel nucleo; qui c'è
   * il riassunto da mostrare e il gettone da rimandare.
   */
  const [rimozioniInAttesa, setRimozioniInAttesa] =
    useStatoPresente<RimozioniDaConfermare | null>(null);
  const [menu, setMenu] = useStatoPresente<Apertura | null>(null);
  const [grande, setGrande] = useState(false);
  /** La skin aperta nello Studio, o `null`. */
  const [studioAperto, setStudioAperto] = useState<string | null>(null);
  /** La finestrella che battezza un tema nuovo è aperta. */
  const [creandoTema, setCreandoTema] = useStatoPresente(false);
  /** Da quale skin partire, quando si è arrivati da «Deriva…». */
  const [baseTema, setBaseTema] = useState<string | null>(null);
  const contenuto = useRef<HTMLDivElement>(null);
  /*
   * Il fuoco, aprendo e chiudendo un album o un artista (R16-1, D-73): vedi
   * `fuocoPagina.ts`. Il cambio di vista smontava la scheda premuta e il fuoco
   * cadeva sul `<body>`; aprendo un disco non si sentiva niente.
   *
   * Aprendo va sulla testata della pagina nuova; chiudendo torna a chi aveva
   * aperto, che si ricorda **per livello**: un album si apre anche dentro un
   * artista, e chiuderlo riporta alla scheda del disco nella pagina
   * dell'artista, chiudere l'artista a quella dell'artista nella griglia.
   * `fuocoDopo` è la richiesta, eseguita dopo il disegno che ha cambiato la
   * pagina; chi la scrive sono `apriAlbum`, `apriArtista` e la scala di
   * Escape (`chiudi`).
   */
  const daChiAlbum = useRef<HTMLElement | null>(null);
  const daChiArtista = useRef<HTMLElement | null>(null);
  const fuocoDopo = useRef<
    | { verso: "testata"; daChi: HTMLElement | null; livello: RefObject<HTMLElement | null> }
    | { verso: "ritorno"; livello: RefObject<HTMLElement | null> }
    | null
  >(null);
  useEffect(() => {
    const chiesto = fuocoDopo.current;
    if (chiesto === null) return;
    fuocoDopo.current = null;
    // La pagina, e sul telefono anche la sua barra in alto, che porta il
    // titolo quando la testata è un'immagine.
    const pagina = {
      querySelector: (selettore: string) =>
        contenuto.current?.querySelector<HTMLElement>(selettore) ??
        document.querySelector<HTMLElement>(`[data-scafale] ${selettore}`),
    };
    const attivo = () => document.activeElement;
    if (chiesto.verso === "testata") {
      chiesto.livello.current = chiesto.daChi;
      fuocoSullaTestata(pagina, attivo);
    } else {
      const chi = chiesto.livello.current;
      chiesto.livello.current = null;
      fuocoDiRitorno([chi, pagina.querySelector(SCHEDA_FERMA)], pagina, attivo);
    }
  }, [aperto, artistaAperto]);
  const riproduzione = useRiproduzione();
  /*
   * Del motore, `App` sa soltanto se c'è un brano: decide se lo schermo intero
   * si può aprire, e se il giro deve mettere qualcosa in coda. Cambia quando si
   * comincia o si smette di ascoltare — non a ogni brano, non a ogni passo del
   * volume, non a ogni pausa. Lo stato intero lo leggono dall'archivio i
   * componenti che lo disegnano, e chi deve reagire a un brano nuovo — la Home,
   * l'accento della copertina — lo segue da sé (`casa.ts`,
   * `aspettoFinestra.ts`). Vedi «Lo stato, fuori da React» in `riproduzione.ts`.
   */
  const conBrano = useArchivio(riproduzione.archivio, (s) => s.brano !== null);
  /*
   * Quel che suonava è finito — la coda svuotata, o arrivata a nessun brano —
   * e lo schermo intero si chiude davvero, non solo smette di disegnarsi:
   * restando `grande` si mangiava il primo Indietro (o Escape) senza che
   * niente cambiasse a schermo, e il brano fatto partire dopo lo riapriva da
   * solo. Una scheda che tarda durante una scansione non lo chiude: vedi
   * `schermoIntero.ts`.
   */
  const finito = useArchivio(riproduzione.archivio, nienteInCorso);
  useEffect(() => {
    if (finito && grande) setGrande(false);
  }, [finito, grande]);

  /**
   * `avvio` all'apertura: conteggi, cartelle, preferenze. Vedi `libreria.ts`.
   *
   * Con `rileggi` e non con `ricarica`: non è un gesto, e un guasto arrivato
   * prima della risposta — un ascolto che non si apre, la skin che non si
   * legge — non deve sparire perché `avvio` è riuscito. Vedi `dopoLaLettura`.
   */
  useEffect(() => {
    void rileggi();
  }, [rileggi]);

  /**
   * La skin indossata, quelle installate, l'anteprima e la prova dello Studio:
   * vedi `skinAttiva.ts`. Qui, nel punto in cui stava la lettura della skin
   * all'apertura — subito dopo `avvio`, e prima del tema che la legge.
   */
  //
  // Una scelta durante «Prova sull'app» chiude la prova (D-67), e lo si dice:
  // lo Studio torna a coprire la finestra, e senza una parola sembrerebbe che
  // la scelta abbia riaperto l'editor da sola. Il nome dall'elenco di adesso,
  // letto quando l'avviso parte e non quando la funzione nasce.
  const elencoSkin = useRef<readonly VoceSkin[]>([]);
  const avvisaProvaChiusa = useCallback(
    (id: string) =>
      setNotizia(t("studio3.try.closed", { nome: elencoSkin.current.find((v) => v.id === id)?.nome ?? id })),
    [setNotizia],
  );
  const skinFinestra = useSkinAttiva(segnalaErrore, avvisaProvaChiusa);
  const { skin, skinAttiva, prova, scegliSkin, ricaricaSkin, finisciProva } = skinFinestra;
  elencoSkin.current = skin;
  const { daIndossare } = skinFinestra;
  const studioInProva = prova !== null;
  /** Lo Studio copre la finestra: l'app non prende né tasti né file. */
  const studioVisibile = studioAperto !== null && !studioInProva;

  /**
   * Tema, movimento, zoom, lingua, scorciatoie e l'accento della copertina:
   * vedi `aspettoFinestra.ts`, e lì l'ordine in cui registra i suoi effetti.
   */
  const aspetto = useAspetto({
    avvio,
    skinAttiva,
    lettore: riproduzione.archivio,
    accentoDaRimettere: skinFinestra.accentoDaRimettere,
    segnalaErrore,
    ricarica,
  });
  const { cambiaZoom, zoomNormale, scorciatoie } = aspetto;

  /**
   * Un profilo è appena stato applicato: si rilegge tutto quel che ha toccato.
   *
   * Il tema e la lingua salvati tornano da rileggere (`dimenticaTema`): è
   * l'unico caso in cui cambiano sotto i piedi della finestra, e la guardia che
   * impedisce di rileggerli a ogni ricarica qui va tolta di mano. «Meno
   * movimento» si rilegge, e l'elenco delle skin pure: lo «in uso» deve
   * seguire la skin del profilo. Quel che il motore audio
   * legge alla sua apertura (volume, equalizzatore, normalizzazione) resta di
   * prima fino al riavvio, e la scheda lo dice invece di lasciarlo scoprire.
   */
  const { dimenticaTema, rileggiAccento, rileggiMovimento } = aspetto;
  const { rileggiSkin } = skinFinestra;
  const dopoProfilo = useCallback(() => {
    dimenticaTema();
    void ricarica();
    rileggiSkin();
    void ricaricaSkin();
    rileggiAccento();
    rileggiMovimento();
  }, [dimenticaTema, ricarica, rileggiSkin, ricaricaSkin, rileggiAccento, rileggiMovimento]);

  /**
   * Fuori dallo Studio, per la sua porta o per quella del suo recinto.
   *
   * Una funzione sola per le due: chiudere dal recinto — lo Studio è caduto —
   * deve lasciare la finestra com'è dopo una chiusura normale, prova
   * dimenticata e skin rilette compresi.
   */
  /**
   * Il controllo che aveva il fuoco quando lo Studio si è aperto, per
   * rimettercelo all'uscita (R14-10). Senza, il fuoco cadeva sul `<body>` e
   * chi usa la tastiera o un lettore di schermo ripartiva dall'inizio della
   * finestra invece che da Impostazioni › Aspetto.
   */
  const fuocoPrimaDelloStudio = useRef<HTMLElement | null>(null);
  const ricordaIlFuoco = useCallback(() => {
    const qui = document.activeElement;
    fuocoPrimaDelloStudio.current = qui instanceof HTMLElement && qui !== document.body ? qui : null;
  }, []);
  const esciDalloStudio = useCallback(() => {
    setStudioAperto(null);
    skinFinestra.scordaProva();
    // Le skin possono essere cambiate mentre si era là dentro.
    void ricaricaSkin();
    // Dopo il disegno: finché lo Studio c'è, la finestra sotto è `inert`.
    const torna = fuocoPrimaDelloStudio.current;
    fuocoPrimaDelloStudio.current = null;
    if (torna !== null) {
      window.requestAnimationFrame(() => {
        if (torna.isConnected) torna.focus();
      });
    }
  }, [skinFinestra, ricaricaSkin]);

  /**
   * La terza colonna, che si chiude da sé quando la finestra si stringe: vedi
   * `terzaColonna.ts`. Qui, dove stava l'ascolto del ridimensionamento.
   */
  const { colonnaAperta, setColonnaAperta, colonnaAMano } = useTerzaColonna();

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
      // Un comando solo: il pacchetto si fa con le risorse e la miniatura
      // della base — fino alla 3.0 si installava il solo manifest, e un tema
      // nato da una skin con un carattere si fermava qui — e la bozza nasce
      // allineata all'installato, in una cartella che non era di nessuno.
      await ipc.studioCrea(dati.base, sorgente);
      await scegliSkin(dati.id);
      setCreandoTema(false);
      setStudioAperto(dati.id);
    } catch (e) {
      segnalaErrore(e);
    }
  };

  /**
   * L'avanzamento della scansione, dagli eventi all'archivio della libreria:
   * vedi `scansione.ts`. Qui, dove stavano i suoi ascolti.
   */
  const { posaScansione, scansioniMie } = useAvanzamentoScansione(libreria);

  // La ricerca ha la precedenza su qualunque vista: quel che si sta cercando è
  // ciò che si vuole vedere. Il passaggio in modalità ricerca è **immediato**,
  // la richiesta no: quel che si chiede al nucleo segue con un ritardo.
  const cercando = query.trim().length > 0;
  const pagina = paginaCorrente({
    vista,
    cercando,
    playlist: playlistAperta,
    album: aperto,
    artista: artistaAperto,
  });

  /*
   * ── le schede del telefono ─────────────────────────────────────────────────
   *
   * La scheda accesa si ricava dalla pagina, non si tiene: vedi
   * `schedeTelefono.ts`. Quel che si tiene è la **memoria** delle schede, perché
   * tornarci vuol dire ritrovarle com'erano: l'ultima pillola della Libreria,
   * l'ultimo segmento di Cerca e l'ultima frase cercata. Tre riferimenti scritti
   * durante il disegno e non tre stati: si leggono solo al tocco su una scheda,
   * e cambiarli non deve ridisegnare niente.
   *
   * Sul computer ci sono anche loro e non servono a nessuno: un `if (MOBILE)`
   * attorno a degli hook non si può scrivere, e tre riferimenti costano meno
   * della regola che si romperebbe.
   */
  const scheda = schedaDi(pagina, vista);
  const pillola = useRef<VistaLibreria>("playlist");
  if (eDellaLibreria(vista)) pillola.current = vista;
  const segmentoCerca = useRef<SegmentoCerca>("cerca");
  if (vista === "cerca" || vista === "esplora") segmentoCerca.current = vista;
  // Si ricorda finché la si svuota a mano o con Indietro: cambiare scheda la
  // toglie dal campo (`vaiA`), e tornando su Cerca la si ritrova lì.
  const ultimaRicerca = useRef("");
  if (MOBILE && cercando) ultimaRicerca.current = query;
  /**
   * Le Impostazioni del telefono hanno due livelli: l'elenco delle sezioni, e
   * una sezione. Falso vuol dire l'elenco. Sul computer non si legge: le
   * sezioni stanno accanto alla scheda, sempre tutte e due in vista.
   */
  const [dentroLaSezione, setDentroLaSezione] = useState(false);
  /**
   * La sezione aperta è stata aperta da fuori (`apriSezione` con `daFuori`):
   * Indietro torna da dove si veniva, senza passare dall'elenco delle sezioni.
   * Vedi `passoIndietroTelefono`.
   */
  const sezioneDaFuori = useRef(false);

  /**
   * Gli elenchi di brani della libreria: una lista rada ciascuno.
   *
   * Erano un `usePagine` solo, con tre sorgenti e una chiave — la ricerca, i
   * preferiti e la vista Brani — che portava le righe dall'alto a duecento per
   * volta. Adesso le viste restano montate (`VISTE_TENUTE`) e ognuna tiene le
   * pagine sue: tornando ai Preferiti dopo i Brani le righe sono quelle
   * lasciate. E la barra di scorrimento è lunga quanto la vista dal primo
   * istante, perché il totale lo dice `avvio.numeri` prima che arrivi una riga:
   * le pagine si chiedono dove si guarda, per offset. Vedi `rada.ts`.
   *
   * Gli archivi stanno qui e non dentro le viste perché i gesti che li cambiano
   * — un voto, un cuore, un brano tolto — partono da qui; ma ridisegnano solo
   * l'elenco che li disegna, non questo componente (`useRada`).
   */
  /**
   * I numeri della libreria vista dal filtro per dispositivo: quelli di `avvio`
   * per «Tutti», gli altri dal nucleo (`conteggi`). Sono la lunghezza degli
   * elenchi che seguono, e la chiave di ognuno porta anche l'origine: un altro
   * filtro è un altro elenco, dal primo posto.
   */
  const numeriDelFiltro = useNumeriDelFiltro(origine, avvio?.numeri, avvio, segnalaErrore);
  const origineIpc = perIlNucleo(origine);
  const chiaveOrigine = origine.tipo === "dispositivo" ? `d:${origine.chiave}` : origine.tipo;
  const radaBrani = useRada<Brano>({
    chiave: `brani:${ordine}:${chiaveOrigine}`,
    totale: numeriDelFiltro?.tracks ?? 0,
    chiedi: (offset, limite) => ipc.brani(ordine, offset, limite, origineIpc),
    onErrore: segnalaErrore,
  });
  const radaPreferiti = useRada<Brano>({
    chiave: `preferiti:${chiaveOrigine}`,
    totale: numeriDelFiltro?.liked ?? 0,
    chiedi: (offset, limite) => ipc.preferiti(offset, limite, origineIpc),
    onErrore: segnalaErrore,
  });

  /**
   * La ricerca: la frase pigra, quanti risultati ha, e le loro righe. Vedi
   * `ricerca.ts`.
   */
  const ricerca = useRicerca(query, segnalaErrore, origine);
  const { queryPigra, risultatiAdesso, contandoRisultati } = ricerca;
  const radaRicerca = ricerca.rada;

  /**
   * Gli album: la griglia intera, e quelli di un artista.
   *
   * Due archivi e non una chiave sola: la griglia degli Album resta montata
   * mentre si guarda un artista, e le sue pagine non devono cedere il posto a
   * quelle di lui. Il totale dell'artista è il suo `albums`, contato dal nucleo
   * con la stessa chiave con cui li raggruppa; se fosse di un istante prima, la
   * lista si corregge da sé alla prima pagina più corta.
   */
  const radaAlbum = useRada<Album>({
    chiave: `album:${chiaveOrigine}`,
    totale: numeriDelFiltro?.albums ?? 0,
    chiedi: (offset, limite) => ipc.album(offset, limite, origineIpc),
    onErrore: segnalaErrore,
  });
  const radaAlbumArtista = useRada<Album>({
    chiave: `artista:${artistaAperto?.name ?? ""}:${chiaveOrigine}`,
    totale: artistaAperto?.albums ?? 0,
    chiedi: (offset, limite) =>
      artistaAperto
        ? ipc.albumArtista(artistaAperto.name, offset, limite, origineIpc)
        : Promise.resolve([]),
    onErrore: segnalaErrore,
  });

  /*
   * ── la pagina di un artista, sotto gli album ───────────────────────────────
   *
   * Tre elenchi, ciascuno una lista rada: i suoi **brani** — tutti, anche quelli
   * senza disco —, e le sue partecipazioni in due specie che non si confondono
   * mai: `esatte` (il suo nome in un disco di altri: le raccolte) ed
   * `euristiche` (il suo nome fra i crediti di un brano di un altro: i «feat.»,
   * una ricerca che può sbagliare e la pagina lo dice). Le chiavi portano il
   * nome e il filtro: un altro artista, o un altro dispositivo, è un altro elenco.
   */
  const [ordineArtista, setOrdineArtista] = useState<OrdineArtista>("album");
  const nomeAperto = artistaAperto?.name ?? "";
  const chiaveArtista = `${nomeAperto}:${chiaveOrigine}`;
  /** Quante partecipazioni, e di quale artista e filtro: un conto vecchio non vale. */
  const [conteggioPartecipazioni, setConteggioPartecipazioni] = useState<{
    chiave: string;
    conto: ContoPartecipazioni;
  } | null>(null);
  const partecipazioni: ContoPartecipazioni =
    conteggioPartecipazioni?.chiave === chiaveArtista
      ? conteggioPartecipazioni.conto
      : { esatte: 0, euristiche: 0 };
  const radaBraniArtista = useRada<Brano>({
    chiave: `brani-artista:${chiaveArtista}:${ordineArtista}`,
    totale: artistaAperto?.tracks ?? 0,
    chiedi: (offset, limite) =>
      nomeAperto === ""
        ? Promise.resolve([])
        : ipc.braniArtista(nomeAperto, ordineArtista, offset, limite, origineIpc),
    onErrore: segnalaErrore,
  });
  const radaEsatte = useRada<Brano>({
    chiave: `compare-anche-in:${chiaveArtista}`,
    totale: partecipazioni.esatte,
    chiedi: (offset, limite) =>
      nomeAperto === ""
        ? Promise.resolve([])
        : ipc.partecipazioni(nomeAperto, "esatte", offset, limite, origineIpc),
    onErrore: segnalaErrore,
  });
  const radaEuristiche = useRada<Brano>({
    chiave: `forse-anche-in:${chiaveArtista}`,
    totale: partecipazioni.euristiche,
    chiedi: (offset, limite) =>
      nomeAperto === ""
        ? Promise.resolve([])
        : ipc.partecipazioni(nomeAperto, "euristiche", offset, limite, origineIpc),
    onErrore: segnalaErrore,
  });
  /** Rilegge l'artista aperto e le sue partecipazioni: cresce a ogni rilettura della libreria. */
  const [giroArtista, setGiroArtista] = useState(0);
  /**
   * L'artista aperto, riletto col filtro di adesso.
   *
   * I suoi numeri — `albums` e `tracks` — sono la lunghezza delle due liste rade
   * qui sopra, e vengono dalla griglia degli artisti **come era col filtro di
   * allora**. Passando da «Questo dispositivo» a «Tutti» una lista rimaneva corta,
   * e una pagina «che si corregge da sé alla prima pagina più corta» si corregge
   * solo in un verso. Un artista che col filtro nuovo non ha niente resta aperto
   * con zero: la pagina lo dice, invece di sparire sotto le dita.
   *
   * Letto anche da una libreria che cambia (`giroArtista`): un brano tolto, una
   * scansione finita. Non per la lettura del catalogo di un dispositivo che il
   * filtro non vede: vedi `ricaricaContorno`.
   *
   * # Due letture, non una (S9-3)
   *
   * I conti delle partecipazioni sono veloci e non aspettano l'elenco intero
   * degli artisti: legati in un `Promise.all`, un database occupato dallo specchio
   * che faceva fallire la seconda lettura toglieva anche le sezioni «Compare anche
   * in». Quanto all'elenco intero — migliaia di artisti, megabyte di JSON, per
   * due numeri — serve un comando del nucleo che dia l'artista solo: finché non
   * c'è, si rilegge meno spesso che si può.
   */
  useEffect(() => {
    if (nomeAperto === "") return;
    let superata = false;
    ipc
      .partecipazioniConta(nomeAperto, origineIpc)
      .then((conto) => {
        if (!superata) setConteggioPartecipazioni({ chiave: chiaveArtista, conto });
      })
      .catch((e: unknown) => {
        if (!superata) segnalaErrore(e);
      });
    ipc
      .artisti(origineIpc)
      .then((elenco) => {
        if (superata) return;
        const suo = elenco.find((a) => a.name === nomeAperto);
        setArtistaAperto((prima) => {
          if (prima === null || prima.name !== nomeAperto) return prima;
          const adesso = suo ?? { ...prima, albums: 0, tracks: 0, covers: [] };
          return adesso.albums === prima.albums &&
            adesso.tracks === prima.tracks &&
            adesso.covers.join() === prima.covers.join()
            ? prima
            : adesso;
        });
      })
      .catch((e: unknown) => {
        if (!superata) segnalaErrore(e);
      });
    return () => {
      superata = true;
    };
    // `origineIpc` e `segnalaErrore` non servono come dipendenze: il filtro è nella
    // chiave, e l'errore si dice con quel che c'è.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [nomeAperto, chiaveArtista, giroArtista]);

  /**
   * Gli artisti della vista Artisti, tenuti finché la libreria non cambia:
   * vedi `artisti.ts`.
   */
  const { artisti, artistiInArrivo, artistiGuasto, invalidaArtisti } = useArtisti(
    vista,
    cercando,
    segnalaErrore,
    origine,
  );

  /**
   * La Home: i ripiani e le raccolte del lunedì, entrando e restando. Vedi
   * `casa.ts`.
   */
  const { casa, guasto: guastoCasa, settimana, setSettimana, rileggiCasa } = useCasa({
    vista,
    cercando,
    lettore: riproduzione.archivio,
    segnalaErrore,
  });

  /**
   * La vista tenuta montata che si sta guardando, o `null` se la pagina è
   * un'altra: vedi `VISTE_TENUTE`.
   */
  const vistaTenuta: VistaTenuta | null =
    pagina.tipo === "libreria" &&
    (VISTE_TENUTE as readonly string[]).includes(pagina.vista)
      ? (pagina.vista as VistaTenuta)
      : null;

  // In cima a ogni cambio di elenco: la posizione di scorrimento di prima
  // appartiene a un contenuto che non c'è più. Tranne entrando in una vista
  // tenuta montata, che lo scorrimento se lo rimette da sé — e prima di questo
  // effetto, che girando dopo glielo toglierebbe: vedi «La memoria» in
  // `virtuale.ts`.
  useEffect(() => {
    if (vistaTenuta !== null) return;
    contenuto.current?.scrollTo({ top: 0 });
  }, [vista, ordine, queryPigra, aperto, playlistAperta, artistaAperto, vistaTenuta]);

  /**
   * Rifà quel che la vista aperta sta mostrando.
   *
   * Non butta niente: le liste rade si **invalidano**, cioè tengono a schermo
   * le righe che hanno e le richiedono — solo quelle che qualcuno guarda, e una
   * vista nascosta quando torna. Prima un «ricarica» rimetteva l'elenco alla
   * prima pagina e lo scorrimento in cima, dopo ogni voto fallito e ogni
   * scansione finita.
   */
  const ricaricaBrani = useCallback(() => {
    radaBrani.invalida();
    radaPreferiti.invalida();
    radaRicerca.invalida();
    // E i tre elenchi della pagina di un artista, che restano montati sotto di
    // lei. I loro totali si rileggono con `ricaricaContorno` (`giroArtista`).
    radaBraniArtista.invalida();
    radaEsatte.invalida();
    radaEuristiche.invalida();
  }, [radaBrani, radaPreferiti, radaRicerca, radaBraniArtista, radaEsatte, radaEuristiche]);
  const ricaricaAlbum = useCallback(() => {
    radaAlbum.invalida();
    radaAlbumArtista.invalida();
  }, [radaAlbum, radaAlbumArtista]);

  /**
   * Riscrive le righe di brani in mano, in tutte e tre le liste: un voto, un
   * cuore, un brano tolto devono vedersi anche nella vista che si ritrova
   * tornandoci, non solo in quella aperta.
   */
  const aggiornaBrani = useCallback(
    (f: (righe: Brano[]) => Brano[]) => {
      radaBrani.aggiorna(f);
      radaPreferiti.aggiorna(f);
      radaRicerca.aggiorna(f);
      radaBraniArtista.aggiorna(f);
      radaEsatte.aggiorna(f);
      radaEuristiche.aggiorna(f);
    },
    [radaBrani, radaPreferiti, radaRicerca, radaBraniArtista, radaEsatte, radaEuristiche],
  );

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
  const ricaricaContorno = useCallback((opzioni?: { senzaArtista?: boolean }) => {
    ricaricaAlbum();
    // L'artista aperto: i suoi numeri sono la lunghezza dei suoi elenchi, e un
    // brano che se ne va li cambia (`giroArtista`). Non per la lettura del
    // catalogo di un dispositivo che il filtro non vede (`senzaArtista`): rileggeva
    // l'elenco intero degli artisti ogni 3 s per numeri che non potevano cambiare
    // (S9-3).
    if (!opzioni?.senzaArtista) setGiroArtista((giro) => giro + 1);
    // Gli artisti si rileggono subito se la vista è aperta, entrandoci se no.
    invalidaArtisti();
    // La Home dopo una scansione o dei brani scaricati: i suoi ripiani sono
    // fatti di «di recente», e quel che è appena entrato in libreria li cambia
    // tutti e quattro. Entrandoci ci pensa invece l'effetto suo, che è quel che
    // fino a poco fa non esisteva e lasciava la schermata vuota.
    if (vista === "home") rileggiCasa();
  }, [ricaricaAlbum, invalidaArtisti, vista, rileggiCasa]);

  const caricaVista = useCallback(
    (opzioni?: { senzaArtista?: boolean }) => {
      ricaricaBrani();
      ricaricaContorno(opzioni);
    },
    [ricaricaBrani, ricaricaContorno],
  );

  /**
   * Rilegge la libreria dopo un cambiamento venuto dal nucleo: una volta per
   * raffica di eventi.
   *
   * Dei brani scaricati che entrano in libreria arrivano con **due** eventi di
   * fila — `scarico:in_libreria` e subito dopo `cartelle:cambiate` — e ognuno
   * rileggeva tutto: due `avvio`, due giri di pagine per ogni elenco aperto,
   * per una scansione sola. Adesso il primo apre una finestra di
   * `ACCORPA_RICARICHE_MS`, il secondo ci entra, e alla chiusura la libreria si
   * rilegge una volta (`accorpa.ts`). Le scansioni della sorveglianza, dalla
   * 3.0.2, mandano già soltanto `cartelle:cambiate`.
   *
   * Il lavoro passa da un riferimento perché `caricaVista` cambia con la vista
   * aperta: quel che parte alla chiusura deve essere quello di adesso, non
   * quello del disegno in cui la finestra si è aperta.
   */
  const ricaricaDalNucleo = useRef(() => {});
  /** Perché si rilegge, nella raffica in corso: vedi `causeDiRilettura`. */
  const cause = useRef(causeDiRilettura());
  useEffect(() => {
    ricaricaDalNucleo.current = () => {
      // Se ogni causa della raffica è la lettura di un catalogo che il filtro non
      // vede, i numeri dell'artista aperto non possono essere cambiati (S9-3).
      const senzaArtista = !cause.current.preleva();
      // `rileggi` e non `ricarica`: la rilettura nata da un evento toglie solo
      // il guasto messo dalla libreria. Una scansione a mano fallita manda
      // `cartelle:cambiate` e poi l'errore, e con `ricarica` la sua fascia
      // spariva cinquanta millesimi dopo. Vedi `dopoLaLettura`.
      void rileggi();
      caricaVista({ senzaArtista });
    };
  });
  const [ricaricaAccorpata] = useState(() =>
    accorpa(() => ricaricaDalNucleo.current(), ACCORPA_RICARICHE_MS),
  );
  useEffect(() => () => ricaricaAccorpata.butta(), [ricaricaAccorpata]);
  /**
   * Chiede la rilettura accorpata, dicendo se la causa può cambiare quel che il
   * filtro mostra: tutte tranne la lettura del catalogo di un dispositivo che il
   * filtro non contiene.
   */
  const chiediRilettura = useCallback(
    (riguardaIlFiltro: boolean) => {
      cause.current.aggiungi(riguardaIlFiltro);
      ricaricaAccorpata.chiama();
    },
    [ricaricaAccorpata],
  );

  /**
   * I brani scaricati sono entrati in libreria.
   *
   * La coda, quando finisce, rifà una scansione da sé: i file presi dai
   * cataloghi diventano brani veri senza che nessuno prema «Scansiona». Ma la vista
   * aperta continuerebbe a mostrare i conteggi di prima — una libreria a cui
   * sono appena arrivati quaranta brani che non compaiono finché non si cambia
   * schermata.
   *
   * Un ascolto suo anche se fa quel che fa `cartelle:cambiate`: l'evento porta
   * quanti brani sono entrati, ed è qui che andrebbe una notizia per i
   * download, se un giorno la si vorrà.
   */
  useAscolto("scarico:in_libreria", () => chiediRilettura(true));

  /*
   * La libreria è cambiata sotto: una scansione — anche quella che parte da
   * sola, dopo i download o dalla sorveglianza delle cartelle — un trasloco,
   * un profilo importato. È il segnale che il nucleo manda da un posto solo
   * (`cartelle::cambiate`), e quello a cui le viste tenute montate devono
   * credere: i conteggi tornano da `avvio`, e con loro la lunghezza degli
   * elenchi; le righe si invalidano, e quelle che si guardano si richiedono.
   * Una vista nascosta non chiede niente finché non torna.
   *
   * Accorpato con `scarico:in_libreria`, vedi sopra. Dopo una scansione a mano
   * arriva invece insieme alla risposta di `scansiona`, che fa le stesse due
   * cose per conto suo e non passa di qui: là la barra resta finché la
   * libreria non è riletta, e quella rilettura `scansiona` la deve aspettare.
   */
  useAscolto("cartelle:cambiate", () => chiediRilettura(true));

  /*
   * Il catalogo di un dispositivo collegato sta entrando in libreria (o ha
   * finito): `lontano:specchio`. Il giro scrive in `tracks` senza mandare
   * `cartelle:cambiate`, quindi gli elenchi non lo saprebbero. Si rileggono a
   * giro finito se è cambiato qualcosa, e durante la lettura ogni tanto: la
   * libreria si usa mentre arriva. Chi arriva dopo parte dalla fotografia.
   */
  const ultimaRilettura = useRef(0);
  useAscolto<AvanzamentoSpecchio>(
    "lontano:specchio",
    (nuovo) => {
      const prima = archivioSpecchio.leggi()[nuovo.dispositivo];
      archivioSpecchio.scrivi(conAvanzamento(archivioSpecchio.leggi(), nuovo));
      const adesso = Date.now();
      if (rileggereDopo(prima, nuovo, adesso, ultimaRilettura.current)) {
        ultimaRilettura.current = adesso;
        // Sotto «Questo dispositivo», o il filtro di un altro, il catalogo che
        // arriva non cambia le righe che si vedono, né i numeri dell'artista.
        chiediRilettura(origineVedeIlDispositivo(origine, nuovo.dispositivo));
      }
    },
    true,
    () => {
      ipc
        .lontanoSpecchioStato()
        .then((elenco) => archivioSpecchio.scrivi(daFotografia(elenco)))
        .catch(() => undefined);
    },
  );

  /*
   * La copertina di una riga di un altro dispositivo è arrivata nello store:
   * `lontano:copertina`. Il protocollo `rymembra-cover` non aspetta la rete — a
   * una copertina che lo store non ha ancora risponde «non c'è» subito, e il nucleo la
   * chiede in disparte (`media/lontane.rs`) —, e i segnaposto (`Copertina`) ci
   * riprovano quando arriva. Gli arrivi si raccolgono: vedi `copertinaArrivata`.
   */
  useAscolto<string>("lontano:copertina", () => copertinaArrivata());

  /*
   * Un brano saltato perché non si apriva: lo dice la fascia, con il titolo e
   * il motivo. Il nucleo riprova il brano dopo quando arriva il suo turno, e se
   * non si apre per colpa sua passa oltre (`riprova_o_salta` in
   * `riproduzione/mod.rs`): senza questo avviso la coda scorrerebbe via in
   * silenzio. Il motivo è la frase del catalogo per il codice, che è la stessa
   * della fascia d'errore; un errore di prima, ormai superato, si toglie.
   */
  useAscolto<{ id: number; titolo: string | null; codice: string }>(
    "riproduzione:saltato",
    ({ titolo, codice }) => {
      riproduzione.scartaErrore();
      const che = titolo
        ? t("playback.skipped", { titolo })
        : t("playback.skipped.unknown");
      const perche = tSe(chiaveDelTestoErrore(`errors.${codice}`), "");
      setNotizia(perche === "" ? che : `${che} ${perche}`);
    },
  );

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
   *
   * «Superata» vuol dire da chiunque, non solo da questo effetto: anche la
   * rilettura dopo un'eliminazione fallita (`elimina`) chiede i brani
   * dell'album, e prima li scriveva comunque, cioè sotto l'album che nel
   * frattempo si era aperto. Le due strade prendono il turno dallo stesso
   * contatore (`ultima.ts`), e chiudere l'album ne prende uno anche lui.
   */
  const albumDiPrima = useRef<string | null>(null);
  const [turnoAlbum] = useState(vinceLUltima);
  useEffect(() => {
    const mio = turnoAlbum();
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
        if (!superata && mio()) setBraniAperto(brani);
      })
      .catch((e: unknown) => {
        if (!superata && mio()) segnalaErrore(e);
      });
    return () => {
      superata = true;
    };
    // `segnalaErrore` e il turno non cambiano mai: ci sono perché l'effetto li
    // legge, non perché lo facciano ripartire.
  }, [aperto, segnalaErrore, turnoAlbum]);

  /** Porta in una vista, azzerando tutto quel che le sta sopra. */
  const vaiA = useCallback((dove: Vista) => {
    // Fuori dalla transizione: vedi «Perché `startTransition`» in
    // `transizione.ts`.
    setQuery("");
    cambiandoVista(() => {
      setAperto(null);
      setArtistaAperto(null);
      // Andare altrove è un'altra strada: la pagina dell'artista non c'è più, e
      // con lei il ritorno a «In riproduzione».
      setArtistaDaInRiproduzione(null);
      setPlaylistAperta(null);
      setVista(dove);
    });
  }, []);

  /**
   * Apre la libreria filtrata per un dispositivo collegato: la barra laterale,
   * Altro e le Impostazioni. Prima era «Sfoglia», un'altra pagina; adesso è la
   * stessa libreria con un filtro (`origine.ts`), dove si era se è una vista che
   * il filtro riguarda e negli Album se no.
   */
  const apriDispositivo = useStabile((chiave: string) => {
    setOrigine(origineDi(chiave));
    vaiA(vistaPerIlFiltro(vista));
  });

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
   * stessa scelta della preferenza del movimento (`aspettoFinestra.ts`).
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
   * che non c'è; il mondo finto di `studio/mondo.ts` serve a chi disegna skin,
   * non a chi ascolta. Senza brani il giro non parte e basta — e siccome questa
   * condizione è uno stato e non un evento, riparte da sé alla prima scansione
   * che finisce con qualcosa dentro, senza che nessuno debba ricordarsene.
   *
   * «Lo faccio dopo» non è quindi un rifiuto del giro: chiude `Primo` e lo
   * rinvia, perché la libreria resta vuota.
   *
   * **Non dentro Esplora.** Chi comincia da lì riempie la libreria un tasto
   * alla volta, e da quando i contatori seguono le aggiunte il primo
   * «Aggiungi» accenderebbe il giro sopra la ricerca, portando la finestra
   * altrove a metà di un gesto. Si aspetta che torni alla libreria, che è
   * quella che il giro racconta.
   *
   * **Non sopra un modale, né sopra lo Studio**: si aspetta che si chiudano.
   * Il perché, e la regola intera, stanno in `comeParteIlGiro` (`guida.ts`).
   * Un modale non è uno stato di questo componente — molti tengono il proprio
   * — quindi la sua chiusura non rifà questo effetto: la si guarda nel
   * documento, e solo finché c'è da aspettare.
   */
  useEffect(() => {
    // `daFare` aspetta anche le Novità: mai due veli sulla stessa finestra. Il
    // giro ha già l'occhio ai modali (`modaleAperto`), ma la finestra delle
    // Novità nasce dopo la risposta del nucleo, e in quel tempo non c'è ancora.
    const come = comeParteIlGiro({
      daFare: giroDaFare && !novitaInAttesa,
      aperto: giroAperto,
      avviato: avvio !== null,
      primoVisibile,
      brani: avvio?.numeri.tracks ?? 0,
      inEsplora: vista === "esplora",
      studioVisibile,
      modaleAperto: unModaleAperto(),
      telefono: MOBILE,
    });
    if (come === "no") return;
    if (come === "parti") {
      setGiroAperto(true);
      return;
    }
    const osserva = new MutationObserver(() => {
      if (unModaleAperto()) return;
      osserva.disconnect();
      setGiroAperto(true);
    });
    // I modali si montano e si smontano: basta guardare chi entra e chi esce,
    // più i due attributi che fanno di un nodo un modale. Niente `style` né
    // `class`, che un elenco virtuale riscrive decine di volte al secondo.
    osserva.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["aria-modal", "role", "hidden"],
    });
    return () => osserva.disconnect();
  }, [giroDaFare, giroAperto, avvio, primoVisibile, vista, studioVisibile, novitaInAttesa]);

  /**
   * Quando il giro si apre senza niente in coda, ci si mette qualcosa — fermo.
   *
   * Quattro passi parlano del lettore, e il lettore si vede solo con un brano:
   * al primo avvio si saltavano tutti e quattro. Il nucleo riempie la coda
   * **solo se è vuota** e non fa partire niente (`coda_prepara`), e i primi
   * passi — il benvenuto, la barra, la ricerca, la riga — lasciano il tempo
   * alla risposta di arrivare prima che serva.
   *
   * Quel che il giro ha messo si ricorda in `codaDelGiro`, per toglierlo alla
   * chiusura (vedi `chiudiGiro`). Se il giro si è già chiuso quando la coda
   * arriva — saltato al primo fumetto — la si toglie subito: nessuno ha avuto
   * il tempo di usarla.
   */
  const codaDelGiro = useRef<number[] | null>(null);
  /** Per chi lo legge in una risposta arrivata dopo: vedi sopra. */
  const giroApertoAdesso = useRef(giroAperto);
  giroApertoAdesso.current = giroAperto;
  useEffect(() => {
    if (!giroAperto || conBrano) return;
    ipc
      .brani("recenti", 0, 50)
      .then(async (elenco) => {
        if (elenco.length === 0) return;
        const ids = elenco.map((b) => b.id);
        if (!(await ipc.codaPrepara(ids))) return;
        if (giroApertoAdesso.current) codaDelGiro.current = ids;
        else await ipc.codaSvuota();
      })
      // Senza, il giro salta quei quattro passi come prima: è un ripiego, non
      // un guasto da mostrare in cima al giro.
      .catch(() => undefined);
    // Solo all'apertura: quando il brano arriva, l'effetto non deve rifarsi.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [giroAperto]);

  /**
   * Com'era la finestra quando il giro è partito: la vista, la pagina aperta
   * sopra di lei, la sezione delle Impostazioni, la colonna, lo schermo intero.
   *
   * In un effetto di disposizione e non al clic che apre il giro: le strade per
   * aprirlo sono due — da sé e da «Rifai il giro» — e questo gira nello stesso
   * passaggio in cui il giro si monta, **prima** del suo primo `onPrepara`,
   * che è il primo a spostare qualcosa. I valori sono quindi quelli di un
   * istante prima del giro, comunque lo si sia aperto.
   */
  const primaDelGiro = useRef<{
    vista: Vista;
    sezione: Sezione;
    query: string;
    aperto: Album | null;
    artistaAperto: Artista | null;
    playlistAperta: Playlist | null;
    grande: boolean;
    colonnaAperta: boolean;
  } | null>(null);
  useLayoutEffect(() => {
    if (!giroAperto) return;
    primaDelGiro.current = {
      vista,
      sezione,
      query,
      aperto,
      artistaAperto,
      playlistAperta,
      grande,
      colonnaAperta,
    };
    // Solo all'apertura: i cambi che seguono sono del giro, non di chi guarda.
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
   *
   * Il menù è l'unico passo che lascia qualcosa da togliere, e restituisce chi
   * lo toglie: vedi «Ogni passo si mette e si toglie» in `Giro.tsx`.
   */
  const preparaGiro = useCallback(
    (passo: NomePasso): (() => void) | undefined => {
      switch (passo) {
        case "ricerca":
        case "riga-brano":
          setGrande(false);
          vaiA("brani");
          break;
        case "menu": {
          // Sui Brani, dove la riga del passo prima è appena stata mostrata: il
          // menù si apre col tasto destro vero su quella riga.
          setGrande(false);
          vaiA("brani");
          const smetti = apriIlMenuDiUnaRiga();
          return () => {
            smetti();
            setMenu(null);
          };
        }
        case "cartelle":
          setGrande(false);
          vaiA("cartelle");
          break;
        case "scorciatoie":
          setGrande(false);
          vaiA("impostazioni");
          setSezione("movimento");
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
          if (conBrano) setGrande(true);
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
    [vaiA, conBrano],
  );

  /**
   * Il giro è finito, saltato, o chiuso con Escape.
   *
   * Tutti e tre segnano «visto»: vedi il `//!` di `src-tauri/src/giro.rs`.
   * `setGiroDaFare(false)` non aspetta la scrittura — se il database non
   * risponde, riaprire il giro sarebbe il rimedio peggiore del male.
   *
   * E tutti e tre rimettono la finestra com'era prima di partire
   * (`primaDelGiro`): il giro la porta in giro — Brani, Cartelle, Impostazioni,
   * lo schermo intero — e chi lo chiude al nono passo si ritrovava nelle
   * Impostazioni senza averci mai messo piede. La coda che il giro ha riempito
   * si toglie se nessuno l'ha usata (`codaDaSvuotare` in `guida.ts`); una
   * usata resta, perché è diventata di chi l'ha usata.
   */
  const chiudiGiro = useCallback(() => {
    setGiroAperto(false);
    setGiroDaFare(false);
    ipc.giroFatto().catch(() => undefined);
    const prima = primaDelGiro.current;
    primaDelGiro.current = null;
    if (prima !== null) {
      setMenu(null);
      setGrande(prima.grande);
      setColonnaAperta(prima.colonnaAperta);
      setSezione(prima.sezione);
      setQuery(prima.query);
      cambiandoVista(() => {
        setAperto(prima.aperto);
        setArtistaAperto(prima.artistaAperto);
        setPlaylistAperta(prima.playlistAperta);
        setVista(prima.vista);
      });
    }
    const messa = codaDelGiro.current;
    codaDelGiro.current = null;
    // Lo stato del lettore di adesso, letto alla chiusura e non a un disegno:
    // la coda può essere stata usata un istante prima.
    if (codaDaSvuotare(messa, riproduzione.archivio.leggi()))
      ipc.codaSvuota().catch(() => undefined);
  }, [riproduzione.archivio]);

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
    // Il nome del volo va alla scheda premuta **prima** della transizione, in
    // un aggiornamento suo: React lo applica subito, e la fotografia di prima
    // la trova già col nome. Tornando indietro resta dov'è, e la copertina
    // rientra nella sua scheda.
    if (quale !== null) setInVolo(quale.albumKey);
    fuocoDopo.current =
      quale !== null
        ? { verso: "testata", daChi: daChi(), livello: daChiAlbum }
        : { verso: "ritorno", livello: daChiAlbum };
    cambiandoVista(() => setAperto(quale));
  }, []);

  /**
   * Apre un artista, o chiude la sua pagina se è `null`.
   *
   * Chiudere la pagina aperta dal nome in «In riproduzione» riapre «In
   * riproduzione» (`tornaAInRiproduzione`): Indietro torna da dove si è partiti.
   * Il fuoco allora lo prende lei, come ogni volta che si apre, e non la scheda
   * dell'artista di una griglia che non si è toccata.
   */
  const apriArtista = useStabile((quale: Artista | null) => {
    const torna = quale === null && tornaAInRiproduzione(artistaDaInRiproduzione, artistaAperto);
    if (!torna) {
      fuocoDopo.current =
        quale !== null
          ? { verso: "testata", daChi: daChi(), livello: daChiArtista }
          : { verso: "ritorno", livello: daChiArtista };
    }
    setArtistaDaInRiproduzione(null);
    if (torna) setGrande(true);
    cambiandoVista(() => setArtistaAperto(quale));
  });

  /**
   * Il primo avvio: scrive le cartelle scelte e comincia subito a leggere.
   *
   * Le due cose insieme e non due pulsanti: fra «ho detto dove sta la musica» e
   * «voglio che la legga» non c'è nessuna decisione in mezzo, e il secondo
   * pulsante esisteva solo perché il primo non sapeva cosa fare dopo.
   *
   * `useStabile` e non `useCallback` con le dipendenze vuote: `scansiona` è
   * definita in questo corpo e cambia identità a ogni disegno, e la versione
   * del primo disegno chiamava `caricaVista` con la `vista` di allora — una
   * domanda in più alla Home da qualunque pagina. Così l'identità resta una, e
   * chiama sempre l'ultima.
   */
  const primoConferma = useStabile(async (cartelle: string[]): Promise<boolean> => {
    // Il `true` dice a `Primo` che le cartelle sono scritte, ed è quel che gli
    // permette di mostrare «Ascolta» (`tastoPrimo.ts`): se la scrittura
    // fallisce «Continua» deve restare, anche se intanto una cartella del
    // cloud ha portato dei brani.
    try {
      await ipc.impostaCartelle(cartelle);
    } catch (e) {
      segnalaErrore(e);
      return false;
    }
    try {
      await ricarica();
      await scansiona();
    } catch (e) {
      segnalaErrore(e);
    }
    return true;
  });

  /** Il dialogo di sistema, per chi la musica la tiene altrove. */
  const primoScegli = useCallback(async () => {
    try {
      const scelta = await open({ directory: true, multiple: false });
      return typeof scelta === "string" ? scelta : null;
    } catch (e) {
      segnalaErrore(e);
      return null;
    }
  }, [segnalaErrore]);

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
  }, [segnalaErrore, setPrimoChiuso]);

  /**
   * Le cartelle sorvegliate: aggiungere, togliere, dove scaricare, e la
   * domanda sul trasloco. Vedi `cartelleSorvegliate.ts`.
   */
  const {
    gemella,
    setGemella,
    scegliCartella,
    aggiungiCartelle,
    aggiungiNuvola,
    scollegaNuvola,
    trasloca,
    scegliCartellaDownload,
    togliCartella,
    occupate: cartelleOccupate,
  } = useCartelleSorvegliate({ libreria, setNotizia, segnalaErrore });

  /**
   * La finestrella «Aggiungi una cartella» aperta, e per chi.
   *
   * `"primo"` quando la apre il primo avvio: lì una cartella scelta non entra
   * fra le sorvegliate da sola, ma nell'elenco spuntato della schermata — come
   * quelle trascinate — e la scrive «Continua». Una del cloud invece il nucleo
   * la scrive subito (`nuvolaAggiungi` non sa solo «risolvere»), e nell'elenco
   * entra spuntata lo stesso: «Continua» la riscrive con le altre, e chi la
   * toglie dall'elenco la toglie davvero.
   */
  const [aggiunta, setAggiunta] = useState<"finestra" | "primo" | null>(null);

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
   * nome proposto è quello della playlist, ripulito di quel che Windows non
   * accetta in un nome di file (`nomeDiFile`): `defaultPath` con l'estensione
   * dentro è ciò che fa comparire «Serata.m3u8» già scritto, invece di un campo
   * vuoto in cui va indovinata anche l'estensione.
   */
  const esportaPlaylist = async (p: Playlist) => {
    try {
      // Sul telefono il formato resta quello del nome proposto: il selettore
      // di Android lascia cambiare il nome, ma il file lo scrive il nucleo
      // prima, con l'estensione di qui. Vedi `dialoghi.ts`.
      const salvata = await salvaCon(
        {
          // Il nome ripulito, non quello della playlist: «AC/DC» diventava la
          // cartella «AC» e il file «DC.m3u8». Vedi `nomeDiFile`.
          defaultPath: `${nomeDiFile(p.name)}.m3u8`,
          filters: [
            { name: "M3U", extensions: ["m3u8", "m3u"] },
            { name: "PLS", extensions: ["pls"] },
            { name: "XSPF", extensions: ["xspf"] },
          ],
          tipo: "audio/x-mpegurl",
        },
        (percorso) => ipc.playlistEsporta(p.id, percorso),
      );
      if (salvata === null) return;
      const { esito: fatta, dove: scelta } = salvata;
      // I brani del cloud e dei cataloghi non entrano nel file (un altro
      // lettore non saprebbe aprirli): se ce n'erano si dice quanti, o una
      // playlist tutta del cloud uscirebbe come un file pieno. Il numero che
      // sceglie la forma (`.one`, `.uno`) è `n`, il numero vero e non già
      // formattato; i brani scritti, nella frase con due numeri, arrivano già
      // detti da `brani_`, come in `delete.done.back`.
      setNotizia(
        fatta.lasciatiFuori > 0
          ? t("playlist.exported.leftOut", {
              brani: brani_(fatta.scritti),
              dove: scelta,
              n: fatta.lasciatiFuori,
            })
          : t("playlist.exported", { n: fatta.scritti, dove: scelta }),
      );
    } catch (e) {
      segnalaErrore(e);
    }
  };

  /**
   * Sceglie il database della versione rilasciata.
   *
   * La cartella dati di questa applicazione è **diversa** da quella della 1.0.0
   * — `%APPDATA%\io.github.federicobaratti.rymembra` contro la cartella dati
   * della 1.0 — apposta perché finché questa non è finita la vecchia deve
   * restare apribile. Perciò il dialogo si apre lì: è dove il file sta, e
   * comporre il percorso a mano è il modo di sbagliarlo su un profilo spostato.
   */
  const scegliDatabase = async () => {
    try {
      const roaming = avvio?.dataDir.replace(/[/\\][^/\\]+$/, "");
      const scelta = await open({
        multiple: false,
        // Sparso e non `defaultPath: … : undefined`: con
        // `exactOptionalPropertyTypes` una proprietà assente e una uguale a
        // `undefined` sono due cose diverse, e il dialogo vuole la prima.
        ...(roaming ? { defaultPath: `${roaming}\\${CARTELLA_DELLA_1_0}` } : {}),
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
    libreria.esito.scrivi(null);
    // Una domanda rimasta aperta dalla passata prima non vale più: il nucleo
    // ha già buttato il suo elenco, e questa passata chiederà da sé.
    setRimozioniInAttesa(null);
    posaScansione({ fatti: 0, totale: 0 });
    scansioniMie.current += 1;
    try {
      const risultato = await ipc.scansiona();
      libreria.esito.scrivi(risultato);
      setRimozioniInAttesa(risultato.daConfermare);
      // Com'è andata, ovunque si sia: prima lo diceva solo Impostazioni ›
      // Cartelle e scansione, e una scansione partita dallo stato vuoto finiva
      // in silenzio — anche quando una cartella del NAS non aveva risposto.
      setNotizia(riassuntoScansione(risultato));
      await ricarica();
      await caricaVista();
    } catch (e) {
      segnalaErrore(e);
    } finally {
      scansioniMie.current -= 1;
      posaScansione(null);
    }
  };

  /**
   * Fa partire una lista a partire da un brano.
   *
   * La coda è **la lista che si sta guardando**, non il brano solo: chi clicca
   * la terza traccia di un album si aspetta che poi parta la quarta, e mandare
   * un brano solo trasformerebbe ogni clic in una fine coda tre minuti dopo.
   *
   * L'errore **non** lo inghiotte: lo rilancia, e mostrarlo tocca a chi chiama
   * (`.catch(segnalaErrore)`). Lo inghiottiva, e chi aveva bisogno di sapere se
   * era partita davvero non poteva saperlo: la raccolta del lunedì si segnava
   * «sentita» anche quando la riproduzione era fallita, cioè il pallino che
   * dice «non l'hai ancora ascoltata» spariva su una raccolta mai ascoltata.
   */
  /**
   * «<PC> non raggiungibile»: un tocco su una riga grigia, o una lista fatta solo
   * di righe grige. Un avviso neutro e non un errore: niente è andato storto, il
   * dispositivo è spento — e la fascia rossa si è già vista abbastanza per dire
   * di un guasto. Né una notizia: quella porta la spunta del successo, e un tocco
   * che non ha suonato niente non è riuscito. L'icona è quella del dispositivo.
   * `null`: la riga è di un dispositivo che non c'è più.
   */
  const avvisaNonRaggiungibile = useCallback(
    (riga: (Pick<StatoDellaRiga, "nome" | "telefono"> & { inGrazia?: true }) | null) => {
      setAvviso({
        testo:
          riga === null || riga.nome === ""
            ? t("library.origin.gone")
            : riga.inGrazia === true
              ? t("library.origin.faded", { nome: riga.nome })
              : t("library.origin.offline", { nome: riga.nome }),
        icona: riga?.telefono === true ? "i-telefono" : "i-computer",
      });
    },
    [setAvviso],
  );

  /**
   * Quel che si suona dalle righe grige.
   *
   * `daUnaRiga`: l'indice è la riga che si è toccato (non la prima di un
   * «Riproduci»). Se è grigia non si suona niente e si dice chi non risponde:
   * partire dalla riga dopo sarebbe suonare un brano che non si è scelto. Il resto
   * della lista salta le righe di un dispositivo spento (`suonabili`), e una lista
   * senza niente di suonabile dice la stessa cosa invece di mandare al lettore un
   * elenco vuoto.
   *
   * «Grigia» vuol dire che non si suona: una riga di un dispositivo spento che ha
   * una copia di qui (`copiaQui`) si suona lo stesso, con la copia, e non è
   * grigia — il nucleo sceglie la copia quando il dispositivo tace (D1 del
   * collaudo del 18/11).
   */
  const suonaDa = useCallback(
    async (elenco: Brano[], indice: number, daUnaRiga = true) => {
      const pannello = archivioLontano.leggi();
      const toccata = elenco[indice];
      if (daUnaRiga && toccata !== undefined) {
        const riga = statoDellaRiga(
          toccata.path,
          pannello,
          toccata.inGrazia === true,
          toccata.copiaQui === true,
        );
        if (!rigaSuonabile(riga)) {
          avvisaNonRaggiungibile(riga);
          return;
        }
      }
      const { brani, indice: da } = suonabili(elenco, indice, pannello);
      if (brani.length === 0) {
        const spento = primoSpento(elenco, pannello);
        if (spento !== null) avvisaNonRaggiungibile(spento);
        return;
      }
      const ids = brani.map((b) => b.id);
      // Una riga di un altro dispositivo sui dati mobili, con «Chiedi»: il gesto
      // aspetta il sì della finestrella invece di fallire — lo fa `ipc.suona`,
      // come ogni comando che apre una riga (S11-4).
      await ipc.suona(ids, da);
    },
    [avvisaNonRaggiungibile],
  );

  /**
   * Mette in coda le righe, a blocchi dei più grandi che il nucleo accetta.
   *
   * Un artista con la discografia intera supera il tetto di una chiamata
   * (`PER_VOLTA_SCARICO`), e prima il nucleo rifiutava tutto. «Dati mobili:
   * Chiedi» rifà il gesto dopo il sì: `fatti` sta fuori dal gesto, così il
   * secondo giro riparte dal blocco dove si era fermato e non rimette in coda
   * quelli già partiti.
   */
  const accodaRighe = useCallback(async (ids: readonly number[]) => {
    const blocchi = inBlocchi(ids, PER_VOLTA_SCARICO);
    let fatti = 0;
    await conIlSiDeiDatiMobili(async () => {
      while (fatti < blocchi.length) {
        const stato = await ipc.lontanoScaricaRighe(blocchi[fatti] ?? []);
        fatti += 1;
        archivioScarico.scrivi(stato);
      }
    });
  }, []);

  /**
   * «Scarica qui»: copia nella cartella dei download di questo dispositivo i
   * brani che stanno su un altro. Quel che già è di qui non c'è da scaricare, e
   * un dispositivo che non risponde lo dice senza provarci.
   *
   * Da `SOGLIA_CONFERMA_SCARICO` brani in su chiede prima, con quanti sono e
   * quanto spazio prendono. È il cancello di tutti i «Scarica qui» — una riga o
   * una selezione, l'album, la playlist, l'artista —, perché passano tutti da
   * qui. Il numero detto è quello del nucleo (`lontano_scarica_anteprima`), che
   * non conta le righe che il proprietario non manda più né le tracce di un CD:
   * se a conti fatti sono meno della soglia, non si chiede.
   */
  const scaricaQui = useCallback(
    async (brani: readonly Brano[]) => {
      const pannello = archivioLontano.leggi();
      const lontani = brani.filter(
        (b) => statoDellaRiga(b.path, pannello, b.inGrazia === true) !== null,
      );
      if (lontani.length === 0) {
        setNotizia(t("devices.download.nothing"));
        return;
      }
      const accesi = lontani.filter(
        (b) => statoDellaRiga(b.path, pannello, b.inGrazia === true)?.spenta !== true,
      );
      if (accesi.length === 0) {
        avvisaNonRaggiungibile(primoSpento(lontani, pannello));
        return;
      }
      const ids = accesi.map((b) => b.id);
      if (chiedeConfermaPerLoScarico(ids.length)) {
        const anteprima = await ipc.lontanoScaricaAnteprima(ids);
        if (chiedeConfermaPerLoScarico(anteprima.brani)) {
          const { titolo, descrizione } = domandaDelloScarico(anteprima, {
            brani: brani_,
            peso: pesoDi,
          });
          setConfermaScarico({
            titolo: t(titolo.chiave, titolo.valori),
            descrizione: t(descrizione.chiave, descrizione.valori),
            conferma: t("devices.download.confirm.go"),
            fai: () => {
              accodaRighe(ids).catch(segnalaErrore);
            },
          });
          return;
        }
      }
      await accodaRighe(ids);
    },
    [avvisaNonRaggiungibile, setNotizia, setConfermaScarico, accodaRighe, segnalaErrore],
  );

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

  /**
   * «Riproduci» e «Casuale» delle testate del telefono (`TestataDettaglio`):
   * l'elenco dal primo, o una sua copia mescolata. Vedi `mescolato.ts` sul
   * perché non è il casuale del lettore.
   */
  const suonaElenco = useCallback(
    (elenco: Brano[], casuale: boolean) => {
      if (elenco.length === 0) return;
      suonaDa(casuale ? mescolato(elenco) : elenco, 0, false).catch(segnalaErrore);
    },
    [suonaDa, segnalaErrore],
  );

  /**
   * I brani di un artista — **suoi**, tutti, anche quelli senza disco — nell'ordine
   * in cui la pagina li mostra.
   *
   * Prima si prendevano i suoi dischi e i brani di ciascuno: una raccolta in cui
   * compare entrava intera, con i brani di chiunque, e i brani senza disco non
   * entravano mai. Adesso è la stessa query della sezione «Brani»
   * (`braniArtista`), pagina dopo pagina: stessi brani, stesso ordine, stesso
   * filtro per dispositivo. Serve a `suonaArtista` e a «Scarica qui».
   */
  const braniDellArtista = useCallback(
    (nome: string) =>
      tutteLePagine(
        (offset, limite) => ipc.braniArtista(nome, ordineArtista, offset, limite, origineIpc),
        MISURA_PAGINA,
      ),
    [ordineArtista, origineIpc],
  );

  /**
   * «Riproduci» e «Casuale» dell'artista: i suoi brani, dal primo o in ordine
   * sparso. Una riga di un dispositivo che non risponde si salta come negli altri
   * elenchi (`suonaDa`).
   */
  const suonaArtista = useCallback(
    async (nome: string, casuale: boolean) => {
      try {
        const tracce = await braniDellArtista(nome);
        if (tracce.length === 0) return;
        await suonaDa(casuale ? mescolato(tracce) : tracce, 0, false);
      } catch (e) {
        segnalaErrore(e);
      }
    },
    [suonaDa, segnalaErrore, braniDellArtista],
  );

  /**
   * «Scarica qui» sull'artista: i suoi brani che stanno su altri dispositivi.
   * `scaricaQui` lascia fuori quelli di qui, e un dispositivo spento lo dice
   * senza provarci.
   */
  const scaricaArtista = useCallback(
    async (nome: string) => {
      try {
        await scaricaQui(await braniDellArtista(nome));
      } catch (e) {
        segnalaErrore(e);
      }
    },
    [scaricaQui, segnalaErrore, braniDellArtista],
  );

  /** Le playlist all'apertura. Vince l'ultima, come `ricarica`: vedi `libreria.ts`. */
  useEffect(() => {
    void ricaricaPlaylist();
  }, [ricaricaPlaylist]);

  /**
   * La playlist aperta: i suoi brani, togliere una riga, il riordino. Vedi
   * `playlistAperta.ts`; qui, dove stava l'effetto che chiede i brani.
   */
  const {
    braniPlaylist,
    setBraniPlaylist,
    playlistCaricata,
    aggiornaPlaylist,
    togliDallaPlaylist,
    spostaNellaPlaylist,
    trascinata,
    mirata,
    presa,
    mira,
    lascia,
    rileggiPlaylist,
  } = usePlaylistAperta({ playlistAperta, setPlaylistAperta, libreria, segnalaErrore });

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
  const elencoCorrenteRef = useRef<Elenco<Brano>>(elencoPieno(NESSUN_BRANO));

  /**
   * Apre la pagina dell'artista di un brano: il nome in «In riproduzione» e
   * «Vai all'artista» del menù.
   *
   * # Quale artista
   *
   * Il **effettivo** (`artistaEffettivo`): quello del disco se c'è. Su un brano
   * di una raccolta l'interprete è «Tale» e la pagina che la griglia ha è «Vari
   * artisti». Un brano senza artista non ha una pagina, e lo dice.
   *
   * # Con quale filtro
   *
   * Quello di adesso se contiene la riga, «Tutti» se no (`origineCheContiene`):
   * l'artista di un brano del PC, con «Questo dispositivo» acceso, sarebbe una
   * pagina vuota. E la griglia degli artisti si legge sotto il filtro **nuovo**.
   *
   * # E Indietro
   *
   * Se «In riproduzione» è aperta — il gesto parte da lei, dal nome o dal suo
   * menù — si chiude per far vedere la pagina, e chiudere la pagina la riapre:
   * Indietro torna da dove si è partiti. Sul computer la schermata a tutto schermo
   * copre la pagina, e prima il gesto aperto da lì non si vedeva.
   *
   * Un artista che la griglia non ha — un nome scritto in un modo solo in quel
   * brano — si cerca per nome, invece di non fare niente.
   */
  const apriArtistaDelBrano = useStabile(async (brano: Brano) => {
    const nome = artistaEffettivo(brano);
    if (nome === "") {
      setAvviso({ testo: t("artist.none"), icona: "i-artist" });
      return;
    }
    const nuova = origineCheContiene(origine, brano.path);
    const stessa = ugualiOrigini(nuova, origine);
    const tutti = stessa && artisti.length > 0 ? artisti : await ipc.artisti(perIlNucleo(nuova));
    const suo = trovaArtista(tutti, nome);
    const daInRiproduzione = grande;
    if (daInRiproduzione) setGrande(false);
    if (!stessa) setOrigine(nuova);
    if (suo === undefined) {
      setQuery(nome);
      return;
    }
    setQuery("");
    setArtistaDaInRiproduzione(daInRiproduzione ? suo.name : null);
    fuocoDopo.current = { verso: "testata", daChi: null, livello: daChiArtista };
    cambiandoVista(() => {
      setPlaylistAperta(null);
      setAperto(null);
      setArtistaAperto(suo);
    });
  });
  /** Il nome dell'artista in «In riproduzione»: apre la sua pagina, e un guasto si dice. */
  const apriArtistaDalNome = useCallback(
    (brano: Brano) => {
      apriArtistaDelBrano(brano).catch(segnalaErrore);
    },
    [apriArtistaDelBrano, segnalaErrore],
  );

  /**
   * Apre il menù contestuale su una selezione di brani.
   *
   * Quali voci, e quando, lo dice `vociBrani` in `menuBrani.ts`; qui c'è quel
   * che ciascuna fa, che tocca la coda, le finestrelle e la pagina.
   */
  /**
   * Il cuore per la voce del menù: `cambiaPreferito` è scritto più giù, e il
   * menù lo usa al clic, quando c'è già.
   */
  const preferitoDalMenu = useRef<(brano: Brano) => Promise<void>>(async () => {});
  const apriMenu = useCallback(
    (
      e: React.MouseEvent,
      elenco: number[],
      /**
       * Il brano su cui si è aperto, quando chi apre lo ha in mano e l'elenco
       * della vista no: la Home, la coda, «In riproduzione». Serve alla testata
       * del foglio del telefono, e a non offrire voci per un brano che
       * l'elenco della vista non conosce.
       */
      conosciuto?: Brano | ((id: number) => Brano | undefined),
      /**
       * Le voci che solo chi apre sa offrire: «Togli da questa playlist», che
       * vuole la posizione della riga. Vedi `menuRigaPlaylist`.
       */
      piu?: Pick<AzioniBrani, "togliDallaPlaylist" | "togliDallaCoda" | "sposta">,
    ) => {
      e.preventDefault();
      // Chi apre il menù da un pannello con le sue righe (le Cartelle) passa
      // il modo di trovarle: l'elenco della vista lì è vuoto (R13b-3).
      const inMano = (id: number) =>
        typeof conosciuto === "function"
          ? (conosciuto(id) ?? elencoCorrenteRef.current.cerca((b) => b.id === id))
          : conosciuto !== undefined && conosciuto.id === id
            ? conosciuto
            : elencoCorrenteRef.current.cerca((b) => b.id === id);
      // Le righe in mano scorse una volta, per «c'è fra gli scelti un brano di
      // un altro dispositivo?»: con «Seleziona tutto» gli scelti sono decine di
      // migliaia, e chiederlo per id scorreva tutte le righe in mano per ognuno
      // (S9-2). Con un elenco che solo chi apre sa trovare (`conosciuto` è una
      // funzione: le Cartelle, le partecipazioni) l'universo delle righe non è
      // scorribile da qui, e `vociBrani` torna alla strada per id.
      const cercaInMano =
        typeof conosciuto === "function"
          ? undefined
          : (predicato: (brano: Brano) => boolean): Brano | undefined =>
              conosciuto !== undefined && predicato(conosciuto)
                ? conosciuto
                : elencoCorrenteRef.current.cerca(predicato);
      // Il brano si legge al momento del gesto: l'elenco che ha aperto il menù
      // ha solo i numeri.
      const conBrano =
        (fai: (brano: Brano) => Promise<void> | void) => (id: number) => {
          ipc
            .braniPerId([id])
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
      const fai: AzioniBrani = {
        radio: (seme) => {
          ipc.radio(seme).catch(segnalaErrore);
        },
        dopo: (brani) => {
          ipc.codaDopo(brani).catch(segnalaErrore);
        },
        accoda: (brani) => {
          ipc.codaAccoda(brani).catch(segnalaErrore);
        },
        aggiungiAPlaylist: (brani) => setDaAggiungere(brani),
        scaricaQui: (ids) => {
          ipc
            .braniPerId(ids)
            .then(scaricaQui)
            .catch(segnalaErrore);
        },
        vaiAlDisco: conBrano(async (brano) => {
          // Sul telefono «In riproduzione» copre la pagina, schede comprese:
          // il disco aperto sotto non si vedrebbe. Si scende, e lo si trova.
          if (MOBILE) setGrande(false);
          const chiave = brano.albumKey;
          // Un brano senza disco — un flusso, un file sparso — ha la voce
          // come tutti: il menù si apre prima di leggere il brano. Il
          // clic almeno lo dice, invece di non fare niente.
          if (chiave === null) {
            setNotizia(t("menu.goToAlbum.none"));
            return;
          }
          const tracce = await ipc.braniAlbum(chiave);
          setQuery("");
          // Il fuoco sulla testata del disco, come aprendolo dalla griglia; chi
          // aveva aperto era una voce del menù, che non c'è più.
          fuocoDopo.current = { verso: "testata", daChi: null, livello: daChiAlbum };
          cambiandoVista(() => {
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
        // Il brano si rilegge, come per il disco: la scheda porta anche l'artista
        // del disco, e la pagina è quella dell'artista effettivo.
        vaiAllArtista: conBrano((brano) => apriArtistaDelBrano(brano)),
        mostraNellaCartella: (id) => {
          ipc.branoMostraNellaCartella(id).catch(segnalaErrore);
        },
        apriSulWeb: (id) => {
          ipc.branoApriSulWeb(id).catch(segnalaErrore);
        },
        apriPagina: (id) => {
          ipc.branoApriPagina(id).catch(segnalaErrore);
        },
        togli: (brani) => setDaEliminare({ brani, dalDisco: false, risposta: unaVolta() }),
        elimina: (brani) => setDaEliminare({ brani, dalDisco: true, risposta: unaVolta() }),
        // Sul telefono la riga non ha il cuore: sta qui. Il brano si rilegge,
        // come per il disco e l'artista, perché il valore da invertire è
        // quello di adesso e non quello dell'elenco che ha aperto il menù.
        preferito: MOBILE
          ? conBrano((brano) => preferitoDalMenu.current(brano))
          : undefined,
        togliDallaPlaylist: piu?.togliDallaPlaylist,
        togliDallaCoda: piu?.togliDallaCoda,
        sposta: piu?.sposta,
      };
      // Su cosa si agisce, per la testata del foglio del telefono: il brano,
      // se è uno solo e lo si ha in mano, o quanti sono.
      const uno = elenco.length === 1 && elenco[0] !== undefined ? inMano(elenco[0]) : undefined;
      setMenu({
        x: e.clientX,
        y: e.clientY,
        testata:
          uno !== undefined
            ? {
                titolo: uno.title,
                sottotitolo: `${nomeArtista(uno.artist)} · ${titoloAlbum(uno.album)}`,
                copertina: uno.coverArtHash,
              }
            : elenco.length > 1
              ? { titolo: brani_(elenco.length) }
              : undefined,
        voci: vociBrani(
          elenco,
          inMano,
          fai,
          t,
          // Letto al momento del tasto destro, senza iscriversi: `App` non deve
          // ridisegnarsi a ogni stato del lettore per sapere se c'è una
          // sessione, e il menù si costruisce adesso. Vedi `vociBrani`.
          !gestoPermesso(riproduzione.archivio.leggi().insieme, "radio"),
          !gestoPermesso(riproduzione.archivio.leggi().insieme, "coda"),
          cercaInMano,
        ),
      });
    },
    [segnalaErrore, apriArtistaDelBrano, riproduzione.archivio, scaricaQui],
  );

  /** Il menù di una playlist nella barra di navigazione: vedi `vociPlaylist`. */
  const menuPlaylist = useCallback(
    (e: React.MouseEvent, p: Playlist) => {
      e.preventDefault();
      const voci = vociPlaylist(
        p,
        {
          suona: () => {
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
          rinomina: () => setDaRinominare(p),
          regole: () => setRegoleAperte({ playlist: p }),
          // Prima la domanda (D-40), poi la cancellazione: vedi `daCancellare`.
          cancella: () => setDaCancellare({ playlist: p, risposta: unaVolta() }),
          esporta: () => void esportaPlaylist(p),
        },
        t,
      );
      setMenu({
        x: e.clientX,
        y: e.clientY,
        voci,
        testata: {
          titolo: p.name,
          sottotitolo: p.isSmart
            ? `${t("page.playlist.smart")} · ${brani_(p.tracks)}`
            : brani_(p.tracks),
        },
      });
    },
    [ricaricaPlaylist, segnalaErrore],
  );

  /** Il menù di una scheda album: i brani si chiedono al momento del comando. */
  const menuAlbum = useCallback(
    (
      e: React.MouseEvent,
      chiave: string,
      /** Il disco, quando chi apre lo ha in mano: la testata del foglio. */
      album?: Album,
    ) => {
      e.preventDefault();
      const conBrani = (comando: (ids: number[]) => Promise<void>) => () => {
        ipc
          .braniAlbum(chiave)
          .then((tracce) => comando(tracce.map((b) => b.id)))
          .catch(segnalaErrore);
      };
      const conRighe = (comando: (righe: Brano[]) => Promise<void> | void) => () => {
        ipc
          .braniAlbum(chiave)
          .then((tracce) => comando(tracce))
          .catch(segnalaErrore);
      };
      const voci = vociAlbum(
        {
          radio: conBrani((ids) => {
            const primo = ids[0];
            return primo === undefined ? Promise.resolve() : ipc.radio(primo);
          }),
          dopo: conBrani(ipc.codaDopo),
          accoda: conBrani(ipc.codaAccoda),
          // Quel che «Sfoglia» offriva sul disco di un altro dispositivo: Casuale
          // e Scarica qui. Il secondo solo se c'è un dispositivo da cui scaricare.
          casuale: conRighe((righe) => suonaElenco(righe, true)),
          scarica: chiaviCollegate ? conRighe(scaricaQui) : undefined,
        },
        t,
        !gestoPermesso(riproduzione.archivio.leggi().insieme, "radio"),
        !gestoPermesso(riproduzione.archivio.leggi().insieme, "coda"),
      );
      // In una sessione dove la coda non è concessa a questo PC, con un disco
      // non resta niente da fare: nessun menù, invece di una scatola vuota.
      if (voci.length === 0) return;
      setMenu({
        x: e.clientX,
        y: e.clientY,
        voci,
        testata:
          album === undefined
            ? undefined
            : {
                titolo: titoloAlbum(album.title),
                sottotitolo: nomeArtista(album.artist),
                copertina: album.coverArtHash,
              },
      });
    },
    [segnalaErrore, riproduzione.archivio, suonaElenco, scaricaQui, chiaviCollegate],
  );

  /** Il menù dell'ordinamento: quattro voci, dove sta il bottone. */
  const menuOrdine = useCallback((e: React.MouseEvent) => {
    const riquadro = e.currentTarget.getBoundingClientRect();
    setMenu({
      x: riquadro.left,
      y: riquadro.bottom + 4,
      testata: { titolo: t("sort.heading") },
      voci: ordinamenti().map(([chiave, etichetta]) => ({
        etichetta,
        azione: () => setOrdine(chiave),
        scelta: chiave === ordine,
      })),
    });
  }, [ordine]);

  /**
   * Cambia la valutazione, ottimisticamente come il preferito.
   *
   * La riga si aggiorna prima della scrittura: cinque stelle che aspettano il
   * disco per accendersi sembrano un clic non registrato, e il rimedio — se la
   * scrittura fallisce — è ricaricare la vista, che rimette il valore vero.
   */
  const cambiaVoto = useCallback(
    async (brano: Brano, stelle: number) => {
      // Una voce di «Ascolta insieme» non è una riga di questa libreria: il voto
      // finirebbe sulla riga che ha lo stesso numero cambiato di segno, o su
      // nessuna. I tasti non ci sono (`idDiLibreria` in `Giudizio`), e questa
      // guardia copre le strade che non passano da un tasto.
      if (!idDiLibreria(brano.id)) return;
      const aggiorna = (elenco: Brano[]) =>
        elenco.map((b) => (b.id === brano.id ? { ...b, rating: stelle } : b));
      aggiornaBrani(aggiorna);
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
    [caricaVista, aggiornaBrani, riproduzione.ritoccaBrano],
  );

  const cambiaPreferito = useCallback(
    async (brano: Brano) => {
      // Come per il voto: vedi `cambiaVoto`.
      if (!idDiLibreria(brano.id)) return;
      const valore = !brano.liked;
      const aggiorna = (elenco: Brano[]) =>
        elenco.map((b) => (b.id === brano.id ? { ...b, liked: valore } : b));
      // Ottimistico: il cuoricino deve rispondere al dito, non al disco. Se la
      // scrittura fallisce si ricarica, e la riga torna com'era.
      aggiornaBrani(aggiorna);
      setBraniAperto(aggiorna);
      setBraniPlaylist(aggiorna);
      // Il cuore della barra sta nel brano del nucleo, non negli elenchi: senza
      // questa riga restava fermo al valore di quando il brano era partito, e la
      // prima pausa — che è la prima occasione in cui il nucleo ricompone lo
      // stato — lo faceva saltare al valore vero.
      riproduzione.ritoccaBrano(brano.id, { liked: valore });
      try {
        await ipc.preferito(brano.id, valore);
        libreria.avvio.scrivi((prima) =>
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
    [caricaVista, aggiornaBrani, riproduzione.ritoccaBrano],
  );
  preferitoDalMenu.current = cambiaPreferito;

  /**
   * La lista rada della pagina che si guarda, quando è un elenco della libreria
   * — Brani, Preferiti, una ricerca — e la sua descrizione per il nucleo.
   *
   * `null` per le pagine che arrivano intere (una playlist, un album) e per
   * quelle che non sono elenchi di brani. Dalla pagina e non dagli stati uno per
   * uno, perché la precedenza è la sua: con una playlist aperta e qualcosa nella
   * casella di ricerca si guarda la ricerca, e prima l'elenco su cui agivano
   * selezione e riproduzione era quello della playlist.
   */
  const radaCorrente: ArchivioRado<Brano> | null =
    pagina.tipo === "ricerca"
      ? radaRicerca
      : // La pagina di un artista: la sezione «Brani» è l'elenco su cui agiscono
        // selezione e riproduzione. Le partecipazioni, sotto, hanno il loro.
        pagina.tipo === "artista"
        ? radaBraniArtista
        : pagina.tipo === "libreria" && pagina.vista === "brani"
          ? radaBrani
          : pagina.tipo === "libreria" && pagina.vista === "preferiti"
            ? radaPreferiti
            : null;
  const tipoVista = radaCorrente === null ? null : pagina.tipo === "ricerca" ? "ricerca" : vista;
  const nomeArtistaCorrente = pagina.tipo === "artista" ? pagina.artista.name : null;
  const vistaBraniCorrente: VistaBrani | null = useMemo(
    () =>
      nomeArtistaCorrente !== null
        ? { tipo: "artista", nome: nomeArtistaCorrente, ordine: ordineArtista }
        : tipoVista === "ricerca"
          ? { tipo: "ricerca", query: queryPigra }
          : tipoVista === "brani"
            ? { tipo: "brani", ordine }
            : tipoVista === "preferiti"
              ? { tipo: "preferiti" }
              : null,
    [nomeArtistaCorrente, ordineArtista, tipoVista, queryPigra, ordine],
  );
  /** Le righe della pagina, quando arrivano intere. */
  const braniPieni: Brano[] =
    pagina.tipo === "playlist"
      ? braniPlaylist
      : pagina.tipo === "album"
        ? braniAperto
        : NESSUN_BRANO;

  /**
   * L'elenco che si sta guardando: quello su cui agiscono selezione e
   * riproduzione.
   *
   * Serve in tre posti — la selezione a intervallo, «seleziona tutto» e i
   * comandi della barra — e derivarlo tre volte vorrebbe dire tre occasioni di
   * dimenticare un ramo quando nascerà una vista nuova. Pieno o rado, si legge
   * allo stesso modo (`Elenco` in `rada.ts`): una riga per volta, al momento
   * del gesto.
   */
  const elencoCorrente: Elenco<Brano> = useMemo(
    () => radaCorrente ?? elencoPieno(braniPieni),
    [radaCorrente, braniPieni],
  );
  // Durante il disegno e non in un effetto: chi legge questo `ref` lo fa a un
  // clic, cioè sempre dopo che il disegno è finito, e un effetto lo
  // aggiornerebbe un fotogramma più tardi senza guadagnarci niente.
  elencoCorrenteRef.current = elencoCorrente;

  /**
   * La selezione multipla della vista che si guarda: vedi `selezioneVista.ts`.
   * Qui, subito dopo l'elenco corrente da cui dipende, dove stava il suo
   * effetto di disposizione.
   */
  const selezioneVista = useSelezione({
    elenco: elencoCorrente,
    rada: radaCorrente,
    pieni: braniPieni,
    vistaBrani: vistaBraniCorrente,
    chiave: chiaveDellaSelezione(
      pagina,
      vistaTenuta,
      // Di un artista, l'ordinamento dei suoi brani: vedi `selezione.ts`.
      pagina.tipo === "artista" ? ordineArtista : ordine,
      query,
      // Lo stesso filtro che dà la chiave alle liste rade: un altro filtro è
      // un'altra selezione (S9-1).
      chiaveOrigine,
    ),
    // Sul telefono la selezione finisce cambiando vista, come in ogni app di
    // Android: tenerla da parte e ritrovarla tornando lì vorrebbe dire una
    // barra di selezione che ricompare da sola, su righe scelte un'ora prima.
    tenuta: vistaTenuta !== null && !MOBILE,
    idDellaVista: (vista) => ipc.vistaBraniId(vista, origineIpc),
    onErrore: segnalaErrore,
  });
  const {
    scelti: selezione,
    selezionati,
    seleziona,
    alterna: alternaSelezione,
    selezionaTutto,
    annulla: annullaSelezione,
    togli: togliDallaSelezione,
  } = selezioneVista;

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
   *
   * # La conferma che non vale più
   *
   * `definitivi` sono i file che la conferma ha mostrato come da cancellare
   * per sempre. Se all'esecuzione un altro file risulta senza Cestino, il
   * nucleo rifiuta con `fs.noRecycleBin` prima di toccare qualunque cosa: qui
   * non è un guasto da mostrare in fascia ma una domanda da rifare, e la
   * conferma si riapre — con `cambiata`, e con la risposta di adesso.
   */
  const elimina = useCallback(
    async (brani: number[], dalDisco: boolean, definitivi: string[] = []) => {
      const via = new Set(brani);
      const senza = (elenco: Brano[]) => elenco.filter((b) => !via.has(b.id));
      // L'album aperto resta senza niente? Si guarda **prima** di togliere:
      // dopo, l'elenco non sa più cosa conteneva.
      const svuotaLAlbum =
        braniAperto.length > 0 && braniAperto.every((b) => via.has(b.id));
      aggiornaBrani(senza);
      setBraniAperto(senza);
      setBraniPlaylist(senza);
      // E dalle selezioni messe da parte dalle viste tenute montate: tornandoci,
      // la barra non deve contare un brano che non c'è più.
      togliDallaSelezione(via);
      try {
        if (dalDisco) {
          const quanti = await ipc.braniElimina(brani, definitivi);
          // Sul telefono il Cestino chiede conferma con un dialogo di Android, e
          // un «no» lì torna come zero brani: non si è mosso niente, e le righe
          // tolte qui sopra in anticipo devono tornare — senza un errore, perché
          // non ce n'è stato uno.
          if (MOBILE && quanti === 0 && brani.length > 0) throw RIPENSATO;
          setNotizia(
            definitivi.length > 0
              ? t("delete.done.forever", { brani: brani_(quanti) })
              : t("delete.done.disk", { brani: brani_(quanti) }),
          );
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
        // Qui e non dentro `ricaricaContorno`, che parte anche dagli eventi
        // della libreria (`caricaVista`): là non è mai stata, e mettercela
        // vorrebbe dire una domanda in più al nucleo a ogni scansione finita.
        void ricaricaPlaylist();
        // I contatori della barra: sono di `avvio`, e né `ricaricaContorno` né
        // `caricaVista` li sfiorano.
        await ricarica();
      } catch (e) {
        // Vedi «La conferma che non vale più»: non si è mosso niente, e la
        // domanda si rifà invece di diventare un errore.
        // Un elenco nuovo e non lo stesso: la conferma rifà la domanda a ogni
        // elenco che riceve, anche se non ha fatto in tempo a chiudersi.
        const daRichiedere = dalDisco && eErroreIpc(e) && e.code === "fs.noRecycleBin";
        if (daRichiedere)
          setDaEliminare({ brani: [...brani], dalDisco: true, cambiata: true, risposta: unaVolta() });
        else if (e !== RIPENSATO) segnalaErrore(e);
        await caricaVista();
        // `caricaVista` rifà gli elenchi impaginati, non questi due: hanno
        // effetti loro, legati all'album e alla playlist aperti, che un errore
        // non fa scattare.
        // Col turno, tutte e due: fra il clic e la risposta si può aver
        // aperto un altro album, o un'altra playlist, e i brani di questo non
        // devono finire sotto quella.
        //
        // Il turno da solo non basta: preso qui, *dopo* le due attese, sarebbe
        // l'ultimo, e farebbe vincere i brani di questo album anche sopra la
        // lettura dell'album aperto nel frattempo. Quindi prima si guarda se
        // l'album aperto adesso (`albumDiPrima`, che l'effetto tiene
        // aggiornato) è ancora quello del clic, e lo si riguarda all'arrivo.
        const chiave = aperto?.albumKey;
        if (chiave !== undefined && albumDiPrima.current === chiave) {
          const mio = turnoAlbum();
          ipc
            .braniAlbum(chiave)
            .then((brani) => {
              if (mio() && albumDiPrima.current === chiave) setBraniAperto(brani);
            })
            .catch((errore: unknown) => {
              if (mio() && albumDiPrima.current === chiave) segnalaErrore(errore);
            });
        }
        if (playlistAperta) rileggiPlaylist();
      }
    },
    [
      aggiornaBrani,
      aperto,
      braniAperto,
      playlistAperta,
      ricarica,
      ricaricaContorno,
      ricaricaPlaylist,
      caricaVista,
      segnalaErrore,
      turnoAlbum,
      rileggiPlaylist,
    ],
  );

  /**
   * Fa partire l'elenco che si sta guardando, dal brano scelto.
   *
   * Un'identità sola per tutte le viste, e non una lambda dentro ogni `map`:
   * `elencoCorrente` **è** già la lista giusta in ognuna delle quattro — la
   * ricerca, la playlist, l'album, i brani — e una prop nuova per riga a ogni
   * disegno annullerebbe il `memo` di `RigaBrano` riga per riga.
   *
   * Da una lista rada la coda è la vista **intera**, chiesta al nucleo: le
   * righe in mano sono qualche pagina, e la coda di prima — le righe arrivate —
   * finiva dove finiva lo scorrimento. Il brano premuto si ritrova per
   * identificativo e non per posto: fra il disegno e il clic la libreria può
   * essere cambiata, e fra due risultati pari della ricerca l'ordine non è
   * promesso.
   */
  const suonaDaUnaLista = useCallback(
    (rada: ArchivioRado<Brano>, vista: VistaBrani, indice: number) => {
      const premuto = rada.riga(indice);
      // Una riga grigia non suona: lo dice, come negli elenchi interi.
      const grigia = statoDellaRiga(
        premuto?.path,
        archivioLontano.leggi(),
        premuto?.inGrazia === true,
        premuto?.copiaQui === true,
      );
      if (!rigaSuonabile(grigia)) {
        avvisaNonRaggiungibile(grigia);
        return;
      }
      ipc
        .vistaBraniId(vista, origineIpc)
        .then((ids) => {
          const dove =
            premuto !== undefined && ids[indice] !== premuto.id
              ? ids.indexOf(premuto.id)
              : indice;
          return ids.length > 0 ? ipc.suona(ids, Math.max(dove, 0)) : undefined;
        })
        .catch(segnalaErrore);
    },
    [segnalaErrore, avvisaNonRaggiungibile, origineIpc],
  );
  const suonaQui = useCallback(
    (indice: number) => {
      if (radaCorrente === null || vistaBraniCorrente === null) {
        void suonaDa(braniPieni, indice).catch(segnalaErrore);
        return;
      }
      suonaDaUnaLista(radaCorrente, vistaBraniCorrente, indice);
    },
    [suonaDa, braniPieni, radaCorrente, vistaBraniCorrente, segnalaErrore, suonaDaUnaLista],
  );

  /*
   * Le righe di «Compare anche in»: elenchi **loro**, non quello su cui agiscono
   * selezione e riproduzione (`radaCorrente`, che nella pagina di un artista è
   * «Brani»). Un tocco suona la loro lista dalla riga premuta, e il menù trova i
   * brani nella loro lista rada: con quelli di «Brani» le posizioni andrebbero a
   * un'altra riga, e il menù — che non ne vedrebbe il percorso — non toglierebbe le
   * voci che toccano il file. Senza selezione: scegliere righe di più elenchi
   * insieme non vuol dire niente.
   */
  const suonaEsatte = useCallback(
    (indice: number) =>
      suonaDaUnaLista(radaEsatte, { tipo: "partecipazioni", nome: nomeAperto, specie: "esatte" }, indice),
    [suonaDaUnaLista, radaEsatte, nomeAperto],
  );
  const suonaEuristiche = useCallback(
    (indice: number) =>
      suonaDaUnaLista(
        radaEuristiche,
        { tipo: "partecipazioni", nome: nomeAperto, specie: "euristiche" },
        indice,
      ),
    [suonaDaUnaLista, radaEuristiche, nomeAperto],
  );
  const menuEsatte = useCallback(
    (e: React.MouseEvent, ids: number[]) =>
      apriMenu(e, ids, (id) => radaEsatte.cerca((b) => b.id === id)),
    [apriMenu, radaEsatte],
  );
  const menuEuristiche = useCallback(
    (e: React.MouseEvent, ids: number[]) =>
      apriMenu(e, ids, (id) => radaEuristiche.cerca((b) => b.id === id)),
    [apriMenu, radaEuristiche],
  );

  /** I quattro ordinamenti dei brani di un artista, col loro nome. */
  const etichettaOrdineArtista = (o: OrdineArtista) =>
    o === "album"
      ? t("artist.sort.album")
      : o === "anno"
        ? t("artist.sort.year")
        : o === "titolo"
          ? t("artist.sort.title")
          : t("artist.sort.played");
  /** Il menù dell'ordinamento della sezione «Brani»: le quattro voci, dove sta il bottone. */
  const menuOrdineArtista = useCallback(
    (e: React.MouseEvent) => {
      const riquadro = e.currentTarget.getBoundingClientRect();
      setMenu({
        x: riquadro.left,
        y: riquadro.bottom + 4,
        testata: { titolo: t("sort.heading") },
        voci: ORDINI_ARTISTA.map((chiave) => ({
          etichetta: etichettaOrdineArtista(chiave),
          azione: () => setOrdineArtista(chiave),
          scelta: chiave === ordineArtista,
        })),
      });
    },
    [ordineArtista],
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
    (
      e: React.MouseEvent,
      elenco: number[],
      conosciuto?: Brano,
      piu?: Pick<AzioniBrani, "togliDallaPlaylist" | "sposta">,
    ) => {
      const dentro = elenco.some((id) => selezione.has(id));
      apriMenu(e, dentro && selezionati.length > 0 ? selezionati : elenco, conosciuto, piu);
    },
    [apriMenu, selezione, selezionati],
  );

  /**
   * Il menù di una riga di una playlist a mano: quello di sempre, più le mosse
   * della riga (su, giù, in cima: R16-6, D-72) e, sul telefono, «Togli da
   * questa playlist».
   *
   * Sul computer la riga ha la sua ×; sul telefono la riga è copertina,
   * titolo e ⋮, e la × sta qui. Vuole la **posizione** e non il brano —
   * lo stesso brano può stare due volte nella stessa playlist — e la
   * posizione la dice la riga stessa, `data-indice`, da cui viene il gesto.
   * Su una selezione di più righe la voce non c'è: le posizioni si
   * sposterebbero a ogni riga tolta, e il nucleo le toglie una per volta.
   * Il brano della riga è il solo di `elenco`, che la riga manda col suo
   * identificativo: il nucleo lo vuole accanto alla posizione, e se lì non
   * c'è più non toglie niente.
   */
  const menuRigaPlaylist = useCallback(
    (e: React.MouseEvent, elenco: number[]) => {
      const riga = (e.target as Element).closest<HTMLElement>("[data-indice]");
      const indice = riga === null ? Number.NaN : Number(riga.dataset["indice"]);
      const brano = elenco.length === 1 ? elenco[0] : undefined;
      const suPiuRighe = elenco.some((id) => selezione.has(id)) && selezionati.length > 1;
      if (!Number.isInteger(indice) || brano === undefined || suPiuRighe) {
        menuSuSelezione(e, elenco);
        return;
      }
      // Le mosse col brano accanto, come il gesto da tastiera: se la playlist
      // è cambiata sotto, il nucleo non sposta un'altra riga.
      const vai = (a: number) => () => spostaNellaPlaylist(indice, a, brano);
      menuSuSelezione(e, elenco, undefined, {
        togliDallaPlaylist: MOBILE ? () => togliDallaPlaylist(indice, brano) : undefined,
        sposta: {
          inCoda: false,
          su: indice > 0 ? vai(indice - 1) : undefined,
          giu: indice < braniPlaylist.length - 1 ? vai(indice + 1) : undefined,
          // Dalla seconda riga «in cima» è «su»: una voce sola basta.
          inCima: indice > 1 ? vai(0) : undefined,
        },
      });
    },
    [
      menuSuSelezione,
      selezione,
      selezionati.length,
      togliDallaPlaylist,
      spostaNellaPlaylist,
      braniPlaylist.length,
    ],
  );

  /** Il ⋮ della barra della selezione, sul telefono: il menù di tutti i scelti. */
  const menuDellaSelezione = useCallback(
    (e: React.MouseEvent) => apriMenu(e, selezionati),
    [apriMenu, selezionati],
  );

  /** Le scorciatoie. La mappa completa sta in `tastiera.ts`. */
  useScorciatoie(
    {
      // Letta al tasto e non al disegno: la durata cambia col brano, e `App`
      // non si ridisegna più a ogni brano. Le azioni stanno in un riferimento
      // (`useScorciatoie`), quindi questa lettura avviene a ogni pressione.
      get durataMs() {
        return riproduzione.archivio.leggi().durataMs;
      },
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
      precedente: () => {
        ipc.precedente().catch(segnalaErrore);
      },
      prossimo: () => {
        ipc.prossimo().catch(segnalaErrore);
      },
      // Letto al tasto, come la durata: un passo da dove il volume è adesso, e
      // il silenziamento si toglie, come quando si trascina il cursore.
      volume: (su) => {
        const { volume } = riproduzione.archivio.leggi();
        const dopo = Math.min(1, Math.max(0, volume + (su ? PASSO_VOLUME : -PASSO_VOLUME)));
        ipc.volume(dopo, false).catch(segnalaErrore);
      },
      muto: () => {
        const { volume, muto } = riproduzione.archivio.leggi();
        ipc.volume(volume, !muto).catch(segnalaErrore);
      },
      inRiproduzione: () => {
        if (conBrano) setGrande((prima) => !prima);
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
      // L'ultimo «Annulla» ancora a schermo — la coda sostituita, la playlist
      // eliminata — dovunque sia il fuoco (D-40). Vedi `annullaATempo.ts`.
      annulla: () => {
        ANNULLI.annullaLUltimo();
      },
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
          annullaSelezione();
          return true;
        }
        if (query.length > 0) {
          setQuery("");
          // Sul telefono Indietro sulla ricerca è «non la voglio più»: tornando
          // su Cerca il campo è vuoto, e non la frase che si è appena tolta.
          ultimaRicerca.current = "";
          return true;
        }
        // I livelli della libreria (il disco, l'artista, la ricerca) si chiudono
        // con Escape come l'album in libreria. Sul telefono li chiude Indietro,
        // che li trova da sé nella pila prima di arrivare qui (R13-5).
        if (!MOBILE && indietroSulComputer()) return true;
        // Sul telefono un disco o un artista aperto da un risultato stanno
        // sopra la ricerca, e chiudendoli si torna ai risultati, come si
        // risale una pila: `vaiAlDisco` ha svuotato il campo per mostrarli, e
        // la frase la ricorda `ultimaRicerca`.
        const tornaAllaRicerca = () => {
          if (MOBILE && vista === "cerca" && ultimaRicerca.current !== "") {
            setQuery(ultimaRicerca.current);
          }
        };
        // Un livello per pressione, nell'ordine in cui ci si è entrati. Prima
        // erano una riga sola che azzerava tutti e due, e da un album aperto
        // dentro un artista Escape saltava la pagina dell'artista — cioè
        // riportava due passi indietro chi ne aveva chiesto uno.
        if (aperto) {
          fuocoDopo.current = { verso: "ritorno", livello: daChiAlbum };
          setAperto(null);
          if (artistaAperto === null) tornaAllaRicerca();
          return true;
        }
        if (artistaAperto) {
          // Dalla stessa strada del tasto della pagina: se l'ha aperta il nome di
          // «In riproduzione», Escape e Indietro tornano lì.
          apriArtista(null);
          tornaAllaRicerca();
          return true;
        }
        // Sul telefono Indietro continua dove il computer si ferma: la
        // playlist aperta, la sezione delle Impostazioni, e la risalita fra le
        // schede fino alla Home. Vedi `passoIndietroTelefono`.
        if (MOBILE) {
          const passo = passoIndietroTelefono({
            vista,
            playlistAperta: playlistAperta !== null,
            dentroLaSezione,
            sezioneDaFuori: sezioneDaFuori.current,
          });
          if (passo === "playlist") {
            cambiandoVista(() => setPlaylistAperta(null));
            return true;
          }
          if (passo === "sezioni") {
            setDentroLaSezione(false);
            // L'elenco dall'inizio: lo scorrimento era quello della sezione.
            contenuto.current?.scrollTo({ top: 0 });
            return true;
          }
          if (passo !== null) {
            vaiA(passo.vai);
            return true;
          }
        }
        return false;
      },
    },
    scorciatoie,
    // Nello Studio i tasti sono suoi: lo Spazio non deve mettere in pausa
    // mentre si scrive un colore, né Ctrl+Z annullare un brano in coda.
    !studioVisibile,
  );

  /**
   * Il trascinamento sulla finestra: cosa farne lo decide `trascinati.ts`,
   * l'ascolto sta in `trascinamento.ts`, e qui c'è il gesto — che tocca le
   * Impostazioni, la coda e il primo avvio, cioè cose di questo componente.
   *
   * Dove si atterra: nella sezione di Impostazioni che mostra il risultato,
   * perché un'azione che avviene fuori dallo schermo è un'azione che sembra
   * non essere avvenuta. Una playlist apre l'importazione, i brani che la
   * libreria conosce vanno in coda, e dei file che non conosce si dice cosa
   * fare invece di tacere.
   */
  useTrascinati(
    // Un file lasciato sullo Studio non va in coda né fra le cartelle: sotto
    // c'è un editor, e quel che ci si lascia cadere è per lui.
    !studioVisibile,
    () => ({
      primoInAttesa:
        primoVisibile && !scansionando && (avvio?.numeri.tracks ?? 0) === 0,
      avviato: avvio !== null,
      // Il primo avvio è un modale anche lui, ma le cartelle sono per lui:
      // conta come «sopra» solo quel che sta oltre.
      finestrellaSopra: modaliAperti() > (primoVisibile ? 1 : 0),
    }),
    (piano) => {
      // Una finestrella aperta: niente dietro il velo. Lo dice la notizia,
      // che si sente anche lì (le regioni d'annuncio restano vive).
      if (piano.tipo === "fermo") {
        setNotizia(t("drop.closeFirst"));
        return;
      }
      if (piano.tipo === "skin") {
        // `vaiA` e non `setVista`: quello nudo lasciava in piedi l'album, la
        // playlist e la ricerca di prima, cioè uno stato in cui l'intestazione
        // e il corpo rispondono a due domande diverse.
        apriSezione("aspetto", true);
        // La stessa strada del selettore: una skin con lo stesso id di una
        // installata non si sostituisce senza chiedere. Vedi `installaDaFile`.
        void skinFinestra.installaDaFile(piano.percorso);
        return;
      }
      if (piano.tipo === "playlist") {
        setFilePlaylist(piano.percorso);
        return;
      }
      if (piano.brani.length > 0) {
        const quanti = piano.brani.length;
        ipc
          .codaAccoda(piano.brani)
          .then(() =>
            setNotizia(
              t("drop.queued", {
                // «1 brano» lo sceglie il motore delle lingue: vedi `plurali.ts`.
                brani: brani_(quanti),
              }),
            ),
          )
          .catch(segnalaErrore);
      }
      const cartelle = piano.cartelle;
      if (cartelle?.dove === "primo") {
        setLasciateAlPrimo((prima) => [...new Set([...prima, ...cartelle.percorsi])]);
        return;
      }
      if (cartelle?.dove === "sorvegliate" && avvio) {
        apriSezione("cartelle", true);
        // Dalla fila delle cartelle e non da `avvio`: un trascinamento mentre si
        // toglie una cartella partiva dall'elenco di prima e la rimetteva. Vedi
        // `elencoCartelle.ts`.
        void aggiungiCartelle(cartelle.percorsi);
        return;
      }
      if (piano.notizia === "fuoriLibreria") setNotizia(t("drop.notInLibrary"));
      else if (piano.notizia === "niente") setNotizia(t("drop.nothing"));
    },
    segnalaErrore,
  );

  /**
   * Il permesso di leggere la musica, sul telefono: negli stati vuoti della
   * libreria, se è lui a svuotarla (vedi `parti/AccessoTelefono.tsx`). Fuori da
   * Android è sempre pieno, e la riga non c'è.
   */
  const accessoTelefono = useAccessoTelefono();
  const numeri = avvio?.numeri;
  const vuota = numeri !== undefined && numeri.tracks === 0;
  /**
   * Nessuna cartella sorvegliata **e** nessun brano.
   *
   * Le cartelle da sole non bastano più a dire che la libreria è vuota: dalla
   * 2.4.0 ci sono brani che non stanno in nessuna cartella, quelli che arrivano
   * da Esplora. Con la condizione di prima chi ascoltava soltanto quelli —
   * l'utente della issue #1, esattamente — si vedeva rispondere «nessuna
   * cartella sorvegliata» da Brani, Album, Artisti, Home e dalla ricerca,
   * subito dopo che Esplora aveva detto «sette aggiunti alla libreria».
   */
  const senzaCartelle =
    avvio !== null && avvio.cartelle.length === 0 && vuota;
  const presenze = useMemo(
    () => mappaPresenze(skinAttiva?.presenze),
    [skinAttiva],
  );
  const guasto = errore ?? guastoDa(riproduzione.errore);
  const messaggio = guasto?.testo ?? null;
  /**
   * Il guasto in fascia si può riprovare.
   *
   * Solo quello che viene dal lettore. `errore` è un `Guasto` e porta il
   * codice (la fascia lo usa per `azioneDelGuasto`), ma non `retryable`, che
   * `guastoDa` non conserva: di lui non si sa se riprovare ha senso, mentre
   * `riproduzione.errore` arriva intero dal nucleo e `eRitentabile` lo legge.
   * Ed è l'unico dei due che abbia un gesto da offrire, `riprovaCorrente`, che
   * riguarda il brano corrente: la cartella di rete che sparisce a metà brano.
   */
  const ritentabile = errore === null && eRitentabile(riproduzione.errore);



  // Con la stessa identità finché i numeri sono gli stessi: sta nelle
  // dipendenze di `contesto`, e un `avvio` riletto dopo ogni gesto — con gli
  // stessi conteggi — ne faceva uno nuovo, e con lui una navigazione
  // ridisegnata per niente. Vedi `conteggiDi` in `libreria.ts`.
  const conteggiDiTutti = useArchivio(libreria.avvio, conteggiDi, ugualiInSuperficie);
  // Col filtro per dispositivo acceso la barra conta quel che la lista mostra (S9-6).
  // Con la stessa identità finché i numeri sono gli stessi, per la stessa ragione.
  const conteggi = useMemo(
    () => conteggiDelFiltro(conteggiDiTutti, numeriDelFiltro, origine.tipo !== "tutti"),
    // `numeriDelFiltro` è un oggetto nuovo a ogni rilettura: contano i quattro numeri.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [
      conteggiDiTutti,
      origine.tipo,
      numeriDelFiltro?.tracks,
      numeriDelFiltro?.albums,
      numeriDelFiltro?.artists,
      numeriDelFiltro?.liked,
    ],
  );

  const etichettaOrdine =
    ordinamenti().find(([c]) => c === ordine)?.[1] ?? t("sort.shelf");

  /**
   * I gesti delle Impostazioni che toccano questo componente: vedi
   * `AzioniImpostazioni` in `ImpostazioniFinestra.tsx`. Con `useStabile` dove
   * la funzione legge lo stato — le cartelle di `avvio`, il conto delle
   * scansioni in volo — perché l'oggetto resti lo stesso e il `memo` di là
   * serva a qualcosa.
   */
  // La finestrella, e non più il dialogo di sistema: da lì «Questo PC» lo apre
  // comunque, accanto ai servizi del cloud.
  const aggiungiCartella = useStabile(() => setAggiunta("finestra"));
  const togliCartellaStabile = useStabile((c: string) => void togliCartella(c));
  const scollegaContoStabile = useStabile(
    (conto: string, togli: boolean) => void scollegaNuvola(conto, togli),
  );
  const cartellaDownload = useStabile(() => void scegliCartellaDownload());
  const scansionaStabile = useStabile(() => void scansiona());
  const importaDatabase = useStabile(() => void scegliDatabase());
  const azioniImpostazioni = useMemo<AzioniImpostazioni>(
    () => ({
      onGiro: rifaiGiro,
      onProfiloImportato: dopoProfilo,
      onAggiungiCartella: aggiungiCartella,
      onTogliCartella: togliCartellaStabile,
      onScollegaConto: scollegaContoStabile,
      onScegliCartellaDownload: cartellaDownload,
      onScansiona: scansionaStabile,
      // Sul telefono i tre tasti non ci sono (lo Studio non c'è: vedi
      // `Studio`); queste guardie sono la seconda cintura, perché una via
      // nuova verso lo Studio non riapra una finestra che non ha niente dietro.
      onCreaTema: () => {
        if (MOBILE) return;
        setBaseTema(null);
        setCreandoTema(true);
      },
      onDeriva: (id) => {
        if (MOBILE) return;
        setBaseTema(id);
        setCreandoTema(true);
      },
      onApriStudio: (id) => {
        if (MOBILE) return;
        // Durante «Prova sull'app» lo Studio è nascosto: aprirne un altro
        // senza chiudere la prova non mostrava niente, e dopo «Torna» lo
        // Studio era ancora sulla bozza di prima. Si torna, e si apre.
        finisciProva();
        ricordaIlFuoco();
        setStudioAperto(id);
      },
      onImporta: importaDatabase,
      onImportaLink: () => setImportandoLink(true),
      onImportaAccount: () => setImportandoAccount(true),
      onVista: vaiA,
      onMostraDispositivo: apriDispositivo,
      onNuvolaRipristina: () => setRipristinando(true),
    }),
    [
      rifaiGiro,
      dopoProfilo,
      aggiungiCartella,
      togliCartellaStabile,
      scollegaContoStabile,
      cartellaDownload,
      scansionaStabile,
      importaDatabase,
      vaiA,
      setCreandoTema,
      setImportandoLink,
      setImportandoAccount,
      setRipristinando,
      finisciProva,
      ricordaIlFuoco,
    ],
  );

  /*
   * Le chiamate del contesto, ciascuna con la sua identità.
   *
   * Erano frecce scritte dentro il `useMemo` qui sotto, cioè nuove ogni volta
   * che quel `useMemo` si rifaceva — a ogni riga scelta, a ogni cambio di
   * colonna — e la navigazione, il lettore e la colonna, sotto `memo`, si
   * sarebbero ridisegnati per una funzione che dice la stessa cosa di prima.
   * Adesso ciascuna cambia soltanto quando cambia quel che legge.
   */
  const apriPlaylist = useCallback(
    (p: Playlist) => {
      setQuery("");
      cambiandoVista(() => {
        setAperto(null);
        setArtistaAperto(null);
        // Da una pagina che non è una destinazione bisogna anche **uscire**:
        // `corpo()` guarda `vista` per prima, quindi senza questa riga la
        // playlist si accendeva nella barra e la pagina restava quella delle
        // impostazioni. Si atterra sui Brani perché è la vista che una
        // playlist somiglia di più, ed è quella che si ritrova chiudendola.
        if (vista === "impostazioni" || vista === "importazioni") {
          setVista(MOBILE ? "playlist" : "brani");
        }
        // Sul telefono le pagine da cui uscire sono anche Altro ed Esplora, e
        // si atterra sulla pillola Playlist: è da lì che le playlist si
        // aprono, ed è lì che si torna chiudendola.
        if (MOBILE && (vista === "altro" || vista === "esplora" || vista === "cerca")) {
          setVista("playlist");
        }
        setPlaylistAperta(p);
      });
    },
    [vista],
  );

  /**
   * Il tocco su una scheda del telefono.
   *
   * Su un'altra scheda: ci si va, e la si ritrova com'era — la Libreria
   * sull'ultima pillola, Cerca sull'ultimo segmento e sull'ultima frase. Sulla
   * scheda in cui si è già: se sopra c'è aperto qualcosa si torna alla sua
   * prima pagina, se si è già lì si torna in cima. È il gesto di ogni app con
   * le schede in basso, e la mano lo prova prima di cercare una freccia.
   */
  /**
   * Apre una sezione delle Impostazioni: dall'indice, o da fuori — un avviso,
   * un tasto della pagina Altro, la notifica della scansione.
   *
   * Sul telefono ci **entra**: le Impostazioni hanno due livelli (vedi
   * `inSezione` in `Impostazioni`), e chi tocca «Apri i dati» vuole la
   * sezione, non l'elenco delle sezioni da cui cercarla. Si torna in cima, per
   * non entrare a metà della sezione allo scorrimento dell'elenco di prima.
   */
  const apriSezione = useStabile((quale: Sezione, daFuori: boolean) => {
    sezioneDaFuori.current = daFuori;
    if (daFuori) vaiA("impostazioni");
    setSezione(quale);
    if (!MOBILE) return;
    setDentroLaSezione(true);
    contenuto.current?.scrollTo({ top: 0 });
  });
  const scegliSezione = useCallback((quale: Sezione) => apriSezione(quale, false), [apriSezione]);

  const scegliScheda = useStabile((dove: SchedaTelefono) => {
    const radice = radiceDellaScheda(dove, pillola.current, segmentoCerca.current);
    const allaRadice =
      vista === radice &&
      aperto === null &&
      artistaAperto === null &&
      playlistAperta === null &&
      !cercando;
    if (dove === scheda && allaRadice) {
      contenuto.current?.scrollTo({ top: 0, behavior: comeScorrere("smooth") });
      return;
    }
    // Sulla scheda Cerca già con i risultati: il campo, per cambiare frase.
    if (dove === "cerca" && pagina.tipo === "ricerca") {
      document.querySelector<HTMLInputElement>("[data-cerca]")?.focus();
      return;
    }
    vaiA(radice);
    if (radice === "cerca" && ultimaRicerca.current !== "") {
      setQuery(ultimaRicerca.current);
    }
  });
  const nuovaPlaylist = useCallback(() => setCreandoPlaylist(true), [setCreandoPlaylist]);
  const nuovaSmart = useCallback(() => setRegoleAperte({ playlist: null }), [setRegoleAperte]);
  const importaFile = useStabile(() => void scegliFilePlaylist());
  // Fuori, nel browser di sistema, e per nome: l'indirizzo lo conosce solo
  // il nucleo, che ne tiene un elenco chiuso — vedi `apri_documento`.
  const dona = useCallback(() => {
    ipc.apriDocumento("donazioni").catch(segnalaErrore);
  }, [segnalaErrore]);
  const apriGrande = useCallback(() => setGrande(true), []);
  const chiudiGrande = useCallback(() => setGrande(false), []);
  /**
   * Il menù di una riga della coda — il ⋮ del telefono, il tasto destro del
   * computer: il menù del brano, più «Togli dalla coda», che è quel che la ×
   * della riga fa, e le mosse della riga (su, giù, in cima: R16-6, D-72), che
   * sono il modo di riordinare senza trascinare. La posizione viene dalla
   * riga: lo stesso brano può stare in coda due volte. La lunghezza della coda
   * si legge al gesto, come la sessione in `apriMenu`.
   */
  const menuDellaCoda = useCallback(
    (e: React.MouseEvent, indice: number, brano: Brano) => {
      const { coda, posizioneCoda } = riproduzione.archivio.leggi();
      const lunga = coda.length;
      // La cima della coda è la cima di quel che resta da sentire: subito dopo
      // il brano in ascolto. Il posto 0 sta nella storia, sopra di lui, e un
      // brano portato lì non suonerebbe più. Dalla riga subito sotto, «in cima»
      // è «su»: una voce sola basta.
      const cima = posizioneCoda !== null && indice > posizioneCoda ? posizioneCoda + 1 : 0;
      // Col brano accanto alla posizione: se nel frattempo la coda è cambiata,
      // il nucleo non tocca un'altra riga. Vedi `codaTogli` e `codaRiordina`.
      const vai = (a: number) => () => {
        ipc.codaRiordina(indice, a, brano.id).catch(segnalaErrore);
      };
      apriMenu(e, [brano.id], brano, {
        togliDallaCoda: () => {
          ipc.codaTogli(indice, brano.id).catch(segnalaErrore);
        },
        sposta: {
          inCoda: true,
          su: indice > 0 ? vai(indice - 1) : undefined,
          giu: indice < lunga - 1 ? vai(indice + 1) : undefined,
          inCima: indice - cima >= 2 ? vai(cima) : undefined,
        },
      });
    },
    [apriMenu, segnalaErrore, riproduzione.archivio],
  );
  /** Il ⋮ di «In riproduzione» sul telefono: il menu del brano che suona. */
  const menuDelBranoGrande = useCallback(
    (e: React.MouseEvent, brano: Brano) => apriMenu(e, [brano.id], brano),
    [apriMenu],
  );
  // Il cuore e le stelle dei widget: `cambiaPreferito` e `cambiaVoto` cambiano
  // identità con la vista (passano da `caricaVista`), e il lettore non deve
  // ridisegnarsi per essere passati da Album a Brani.
  const preferitoDelLettore = useStabile(cambiaPreferito);
  const votoDelLettore = useStabile(cambiaVoto);
  const selezioneRiproduci = useCallback(() => {
    // `selezionati` è già nell'ordine della vista, anche per le righe che
    // la finestra non ha in mano.
    if (selezionati.length > 0) ipc.suona(selezionati, 0).catch(segnalaErrore);
  }, [selezionati, segnalaErrore]);
  const selezioneDopo = useCallback(() => {
    ipc.codaDopo(selezionati).catch(segnalaErrore);
  }, [selezionati, segnalaErrore]);
  const selezioneAccoda = useCallback(() => {
    ipc.codaAccoda(selezionati).catch(segnalaErrore);
  }, [selezionati, segnalaErrore]);
  const selezionePlaylist = useCallback(
    () => setDaAggiungere(selezionati),
    [selezionati, setDaAggiungere],
  );
  const selezioneTuttiOAnnulla = useCallback(() => {
    if (selezionati.length === elencoCorrente.totale) annullaSelezione();
    else selezionaTutto();
  }, [selezionati, elencoCorrente, annullaSelezione, selezionaTutto]);

  /**
   * Tutto quel che i widget sanno del mondo, in un oggetto solo.
   *
   * È l'unico imbuto per lo stato di `App` che i widget leggono: la vista, la
   * playlist aperta, le tre aperture — colonna, coda, schermo intero — e la
   * selezione, più i conteggi e le playlist della libreria. Prima scendevano a
   * mano: `Lettore` prende sei prop, `Colonna` sei, `Coda` tre, e aggiungere un
   * widget voleva dire farne passare un'altra attraverso tre livelli che non la
   * usavano. Qui la lista si scrive una volta.
   *
   * Lo stato del motore non c'è più: lo leggono i widget dall'archivio, e
   * questo oggetto resta lo stesso mentre la musica suona, si ferma, cambia
   * volume. Vedi «Lo stato del motore non passa dal contesto» in
   * `Impaginazione.tsx`.
   */
  const contesto: ContestoScafale = useMemo(
    () => ({
      vista,
      playlistAperta,
      inLibreria: !cercando && !aperto && !artistaAperto,
      conteggi,
      playlist,
      colonnaAperta,
      codaAperta,
      grande,
      selezionati,
      tuttiSelezionati: selezionati.length === elencoCorrente.totale,
      onVista: vaiA,
      onPlaylist: apriPlaylist,
      onMenuPlaylist: menuPlaylist,
      onNuovaPlaylist: nuovaPlaylist,
      onNuovaSmart: nuovaSmart,
      onImportaFile: importaFile,
      onDona: dona,
      onColonna: colonnaAMano,
      onCoda: setCodaAperta,
      onGrande: apriGrande,
      onPreferito: preferitoDelLettore,
      onVoto: votoDelLettore,
      onErrore: segnalaErrore,
      onSelezioneRiproduci: selezioneRiproduci,
      onSelezioneDopo: selezioneDopo,
      onSelezioneAccoda: selezioneAccoda,
      onSelezionePlaylist: selezionePlaylist,
      onSelezioneTuttiOAnnulla: selezioneTuttiOAnnulla,
      onSelezioneChiudi: annullaSelezione,
      onSelezioneMenu: menuDellaSelezione,
      scheda,
      onScheda: scegliScheda,
      origine,
      onDispositivo: apriDispositivo,
      onArtista: apriArtistaDalNome,
    }),
    [
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
      vaiA,
      apriPlaylist,
      menuPlaylist,
      nuovaPlaylist,
      nuovaSmart,
      importaFile,
      dona,
      colonnaAMano,
      setCodaAperta,
      apriGrande,
      preferitoDelLettore,
      votoDelLettore,
      segnalaErrore,
      selezioneRiproduci,
      selezioneDopo,
      selezioneAccoda,
      selezionePlaylist,
      selezioneTuttiOAnnulla,
      annullaSelezione,
      menuDellaSelezione,
      scheda,
      scegliScheda,
      origine,
      apriDispositivo,
      apriArtistaDalNome,
    ],
  );

  /**
   * La testata del telefono: una barra, e non l'intestazione del computer.
   *
   * Vedi `parti/BarraAlta.tsx` per la forma. Le prime pagine delle quattro
   * schede hanno il titolo grande; le pagine dentro — un disco, un artista,
   * una playlist, le Impostazioni — la freccia e il titolo piccolo. La freccia
   * fa quel che fa il tasto Indietro del telefono, cioè la stessa scala
   * (`indietro.ts`): due modi di dire «torna» che tornassero in due posti
   * diversi sarebbero due cose da imparare.
   *
   * La ricerca sta solo nella scheda Cerca, e per le tre pagine di Cerca la
   * testata è sempre `BarraCerca`, diretta: vedi la sua nota sul fuoco.
   */
  const testaTelefono = () => {
    const torna = () => void indietro();
    const cerca = (
      <TastoBarra
        icona="i-search"
        etichetta={t("nav.search")}
        onClick={() => scegliScheda("cerca")}
      />
    );
    switch (pagina.tipo) {
      case "ricerca":
      case "cerca":
      case "esplora":
        return (
          <BarraCerca
            query={query}
            onQuery={(testo) => {
              // Svuotato a mano: tornando su Cerca il campo resta vuoto.
              if (testo === "") ultimaRicerca.current = "";
              setQuery(testo);
            }}
            sotto={
              pagina.tipo === "esplora" ? undefined : (
                <PilloleOrigine origine={origine} onOrigine={setOrigine} />
              )
            }
            segmento={pagina.tipo === "esplora" ? "esplora" : "cerca"}
            onSegmento={(segmento) => {
              vaiA(segmento);
              if (segmento === "cerca" && ultimaRicerca.current !== "") {
                setQuery(ultimaRicerca.current);
              }
            }}
            sottotitolo={
              pagina.tipo !== "ricerca"
                ? undefined
                : contandoRisultati || risultatiAdesso === null
                  ? t("common.loading")
                  : t("page.search.results", { n: risultatiAdesso })
            }
          />
        );
      case "home":
        return (
          <BarraAlta grande {...testaDellaHome} azioni={cerca} />
        );
      case "libreria":
        return (
          <BarraAlta
            grande
            titolo={t("nav.library")}
            azioni={
              <>
                {pagina.vista === "brani" && (
                  <TastoBarra
                    icona="i-sort"
                    etichetta={t("header.sort", { nome: etichettaOrdine })}
                    onClick={menuOrdine}
                  />
                )}
                {cerca}
              </>
            }
            sotto={
              <>
                <PilloleLibreria vista={pagina.vista} onVista={vaiA} />
                {/* Il filtro per dispositivo, solo sulle viste che ne dipendono: le
                    playlist nominano le loro righe una per una, e le cartelle sono di
                    questa macchina. */}
                {eFiltrabile(pagina.vista) && (
                  <PilloleOrigine origine={origine} onOrigine={setOrigine} />
                )}
              </>
            }
          />
        );
      case "altro":
        return <BarraAlta grande titolo={t("nav.more")} />;
      case "impostazioni":
        // Dentro una sezione la barra dice quale, e la freccia torna
        // all'elenco (`passoIndietroTelefono`, «sezioni»).
        return (
          <BarraAlta
            indietro={torna}
            titolo={dentroLaSezione ? etichettaSezione(sezione) : t("page.settings.title")}
          />
        );
      case "importazioni":
        return (
          <BarraAlta
            indietro={torna}
            titolo={t("imports.title")}
            azioni={
              <TastoBarra
                icona="i-external"
                etichetta={t("imports.pasteLink")}
                onClick={() => setImportandoLink(true)}
              />
            }
          />
        );
      case "playlist":
        return (
          <BarraAlta
            indietro={torna}
            ritirato={braniPlaylist.length > 0}
            titolo={pagina.playlist.name}
            sottotitolo={`${brani_(pagina.playlist.tracks)}${
              pagina.playlist.durationMs > 0 ? ` · ${durata(pagina.playlist.durationMs)}` : ""
            }`}
            azioni={
              <TastoBarra
                icona="i-dots-v"
                etichetta={t("row.more", { titolo: pagina.playlist.name })}
                onClick={(e) => menuPlaylist(e, pagina.playlist)}
              />
            }
          />
        );
      case "album":
        return (
          <BarraAlta
            indietro={torna}
            ritirato
            titolo={titoloAlbum(pagina.album.title)}
            sottotitolo={`${nomeArtista(pagina.album.artist)}${
              pagina.album.year ? ` · ${pagina.album.year}` : ""
            }`}
            azioni={
              <TastoBarra
                icona="i-dots-v"
                etichetta={t("row.more", { titolo: titoloAlbum(pagina.album.title) })}
                onClick={(e) => menuAlbum(e, pagina.album.albumKey, pagina.album)}
              />
            }
          />
        );
      case "artista":
        return (
          <BarraAlta
            indietro={torna}
            ritirato
            titolo={nomeArtista(pagina.artista.name)}
            sottotitolo={t("page.artist.sub", {
              brani: brani_(pagina.artista.tracks),
              album: t("artists.albums", { n: pagina.artista.albums }),
            })}
            // Il filtro per dispositivo, come in Libreria: vale anche qui.
            sotto={<PilloleOrigine origine={origine} onOrigine={setOrigine} />}
          />
        );
    }
  };

  /**
   * L'intestazione, che cambia con quel che si sta guardando.
   *
   * Quale pagina sia lo dice `pagina`, e lo dice una volta sola per
   * l'intestazione e per il corpo: vedi `pagina.ts`.
   *
   * Ogni pagina con la casella di ricerca restituisce `Intestazione` **diretta**,
   * mai un componente che la avvolge: alla prima lettera la pagina diventa la
   * ricerca, e un tipo diverso nello stesso posto fa rifare a React la testata
   * da capo — col campo nuovo senza fuoco, e chi scriveva doveva ricliccarci.
   * Le testate di Home e libreria arrivano per questo come prop
   * (`testaHome`, `testaLibreria`).
   */
  const testa = () => {
    if (MOBILE) return testaTelefono();
    if (pagina.tipo === "importazioni") {
      return (
        <ConImportazioni>
          {(importazioni) => (
            <TestaImportazioni
              importazioni={importazioni}
              onIncollaLink={() => setImportandoLink(true)}
            />
          )}
        </ConImportazioni>
      );
    }
    if (pagina.tipo === "impostazioni") {
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
    if (pagina.tipo === "esplora") {
      return (
        <Intestazione
          occhiello={t("page.explore.eyebrow")}
          titolo={t("page.explore.title")}
          sottotitolo={t("page.explore.sub")}
        />
      );
    }
    if (pagina.tipo === "ricerca") {
      return (
        <Intestazione
          occhiello={t("page.search.eyebrow")}
          titolo={`«${query.trim()}»`}
          /* Il conteggio del nucleo, non la lunghezza dell'elenco in mano:
             quello diceva «60 risultati» per una ricerca che ne aveva
             trecento, cioè il limite travestito da numero. Finché non è
             arrivato non si scrive niente — un numero provvisorio che poi
             cambia è peggio di nessun numero.

             E nemmeno nei 140 ms fra il tasto e la richiesta: lì il titolo
             dice già la frase nuova e `risultatiAdesso` è ancora quello della
             vecchia, cioè «12 risultati» di «sub» sotto «subs».
             `contandoRisultati` copre anche quel tratto. */
          sottotitolo={
            contandoRisultati || risultatiAdesso === null
              ? t("common.loading")
              : t("page.search.results", { n: risultatiAdesso })
          }
          query={query}
          onQuery={setQuery}
          azioni={<PilloleOrigine origine={origine} onOrigine={setOrigine} />}
        />
      );
    }
    if (pagina.tipo === "playlist") {
      return (
        <Intestazione
          occhiello={
            pagina.playlist.isSmart
              ? t("page.playlist.smart")
              : t("page.playlist")
          }
          titolo={pagina.playlist.name}
          sottotitolo={`${brani_(pagina.playlist.tracks)}${
            pagina.playlist.durationMs > 0
              ? ` · ${durata(pagina.playlist.durationMs)}`
              : ""
          }`}
          query={query}
          onQuery={setQuery}
          azioni={
            <button
              type="button"
              className="pillola btn-accent"
              disabled={braniPlaylist.length === 0 || !riproduzione.disponibile}
              onClick={() => void suonaDa(braniPlaylist, 0, false).catch(segnalaErrore)}
            >
              <Icona nome="i-play" dim={14} />
              {t("action.play")}
            </button>
          }
        />
      );
    }
    if (pagina.tipo === "album") {
      return (
        <Intestazione
          occhiello={t("page.album")}
          titolo={titoloAlbum(pagina.album.title)}
          sottotitolo={`${nomeArtista(pagina.album.artist)}${pagina.album.year ? ` · ${pagina.album.year}` : ""} · ${brani_(
            pagina.album.totalTracks,
          )}${pagina.album.genre ? ` · ${pagina.album.genre}` : ""}`}
          /* L'unica pagina che ha un'immagine sua, e per questo la porta:
             «Album» senza la copertina è un titolo, con la copertina è un
             disco. Piena, non miniatura — a novantasei pixel su uno schermo a
             150% la miniatura da 160 è già al limite. */
          copertina={
            <Presenza parte="hero-art" nome={COPERTINA_IN_VOLO} compagna="card-cover">
              <Copertina
                hash={pagina.album.coverArtHash}
                titolo={titoloAlbum(pagina.album.title)}
                classe="hero-art"
                piena
                accanto
              />
            </Presenza>
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
                className="pillola btn-ghost back-button"
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
                onClick={() => void suonaDa(braniAperto, 0, false).catch(segnalaErrore)}
              >
                <Icona nome="i-play" dim={14} />
                {t("action.play")}
              </button>
              {/* Quel che «Sfoglia» offriva sul disco: Casuale, e Scarica qui se
                  il disco sta su un altro dispositivo. */}
              <button
                type="button"
                className="pillola btn-ghost"
                disabled={braniAperto.length === 0 || !riproduzione.disponibile}
                onClick={() => suonaElenco(braniAperto, true)}
              >
                <Icona nome="i-shuffle" dim={14} />
                {t("action.shuffle")}
              </button>
              {braniAperto.some((b) => dispositivoDellaRiga(b.path) !== null) && (
                <button
                  type="button"
                  className="pillola btn-ghost"
                  onClick={() => void scaricaQui(braniAperto).catch(segnalaErrore)}
                >
                  <Icona nome="i-import" dim={14} />
                  {t("devices.download.album")}
                </button>
              )}
            </>
          }
        />
      );
    }
    if (pagina.tipo === "artista") {
      const nome = pagina.artista.name;
      const nonSuonabile = pagina.artista.tracks === 0 || !riproduzione.disponibile;
      return (
        <Intestazione
          occhiello={t("page.artist")}
          titolo={nomeArtista(nome)}
          sottotitolo={t("page.artist.sub", {
            brani: brani_(pagina.artista.tracks),
            album: t("artists.albums", { n: pagina.artista.albums }),
          })}
          query={query}
          onQuery={setQuery}
          azioni={
            <>
              {/* Torna da dove si è entrati, e lo dice: dal nome in «In
                  riproduzione» si torna lì, da una griglia agli artisti. */}
              <button
                type="button"
                className="pillola btn-ghost back-button"
                onClick={() => apriArtista(null)}
              >
                <Icona nome="i-chev-l" dim={14} />
                {tornaAInRiproduzione(artistaDaInRiproduzione, artistaAperto)
                  ? t("np.nowPlaying")
                  : t("nav.artists")}
              </button>
              {/* I suoi brani, come sulla pagina di un disco: tre bottoni veri,
                  con un nome che dice di chi. Prima c'erano solo sul telefono, e
                  sul computer la pagina dell'artista non si poteva suonare. */}
              <button
                type="button"
                className="pillola btn-accent"
                disabled={nonSuonabile}
                aria-label={t("artist.play.aria", { nome: nomeArtista(nome) })}
                onClick={() => void suonaArtista(nome, false)}
              >
                <Icona nome="i-play" dim={14} />
                {t("action.play")}
              </button>
              <button
                type="button"
                className="pillola btn-ghost"
                disabled={nonSuonabile}
                aria-label={t("artist.shuffle.aria", { nome: nomeArtista(nome) })}
                onClick={() => void suonaArtista(nome, true)}
              >
                <Icona nome="i-shuffle" dim={14} />
                {t("action.shuffle")}
              </button>
              {chiaviCollegate && origine.tipo !== "locale" && (
                <button
                  type="button"
                  className="pillola btn-ghost"
                  aria-label={t("artist.download.aria", { nome: nomeArtista(nome) })}
                  onClick={() => void scaricaArtista(nome)}
                >
                  <Icona nome="i-import" dim={14} />
                  {t("devices.download")}
                </button>
              )}
              {/* Il filtro per dispositivo vale anche qui: i suoi brani, i dischi
                  e le partecipazioni sono quelli del filtro. */}
              <PilloleOrigine origine={origine} onOrigine={setOrigine} />
            </>
          }
        />
      );
    }
    if (pagina.tipo === "home") {
      return <Intestazione {...testaDellaHome} query={query} onQuery={setQuery} />;
    }
    // Le due pagine del solo telefono: sul computer non ci si arriva, e la
    // testata del telefono le ha già prese sopra.
    if (pagina.tipo === "altro" || pagina.tipo === "cerca") return null;
    return (
      <Intestazione
        {...testaLibreria({
          vista: pagina.vista,
          // Col filtro acceso, la lista che sta sotto (S9-6).
          numeri: numeriDelFiltro,
          cartelle: avvio?.cartelle.length ?? 0,
          ordinamento: { etichetta: etichettaOrdine, onApri: menuOrdine },
        })}
        query={query}
        onQuery={setQuery}
        azioni={
          eFiltrabile(pagina.vista) ? (
            <PilloleOrigine origine={origine} onOrigine={setOrigine} />
          ) : undefined
        }
      />
    );
  };

  /**
   * La vista tenuta montata da mostrare, o `null` se il corpo è un'altra cosa.
   *
   * Non basta `vistaTenuta`: con la libreria vuota o senza cartelle il corpo
   * dice quello, e la vista resta nascosta.
   */
  const tenutaVisibile: VistaTenuta | null =
    vistaTenuta !== null && !(senzaCartelle && !messaggio) && !(vuota && !scansionando)
      ? vistaTenuta
      : null;
  /**
   * Le viste tenute montate già aperte almeno una volta: solo loro si montano.
   *
   * Uno stato che si aggiorna durante il disegno, ed è il modo che React prevede
   * per uno stato che dipende da quel che si è visto prima: la vista appena
   * aperta entra nell'elenco nello stesso disegno in cui la si guarda, senza un
   * fotogramma vuoto in mezzo.
   */
  const [visitate, setVisitate] = useState<readonly VistaTenuta[]>([]);
  if (tenutaVisibile !== null && !visitate.includes(tenutaVisibile)) {
    setVisitate([...visitate, tenutaVisibile]);
  }

  /** Il corpo quando non si guarda una vista tenuta montata. */
  const corpoAltro = () => {
    if (pagina.tipo === "importazioni") {
      return (
        <ConImportazioni>
          {(importazioni) => (
            <SchermataImportazioni
              importazioni={importazioni}
              onIncollaLink={() => setImportandoLink(true)}
              onApriArchivio={() => setImportandoAccount(true)}
            />
          )}
        </ConImportazioni>
      );
    }
    if (pagina.tipo === "esplora") {
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
               funzionato. `caricaVista` e non le due `ricarica` a mano:
               cambia anche la griglia degli artisti e la Home. */
            caricaVista();
            /* E i contatori della barra, che sono di `avvio`: nessuna delle
               due righe sopra li sfiora, e restavano ai numeri di prima
               finché non si riavviava. Da loro dipende anche se la libreria
               è vuota — cioè se Brani mostra i brani appena aggiunti o
               «nessuna cartella sorvegliata». */
            void ricarica();
          }}
        />
      );
    }
    if (pagina.tipo === "impostazioni") {
      return (
        <ImpostazioniDellaFinestra
          sezione={sezione}
          onSezione={scegliSezione}
          inSezione={MOBILE ? dentroLaSezione : undefined}
          onNotizia={setNotizia}
          onErrore={segnalaErrore}
          aspetto={aspetto}
          skin={skinFinestra}
          azioni={azioniImpostazioni}
          cartelleOccupate={cartelleOccupate}
        />
      );
    }

    // Le due pagine del solo telefono che non guardano la libreria: ci sono
    // anche con la libreria vuota, come le Impostazioni.
    if (pagina.tipo === "altro") {
      return (
        <Altro
          onInsieme={apriFinestra}
          onImportazioni={() => vaiA("importazioni")}
          onIncollaLink={() => setImportandoLink(true)}
          onCartelle={() => {
            apriSezione("cartelle", true);
          }}
          onImpostazioni={() => {
            vaiA("impostazioni");
            setDentroLaSezione(false);
            sezioneDaFuori.current = false;
          }}
          onDona={dona}
          onDispositivo={apriDispositivo}
          onCollega={() => apriSezione("dispositivi", true)}
        />
      );
    }
    if (pagina.tipo === "cerca") {
      return (
        <StatoVuoto
          icona="i-search"
          titolo={t("search.landing.title")}
          testo={t("search.landing.body")}
        />
      );
    }

    if (senzaCartelle && !messaggio) {
      return (
        <StatoVuoto
          icona="i-folder"
          titolo={t("empty.noFolders.title")}
          testo={t("empty.noFolders.body")}
          nota={notaDelPermesso(accessoTelefono)}
          azione={{
            etichetta: t("empty.noFolders.cta"),
            fai: () => setAggiunta("finestra"),
          }}
        />
      );
    }

    if (vuota && !scansionando && !cercando) {
      return (
        <StatoVuoto
          icona="i-scan"
          titolo={t("empty.library.title")}
          testo={t("empty.library.body")}
          nota={notaDelPermesso(accessoTelefono)}
          azione={{ etichetta: t("empty.library.cta"), fai: () => void scansiona() }}
        />
      );
    }

    if (pagina.tipo === "ricerca") {
      // I segnaposto finché non si sa quanti sono: fra un tasto premuto e la
      // richiesta c'è il ritardo dei 140 ms, e il conteggio è una chiamata sua.
      // Poi l'elenco è lungo quanto i risultati, con gli scheletri dove le
      // righe non sono ancora arrivate.
      if (contandoRisultati) return <ElencoFinto />;
      return risultatiAdesso === 0 ? (
        <StatoVuoto
          icona="i-search"
          titolo={t("empty.search.title")}
          testo={t("empty.search.body")}
        />
      ) : (
        <>
          <ElencoBraniCollegato
            key="cerca"
            rada={radaRicerca}
            scorrevole={contenuto}
            suonabile={riproduzione.disponibile}
            selezione={selezione}
            onSuona={suonaQui}
            onPreferito={cambiaPreferito}
            onVoto={cambiaVoto}
            onMenu={menuSuSelezione}
            onSeleziona={seleziona}
            onAlternaSelezione={alternaSelezione}
            inSelezione={MOBILE && selezionati.length > 0}
            onSelezionaTutto={selezionaTutto}
          />
        </>
      );
    }

    if (
      pagina.tipo === "playlist" &&
      playlistCaricata === pagina.playlist.id &&
      braniPlaylist.length === 0
    ) {
      // Una playlist vuota mostrava l'intestazione di colonna dell'elenco e
      // sotto niente, cioè una tabella senza righe che sembra non caricata.
      // Qui si dice che è vuota, e come si riempie: le due strade sono diverse
      // per le due specie.
      return (
        <StatoVuoto
          icona={pagina.playlist.isSmart ? "i-settings" : "i-list"}
          titolo={t("empty.playlist.title")}
          testo={
            pagina.playlist.isSmart
              ? t("empty.playlist.smart")
              : t("empty.playlist.body")
          }
        />
      );
    }

    if (pagina.tipo === "playlist") {
      return (
        <>
          {MOBILE && (
            <TestataDettaglio
              occhiello={
                pagina.playlist.isSmart ? t("page.playlist.smart") : t("page.playlist")
              }
              titolo={pagina.playlist.name}
              sottotitolo={`${brani_(pagina.playlist.tracks)}${
                pagina.playlist.durationMs > 0 ? ` · ${durata(pagina.playlist.durationMs)}` : ""
              }`}
              // Le prime copertine diverse dei suoi brani: una playlist una
              // copertina sua non ce l'ha.
              copertine={[...new Set(braniPlaylist.map((b) => b.coverArtHash))].slice(0, 4)}
              spento={braniPlaylist.length === 0 || !riproduzione.disponibile}
              onRiproduci={() => suonaElenco(braniPlaylist, false)}
              onCasuale={() => suonaElenco(braniPlaylist, true)}
            />
          )}
          {pagina.playlist.isSmart && (
            <p className="nota">
              <Trans
                k="page.playlist.smart.note"
                v={{ adesso: <strong>{t("rules.intro.now")}</strong> }}
              />
            </p>
          )}
          <ElencoBraniCollegato
            key="playlist"
            righe={braniPlaylist}
            scorrevole={contenuto}
            suonabile={riproduzione.disponibile}
            selezione={selezione}
            onSuona={suonaQui}
            onPreferito={cambiaPreferito}
            onVoto={cambiaVoto}
            onMenu={!pagina.playlist.isSmart ? menuRigaPlaylist : menuSuSelezione}
            onSeleziona={seleziona}
            onAlternaSelezione={alternaSelezione}
            inSelezione={MOBILE && selezionati.length > 0}
            onSelezionaTutto={selezionaTutto}
            /* Lo stesso brano può stare due volte in una playlist: la chiave
               porta anche la posizione, o React accoppierebbe le due righe. */
            chiaveConIndice
            conTogli={!pagina.playlist.isSmart}
            /* Solo in una playlist a mano: l'appartenenza di una automatica la
               decidono le sue regole, e un ordine deciso qui sarebbe cancellato
               dal primo ricalcolo. */
            {...(pagina.playlist.isSmart
              ? {}
              : {
                  onTogli: togliDallaPlaylist,
                  onRiordina: spostaNellaPlaylist,
                  onPresa: presa,
                  onMira: mira,
                  onLascia: lascia,
                  mirata,
                  trascinata,
                })}
          />
        </>
      );
    }

    if (pagina.tipo === "album") {
      return (
        <>
          {MOBILE && (
            <TestataDettaglio
              occhiello={t("page.album")}
              titolo={titoloAlbum(pagina.album.title)}
              sottotitolo={`${nomeArtista(pagina.album.artist)}${
                pagina.album.year ? ` · ${pagina.album.year}` : ""
              } · ${brani_(pagina.album.totalTracks)}`}
              copertine={[pagina.album.coverArtHash]}
              spento={braniAperto.length === 0 || !riproduzione.disponibile}
              onRiproduci={() => suonaElenco(braniAperto, false)}
              onCasuale={() => suonaElenco(braniAperto, true)}
              sotto={
                braniAperto.some((b) => dispositivoDellaRiga(b.path) !== null) ? (
                  <div className="azioni">
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      onClick={() => void scaricaQui(braniAperto).catch(segnalaErrore)}
                    >
                      <Icona nome="i-import" dim={14} />
                      {t("devices.download.album")}
                    </button>
                  </div>
                ) : undefined
              }
            />
          )}
          <ElencoBraniCollegato
            key="album-aperto"
            righe={braniAperto}
            scorrevole={contenuto}
            suonabile={riproduzione.disponibile}
            selezione={selezione}
            onSuona={suonaQui}
            onPreferito={cambiaPreferito}
            onVoto={cambiaVoto}
            onMenu={menuSuSelezione}
            onSeleziona={seleziona}
            onAlternaSelezione={alternaSelezione}
            inSelezione={MOBILE && selezionati.length > 0}
            onSelezionaTutto={selezionaTutto}
            /* L'unico posto in cui `#` è il numero del disco: qui la colonna dice
               dove sta il pezzo sulla custodia. */
            numeroTraccia
          />
        </>
      );
    }

    if (pagina.tipo === "artista") {
      const nome = pagina.artista.name;
      // Col filtro di adesso questo artista non ha niente: lo dice, invece di una
      // pagina con tre sezioni vuote. Con «Tutti» no — lì un artista c'è perché ha
      // dei brani — e il vuoto non si dice mai prima di aver letto i numeri.
      if (
        origine.tipo !== "tutti" &&
        pagina.artista.tracks === 0 &&
        pagina.artista.albums === 0 &&
        partecipazioni.esatte === 0 &&
        partecipazioni.euristiche === 0
      ) {
        return (
          <StatoVuoto
            icona="i-artist"
            titolo={t("library.origin.empty.title")}
            testo={t("artist.empty.filter")}
          />
        );
      }
      // `radaAlbumArtista` **è** già l'elenco dell'artista: il nucleo lo filtra
      // con la stessa chiave con cui raggruppa gli artisti. Prima si filtrava
      // qui la pagina già scaricata, ed era il motivo per cui un artista oltre il
      // quattrocentesimo album dava una griglia vuota.
      return (
        <>
          {MOBILE && (
            <TestataDettaglio
              occhiello={t("page.artist")}
              titolo={nomeArtista(nome)}
              sottotitolo={t("page.artist.sub", {
                brani: brani_(pagina.artista.tracks),
                album: t("artists.albums", { n: pagina.artista.albums }),
              })}
              copertine={pagina.artista.covers}
              rotonda
              spento={pagina.artista.tracks === 0 || !riproduzione.disponibile}
              onRiproduci={() => void suonaArtista(nome, false)}
              onCasuale={() => void suonaArtista(nome, true)}
              sotto={
                // Solo se c'è un dispositivo da cui scaricare, e un filtro che
                // può mostrarne le righe: come sulla pagina di un disco.
                chiaviCollegate && origine.tipo !== "locale" ? (
                  <div className="azioni">
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      aria-label={t("artist.download.aria", { nome: nomeArtista(nome) })}
                      onClick={() => void scaricaArtista(nome)}
                    >
                      <Icona nome="i-import" dim={14} />
                      {t("devices.download")}
                    </button>
                  </div>
                ) : undefined
              }
            />
          )}
          <GrigliaAlbum
            key={`artista:${nome}`}
            rada={radaAlbumArtista}
            sotto="anno"
            onApri={apriAlbum}
            onMenu={menuAlbum}
            scorrevole={contenuto}
          />
          {pagina.artista.tracks > 0 && (
            <SezioneArtista
              id="artista-brani"
              titolo={t("artist.tracks.title")}
              conto={brani_(pagina.artista.tracks)}
              azioni={
                <button
                  type="button"
                  className="pillola btn-ghost"
                  onClick={menuOrdineArtista}
                >
                  <Icona nome="i-sort" dim={14} />
                  {t("header.sort", { nome: etichettaOrdineArtista(ordineArtista) })}
                </button>
              }
            >
              {/* Senza `chiave`: cambiare l'ordinamento riporterebbe in cima a
                  tutta la pagina — i dischi — mentre chi ha cambiato l'ordine sta
                  guardando i brani. La lista rada si rifà da sé (`radaBraniArtista`). */}
              <ElencoBraniCollegato
                key={`brani-artista:${nome}`}
                rada={radaBraniArtista}
                scorrevole={contenuto}
                suonabile={riproduzione.disponibile}
                selezione={selezione}
                onSuona={suonaQui}
                onPreferito={cambiaPreferito}
                onVoto={cambiaVoto}
                onMenu={menuSuSelezione}
                onSeleziona={seleziona}
                onAlternaSelezione={alternaSelezione}
                inSelezione={MOBILE && selezionati.length > 0}
                onSelezionaTutto={selezionaTutto}
              />
            </SezioneArtista>
          )}
          {partecipazioni.esatte > 0 && (
            <SezioneArtista
              id="artista-compare"
              titolo={t("artist.appears.title")}
              conto={brani_(partecipazioni.esatte)}
              nota={t("artist.appears.sub")}
            >
              <ElencoBraniCollegato
                key={`compare:${nome}`}
                rada={radaEsatte}
                scorrevole={contenuto}
                suonabile={riproduzione.disponibile}
                selezione={NESSUNA_SCELTA}
                onSuona={suonaEsatte}
                onPreferito={cambiaPreferito}
                onVoto={cambiaVoto}
                onMenu={menuEsatte}
                onSeleziona={senzaSelezione}
                onAlternaSelezione={senzaSelezione}
              />
            </SezioneArtista>
          )}
          {partecipazioni.euristiche > 0 && (
            <SezioneArtista
              id="artista-forse"
              titolo={t("artist.maybe.title")}
              conto={brani_(partecipazioni.euristiche)}
              nota={t("artist.maybe.sub")}
            >
              <ElencoBraniCollegato
                key={`forse:${nome}`}
                rada={radaEuristiche}
                scorrevole={contenuto}
                suonabile={riproduzione.disponibile}
                selezione={NESSUNA_SCELTA}
                onSuona={suonaEuristiche}
                onPreferito={cambiaPreferito}
                onVoto={cambiaVoto}
                onMenu={menuEuristiche}
                onSeleziona={senzaSelezione}
                onAlternaSelezione={senzaSelezione}
              />
            </SezioneArtista>
          )}
        </>
      );
    }

    if (pagina.tipo === "home") {
      return (
        <Home
          casa={casa}
          guasto={guastoCasa}
          onRiprova={rileggiCasa}
          settimana={settimana}
          onAggiungiCartella={aggiungiCartella}
          onSuona={(elenco, indice) => void suonaDa(elenco, indice).catch(segnalaErrore)}
          onApriRaccolta={(raccolta) => {
            void (async () => {
              try {
                await suonaDa(raccolta.brani, 0, false);
              } catch (e) {
                segnalaErrore(e);
                return;
              }
              // Segnata aperta **dopo** che è partita: se il comando di
              // riproduzione fallisce, la raccolta è ancora nuova — e il
              // pallino che dice «non l'hai ancora sentita» dice il vero.
              // Vero da quando `suonaDa` rilancia l'errore: prima lo
              // inghiottiva, e si arrivava qui comunque.
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
          onMenu={(e, brano) => menuSuSelezione(e, [brano.id], brano)}
          onApriAlbum={apriAlbum}
          onMenuAlbum={(e, album) => menuAlbum(e, album.albumKey, album)}
        />
      );
    }

    // Le Cartelle non sono una lista rada e non restano montate: l'albero se lo
    // chiede da sé, un livello alla volta, e uno scheletro di righe di brani
    // sopra un albero sarebbe l'attesa di qualcos'altro.
    // La pillola Playlist del telefono: l'elenco che sul computer sta nella
    // barra laterale.
    if (pagina.tipo === "libreria" && pagina.vista === "playlist") {
      return (
        <ElencoPlaylist
          playlist={playlist}
          onApri={apriPlaylist}
          onMenu={menuPlaylist}
          onNuova={nuovaPlaylist}
          onNuovaSmart={nuovaSmart}
          onImportaFile={importaFile}
        />
      );
    }

    if (vista === "cartelle") {
      // Senza `avvio` le radici non si sanno: un elenco vuoto qui farebbe dire
      // alla pagina «nessuna cartella», che non è vero. Vedi `CartelleFinte`.
      if (avvio === null) return <CartelleFinte />;
      return (
        <CartelleCollegate
          radici={avvio.cartelle}
          onMenu={setMenu}
          onErrore={segnalaErrore}
          onAggiungiCartella={aggiungiCartella}
          /* Il pannello dei brani della cartella è l'elenco di sempre, e si
             comporta come gli altri: stesso menù, stessi cuori e stelle, la
             stessa finestrella delle playlist. */
          mani={{
            suonabile: riproduzione.disponibile,
            onPreferito: cambiaPreferito,
            onVoto: cambiaVoto,
            onMenu: apriMenu,
            onAggiungiAPlaylist: (brani) => setDaAggiungere(brani),
          }}
        />
      );
    }

    // Le altre viste della libreria stanno sotto `<Activity>`: vedi `corpo`.
    return null;
  };

  /**
   * Una vista tenuta montata, disegnata: vedi `VISTE_TENUTE`.
   *
   * `memoria` su tutte e quattro: lo scorrimento si ricorda e torna con lei. La
   * nascosta riceve una selezione vuota e non quella di adesso, che è della
   * vista che si guarda — senza, ogni Ctrl+clic nei Preferiti ridisegnerebbe,
   * dietro le quinte, anche i Brani.
   */
  const disegnaTenuta = (v: VistaTenuta, attiva: boolean): React.ReactNode => {
    const scelti = attiva ? selezione : NESSUNA_SCELTA;
    switch (v) {
      case "brani":
      case "preferiti":
        // Un filtro per dispositivo che non ha niente: dice perché (sta ancora
        // leggendo, è spento, è di una versione vecchia) invece di un elenco vuoto.
        if (
          origine.tipo !== "tutti" &&
          numeriDelFiltro !== undefined &&
          (v === "brani" ? numeriDelFiltro.tracks : numeriDelFiltro.liked) === 0
        ) {
          return <VuotoDelFiltro origine={origine} onErrore={segnalaErrore} />;
        }
        // Nessun preferito: l'elenco disegnava l'intestazione delle colonne
        // sopra il vuoto, cioè una tabella che sembra non essersi caricata. Il
        // conto viene da `avvio`, che il cuore tiene allineato da sé
        // (`cambiaPreferito`), quindi non c'è un istante in cui menta.
        if (v === "preferiti" && numeriDelFiltro !== undefined && numeriDelFiltro.liked === 0) {
          return (
            <StatoVuoto
              icona="i-heart"
              titolo={t("empty.liked.title")}
              testo={t("empty.liked.body")}
            />
          );
        }
        return (
          <ElencoBraniCollegato
            rada={v === "brani" ? radaBrani : radaPreferiti}
            /* Un altro ordinamento, o un altro filtro per dispositivo, è un altro
               elenco: in cima (S9-7). Solo scelte dell'utente: un totale che
               cresce leggendo un catalogo non deve riportare in cima. */
            chiave={v === "brani" ? `${ordine}:${chiaveOrigine}` : chiaveOrigine}
            memoria
            scorrevole={contenuto}
            suonabile={riproduzione.disponibile}
            selezione={scelti}
            onSuona={suonaQui}
            onPreferito={cambiaPreferito}
            onVoto={cambiaVoto}
            onMenu={menuSuSelezione}
            onSeleziona={seleziona}
            onAlternaSelezione={alternaSelezione}
            inSelezione={MOBILE && selezionati.length > 0}
            onSelezionaTutto={selezionaTutto}
          />
        );
      case "album":
        if (origine.tipo !== "tutti" && numeriDelFiltro !== undefined && numeriDelFiltro.albums === 0) {
          return <VuotoDelFiltro origine={origine} onErrore={segnalaErrore} />;
        }
        return (
          <GrigliaAlbum
            rada={radaAlbum}
            sotto="artista"
            onApri={apriAlbum}
            onMenu={menuAlbum}
            scorrevole={contenuto}
            memoria
            chiave={chiaveOrigine}
          />
        );
      case "artisti":
        if (origine.tipo !== "tutti" && numeriDelFiltro !== undefined && numeriDelFiltro.artists === 0) {
          return <VuotoDelFiltro origine={origine} onErrore={segnalaErrore} />;
        }
        // I segnaposto solo la prima volta: una rilettura degli artisti tiene a
        // schermo quelli di prima finché non arrivano i nuovi.
        if (artisti.length === 0 && artistiInArrivo) return <GrigliaFinta />;
        // Una lettura fallita non è una libreria senza artisti: lo stato vuoto
        // qui direbbe «nessun artista», che è la frase sbagliata. Con degli
        // artisti già in mano si tengono quelli, e il guasto lo dice la fascia.
        if (artisti.length === 0 && artistiGuasto) {
          return (
            <StatoVuoto
              icona="i-alert"
              titolo={t("artists.error.title")}
              testo={t("artists.error.hint")}
              azione={{ etichetta: t("common.retry"), fai: invalidaArtisti }}
            />
          );
        }
        return (
          <Artisti
            artisti={artisti}
            scorrevole={contenuto}
            memoria
            chiave={chiaveOrigine}
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
                testata: {
                  titolo: nomeArtista(a.name),
                  sottotitolo: t("artists.albums", { n: a.albums }),
                },
                voci: vociArtista(
                  { apri: () => apriArtista(a), cercaPerNome: () => setQuery(a.name) },
                  t,
                ),
              });
            }}
          />
        );
    }
  };

  /**
   * Il corpo, cioè quel che sta sotto l'intestazione.
   *
   * Le viste tenute montate già visitate, ciascuna sotto il suo `<Activity>` —
   * visibile quella che si guarda, nascoste le altre — e sotto, quando non si
   * guarda una di loro, la pagina di adesso (`corpoAltro`). Una vista mai
   * aperta non si monta: tenerla nascosta dal primo istante vorrebbe dire
   * disegnarla per nessuno.
   *
   * La libreria vuota e quella senza cartelle si dicono al posto della vista:
   * la vista resta montata e nascosta, e torna quando la libreria si riempie.
   */
  /*
   * Ogni pagina nel suo recinto (`Recinto.tsx`): una che cade prende il posto
   * di sé stessa e basta, e la navigazione, il lettore e la coda restano. Le
   * viste tenute montate ne hanno uno ciascuna, dentro il loro `<Activity>`;
   * le altre ne dividono uno che si rialza da solo cambiando pagina.
   */
  const corpo = () => (
    <>
      {VISTE_TENUTE.filter((v) => visitate.includes(v)).map((v) => (
        <Activity key={v} mode={v === tenutaVisibile ? "visible" : "hidden"}>
          <RecintoErrori dove={`pagina ${v}`}>
            {disegnaTenuta(v, v === tenutaVisibile)}
          </RecintoErrori>
        </Activity>
      ))}
      {tenutaVisibile === null && (
        <RecintoErrori dove="pagina" chiave={chiaveDiPagina(pagina)}>
          {corpoAltro()}
        </RecintoErrori>
      )}
    </>
  );

  /*
   * La notizia e l'errore della fascia in cima alla pagina.
   *
   * Una costante e non un pezzo scritto dentro `.dentro`, perché servono in due
   * posti: là, e sopra lo schermo intero — che copre la pagina e la rende
   * `inert`, cioè proprio dove un brano che non si apre falliva senza dirlo.
   * Gli stessi due stati, quindi chiuderne uno lo chiude in tutti e due i posti.
   *
   * Il guasto va anche in un terzo posto, il pannello di «Ascolta insieme»
   * (`InsiemeInApp`), che copre la pagina come lo schermo intero: per questo
   * chiuderlo è una funzione sola, la stessa per la × della fascia e per quella
   * del pannello.
   */
  const chiudiErrore = () => {
    setErrore(null);
    riproduzione.scartaErrore();
  };
  /*
   * «Il file di questo brano non si raggiunge», nato con una radice staccata,
   * restava in fascia anche dopo che la radice era tornata e «Ricontrolla» lo
   * aveva visto (R18-11). La sonda delle radici si annuncia (`radiciVive.ts`):
   * quando tutte rispondono, la fascia non descrive più niente e se ne va. Un
   * riferimento, perché l'ascolto è uno solo e il guasto cambia a ogni disegno.
   */
  const ritiraSeTornate = useRef<(vive: readonly boolean[]) => void>(() => undefined);
  ritiraSeTornate.current = (vive) => {
    if (fasciaDaRitirare(guasto?.codice, vive)) chiudiErrore();
  };
  useEffect(
    () => quandoRispondonoLeRadici((_radici, vive) => ritiraSeTornate.current(vive)),
    [],
  );
  /**
   * Il gesto che porta alla soluzione del guasto in fascia, quando la frase ne
   * nomina una.
   *
   * Le frasi del catalogo dicono «ricollega l'account nelle Impostazioni», «fai
   * una scansione», e la fascia offriva soltanto «Riprova», e solo per i guasti
   * ritentabili: la strada toccava cercarla a chi leggeva. Il codice non porta
   * parametri — non dice **quale** conto —, quindi il gesto porta alla scheda
   * dei conti, dove ognuno ha il suo «Ricollega…»; l'avviso dei conti
   * (`AvvisoConti`) invece il conto lo sa, e lo nomina.
   */
  const azioneDelGuasto = (
    codice: string | null,
  ): { etichetta: string; onClick: () => void } | undefined => {
    switch (codice) {
      case "cloud.authExpired":
      case "cloud.notConnected":
        return {
          etichetta: t("errors.action.cloudAccounts"),
          onClick: () => {
            chiudiErrore();
            apriSezione("cartelle", true);
          },
        };
      // Un file che non si apre arriva come `playback.sourceUnavailable`: la
      // scansione ritrova un file spostato, toglie quello sparito, e dice
      // quale cartella non ha risposto.
      case "fs.notFound":
      case "playback.sourceUnavailable":
        return {
          etichetta: t("settings.roots.scan"),
          onClick: () => {
            chiudiErrore();
            void scansiona();
          },
        };
      case "fs.networkUnavailable":
        return {
          etichetta: t("errors.action.folders"),
          onClick: () => {
            chiudiErrore();
            vaiA("cartelle");
          },
        };
      // Il database della libreria che non si apre, o che non si lascia
      // scrivere: le frasi parlano della cartella dei dati — la copia di prima
      // dell'aggiornamento sta lì, accanto a `rymembra.db` — e `%APPDATA%` in
      // Esplora risorse è nascosta. La fascia resta: il gesto non la chiude.
      // Sul telefono la cartella è privata dell'app, e il gesto non c'è.
      case "db.migrationFailed":
      case "db.versionAhead":
      case "db.openFailed":
      case "db.corrupt":
      case "db.readOnly":
      case "db.ioFailed":
        return MOBILE
          ? undefined
          : {
              etichetta: t("errors.action.dataFolder"),
              onClick: () => {
                ipc.manutenzioneApriCartellaDati().catch(segnalaErrore);
              },
            };
      default:
        return undefined;
    }
  };
  const notizieDiFascia = (
    <Fascia
      notizia={notizia}
      onChiudiNotizia={() => setNotizia(null)}
      avviso={avviso}
      onChiudiAvviso={() => setAvviso(null)}
      messaggio={messaggio}
      dettaglio={guasto?.dettaglio ?? null}
      onRiprova={
        ritentabile
          ? () => {
              riproduzione.scartaErrore();
              ipc.riprovaCorrente().catch(segnalaErrore);
            }
          : undefined
      }
      azione={azioneDelGuasto(guasto?.codice ?? null)}
      onChiudiErrore={chiudiErrore}
    />
  );

  /*
   * «Playlist eliminata — Annulla».
   *
   * La stessa forma dell'«Annulla» della coda, classi comprese: è lo stesso
   * gesto — rimettere quel che un clic ha appena tolto — e una skin che ha
   * vestito l'uno ha già vestito l'altro. In due posti come la fascia, e per la
   * stessa ragione. Il gettone si legge al clic e l'avviso se ne va subito: un
   * secondo clic non deve chiedere un secondo ripristino.
   *
   * Non parla lui a chi ascolta lo schermo: nasce già pieno, e in due copie.
   * Lo dice una regione sola, sempre montata, accanto a quella delle notizie.
   *
   * Il gesto lo fa `annulloPlaylist` (vedi la sua dichiarazione): dal tasto o
   * da `Ctrl+Z`, una volta sola, e l'avviso se ne va da tutte e due le copie.
   */
  rimettiPlaylist.current = (quale) => {
    ipc
      .playlistRipristina(quale.gettone)
      .then((tornata) => {
        if (tornata === null) {
          setNotizia(t("playlist.deleted.late", { nome: quale.nome }));
          return;
        }
        setNotizia(t("playlist.deleted.back", { nome: tornata.name }));
        return ricaricaPlaylist();
      })
      .catch(segnalaErrore);
  };
  /** La cancellazione, dopo il sì della domanda: vedi `daCancellare`. */
  const cancellaPlaylist = (p: Playlist) => {
    ipc
      .playlistCancella(p.id)
      .then((gettone) => {
        setPlaylistAperta((prima) => (prima && prima.id === p.id ? null : prima));
        annulloPlaylist.mostra({ gettone, nome: p.name });
        return ricaricaPlaylist();
      })
      .catch(segnalaErrore);
  };
  const avvisoCancellata = cancellata !== null && (
    <div className="notizia toast-card" {...annulloPlaylist.presa}>
      <Icona nome="i-check" dim={16} />
      <span>{t("playlist.deleted", { nome: cancellata.nome })}</span>
      <button
        type="button"
        className="bottone minuto btn-ghost"
        aria-label={t("playlist.deleted.undo.aria", { nome: cancellata.nome })}
        onClick={annulloPlaylist.annulla}
      >
        {t("playlist.deleted.undo")}
      </button>
      <button
        type="button"
        className="tasto icon-btn"
        aria-label={t("common.close")}
        onClick={() => annulloPlaylist.mostra(null)}
      >
        <Icona nome="i-x" dim={14} />
      </button>
    </div>
  );

  /*
   * «“Notte” ha preso il posto di “La mia notte” — Rimettila»: dopo una skin
   * sostituita da un file. Negli stessi due posti dell'«Annulla» della
   * playlist, e per la stessa ragione. Vedi `AvvisoSostituita`.
   */
  const sostituitaOra = skinFinestra.sostituita;
  const tornataOra = skinFinestra.tornataAPlain;
  const avvisoSostituita = (
    <>
      {sostituitaOra !== null && (
        <AvvisoSostituita
          sostituita={sostituitaOra}
          onRimetti={() => void skinFinestra.rimettiSostituita()}
          onChiudi={skinFinestra.chiudiSostituita}
        />
      )}
      {/* Maiusc tenuto all'avvio ha rimesso Plain: la stessa notizia, lo
          stesso gesto per tornare indietro. Vedi `tieniSkin.ts`. */}
      {tornataOra !== null && (
        <AvvisoMaiusc
          nome={tornataOra.nome}
          onRimetti={() => void skinFinestra.rimettiDopoMaiusc()}
          onChiudi={skinFinestra.chiudiTornataAPlain}
        />
      )}
    </>
  );

  /**
   * Le notizie sopra «In riproduzione»: vedi `avvisi` in `InRiproduzione`.
   * Una costante sola per le due forme della schermata, che non sono mai
   * montate insieme.
   */
  const avvisiGrande = (
    <>
      {/* Una seconda copia, montata solo qui: quella nella pagina è sotto,
          `inert`. Vede gli avvisi che nascono a schermo intero; quelli di
          prima erano già a schermo. */}
      <AvvisoCoda onErrore={segnalaErrore} />
      {/* Gli inviti e la sessione, per la stessa ragione: chi ascolta a
          schermo intero è proprio chi può ricevere un invito senza guardare
          altro. Qui senza contenitore aderente: la pila sta già ferma da sé. */}
      <InvitiInsieme onErrore={segnalaErrore} />
      <AvvisiInsieme onErrore={segnalaErrore} />
      {avvisoCancellata}
      {avvisoSostituita}
      {notizieDiFascia}
    </>
  );

  return (
    <Fornitori
      presenze={presenze}
      volo={inVolo}
      lettore={riproduzione.archivio}
      libreria={libreria}
      segnalaErrore={segnalaErrore}
    >
      {/* L'applicazione resta montata anche con lo Studio aperto: la
          riproduzione continua a dire dov'è, e «Prova sull'app» ha una
          finestra vera su cui provare. `inert` la toglie dal fuoco e dai clic
          finché lo Studio la copre. */}
      <div className="finestra-app" inert={studioVisibile}>
        {/* Il primo comando della finestra: «Salta al contenuto», che si vede
            solo col fuoco e porta oltre la navigazione (R16-10). Sul telefono
            no: la si percorre col dito o con TalkBack, che ha i suoi salti. */}
        {!MOBILE && <SaltaAlContenuto />}
       {/* Il menù delle righe della coda, per tutti i posti dove la coda si
           disegna: vedi `MenuDellaCoda` in `Coda.tsx`. */}
       <MenuDellaCoda value={menuDellaCoda}>
        {/* Prima di tutto il resto e sopra tutto il resto: al primo avvio non c'è
            una libreria da guardare dietro, e la schermata che chiede dove sta la
            musica è l'unica cosa che ha senso leggere. */}
        {primoVisibile && (
          <ConScansione>
            {(scansione) => (
              <Primo
                onCartelle={primoConferma}
                scritte={avvio?.cartelle ?? []}
                onScegliCartella={() => setAggiunta("primo")}
                scansione={scansione}
                brani={numeri?.tracks ?? 0}
                esito={esito}
                lasciate={lasciateAlPrimo}
                avvisi={notizieDiFascia}
                onAscolta={primoAscolta}
                onSalta={() => setPrimoChiuso(true)}
              />
            )}
          </ConScansione>
        )}
        {/* Dal primo avvio la finestrella sta in un guscio che la alza sopra di
            lui: `.primo` copre la finestra a uno strato più alto del velo delle
            finestrelle, e senza il guscio si aprirebbe dietro. Fuori dal
            primo avvio il guscio non c'è, e la finestrella resta sotto i toast
            come tutte le altre.

            Il guscio resta com'era, ed è provato dal vivo. Per tutte le altre
            finestrelle — la libreria di prima qui sotto, una skin o una playlist
            lasciate cadere sul primo avvio — non ce n'è uno per ognuna: le alza
            una regola sola, `.velo.scuro` accanto a `.primo` in
            `finestrelle.css`. */}
        {aggiunta !== null &&
          (aggiunta === "primo" ? (
            <div className="sopra-primo">
              <AggiungiCartella
                onQuestoPC={async () => {
                  const scelta = await primoScegli();
                  if (scelta === null) return false;
                  setLasciateAlPrimo((prima) => [...new Set([...prima, scelta])]);
                  return true;
                }}
                onUsa={async (conto, id, nomi) => {
                  const nuove = await aggiungiNuvola(conto, id, nomi);
                  setLasciateAlPrimo((prima) => [...new Set([...prima, ...nuove])]);
                }}
                onChiudi={() => setAggiunta(null)}
              />
            </div>
          ) : (
            <AggiungiCartella
              onQuestoPC={scegliCartella}
              onUsa={async (conto, id, nomi) => {
                await aggiungiNuvola(conto, id, nomi);
              }}
              onChiudi={() => setAggiunta(null)}
            />
          ))}
        {/* La copia del Microsoft Store, al primo avvio, propone la libreria
            che c'era: vedi `parti/LibreriaDiPrima.tsx`.

            Proprio al primo avvio serve, e proprio allora `.primo` copre la
            finestra. Qui e non dentro `.dentro.page-content`, dove stava: la
            finestrella dev'essere **fratella** di `.primo`, perché la regola di
            `.velo.scuro` in `finestrelle.css` la alza sopra di lui guardando i
            fratelli, e dalla pagina non la raggiungeva — il velo restava a
            `--strato-velo`, sotto `--strato-primo`, e un clic sulla scheda del
            primo avvio cadeva sul velo invisibile e la chiudeva (D-97). La
            pagina poi si smonta e `inert` la spegne a schermo intero: una
            proposta che si fa una volta sola non dipende da lei. Fuori dal
            primo avvio la regola non c'è, e la finestrella resta sotto i toast
            come tutte le altre. */}
        {!MOBILE && <LibreriaDiPrima onErrore={segnalaErrore} />}
        <Impaginazione
          // Sul telefono lo scafale è uno solo, per tutte le skin: vedi
          // `scafaleTelefono.ts`.
          albero={MOBILE ? SCAFALE_TELEFONO : (skinAttiva?.layout.shell ?? null)}
          contesto={contesto}
          inerte={MOBILE && grande && conBrano}
          slot={{
            // La testata cambia con la pagina, e con la pagina si anima: la
            // stessa rotta, un gruppo suo.
            intestazione: <PresenzaPagina>{testa()}</PresenzaPagina>,
            contenuto: (
              <>
                {/* A schermo intero la pagina resta montata sotto: smontarla farebbe
                    perdere la posizione di scorrimento dell'elenco, e chiudere
                    riporterebbe in cima a una libreria che si stava guardando a metà. */}
                {/* Sul telefono no: là è un foglio sopra lo scafale, montato
                    più sotto accanto a `Impaginazione`. Qui dentro il
                    contenuto ha `overflow: hidden` e `container-type`, che
                    taglierebbero un elemento `fixed` alla sua scatola. */}
                {!MOBILE && grande && conBrano ? (
                  <RecintoErrori
                    dove="in riproduzione"
                    uscita={{ etichetta: t("common.close"), fai: () => setGrande(false) }}
                  >
                    <InRiproduzioneCollegata
                      onChiudi={() => setGrande(false)}
                      onPreferito={cambiaPreferito}
                      onVoto={cambiaVoto}
                      onArtista={apriArtistaDalNome}
                      onErrore={segnalaErrore}
                      avvisi={avvisiGrande}
                    />
                  </RecintoErrori>
                ) : null}
                {/* `inert` a schermo intero: la pagina resta montata ma è
                    coperta, e un Tab che ci entrasse porterebbe il fuoco su un
                    elenco che non si vede. Vedi «il fuoco» in `InRiproduzione`. */}
                <PresenzaPagina>
                  <div
                    className="dentro page-content"
                    ref={contenuto}
                    inert={grande && conBrano}
                  >
                    {/* Prima di tutto: un database danneggiato è l'unica fascia
                        che parla di qualcosa che si può ancora perdere. Si
                        disegna da sé quando il controllo del nucleo lo dice, e
                        non se ne va: vedi `AvvisoDatabase` in `parti/Avvisi.tsx`. */}
                    <AvvisoDatabase
                      onVaiAiDati={() => {
                        apriSezione("dati", true);
                      }}
                    />
                    {/* Subito dopo: un conto cloud da ricollegare è l'altro
                        guasto che dura finché qualcuno non fa qualcosa. Vedi
                        `parti/AvvisoConti.tsx`. */}
                    <AvvisoConti onNotizia={setNotizia} />
                    {/* Qui e non solo nella terza colonna: la colonna si può
                        chiudere, e il lettore flottante sparisce quando non c'è un
                        brano — cioè proprio nei due casi in cui il dispositivo
                        manca da prima che si provasse a suonare qualcosa. Una
                        fascia che si può non vedere non è una fascia.

                        Il componente decide da sé se disegnarsi, e cosa: il rosso
                        di «non c'è audio», l'avviso che passa da sé quando il
                        suono si è spostato su un'altra uscita, o niente. */}
                    <AvvisoAudioCollegato
                      dove="fascia"
                      onErrore={segnalaErrore}
                    />
                    {/* Gli avvisi che restano in vista a elenco scorso, in un
                        contenitore aderente solo: ognuno aderente per conto suo,
                        si fermavano tutti nello stesso punto uno sopra l'altro,
                        e di tre se ne leggeva uno. */}
                    <div className="aderenti-pagina">
                      {/* «Coda sostituita — Annulla»: si disegna da sé quando il
                          nucleo manda `coda:sostituita`, e passa da sé. */}
                      <AvvisoCoda onErrore={segnalaErrore} />
                      {/* «Ascolta insieme»: gli inviti che scadono in un
                          minuto. Dopo l'«Annulla» della coda e non prima:
                          quello vale dieci secondi e riguarda un gesto appena
                          fatto, gli inviti aspettano un minuto. */}
                      <InvitiInsieme onErrore={segnalaErrore} />
                      {/* «Playlist eliminata — Annulla», gemello del primo. */}
                      {avvisoCancellata}
                      {/* «Skin sostituita — Rimettila», gemello dei due. */}
                      {avvisoSostituita}
                    </div>
                    {/* «Ascolta insieme»: l'ultimo avviso, e la sessione in
                        corso col suo «Esci», che vale tutta la sessione. Si
                        disegna da sé, e quasi sempre non c'è. */}
                    <AvvisiInsieme onErrore={segnalaErrore} />
                    {/* Le notizie, gli avvisi e i guasti della fascia non stanno
                        più nel flusso della pagina, né sul computer né sul
                        telefono: stanno nella pila dei toast (più sotto), perché
                        nel flusso spingevano giù la pagina — titolo, indice,
                        filtro — proprio mentre si stava per cliccarci (R18-17).
                        Sul telefono c'era anche di peggio: nelle pagine d'album
                        la tinta sale fin sotto la barra di stato, il contenuto
                        parte dal bordo dello schermo e la fascia, prima di tutto,
                        copriva l'orologio. La pila rispetta il riquadro di sotto
                        (`--riquadro-basso`), e non ce n'è uno di sopra da
                        rispettare. */}
                    {corpo()}
                  </div>
                </PresenzaPagina>
              </>
            ),
          }}
        />
        {/* «In riproduzione» del telefono: dopo lo scafale e fuori da lui,
            perché copre tutto, schede comprese, e prima delle finestrelle e
            del menu, che gli si aprono sopra. Vedi `InRiproduzioneTelefono`. */}
        {MOBILE && grande && conBrano && (
          <RecintoErrori
            dove="in riproduzione"
            uscita={{ etichetta: t("common.close"), fai: () => setGrande(false) }}
          >
            <InRiproduzioneTelefonoCollegata
              onChiudi={chiudiGrande}
              onPreferito={cambiaPreferito}
              onVoto={cambiaVoto}
              onMenu={menuDelBranoGrande}
              onMenuCoda={menuDellaCoda}
              onArtista={apriArtistaDalNome}
              onErrore={segnalaErrore}
              avvisi={avvisiGrande}
            />
          </RecintoErrori>
        )}

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
          <ConImportazioni>
            {(importazioni) => (
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
          </ConImportazioni>
        )}

        {importandoAccount && (
          <ConImportazioni>
            {(importazioni) => (
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
          </ConImportazioni>
        )}

        {daEliminare && (
          <ConfermaEliminazione
            brani={daEliminare.brani}
            dalDisco={daEliminare.dalDisco}
            cambiata={daEliminare.cambiata}
            // Tutte e due le uscite dallo stesso cancello: vedi `risposta`.
            onChiudi={() => {
              if (daEliminare.risposta()) setDaEliminare(null);
            }}
            onConferma={(definitivi) => {
              if (!daEliminare.risposta()) return;
              const cosa = daEliminare;
              setDaEliminare(null);
              void elimina(cosa.brani, cosa.dalDisco, definitivi);
            }}
          />
        )}

        {daCancellare && (
          <ConfermaGesto
            domanda={{
              titolo: t("playlist.delete.confirm.title", { nome: daCancellare.playlist.name }),
              descrizione: t("playlist.delete.confirm.body"),
              conferma: t("common.delete"),
              fai: () => {
                if (daCancellare.risposta()) cancellaPlaylist(daCancellare.playlist);
              },
            }}
            onChiudi={() => setDaCancellare(null)}
          />
        )}

        {confermaScarico && (
          <ConfermaGesto
            domanda={confermaScarico}
            onChiudi={() => setConfermaScarico(null)}
          />
        )}

        {gemella && (
          <ConfermaTrasloco
            gemella={gemella}
            onChiudi={() => setGemella(null)}
            onSposta={() => trasloca(gemella)}
          />
        )}

        {rimozioniInAttesa && (
          <ConfermaRimozioni
            // Una domanda nuova è un'altra finestrella, con la sua risposta:
            // quella di prima ha già usato la sua (`unaVolta`).
            key={rimozioniInAttesa.gettone}
            attesa={rimozioniInAttesa}
            onChiudi={() => {
              // «Lasciali»: le righe restano, e il nucleo dimentica l'elenco.
              // Un errore qui non ha niente da proteggere — la prossima
              // scansione lo svuota comunque — e non si mostra.
              setRimozioniInAttesa(null);
              void ipc.rimozioniScarta().catch(() => undefined);
            }}
            onTogli={() => {
              const gettone = rimozioniInAttesa.gettone;
              setRimozioniInAttesa(null);
              void (async () => {
                try {
                  const fatto = await ipc.rimozioniConferma(gettone);
                  setNotizia(t("delete.done", { brani: brani_(fatto.tolti) }));
                  // L'esito della scansione che si legge nelle Impostazioni
                  // diceva «rinviate»: adesso sono tolte.
                  libreria.esito.scrivi((prima) =>
                    prima === null
                      ? prima
                      : {
                          ...prima,
                          tolti: prima.tolti + fatto.tolti,
                          rimozioniRinviate: 0,
                          daConfermare: null,
                          numeri: fatto.numeri,
                        },
                  );
                  await ricarica();
                  await caricaVista();
                  // Il numero accanto a ogni playlist: un brano tolto esce
                  // anche dalle playlist.
                  void ricaricaPlaylist();
                } catch (e) {
                  segnalaErrore(e);
                }
              })();
            }}
          />
        )}

        {daIndossare?.esame && (
          <ConfermaSkin
            // Come per le rimozioni: un'altra skin, un'altra risposta.
            key={daIndossare.id}
            nome={daIndossare.nome}
            esame={daIndossare.esame}
            onChiudi={skinFinestra.nonOraEsaminata}
            onIndossa={skinFinestra.indossaEsaminata}
          />
        )}

        {skinFinestra.daTenere !== null && (
          <TieniSkin
            // Una skin nuova, una domanda nuova col suo conto da quindici.
            key={chiaveDellApertura(skinFinestra.daTenere)}
            daTenere={skinFinestra.daTenere}
            onTieni={skinFinestra.tieniSkin}
            onTorna={() => void skinFinestra.tornaIndietro()}
          />
        )}

        {skinFinestra.daSostituire !== null && (
          <ConfermaSostituzione
            // Un'altra domanda, anche sullo stesso file: col percorso come chiave
            // la stessa domanda rifatta in fretta riusava la finestrella col
            // cancello già consumato, e non rispondeva più (R04b-7).
            key={chiaveDellApertura(skinFinestra.daSostituire)}
            domanda={skinFinestra.daSostituire.domanda}
            onChiudi={skinFinestra.annullaSostituzione}
            onSostituisci={() => void skinFinestra.confermaSostituzione()}
          />
        )}

        {daAggiungere && (
          <AggiungiAPlaylist
            brani={daAggiungere}
            playlist={playlist}
            onChiudi={() => setDaAggiungere(null)}
            onFatto={(fatta) => {
              // La finestrella si chiude: la notizia della riuscita — e dei
              // brani che nel frattempo non c'erano più — la dice la fascia.
              setNotizia(fraseDellAggiunta(fatta));
              void ricaricaPlaylist();
            }}
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

        {!MOBILE && creandoTema && (
          <NuovoTema
            skin={skin}
            baseIniziale={baseTema}
            onChiudi={() => {
              setCreandoTema(false);
              setBaseTema(null);
            }}
            onCrea={creaTema}
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
              // decisione dell'utente e dura venti secondi. Le playlist hanno
              // il loro elenco, e la skin scelta può essere quella del backup:
              // si rilegge e si indossa, come dopo un profilo.
              void ricarica();
              void ricaricaPlaylist();
              void ricaricaSkin();
              rileggiSkin();
            }}
            // Per un ripristino che finisce dopo che la finestrella è stata
            // chiusa con Escape: l'esito arriva qui invece che a nessuno.
            onErrore={segnalaErrore}
            onNotizia={setNotizia}
          />
        )}

        {/* Una pila e non due riquadri fissi.
            Finché il toast era uno, «in basso a destra» bastava a dire dove.
            Adesso sono due e possono stare in piedi insieme — una scansione parte
            da sé quando la coda ha finito di scaricare — e due elementi fissi allo
            stesso angolo si coprono a vicenda. La pila li impila, e ognuno entra
            ed esce da sé: vedi `Presenza`. */}
        {/* Inizio e fine della scansione, a chi ascolta lo schermo: con la
            scansione vera, anche sulla pagina che nasconde il riquadro. */}
        <ConScansione>
          {(scansione) => (
            <>
              <AnnuncioScansione scansione={scansione} />
              {/* Nascosto solo in Impostazioni › Cartelle e scansione, che ha
                  la sua barra: prima spariva in tutte le Impostazioni, e chi
                  apriva «Aspetto» a scansione in corso non la vedeva da
                  nessuna parte. */}
              <PilaToast
                scansione={
                  vista !== "impostazioni" || sezione !== "cartelle" ? scansione : null
                }
                onApriScansione={() => {
                  apriSezione("cartelle", true);
                }}
                onAnnullaScansione={() => {
                  // Come «Annulla» nelle Impostazioni: la barra resta finché il
                  // nucleo non risponde, alla fine del lotto in corso.
                  ipc.annullaScansione().catch(segnalaErrore);
                }}
              >
                {/* Le fasce di notizia e d'errore, sul computer e sul telefono:
                    galleggiano qui sopra il lettore (o sopra le schede)
                    invece di entrare nel flusso della pagina, che spingevano
                    giù di una fascia (R18-17), e sul telefono coprivano la
                    barra di stato nelle pagine d'album (D7 del collaudo del
                    18/11). Una copia viva sola: non col primo avvio, che le
                    mostra da sé, né a schermo intero, che ha la sua
                    (`avvisiGrande`); con lo Studio aperto questa è sotto di
                    lui e inerte, e parla la sua. */}
                {!primoVisibile && !(grande && conBrano) && notizieDiFascia}
                {/* La coda segue chi se ne va, per la ragione scritta sopra — e a
                    maggior ragione: la scansione dura venti secondi, questa un'ora, e
                    sopravvive alla chiusura dell'applicazione. Sulla pagina stessa il
                    toast non compare: sarebbe la stessa barra due volte. */}
                {/* Il primo collegamento di un dispositivo: «Leggo la libreria di <PC>…
                    12.000/50.000». Nel frattempo la libreria si usa. */}
                <StrisciaSpecchio onErrore={segnalaErrore} />
                <StrisciaScarico onErrore={segnalaErrore} />
                <ConImportazioni>
                  {(importazioni) => (
                    <ToastImportazioni
                      stato={importazioni.stato}
                      rientro={importazioni.rientro}
                      giaLì={vista === "importazioni"}
                      onApri={() => vaiA("importazioni")}
                      onChiudiRientro={importazioni.dimenticaRientro}
                    />
                  )}
                </ConImportazioni>
                {/* L'aggiornamento: una scheda nella pila, e non una fascia nel flusso
                    della pagina, che spingeva giù il titolo, l'indice e il filtro
                    mentre si stava per cliccarci. Il componente decide da sé se
                    disegnarsi — non c'è niente da mostrare quasi sempre — e il
                    «Non ora» dura la sessione, mentre «Salta questa versione» è
                    scritto nel database. Dopo l'audio perso, che è l'ordine della
                    fretta: l'audio che non si sente è adesso, una versione nuova
                    può aspettare che si finisca di ascoltare. */}
                <AvvisoAggiornamento onErrore={segnalaErrore} />
                {/* Una volta sola, sul computer e sul telefono: la risposta la
                    ricorda il nucleo, e il componente ha le parole di tutti e
                    due. */}
                <PropostaSottofondo
                  pronta={propostaPuoMostrarsi}
                  onErrore={segnalaErrore}
                  onNotizia={setNotizia}
                />
              </PilaToast>
            </>
          )}
        </ConScansione>

        {/* Un menù per apertura, e ognuno chiude soltanto sé stesso: sul
            telefono un menù aperto mentre il precedente scendeva riusava il
            suo foglio, e a discesa finita si chiudeva senza essersi visto.
            Vedi `chiaveDellApertura` in `foglio.ts`. */}
        {menu && (
          <Menu
            key={chiaveDellApertura(menu)}
            apertura={menu}
            onChiudi={() => setMenu(chiusuraDi(menu))}
          />
        )}

        {/* Ultimo figlio, e non per ordine di importanza: il velo del giro copre
            la finestra intera, quindi deve stare **dopo** tutto quel che potrebbe
            disegnarsi sopra di lui a parità di strato — il menù contestuale, i
            toast. Lo strato vero lo dà il foglio, che è dove la scala vive; qui
            si dice soltanto che viene per ultimo. */}
        {/* Non sul telefono: i suoi quattordici passi raccontano la finestra
            del computer — la barra laterale, la terza colonna, le scorciatoie —
            e sul telefono indicherebbero posti che non ci sono. */}
        {giroAperto && !MOBILE && <Giro onPrepara={preparaGiro} onChiudi={chiudiGiro} />}
        {/* Le Novità dopo un aggiornamento, una volta: un Dialogo modale, a schermo
            intero sul telefono. Si apre da sola quando non c'è il primo avvio,
            lo Studio, il giro o un altro dialogo, e la proposta del sottofondo
            aspetta che si chiuda. */}
        <NovitaAllAvvio
          avviato={avvio !== null}
          primoVisibile={primoVisibile}
          studioVisibile={studioVisibile}
          giroAperto={giroAperto}
          onInAttesa={setNovitaInAttesa}
        />
       </MenuDellaCoda>
      </div>

      {/* Le domande che arrivano da sole, fuori da `.finestra-app` e sopra lo
          Studio (`.strato-domande`): «X vuole collegarsi» e il codice di
          «Ascolta insieme» arrivano anche mentre si disegna una skin, e
          montate là dentro — che lo Studio rende inerte — non si vedevano, e
          il loro velo congelava la finestra intera.

          `InsiemeInApp`: una volta sola, fuori dalle due copie della pagina,
          gli ascolti di `insieme:stato` e `insieme:avviso` e le finestrelle
          che ne nascono — il codice dell'associazione, la spiegazione del
          firewall, il pannello aperto dalla fascia o dalla colonna. Vedi
          `parti/Insieme`. Il guasto della fascia gli arriva per disegnarlo
          anche dentro il pannello, che copre la fascia: vedi
          `VistaInsieme.guasto`.

          `LontanoInApp`: l'ascolto di `lontano:stato` e `lontano:link`, e le
          tre finestrelle del collegamento via internet — chi chiede di
          collegarsi, il link aperto da fuori, i dati mobili. Vedi
          `parti/Dispositivi.tsx`. */}
      <div className="strato-domande">
        <InsiemeInApp onErrore={segnalaErrore} guasto={guasto} onChiudiGuasto={chiudiErrore} />
        <LontanoInApp onErrore={segnalaErrore} onNotizia={setNotizia} />
        {/* Le righe che un dispositivo non manda più, e che aspettano un sì per
            andarsene: vedi `ConfermaSpecchio.tsx`. */}
        <DomandaSpecchio
          onErrore={segnalaErrore}
          onNotizia={setNotizia}
          onRilegge={() => chiediRilettura(true)}
        />
      </div>

      {/* Le notizie della fascia e «Playlist eliminata — Annulla», a chi
          ascolta lo schermo: una regione sola per ciascuna, sempre montata, e
          non i riquadri, che nascono già pieni e in più copie (vedi
          `Annuncio.tsx`). Fuori da `.finestra-app`, che diventa `inert` mentre
          lo Studio la copre: anche lì le notizie si vedono, e vanno dette. E
          fuori dalla fascia, perché nel primo avvio la fascia sta in un
          contenitore che si nasconde quando è vuoto (`.primo-avvisi:empty`). */}
      <Annuncio testo={notizia} />
      <Annuncio testo={avviso?.testo ?? null} />
      <Annuncio
        testo={
          cancellata === null
            ? null
            : t("playlist.deleted", { nome: cancellata.nome })
        }
      />
      {/* E il brano che parte, a ogni cambio — anche quello che arriva da sé
          alla fine di un brano (D-71): «Brano successivo» cambiava il titolo
          lontano dal fuoco, e nessuno lo diceva. Vedi `annuncioBrano.ts`. */}
      <AnnuncioDelBrano />

      {/* Lo Studio occupa la finestra intera, navigazione compresa: è un'altra
          applicazione dentro la stessa finestra, e tenere la barra della
          libreria accanto a un editor di skin darebbe due navigazioni che non
          c'entrano. Sotto `<Activity>`: quando si prova la bozza sull'app lo
          Studio si nasconde senza perdere niente di quel che ha in mano. */}
      {Studio !== null && studioAperto !== null && (
        <Activity mode={studioInProva ? "hidden" : "visible"}>
          <div className="strato-studio">
            {/* Il recinto attorno al caricamento, non solo allo Studio: un
                pacchetto che non arriva cade qui come un guasto di disegno, e
                senza «Chiudi lo Studio» resterebbe una finestra coperta da una
                fascia rossa. Gli avvisi qui sotto restano fuori: devono poter
                dire anche questo. */}
            <RecintoErrori
              dove="studio"
              uscita={{ etichetta: t("crash.studio.close"), fai: esciDalloStudio }}
            >
              <Suspense
                fallback={
                  <div className="studio3 caricando" role="status">
                    {t("common.loading")}
                  </div>
                }
              >
                {/* `key`: lo Studio legge il documento una volta per montaggio,
                    e un tema nuovo o «Apri nello Studio» su un'altra skin deve
                    aprire davvero quella. Quel che aveva in mano lo salva lo
                    Studio stesso, smontandosi. */}
                <Studio
                  key={studioAperto}
                  id={studioAperto}
                  onEsci={esciDalloStudio}
                  // «Salva e usa» ridipinge la finestra intera, non solo
                  // l'anteprima dentro lo Studio: è la stessa strada di quando si
                  // sceglie una skin.
                  onInstallata={(id) => void scegliSkin(id)}
                  // La chat dello Studio, senza un modello configurato, non
                  // promette niente e porta qui: la scheda sta in una finestra che
                  // lo Studio nasconde per intero, e dire «configurane uno» senza
                  // portarci lascerebbe da cercare.
                  onModelli={() => {
                    setStudioAperto(null);
                    skinFinestra.scordaProva();
                    void ricaricaSkin();
                    vaiA("impostazioni");
                    setSezione("modelli");
                  }}
                  onProva={skinFinestra.iniziaProva}
                  onErrore={segnalaErrore}
                />
              </Suspense>
            </RecintoErrori>
            {/* Gli avvisi anche qui. Lo Studio copre la finestra, e la fascia
                della pagina resta sotto di lui: un errore di «Esporta» o di
                «Prova sull'app» finiva lì, dove nessuno lo vedeva. Sono gli
                stessi due stati di sempre, quindi chiuderlo qui lo chiude
                anche sotto. */}
            <div className="avvisi-studio">{notizieDiFascia}</div>
          </div>
        </Activity>
      )}
      {!MOBILE && prova !== null && (
        <PillolaProva
          nome={prova.skin.id}
          onTorna={skinFinestra.finisciProva}
          onTieni={() => void skinFinestra.tieniProva()}
        />
      )}
    </Fornitori>
  );
}
