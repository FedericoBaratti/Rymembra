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
import { open, save } from "@tauri-apps/plugin-dialog";
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
} from "react";

import { Account } from "./Account";
import { AggiungiAPlaylist, fraseDellAggiunta } from "./AggiungiAPlaylist";
import { ugualiInSuperficie, useArchivio } from "./archivio";
import { useArtisti } from "./artisti";
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
} from "./collegati";
import { ConfermaEliminazione } from "./ConfermaEliminazione";
import { ConfermaRimozioni } from "./ConfermaRimozioni";
import { ConfermaSkin } from "./ConfermaSkin";
import { ConfermaTrasloco } from "./ConfermaTrasloco";
import { Copertina } from "./Copertina";
import { CARTELLA_DELLA_1_0 } from "./eredita";
import { brani_, durata, nomeArtista, titoloAlbum } from "./formato";
import { Fornitori } from "./Fornitori";
import {
  Giro,
  apriIlMenuDiUnaRiga,
  unModaleAperto,
  type NomePasso,
} from "./Giro";
import { codaDaSvuotare, comeParteIlGiro } from "./guida";
import { Impaginazione, type ContestoScafale } from "./Impaginazione";
import { Importa, ImportaLink } from "./Importa";
import { ImportaPlaylist } from "./ImportaPlaylist";
import {
  ImpostazioniDellaFinestra,
  type AzioniImpostazioni,
} from "./ImpostazioniFinestra";
import {
  eRitentabile,
  guastoDa,
  ipc,
  type Guasto,
  type Album,
  type Artista,
  type Brano,
  type Cancellazione,
  type Ordine,
  type RimozioniDaConfermare,
  type Playlist,
  type VistaBrani,
} from "./ipc";
import { conteggiDi, creaLibreria } from "./libreria";
import { t } from "./lingue";
import { Trans } from "./lingue/Trans";
import { Menu, type Apertura } from "./Menu";
import {
  vociAlbum,
  vociArtista,
  vociBrani,
  vociPlaylist,
  type AzioniBrani,
} from "./menuBrani";
import { NuovoTema } from "./NuovoTema";
import { paginaCorrente } from "./pagina";
import { useAscolto, useRada } from "./pagine";
import { AnnuncioScansione } from "./parti/AnnuncioScansione";
import { AvvisoAggiornamento } from "./parti/Aggiornamenti";
import { AvvisoCoda } from "./parti/AvvisoCoda";
import { AvvisoDatabase } from "./parti/Avvisi";
import { Fascia } from "./parti/Fascia";
import { Icona } from "./parti/Icone";
import { ConImportazioni } from "./parti/Importazioni";
import { Intestazione } from "./parti/Intestazione";
import type { Vista } from "./parti/Navigazione";
import { PilaToast } from "./parti/PilaToast";
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
import { elencoPieno, type ArchivioRado, type Elenco } from "./rada";
import { Regole } from "./Regole";
import { useRicerca } from "./ricerca";
import { Ripristino } from "./Ripristino";
import { useRiproduzione } from "./riproduzione";
import { useAvanzamentoScansione } from "./scansione";
import { Artisti } from "./schermate/Artisti";
import { ElencoFinto, GrigliaFinta } from "./schermate/Elenco";
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
import type { Sezione } from "./schermate/Impostazioni";
import { GrigliaAlbum, StatoVuoto, TestaLibreria } from "./schermate/Libreria";
import { chiaveDellaSelezione } from "./selezione";
import { useSelezione } from "./selezioneVista";
import { useSkinAttiva } from "./skinAttiva";
import { useStabile } from "./stabile";
import { PillolaProva } from "./studio/PillolaProva";
import { sorgenteNuova, type DatiTema } from "./studio/nuovo";
import { campoRicerca, useScorciatoie } from "./tastiera";
import { useTerzaColonna } from "./terzaColonna";
import { useTrascinati } from "./trascinamento";
import { cambiandoVista } from "./transizione";

/**
 * Lo Studio, caricato quando si apre.
 *
 * È la parte più pesante dell'applicazione — l'editor di codice, la timeline,
 * i controlli di ogni proprietà — e chi ascolta musica non la apre quasi mai:
 * tenerla nel pacchetto principale voleva dire leggerla a ogni avvio. Da file
 * locale arriva in un attimo, e nel frattempo si vede lo Studio che carica.
 */
const Studio = lazy(() => import("./studio/Guscio").then((m) => ({ default: m.Studio })));

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

/** Nessun brano: un elenco vuoto con un'identità sola, per le dipendenze. */
const NESSUN_BRANO: Brano[] = [];

/** Nessuna scelta: la selezione delle viste nascoste, con un'identità sola. */
const NESSUNA_SCELTA: ReadonlySet<number> = new Set();


export function App() {
  const [errore, setErrore] = useStatoPresente<Guasto | null>(null);
  const segnalaErrore = useCallback((e: unknown) => setErrore(guastoDa(e)), []);
  /**
   * `avvio`, le playlist, la scansione e il suo esito: vedi `libreria.ts`, e lì
   * perché sono quattro archivi e non quattro stati di questo componente.
   * `App` si iscrive ad `avvio` e alle playlist, che decidono la pagina; della
   * scansione vuole sapere soltanto **se** ce n'è una (`scansionando`), e il
   * suo avanzamento lo leggono i componenti che lo disegnano (`ConScansione`).
   */
  const [libreria] = useState(() =>
    creaLibreria(
      { avvio: () => ipc.avvio(), playlistElenco: () => ipc.playlistElenco() },
      { onErrore: segnalaErrore, onLetta: () => setErrore(null) },
    ),
  );
  const { ricarica, ricaricaPlaylist } = libreria;
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
   */
  const [cancellata, setCancellata] = useState<{
    gettone: number;
    nome: string;
  } | null>(null);
  useEffect(() => {
    if (cancellata === null) return;
    const conto = window.setTimeout(() => setCancellata(null), 10_000);
    return () => window.clearTimeout(conto);
  }, [cancellata]);
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
   */
  const [daEliminare, setDaEliminare] = useStatoPresente<{
    brani: number[];
    dalDisco: boolean;
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

  /** `avvio` all'apertura: conteggi, cartelle, preferenze. Vedi `libreria.ts`. */
  useEffect(() => {
    void ricarica();
  }, [ricarica]);

  /**
   * La skin indossata, quelle installate, l'anteprima e la prova dello Studio:
   * vedi `skinAttiva.ts`. Qui, nel punto in cui stava la lettura della skin
   * all'apertura — subito dopo `avvio`, e prima del tema che la legge.
   */
  const skinFinestra = useSkinAttiva(segnalaErrore);
  const { skin, skinAttiva, prova, scegliSkin, ricaricaSkin } = skinFinestra;
  const { daIndossare, setDaIndossare } = skinFinestra;
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
  const radaBrani = useRada<Brano>({
    chiave: `brani:${ordine}`,
    totale: avvio?.numeri.tracks ?? 0,
    chiedi: (offset, limite) => ipc.brani(ordine, offset, limite),
    onErrore: segnalaErrore,
  });
  const radaPreferiti = useRada<Brano>({
    chiave: "preferiti",
    totale: avvio?.numeri.liked ?? 0,
    chiedi: (offset, limite) => ipc.preferiti(offset, limite),
    onErrore: segnalaErrore,
  });

  /**
   * La ricerca: la frase pigra, quanti risultati ha, e le loro righe. Vedi
   * `ricerca.ts`.
   */
  const ricerca = useRicerca(query, segnalaErrore);
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
    chiave: "album",
    totale: avvio?.numeri.albums ?? 0,
    chiedi: (offset, limite) => ipc.album(offset, limite),
    onErrore: segnalaErrore,
  });
  const radaAlbumArtista = useRada<Album>({
    chiave: `artista:${artistaAperto?.name ?? ""}`,
    totale: artistaAperto?.albums ?? 0,
    chiedi: (offset, limite) =>
      artistaAperto
        ? ipc.albumArtista(artistaAperto.name, offset, limite)
        : Promise.resolve([]),
    onErrore: segnalaErrore,
  });

  /**
   * Gli artisti della vista Artisti, tenuti finché la libreria non cambia:
   * vedi `artisti.ts`.
   */
  const { artisti, artistiInArrivo, invalidaArtisti } = useArtisti(
    vista,
    cercando,
    segnalaErrore,
  );

  /**
   * La Home: i ripiani e le raccolte del lunedì, entrando e restando. Vedi
   * `casa.ts`.
   */
  const { casa, settimana, setSettimana, rileggiCasa } = useCasa({
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
  }, [radaBrani, radaPreferiti, radaRicerca]);
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
    },
    [radaBrani, radaPreferiti, radaRicerca],
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
  const ricaricaContorno = useCallback(() => {
    ricaricaAlbum();
    // Gli artisti si rileggono subito se la vista è aperta, entrandoci se no.
    invalidaArtisti();
    // La Home dopo una scansione o dei brani scaricati: i suoi ripiani sono
    // fatti di «di recente», e quel che è appena entrato in libreria li cambia
    // tutti e quattro. Entrandoci ci pensa invece l'effetto suo, che è quel che
    // fino a poco fa non esisteva e lasciava la schermata vuota.
    if (vista === "home") rileggiCasa();
  }, [ricaricaAlbum, invalidaArtisti, vista, rileggiCasa]);

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
   * La libreria è cambiata sotto: una scansione — anche quella che parte da
   * sola, dopo i download o dalla sorveglianza delle cartelle — un trasloco,
   * un profilo importato. È il segnale che il nucleo manda da un posto solo
   * (`cartelle::cambiate`), e quello a cui le viste tenute montate devono
   * credere: i conteggi tornano da `avvio`, e con loro la lunghezza degli
   * elenchi; le righe si invalidano, e quelle che si guardano si richiedono.
   * Una vista nascosta non chiede niente finché non torna.
   *
   * Dopo una scansione a mano arriva insieme alla risposta di `scansiona`, che
   * fa le stesse due cose: due invalidazioni di fila costano una pagina
   * richiesta due volte, e saltarne una vorrebbe dire sapere quale delle due
   * strade ha portato il cambiamento.
   */
  useAscolto("cartelle:cambiate", () => {
    void ricarica();
    caricaVista();
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
    // Fuori dalla transizione: vedi «Perché `startTransition`» in
    // `transizione.ts`.
    setQuery("");
    cambiandoVista(() => {
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
    const come = comeParteIlGiro({
      daFare: giroDaFare,
      aperto: giroAperto,
      avviato: avvio !== null,
      primoVisibile,
      brani: avvio?.numeri.tracks ?? 0,
      inEsplora: vista === "esplora",
      studioVisibile,
      modaleAperto: unModaleAperto(),
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
  }, [giroDaFare, giroAperto, avvio, primoVisibile, vista, studioVisibile]);

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
    // `scansiona` è definita in questo corpo e cambia identità a ogni disegno
    // (`ricarica` no: è della libreria, `libreria.ts`): entrarci nelle
    // dipendenze farebbe rifare questa lambda a ogni disegno per niente. Chi
    // la riceve non la confronta.
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

  /**
   * Le cartelle sorvegliate: aggiungere, togliere, dove scaricare, e la
   * domanda sul trasloco. Vedi `cartelleSorvegliate.ts`.
   */
  const {
    gemella,
    setGemella,
    scegliCartella,
    dopoLeCartelle,
    trasloca,
    scegliCartellaDownload,
    togliCartella,
  } = useCartelleSorvegliate({ avvio, ricarica, setNotizia, segnalaErrore });

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
      // Una chiave sola: `.one` la sceglie il motore delle lingue, col
      // numero vero (non già formattato, o la forma non si sceglierebbe).
      setNotizia(t("playlist.exported", { n: quanti, dove: scelta }));
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
  const suonaDa = useCallback(async (elenco: Brano[], indice: number) => {
    await ipc.suona(
      elenco.map((b) => b.id),
      indice,
    );
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
   * Apre il menù contestuale su una selezione di brani.
   *
   * Quali voci, e quando, lo dice `vociBrani` in `menuBrani.ts`; qui c'è quel
   * che ciascuna fa, che tocca la coda, le finestrelle e la pagina.
   */
  const apriMenu = useCallback(
    (e: React.MouseEvent, elenco: number[]) => {
      e.preventDefault();
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
        vaiAlDisco: conBrano(async (brano) => {
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
        vaiAllArtista: conBrano(async (brano) => {
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
          setQuery("");
          cambiandoVista(() => {
            setPlaylistAperta(null);
            setAperto(null);
            setArtistaAperto(suo);
          });
        }),
        mostraNellaCartella: (id) => {
          ipc.branoMostraNellaCartella(id).catch(segnalaErrore);
        },
        apriPagina: (id) => {
          ipc.branoApriPagina(id).catch(segnalaErrore);
        },
        togli: (brani) => setDaEliminare({ brani, dalDisco: false }),
        elimina: (brani) => setDaEliminare({ brani, dalDisco: true }),
      };
      setMenu({
        x: e.clientX,
        y: e.clientY,
        voci: vociBrani(
          elenco,
          (id) => elencoCorrenteRef.current.cerca((b) => b.id === id),
          fai,
          t,
        ),
      });
    },
    [segnalaErrore, artisti],
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
          cancella: () => {
            ipc
              .playlistCancella(p.id)
              .then((gettone) => {
                setPlaylistAperta((prima) =>
                  prima && prima.id === p.id ? null : prima,
                );
                setCancellata({ gettone, nome: p.name });
                return ricaricaPlaylist();
              })
              .catch(segnalaErrore);
          },
          esporta: () => void esportaPlaylist(p),
        },
        t,
      );
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
        voci: vociAlbum(
          {
            radio: conBrani((ids) => {
              const primo = ids[0];
              return primo === undefined ? Promise.resolve() : ipc.radio(primo);
            }),
            dopo: conBrani(ipc.codaDopo),
            accoda: conBrani(ipc.codaAccoda),
          },
          t,
        ),
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
      : pagina.tipo === "libreria" && pagina.vista === "brani"
        ? radaBrani
        : pagina.tipo === "libreria" && pagina.vista === "preferiti"
          ? radaPreferiti
          : null;
  const tipoVista = radaCorrente === null ? null : pagina.tipo === "ricerca" ? "ricerca" : vista;
  const vistaBraniCorrente: VistaBrani | null = useMemo(
    () =>
      tipoVista === "ricerca"
        ? { tipo: "ricerca", query: queryPigra }
        : tipoVista === "brani"
          ? { tipo: "brani", ordine }
          : tipoVista === "preferiti"
            ? { tipo: "preferiti" }
            : null,
    [tipoVista, queryPigra, ordine],
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
    chiave: chiaveDellaSelezione(pagina, vistaTenuta, ordine, query),
    tenuta: vistaTenuta !== null,
    idDellaVista: ipc.vistaBraniId,
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
   */
  const elimina = useCallback(
    async (brani: number[], dalDisco: boolean, definitivi = false) => {
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
          setNotizia(
            definitivi
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
      aggiornaBrani,
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
  const suonaQui = useCallback(
    (indice: number) => {
      if (radaCorrente === null || vistaBraniCorrente === null) {
        void suonaDa(braniPieni, indice).catch(segnalaErrore);
        return;
      }
      const premuto = radaCorrente.riga(indice);
      ipc
        .vistaBraniId(vistaBraniCorrente)
        .then((ids) => {
          const dove =
            premuto !== undefined && ids[indice] !== premuto.id
              ? ids.indexOf(premuto.id)
              : indice;
          return ids.length > 0 ? ipc.suona(ids, Math.max(dove, 0)) : undefined;
        })
        .catch(segnalaErrore);
    },
    [suonaDa, braniPieni, radaCorrente, vistaBraniCorrente, segnalaErrore],
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
    }),
    (piano) => {
      if (piano.tipo === "skin") {
        // `vaiA` e non `setVista`: quello nudo lasciava in piedi l'album, la
        // playlist e la ricerca di prima, cioè uno stato in cui l'intestazione
        // e il corpo rispondono a due domande diverse.
        vaiA("impostazioni");
        setSezione("aspetto");
        ipc
          .skinInstalla(piano.percorso)
          .then(skinFinestra.dopoInstallazione)
          .catch(segnalaErrore);
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
        vaiA("impostazioni");
        setSezione("cartelle");
        const unite = [...new Set([...avvio.cartelle, ...cartelle.percorsi])];
        ipc
          .impostaCartelle(unite)
          .then(async (esito) => {
            await ricarica();
            dopoLeCartelle(esito);
          })
          .catch(segnalaErrore);
        return;
      }
      if (piano.notizia === "fuoriLibreria") setNotizia(t("drop.notInLibrary"));
      else if (piano.notizia === "niente") setNotizia(t("drop.nothing"));
    },
    segnalaErrore,
  );

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
   * Solo quello che viene dal lettore: `errore` è già una stringa — di lui non
   * resta il codice, e senza codice non si sa se riprovare ha senso — mentre
   * `riproduzione.errore` arriva intero dal nucleo. È anche l'unico dei due che
   * abbia un gesto da offrire: la cartella di rete che sparisce a metà brano.
   */
  const ritentabile = errore === null && eRitentabile(riproduzione.errore);



  // Con la stessa identità finché i numeri sono gli stessi: sta nelle
  // dipendenze di `contesto`, e un `avvio` riletto dopo ogni gesto — con gli
  // stessi conteggi — ne faceva uno nuovo, e con lui una navigazione
  // ridisegnata per niente. Vedi `conteggiDi` in `libreria.ts`.
  const conteggi = useArchivio(libreria.avvio, conteggiDi, ugualiInSuperficie);

  const etichettaOrdine =
    ordinamenti().find(([c]) => c === ordine)?.[1] ?? t("sort.shelf");

  /**
   * I gesti delle Impostazioni che toccano questo componente: vedi
   * `AzioniImpostazioni` in `ImpostazioniFinestra.tsx`. Con `useStabile` dove
   * la funzione legge lo stato — le cartelle di `avvio`, il conto delle
   * scansioni in volo — perché l'oggetto resti lo stesso e il `memo` di là
   * serva a qualcosa.
   */
  const aggiungiCartella = useStabile(() => void scegliCartella());
  const togliCartellaStabile = useStabile((c: string) => void togliCartella(c));
  const cartellaDownload = useStabile(() => void scegliCartellaDownload());
  const scansionaStabile = useStabile(() => void scansiona());
  const importaDatabase = useStabile(() => void scegliDatabase());
  const azioniImpostazioni = useMemo<AzioniImpostazioni>(
    () => ({
      onGiro: rifaiGiro,
      onProfiloImportato: dopoProfilo,
      onAggiungiCartella: aggiungiCartella,
      onTogliCartella: togliCartellaStabile,
      onScegliCartellaDownload: cartellaDownload,
      onScansiona: scansionaStabile,
      onCreaTema: () => {
        setBaseTema(null);
        setCreandoTema(true);
      },
      onDeriva: (id) => {
        setBaseTema(id);
        setCreandoTema(true);
      },
      onApriStudio: setStudioAperto,
      onImporta: importaDatabase,
      onImportaLink: () => setImportandoLink(true),
      onImportaAccount: () => setImportandoAccount(true),
      onVista: vaiA,
      onNuvolaRipristina: () => setRipristinando(true),
    }),
    [
      rifaiGiro,
      dopoProfilo,
      aggiungiCartella,
      togliCartellaStabile,
      cartellaDownload,
      scansionaStabile,
      importaDatabase,
      vaiA,
      setCreandoTema,
      setImportandoLink,
      setImportandoAccount,
      setRipristinando,
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
          setVista("brani");
        }
        setPlaylistAperta(p);
      });
    },
    [vista],
  );
  const nuovaPlaylist = useCallback(() => setCreandoPlaylist(true), [setCreandoPlaylist]);
  const nuovaSmart = useCallback(() => setRegoleAperte({ playlist: null }), [setRegoleAperte]);
  const importaFile = useStabile(() => void scegliFilePlaylist());
  // Fuori, nel browser di sistema, e per nome: l'indirizzo lo conosce solo
  // il nucleo, che ne tiene un elenco chiuso — vedi `apri_documento`.
  const dona = useCallback(() => {
    ipc.apriDocumento("donazioni").catch(segnalaErrore);
  }, [segnalaErrore]);
  const apriGrande = useCallback(() => setGrande(true), []);
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
    ],
  );

  /**
   * L'intestazione, che cambia con quel che si sta guardando.
   *
   * Quale pagina sia lo dice `pagina`, e lo dice una volta sola per
   * l'intestazione e per il corpo: vedi `pagina.ts`.
   */
  const testa = () => {
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
              onClick={() => void suonaDa(braniPlaylist, 0).catch(segnalaErrore)}
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
                onClick={() => void suonaDa(braniAperto, 0).catch(segnalaErrore)}
              >
                <Icona nome="i-play" dim={14} />
                {t("action.play")}
              </button>
            </>
          }
        />
      );
    }
    if (pagina.tipo === "artista") {
      return (
        <Intestazione
          occhiello={t("page.artist")}
          titolo={nomeArtista(pagina.artista.name)}
          sottotitolo={t("page.artist.sub", {
            brani: brani_(pagina.artista.tracks),
            album: t("artists.albums", { n: pagina.artista.albums }),
          })}
          query={query}
          onQuery={setQuery}
          azioni={
            <button
              type="button"
              className="pillola btn-ghost back-button"
              onClick={() => apriArtista(null)}
            >
              <Icona nome="i-chev-l" dim={14} />
              {t("nav.artists")}
            </button>
          }
        />
      );
    }
    if (pagina.tipo === "home") {
      return <TestaHome query={query} onQuery={setQuery} />;
    }
    return (
      <TestaLibreria
        vista={pagina.vista}
        numeri={numeri}
        cartelle={avvio?.cartelle.length ?? 0}
        query={query}
        onQuery={setQuery}
        ordinamento={{ etichetta: etichettaOrdine, onApri: menuOrdine }}
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
          onSezione={setSezione}
          onNotizia={setNotizia}
          onErrore={segnalaErrore}
          aspetto={aspetto}
          skin={skinFinestra}
          azioni={azioniImpostazioni}
        />
      );
    }

    if (senzaCartelle && !messaggio) {
      return (
        <StatoVuoto
          icona="i-folder"
          titolo={t("empty.noFolders.title")}
          testo={t("empty.noFolders.body")}
          azione={{ etichetta: t("empty.noFolders.cta"), fai: () => void scegliCartella() }}
        />
      );
    }

    if (vuota && !scansionando && !cercando) {
      return (
        <StatoVuoto
          icona="i-scan"
          titolo={t("empty.library.title")}
          testo={t("empty.library.body")}
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
            onMenu={menuSuSelezione}
            onSeleziona={seleziona}
            onAlternaSelezione={alternaSelezione}
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
          onSelezionaTutto={selezionaTutto}
          /* L'unico posto in cui `#` è il numero del disco: qui la colonna dice
             dove sta il pezzo sulla custodia. */
          numeroTraccia
        />
      );
    }

    if (pagina.tipo === "artista") {
      // `radaAlbumArtista` **è** già l'elenco dell'artista: il nucleo lo filtra
      // con la stessa chiave con cui raggruppa gli artisti. Prima si filtrava
      // qui la pagina già scaricata, ed era il motivo per cui un artista oltre il
      // quattrocentesimo album dava una griglia vuota.
      return (
        <GrigliaAlbum
          key={`artista:${pagina.artista.name}`}
          rada={radaAlbumArtista}
          sotto="anno"
          onApri={apriAlbum}
          onMenu={menuAlbum}
          scorrevole={contenuto}
        />
      );
    }

    if (pagina.tipo === "home") {
      return (
        <Home
          casa={casa}
          settimana={settimana}
          onSuona={(elenco, indice) => void suonaDa(elenco, indice).catch(segnalaErrore)}
          onApriRaccolta={(raccolta) => {
            void (async () => {
              try {
                await suonaDa(raccolta.brani, 0);
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
          onMenu={(e, brano) => menuSuSelezione(e, [brano.id])}
          onApriAlbum={apriAlbum}
          onMenuAlbum={(e, album) => menuAlbum(e, album.albumKey)}
        />
      );
    }

    // Le Cartelle non sono una lista rada e non restano montate: l'albero se lo
    // chiede da sé, un livello alla volta, e uno scheletro di righe di brani
    // sopra un albero sarebbe l'attesa di qualcos'altro.
    if (vista === "cartelle") {
      return (
        <CartelleCollegate
          radici={avvio?.cartelle ?? []}
          onMenu={setMenu}
          onErrore={segnalaErrore}
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
        return (
          <ElencoBraniCollegato
            rada={v === "brani" ? radaBrani : radaPreferiti}
            /* Un altro ordinamento è un altro elenco: in cima. */
            chiave={v === "brani" ? ordine : undefined}
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
            onSelezionaTutto={selezionaTutto}
          />
        );
      case "album":
        return (
          <GrigliaAlbum
            rada={radaAlbum}
            sotto="artista"
            onApri={apriAlbum}
            onMenu={menuAlbum}
            scorrevole={contenuto}
            memoria
          />
        );
      case "artisti":
        // I segnaposto solo la prima volta: una rilettura degli artisti tiene a
        // schermo quelli di prima finché non arrivano i nuovi.
        if (artisti.length === 0 && artistiInArrivo) return <GrigliaFinta />;
        return (
          <Artisti
            artisti={artisti}
            scorrevole={contenuto}
            memoria
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
  const corpo = () => (
    <>
      {VISTE_TENUTE.filter((v) => visitate.includes(v)).map((v) => (
        <Activity key={v} mode={v === tenutaVisibile ? "visible" : "hidden"}>
          {disegnaTenuta(v, v === tenutaVisibile)}
        </Activity>
      ))}
      {tenutaVisibile === null && corpoAltro()}
    </>
  );

  /*
   * La notizia e l'errore della fascia in cima alla pagina.
   *
   * Una costante e non un pezzo scritto dentro `.dentro`, perché servono in due
   * posti: là, e sopra lo schermo intero — che copre la pagina e la rende
   * `inert`, cioè proprio dove un brano che non si apre falliva senza dirlo.
   * Gli stessi due stati, quindi chiuderne uno lo chiude in tutti e due i posti.
   */
  const notizieDiFascia = (
    <Fascia
      notizia={notizia}
      onChiudiNotizia={() => setNotizia(null)}
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
      onChiudiErrore={() => {
        setErrore(null);
        riproduzione.scartaErrore();
      }}
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
   */
  const avvisoCancellata = cancellata !== null && (
    <div className="notizia avviso-coda toast-card" role="status">
      <Icona nome="i-check" dim={16} />
      <span>{t("playlist.deleted", { nome: cancellata.nome })}</span>
      <button
        type="button"
        className="bottone minuto btn-ghost"
        aria-label={t("playlist.deleted.undo.aria", { nome: cancellata.nome })}
        onClick={() => {
          const quale = cancellata;
          setCancellata(null);
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
        }}
      >
        {t("playlist.deleted.undo")}
      </button>
      <button
        type="button"
        className="tasto icon-btn"
        aria-label={t("common.close")}
        onClick={() => setCancellata(null)}
      >
        <Icona nome="i-x" dim={14} />
      </button>
    </div>
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
        {/* Prima di tutto il resto e sopra tutto il resto: al primo avvio non c'è
            una libreria da guardare dietro, e la schermata che chiede dove sta la
            musica è l'unica cosa che ha senso leggere. */}
        {primoVisibile && (
          <ConScansione>
            {(scansione) => (
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
          </ConScansione>
        )}
        <Impaginazione
          albero={skinAttiva?.layout.shell ?? null}
          contesto={contesto}
          slot={{
            // La testata cambia con la pagina, e con la pagina si anima: la
            // stessa rotta, un gruppo suo.
            intestazione: <PresenzaPagina>{testa()}</PresenzaPagina>,
            contenuto: (
              <>
                {/* A schermo intero la pagina resta montata sotto: smontarla farebbe
                    perdere la posizione di scorrimento dell'elenco, e chiudere
                    riporterebbe in cima a una libreria che si stava guardando a metà. */}
                {grande && conBrano ? (
                  <InRiproduzioneCollegata
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
                        {avvisoCancellata}
                        {notizieDiFascia}
                      </>
                    }
                  />
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
                        vaiA("impostazioni");
                        setSezione("dati");
                      }}
                    />
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
                    {/* «Playlist eliminata — Annulla», gemello di quello sopra. */}
                    {avvisoCancellata}
                    {/* Col primo avvio aperto le notizie le mostra lui: qui
                        sarebbero sotto la sua schermata, e due copie della stessa
                        fascia d'errore si annuncerebbero due volte. */}
                    {!primoVisibile && notizieDiFascia}
                    {corpo()}
                  </div>
                </PresenzaPagina>
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
            onChiudi={() => setDaEliminare(null)}
            onConferma={(definitivi) => {
              const cosa = daEliminare;
              setDaEliminare(null);
              void elimina(cosa.brani, cosa.dalDisco, definitivi);
            }}
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
            nome={daIndossare.nome}
            esame={daIndossare.esame}
            onChiudi={() => setDaIndossare(null)}
            onIndossa={() => {
              const quale = daIndossare;
              setDaIndossare(null);
              void scegliSkin(quale.id);
            }}
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
              // decisione dell'utente e dura venti secondi. Le playlist hanno
              // il loro elenco, e la skin scelta può essere quella del backup:
              // si rilegge e si indossa, come dopo un profilo.
              void ricarica();
              void ricaricaPlaylist();
              void ricaricaSkin();
              rileggiSkin();
            }}
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
              <PilaToast
                scansione={vista !== "impostazioni" ? scansione : null}
                onApriScansione={() => {
                  vaiA("impostazioni");
                  setSezione("cartelle");
                }}
              >
                {/* La coda segue chi se ne va, per la ragione scritta sopra — e a
                    maggior ragione: la scansione dura venti secondi, questa un'ora, e
                    sopravvive alla chiusura dell'applicazione. Sulla pagina stessa il
                    toast non compare: sarebbe la stessa barra due volte. */}
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
              </PilaToast>
            </>
          )}
        </ConScansione>

        {menu && <Menu apertura={menu} onChiudi={() => setMenu(null)} />}

        {/* Ultimo figlio, e non per ordine di importanza: il velo del giro copre
            la finestra intera, quindi deve stare **dopo** tutto quel che potrebbe
            disegnarsi sopra di lui a parità di strato — il menù contestuale, i
            toast. Lo strato vero lo dà il foglio, che è dove la scala vive; qui
            si dice soltanto che viene per ultimo. */}
        {giroAperto && <Giro onPrepara={preparaGiro} onChiudi={chiudiGiro} />}
      </div>

      {/* Lo Studio occupa la finestra intera, navigazione compresa: è un'altra
          applicazione dentro la stessa finestra, e tenere la barra della
          libreria accanto a un editor di skin darebbe due navigazioni che non
          c'entrano. Sotto `<Activity>`: quando si prova la bozza sull'app lo
          Studio si nasconde senza perdere niente di quel che ha in mano. */}
      {studioAperto !== null && (
        <Activity mode={studioInProva ? "hidden" : "visible"}>
          <div className="strato-studio">
            <Suspense
              fallback={
                <div className="studio3 caricando" role="status">
                  {t("common.loading")}
                </div>
              }
            >
              <Studio
                id={studioAperto}
                onEsci={() => {
                  setStudioAperto(null);
                  skinFinestra.scordaProva();
                  // Le skin possono essere cambiate mentre si era là dentro.
                  void ricaricaSkin();
                }}
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
            {/* Gli avvisi anche qui. Lo Studio copre la finestra, e la fascia
                della pagina resta sotto di lui: un errore di «Esporta» o di
                «Prova sull'app» finiva lì, dove nessuno lo vedeva. Sono gli
                stessi due stati di sempre, quindi chiuderlo qui lo chiude
                anche sotto. */}
            <div className="avvisi-studio">{notizieDiFascia}</div>
          </div>
        </Activity>
      )}
      {prova !== null && (
        <PillolaProva
          nome={prova.skin.id}
          onTorna={skinFinestra.finisciProva}
          onTieni={() => void skinFinestra.tieniProva()}
        />
      )}
    </Fornitori>
  );
}
