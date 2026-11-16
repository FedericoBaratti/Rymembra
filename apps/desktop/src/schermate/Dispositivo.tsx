/**
 * «Sfoglia <dispositivo>»: la musica di un altro dispositivo collegato via
 * internet.
 *
 * # Una pagina a parte, non la libreria
 *
 * I brani dell'altro non entrano in questa libreria: non si contano, non si
 * mettono nelle playlist, non si votano. Si guardano, si ascoltano, e il
 * proprietario conta l'ascolto (`lontano/sfoglia.rs`). Per questo la pagina
 * ha i suoi elenchi, più semplici di quelli della libreria.
 *
 * # Com'è fatta: i pezzi di questa libreria, con la musica dell'altro
 *
 * Chi sfoglia il PC dal telefono deve riconoscere quel che vede, e la pagina
 * si veste con le parti che l'app ha già:
 *
 * - **In evidenza** apre la pagina come la Home apre l'app: i brani che
 *   l'altro ascolta di più e i dischi entrati da poco, in ripiani (`casa`),
 *   ognuno col suo «Mostra tutti» verso l'elenco intero. Sono due domande
 *   piccole al posto dei duecento album di prima, ed è il primo colpo
 *   d'occhio, spesso sui dati mobili.
 * - **Album**, **Artisti** e **Brani** sono le griglie e l'elenco della
 *   libreria: le stesse schede, il mosaico degli artisti, i brani nei quattro
 *   ordini della libreria.
 * - Un disco o un artista aperto ha la testata delle pagine di un disco
 *   (`TestataDettaglio`): la copertina grande, «Riproduci» e «Casuale». Sul
 *   computer la stessa testata sta di fianco invece che in colonna.
 * - Una riga è la copertina, il titolo, l'artista e la durata. Il tocco la
 *   suona (sul computer il doppio clic, o la copertina); il resto — dopo, in
 *   coda, il suo disco, «Scarica qui» — sta nel menù, sul ⋮ o col destro, come
 *   in libreria. Erano quattro tasti per riga: su un telefono, quattro
 *   bersagli piccoli stretti accanto al titolo da leggere.
 *
 * Le schede già aperte restano montate, nascoste, con le loro pagine e il
 * punto dov'erano: tornare su «In evidenza» non rifà le domande. Lo stesso per
 * i livelli: sotto un disco aperto resta l'elenco da cui lo si è aperto.
 *
 * # Gli elenchi arrivano a pagine
 *
 * Ogni elenco è una domanda all'altro dispositivo, a pagine da `PAGINA` voci,
 * con «Mostra altri» in fondo. Niente scorrimento infinito: ogni pagina è
 * traffico dati, magari mobile, e una domanda in più deve essere un gesto.
 *
 * # I gesti sui brani
 *
 * Riproduci, Casuale, Riproduci dopo, Accoda e «Scarica qui» passano da
 * `gestoLontano`, che sui dati mobili con «Chiedi» mette il gesto da parte e fa
 * comparire la domanda (`LontanoInApp`). Riproduci suona l'elenco intero da
 * quel brano, come in libreria. Su un disco o un artista dal menù della
 * scheda i brani si chiedono al momento, dentro lo stesso gesto: rifatto dopo
 * un sì, li richiede.
 *
 * «Scarica qui» copia i file nella cartella dei download di questo
 * dispositivo (`lontano/scarico.rs`), uno alla volta; la striscia in cima alla
 * pagina dice a che punto è. I brani di un'immagine CD non scendono, e la
 * voce lo dice.
 */
import {
  createContext,
  memo,
  useCallback,
  useContext,
  useEffect,
  useId,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type ReactNode,
} from "react";

import { useArchivio } from "../archivio";
import { Copertina } from "../Copertina";
import { brani_, dataOraBreve, durata, nomeArtista, numero, titoloAlbum } from "../formato";
import { suIndietro } from "../indietro";
import {
  ipc,
  testoErrore,
  type Album,
  type Artista,
  type BranoRemoto,
  type OrdineRemoto,
} from "../ipc";
import { t } from "../lingue";
import {
  aGruppi,
  albumDaiBrani,
  albumDi,
  archivioLontano,
  archivioScarico,
  copertineDegliAlbum,
  durataTotale,
  eTelefono,
  gestoLontano,
  passoIndietroSfoglia,
  percentualeScarico,
  sceltaDelDispositivo,
} from "../lontano";
import type { Apertura, TestataMenu, Voce } from "../Menu";
import { mescolato } from "../mescolato";
import { ConAltro } from "../parti/ConAltro";
import { Icona } from "../parti/Icone";
import { Segmentato } from "../parti/Segmentato";
import { TestataDettaglio } from "../parti/TestataDettaglio";
import { MOBILE } from "../piattaforma";
import { scorrevoleDi } from "../riordino";
import { conIndice } from "../segnali";
import { useStabile } from "../stabile";
import { StatoVuoto } from "./Libreria";

/** Quante voci per domanda: il protocollo ne ammette fino a 500. */
const PAGINA = 200;

/** Quanto si aspetta dopo l'ultima lettera prima di chiedere. */
const ATTESA_RICERCA_MS = 300;

/** Le schede di un ripiano di «In evidenza»: dodici, come la Home. */
const RIPIANO = 12;

/**
 * Quanti brani aggiunti di recente si chiedono per trovare i dischi del
 * ripiano: un disco nuovo è di solito una decina di brani, e sessanta ne
 * danno abbastanza senza chiedere una pagina intera.
 */
const RECENTI = 60;

/** Quante domande dei brani di un artista sono in volo insieme: vedi `aGruppi`. */
const IN_VOLO = 4;

type SchedaSfoglia = "evidenza" | "album" | "artisti" | "brani";

/**
 * Gli ordini dei brani, come li dice la libreria (`App.tsx`, `ordinamenti`):
 * gli stessi nomi, che il nucleo dell'altro capisce (`servizio.rs`,
 * `ordine_da`). Una funzione perché porta testo.
 */
function ordiniBrani(): readonly (readonly [OrdineRemoto, string])[] {
  return [
    ["scaffale", t("sort.shelf")],
    ["recenti", t("sort.recent")],
    ["ascoltati", t("sort.played")],
    ["titolo", t("sort.title")],
  ];
}

/** Un elenco dell'altro dispositivo, e a che punto è. */
interface Pagine<T> {
  voci: readonly T[];
  /** L'ultima pagina era corta: non ce ne sono altre. */
  finito: boolean;
  caricando: boolean;
  errore: unknown;
}

const NIENTE: Pagine<never> = { voci: [], finito: false, caricando: false, errore: null };

/**
 * Un elenco a pagine. `chi` dice **che cosa** si sta chiedendo: quando cambia
 * l'elenco riparte da capo, e una risposta arrivata tardi per la domanda di
 * prima si butta. `null` non chiede niente.
 *
 * Con `aPagine` falso la domanda risponde tutto in una volta (i brani di un
 * album, gli album di un artista, un ripiano): l'elenco è finito alla prima
 * risposta.
 */
function usePagine<T>(
  chi: string | null,
  carica: (da: number, quanti: number) => Promise<T[]>,
  aPagine = true,
): Pagine<T> & { altre: () => void } {
  const [stato, setStato] = useState<Pagine<T>>(NIENTE);
  const ultima = useRef(carica);
  ultima.current = carica;
  const giro = useRef(0);

  const prendi = useCallback(
    (prima: readonly T[]) => {
      giro.current += 1;
      const mio = giro.current;
      setStato({ voci: prima, finito: false, caricando: true, errore: null });
      ultima.current(prima.length, PAGINA).then(
        (nuove) => {
          if (giro.current !== mio) return;
          setStato({
            voci: [...prima, ...nuove],
            finito: !aPagine || nuove.length < PAGINA,
            caricando: false,
            errore: null,
          });
        },
        (errore: unknown) => {
          if (giro.current !== mio) return;
          setStato({ voci: prima, finito: false, caricando: false, errore });
        },
      );
    },
    [aPagine],
  );

  useEffect(() => {
    if (chi === null) {
      giro.current += 1;
      setStato(NIENTE);
      return;
    }
    prendi([]);
  }, [chi, prendi]);

  return { ...stato, altre: () => prendi(stato.voci) };
}

/** Finito, senza errori e senza niente dentro. */
function vuoto(pagine: Pagine<unknown>): boolean {
  return pagine.finito && pagine.errore === null && pagine.voci.length === 0;
}

/** «Mostra altri», il «Caricamento…» e l'errore con «Riprova», in fondo a un elenco. */
function PiedeElenco<T>({ pagine }: { pagine: Pagine<T> & { altre: () => void } }) {
  if (pagine.caricando) {
    return (
      <p className="nota" role="status">
        {t("common.loading")}
      </p>
    );
  }
  if (pagine.errore !== null) {
    return (
      <div className="azioni">
        <p className="nota" role="alert">
          {testoErrore(pagine.errore)}
        </p>
        <button type="button" className="bottone btn-ghost" onClick={pagine.altre}>
          {t("devices.browse.retry")}
        </button>
      </div>
    );
  }
  if (pagine.finito) return null;
  return (
    <div className="azioni">
      <button type="button" className="bottone btn-ghost" onClick={pagine.altre}>
        {t("devices.browse.more")}
      </button>
    </div>
  );
}

/**
 * Quel che serve a ogni parte della pagina: il dispositivo, dove dire gli
 * errori, il menù di `App`, e i due livelli che si aprono da ovunque.
 */
interface Sfoglia {
  chiave: string;
  onErrore: (errore: unknown) => void;
  onMenu: (apertura: Apertura) => void;
  apriAlbum: (album: Album) => void;
  apriArtista: (artista: Artista) => void;
}

const ContestoSfoglia = createContext<Sfoglia | null>(null);

function useSfoglia(): Sfoglia {
  const sfoglia = useContext(ContestoSfoglia);
  if (sfoglia === null) throw new Error("una parte di «Sfoglia» fuori dalla pagina");
  return sfoglia;
}

/** Dei brani da dove si sa prenderli: già in mano, o da chiedere. */
type DaDove = () => Promise<readonly BranoRemoto[]>;

const inMano =
  (brani: readonly BranoRemoto[]): DaDove =>
  () =>
    Promise.resolve(brani);

/** I brani di dei dischi dell'altro, disco dopo disco: vedi `aGruppi`. */
async function braniDei(chiave: string, dischi: readonly Album[]): Promise<BranoRemoto[]> {
  return (await aGruppi(dischi, IN_VOLO, (d) => ipc.lontanoBraniAlbum(chiave, d.albumKey))).flat();
}

/** I gesti sui brani dell'altro: prima li si prende, poi il gesto, tutto in uno. */
function gesti({ chiave, onErrore }: Sfoglia) {
  const con = (prendi: DaDove, poi: (brani: BranoRemoto[]) => Promise<void>) =>
    void gestoLontano(async () => {
      const brani = [...(await prendi())];
      if (brani.length > 0) await poi(brani);
    }, onErrore);
  return {
    suona: (prendi: DaDove, indice = 0) =>
      con(prendi, (brani) => ipc.lontanoSuona(chiave, brani, indice)),
    casuale: (prendi: DaDove) =>
      con(prendi, (brani) => ipc.lontanoSuona(chiave, mescolato(brani), 0)),
    dopo: (prendi: DaDove) => con(prendi, (brani) => ipc.lontanoDopo(chiave, brani)),
    accoda: (prendi: DaDove) => con(prendi, (brani) => ipc.lontanoAccoda(chiave, brani)),
    // Tutti, anche quelli di un'immagine CD: il nucleo li salta e li conta.
    scarica: (prendi: DaDove) =>
      con(prendi, async (brani) => {
        archivioScarico.scrivi(await ipc.lontanoScarica(chiave, brani));
      }),
  };
}

/** Il menù, accanto al ⋮ che l'ha aperto o dove si è premuto il destro. */
function apriMenu(
  e: React.MouseEvent,
  sfoglia: Sfoglia,
  testata: TestataMenu,
  voci: Voce[],
): void {
  e.preventDefault();
  // Sul telefono il tocco sulla riga la suona: il ⋮ è suo e non della riga.
  e.stopPropagation();
  const destro = e.type === "contextmenu";
  const riquadro = e.currentTarget.getBoundingClientRect();
  sfoglia.onMenu({
    x: destro ? e.clientX : riquadro.left,
    y: destro ? e.clientY : riquadro.bottom + 4,
    testata,
    voci,
  });
}

/** Le voci del menù di un brano. `nelDisco`: si è già nel suo disco, e «Vai all'album» non serve. */
function vociBrano(
  sfoglia: Sfoglia,
  brano: BranoRemoto,
  elenco: readonly BranoRemoto[],
  indice: number,
  nelDisco: boolean,
): Voce[] {
  const fai = gesti(sfoglia);
  const disco = nelDisco ? null : albumDi(brano);
  return [
    { etichetta: t("action.play"), icona: "i-play", azione: () => fai.suona(inMano(elenco), indice) },
    { etichetta: t("action.playNext"), icona: "i-dopo", azione: () => fai.dopo(inMano([brano])) },
    { etichetta: t("action.enqueue"), icona: "i-queue", azione: () => fai.accoda(inMano([brano])) },
    ...(disco === null
      ? []
      : [
          {
            etichetta: t("menu.goToAlbum"),
            icona: "i-album" as const,
            azione: () => sfoglia.apriAlbum(disco),
          },
        ]),
    {
      etichetta: t("devices.download"),
      icona: "i-import",
      azione: () => fai.scarica(inMano([brano])),
      spenta: brano.segmento === null ? undefined : t("devices.download.cue"),
    },
  ];
}

/** Le voci del menù di un disco o di un artista: i brani si chiedono al momento. */
function vociInsieme(sfoglia: Sfoglia, prendi: DaDove): Voce[] {
  const fai = gesti(sfoglia);
  return [
    { etichetta: t("action.play"), icona: "i-play", azione: () => fai.suona(prendi) },
    { etichetta: t("action.shuffle"), icona: "i-shuffle", azione: () => fai.casuale(prendi) },
    { etichetta: t("action.playNext"), icona: "i-dopo", azione: () => fai.dopo(prendi) },
    { etichetta: t("action.enqueue"), icona: "i-queue", azione: () => fai.accoda(prendi) },
    { etichetta: t("devices.download"), icona: "i-import", azione: () => fai.scarica(prendi) },
  ];
}

/** La striscia di «Scarica qui», finché scende qualcosa. */
function StrisciaScarico({ onErrore }: { onErrore: (errore: unknown) => void }) {
  const stato = useArchivio(archivioScarico);
  if (stato === null || stato.titolo === null) return null;
  return (
    <div className="azioni" role="status">
      <p className="nota">
        {t("devices.download.running", {
          titolo: stato.titolo,
          percento: percentualeScarico(stato),
        })}
        {stato.inCoda > 0 ? ` · ${t("devices.download.queued", { n: stato.inCoda })}` : ""}
      </p>
      <button
        type="button"
        className="bottone minuto btn-ghost"
        onClick={() => {
          ipc
            .lontanoScaricoAnnulla()
            .then((adesso) => archivioScarico.scrivi(adesso))
            .catch(onErrore);
        }}
      >
        {t("common.cancel")}
      </button>
    </div>
  );
}

/**
 * Un brano dell'altro in un elenco.
 *
 * Il primo comando è la copertina (dentro un disco il numero), che lo suona:
 * è quel che si raggiunge col tabulatore. Sul telefono suona anche il tocco
 * sulla riga, come in libreria; sul computer il doppio clic, perché il clic
 * da solo in un elenco si aspetta di scegliere, non di far partire.
 */
const RigaBrano = memo(function RigaBrano({
  brano,
  indice,
  elenco,
  nelDisco,
  conAscolti,
}: {
  brano: BranoRemoto;
  indice: number;
  /**
   * L'elenco intero, che Riproduci suona da qui: un riferimento e non l'array,
   * che è nuovo a ogni pagina e ridisegnerebbe tutte le righe (R13-11).
   */
  elenco: { readonly current: readonly BranoRemoto[] };
  /** Dentro il suo disco: il numero al posto della copertina, e niente nome del disco. */
  nelDisco: boolean;
  /**
   * I brani in ordine di ascolti: quanti, sotto il titolo al posto del disco.
   * In una colonna sua, sul telefono, stringeva il titolo a metà.
   */
  conAscolti: boolean;
}) {
  const sfoglia = useSfoglia();
  const suona = () => gesti(sfoglia).suona(inMano(elenco.current), indice);
  const menu = (e: React.MouseEvent) =>
    apriMenu(
      e,
      sfoglia,
      {
        titolo: brano.title,
        sottotitolo: `${nomeArtista(brano.artist)} · ${titoloAlbum(brano.album)}`,
        copertina: brano.coverArtHash,
      },
      vociBrano(sfoglia, brano, elenco.current, indice, nelDisco),
    );
  const accanto =
    conAscolti && brano.playCount > 0
      ? t("devices.browse.plays", { n: brano.playCount })
      : !nelDisco && brano.album !== ""
        ? titoloAlbum(brano.album)
        : null;
  const sotto =
    accanto === null ? nomeArtista(brano.artist) : `${nomeArtista(brano.artist)} · ${accanto}`;
  // Il numero che si vede dentro il suo disco, e che il nome del tasto deve
  // contenere (WCAG 2.5.3): come nell'elenco dei brani.
  const numero = brano.trackNumber ?? indice + 1;
  return (
    <li
      className="riga-lontana"
      onClick={MOBILE ? suona : undefined}
      onDoubleClick={
        MOBILE
          ? undefined
          : (e) => {
              // Sulla riga, non sui suoi comandi: `stopPropagation` sul `click`
              // non ferma il `dblclick`, e un doppio clic sul ⋮ apriva il menù
              // e sostituiva la coda con l'elenco dell'altro (R13-8). Come in
              // `Elenco.tsx`.
              if ((e.target as Element).closest("button")) return;
              suona();
            }
      }
      onContextMenu={menu}
    >
      <button
        type="button"
        className="avvia-lontana"
        data-numero={nelDisco || undefined}
        aria-label={
          nelDisco
            ? t("list.playNumbered", { numero: String(numero), titolo: brano.title })
            : t("list.play", { titolo: brano.title })
        }
        onClick={(e) => {
          e.stopPropagation();
          // Il secondo clic di un doppio clic è già stato servito dal primo:
          // la copertina suona da sé, e senza questo partiva due volte (con la
          // tastiera `detail` è 0 e il tasto suona sempre).
          if (e.detail > 1) return;
          suona();
        }}
      >
        {nelDisco ? (
          <span className="numero-lontana">{numero}</span>
        ) : (
          <Copertina
            hash={brano.coverArtHash}
            titolo={titoloAlbum(brano.album)}
            classe="miniatura"
            accanto
          />
        )}
        <span className="via-lontana" aria-hidden="true">
          <Icona nome="i-play" dim={14} />
        </span>
      </button>
      <span className="testi-lontana">
        <span className="nome-lontana" title={brano.title} dir="auto">
          {brano.title}
        </span>
        <span className="sotto-lontana" title={sotto} dir="auto">
          {sotto}
        </span>
      </span>
      {/* Il cuore del proprietario: da guardare, non da toccare — qui non si
          vota la libreria di un altro. */}
      {brano.liked && (
        <span
          className="cuore-lontana"
          role="img"
          aria-label={t("devices.browse.liked")}
          title={t("devices.browse.liked")}
        >
          <Icona nome="i-heart-f" dim={13} />
        </span>
      )}
      {brano.durationMs > 0 && <span className="durata-lontana">{durata(brano.durationMs)}</span>}
      <button
        type="button"
        className="altro-lontana"
        aria-label={t("row.more.aria", { titolo: brano.title })}
        onClick={menu}
      >
        <Icona nome="i-dots-v" dim={MOBILE ? 20 : 16} />
      </button>
    </li>
  );
});

/** I brani dell'altro, una riga ciascuno. */
function ElencoBrani({
  brani,
  nelDisco = false,
  conAscolti = false,
}: {
  brani: readonly BranoRemoto[];
  nelDisco?: boolean;
  conAscolti?: boolean;
}) {
  // L'elenco intero scende alle righe come riferimento: l'array è nuovo a ogni
  // pagina, e passato com'è ridisegnerebbe tutte le righe a ogni «Mostra altri»
  // (R13-11). Lo si legge al gesto, e allora è quello di quel momento.
  const dellElenco = useRef(brani);
  dellElenco.current = brani;
  if (brani.length === 0) return null;
  return (
    <ul className="righe-lontane">
      {brani.map((brano, indice) => (
        <RigaBrano
          key={`${brano.id}-${indice}`}
          brano={brano}
          indice={indice}
          elenco={dellElenco}
          nelDisco={nelDisco}
          conAscolti={conAscolti}
        />
      ))}
    </ul>
  );
}

/** Un brano in un ripiano: la scheda della Home (`Home.tsx`, `Scheda`). */
function SchedaBrano({
  brano,
  indice,
  ripiano,
}: {
  brano: BranoRemoto;
  indice: number;
  ripiano: readonly BranoRemoto[];
}) {
  const sfoglia = useSfoglia();
  const menu = (e: React.MouseEvent) =>
    apriMenu(
      e,
      sfoglia,
      {
        titolo: brano.title,
        sottotitolo: `${nomeArtista(brano.artist)} · ${titoloAlbum(brano.album)}`,
        copertina: brano.coverArtHash,
      },
      vociBrano(sfoglia, brano, ripiano, indice, false),
    );
  return (
    <ConAltro etichetta={t("row.more.aria", { titolo: brano.title })} onMenu={menu}>
      <button
        type="button"
        className="scheda-brano track-card"
        style={conIndice(indice)}
        onClick={() => gesti(sfoglia).suona(inMano(ripiano), indice)}
        onContextMenu={menu}
        title={`${brano.title} — ${nomeArtista(brano.artist)}`}
      >
        <Copertina
          hash={brano.coverArtHash}
          titolo={titoloAlbum(brano.album)}
          classe="copertina card-cover"
          accanto
        />
        <span className="titolo card-title" dir="auto">
          {brano.title}
        </span>
        <span className="artista card-subtitle" dir="auto">
          {nomeArtista(brano.artist)}
        </span>
      </button>
    </ConAltro>
  );
}

/**
 * Un disco dell'altro: la scheda della griglia degli album. Il clic lo apre, il
 * menù lo suona o lo scarica senza aprirlo.
 */
function SchedaAlbum({
  album,
  indice,
  conAnno,
}: {
  album: Album;
  indice: number;
  /** Nella pagina di un artista l'artista è già scritto in cima: sotto, l'anno. */
  conAnno: boolean;
}) {
  const sfoglia = useSfoglia();
  const titolo = titoloAlbum(album.title);
  const menu = (e: React.MouseEvent) =>
    apriMenu(
      e,
      sfoglia,
      { titolo, sottotitolo: nomeArtista(album.artist), copertina: album.coverArtHash },
      vociInsieme(sfoglia, () => ipc.lontanoBraniAlbum(sfoglia.chiave, album.albumKey)),
    );
  const conteggio = album.totalTracks > 1 ? ` · ${numero(album.totalTracks)}` : "";
  return (
    <ConAltro etichetta={t("row.more.aria", { titolo })} onMenu={menu}>
      <button
        type="button"
        className="scheda card"
        style={conIndice(indice)}
        onClick={() => sfoglia.apriAlbum(album)}
        onContextMenu={menu}
        title={`${titolo} — ${nomeArtista(album.artist)}`}
      >
        <Copertina
          hash={album.coverArtHash}
          titolo={titolo}
          classe="copertina card-cover"
          misura="scheda"
          accanto
        />
        <div className="titolo card-title" dir="auto">
          {titolo}
        </div>
        <div className="sotto card-subtitle">
          {conAnno ? (album.year ?? "") : <bdi>{nomeArtista(album.artist)}</bdi>}
          {conteggio}
        </div>
      </button>
    </ConAltro>
  );
}

/** La griglia dei dischi dell'altro. Le copertine le porta `lontano/sfoglia.rs`. */
function GrigliaAlbum({ album, conAnno = false }: { album: readonly Album[]; conAnno?: boolean }) {
  if (album.length === 0) return null;
  return (
    <div className="griglia track-grid">
      {album.map((a, i) => (
        <SchedaAlbum key={a.albumKey} album={a} indice={i} conAnno={conAnno} />
      ))}
    </div>
  );
}

/** Un artista dell'altro: il mosaico dei suoi dischi, come in libreria (`Artisti.tsx`). */
function SchedaArtista({ artista, indice }: { artista: Artista; indice: number }) {
  const sfoglia = useSfoglia();
  const nome = nomeArtista(artista.name);
  const copertine = artista.covers.slice(0, 4);
  const menu = (e: React.MouseEvent) =>
    apriMenu(
      e,
      sfoglia,
      { titolo: nome, sottotitolo: brani_(artista.tracks), copertina: copertine[0] ?? null },
      vociInsieme(sfoglia, async () =>
        braniDei(sfoglia.chiave, await ipc.lontanoAlbumArtista(sfoglia.chiave, artista.name)),
      ),
    );
  return (
    <ConAltro etichetta={t("row.more.aria", { titolo: nome })} onMenu={menu}>
      <button
        type="button"
        className="scheda-artista artist-card"
        style={conIndice(indice)}
        onClick={() => sfoglia.apriArtista(artista)}
        onContextMenu={menu}
      >
        <span className="mosaico artist-mosaic" data-quante={copertine.length} aria-hidden="true">
          {copertine.length === 0 ? (
            <span className="senza">
              <Icona nome="i-artist" dim={22} />
            </span>
          ) : (
            copertine.map((hash) => (
              <Copertina
                key={hash}
                hash={hash}
                titolo={nome}
                classe="tassello"
                misura="scheda"
                accanto
              />
            ))
          )}
        </span>
        <span className="nome" title={nome} dir="auto">
          {nome}
        </span>
        <span className="quanti">
          {t("artists.counts", {
            album: t("artists.albums", { n: artista.albums }),
            brani: brani_(artista.tracks),
          })}
        </span>
      </button>
    </ConAltro>
  );
}

/** La testata di un ripiano: il nome, e «Mostra tutti» verso l'elenco intero. */
function TestaRipiano({ titolo, onTutti }: { titolo: string; onTutti: () => void }) {
  const id = useId();
  return (
    <div className="testa-ripiano">
      <h2 className="casa-titolo home-shelf-title" id={id}>
        {titolo}
      </h2>
      <button type="button" className="tutti-ripiano" aria-describedby={id} onClick={onTutti}>
        {t("devices.browse.all")}
        <Icona nome="i-chev-r" dim={14} />
      </button>
    </div>
  );
}

/**
 * «In evidenza»: quel che l'altro ascolta di più, e i dischi entrati da poco.
 *
 * Un ripiano vuoto non si disegna, come nella Home: una libreria in cui non si
 * è ascoltato niente non ha «più ascoltati», e una fila vuota sotto un titolo
 * direbbe che manca qualcosa.
 */
function InEvidenza({
  onTutti,
}: {
  onTutti: (scheda: SchedaSfoglia, ordine: OrdineRemoto) => void;
}) {
  const { chiave } = useSfoglia();
  const ascoltati = usePagine(
    `${chiave}:ascoltati`,
    () => ipc.lontanoBrani(chiave, "ascoltati", 0, RIPIANO),
    false,
  );
  const recenti = usePagine(
    `${chiave}:recenti`,
    () => ipc.lontanoBrani(chiave, "recenti", 0, RECENTI),
    false,
  );
  // I più ascoltati sono i primi per ascolti: in una libreria nuova, i primi
  // per titolo con zero ascolti. Quelli no.
  const piu = ascoltati.voci.filter((b) => b.playCount > 0);
  const dischi = albumDaiBrani(recenti.voci, RIPIANO);
  // Le due domande partono insieme: un «Caricamento…» solo, e sotto ogni
  // ripiano il suo errore col suo «Riprova».
  const caricando = ascoltati.caricando || recenti.caricando;

  if (vuoto(recenti)) {
    return (
      <StatoVuoto
        icona="i-album"
        titolo={t("devices.browse.empty")}
        testo={t("devices.browse.empty.sub")}
      />
    );
  }
  return (
    <div className="casa">
      {piu.length > 0 && (
        <section className="casa-ripiano home-shelf">
          <TestaRipiano
            titolo={t("devices.browse.mostPlayed")}
            onTutti={() => onTutti("brani", "ascoltati")}
          />
          <div className="casa-schede track-grid">
            {piu.map((b, i) => (
              <SchedaBrano key={b.id} brano={b} indice={i} ripiano={piu} />
            ))}
          </div>
        </section>
      )}
      {ascoltati.errore !== null && <PiedeElenco pagine={ascoltati} />}
      {dischi.length > 0 && (
        <section className="casa-ripiano home-shelf">
          <TestaRipiano titolo={t("home.added")} onTutti={() => onTutti("brani", "recenti")} />
          <div className="casa-schede track-grid">
            {dischi.map((a, i) => (
              <SchedaAlbum key={a.albumKey} album={a} indice={i} conAnno={false} />
            ))}
          </div>
        </section>
      )}
      {recenti.errore !== null && <PiedeElenco pagine={recenti} />}
      {caricando && (
        <p className="nota" role="status">
          {t("common.loading")}
        </p>
      )}
    </div>
  );
}

/** Tutti i dischi dell'altro, a pagine. */
function TuttiGliAlbum() {
  const { chiave } = useSfoglia();
  const album = usePagine(`${chiave}:album`, (da, quanti) => ipc.lontanoAlbum(chiave, da, quanti));
  return (
    <>
      {vuoto(album) ? (
        <StatoVuoto
          icona="i-album"
          titolo={t("devices.browse.empty")}
          testo={t("devices.browse.empty.sub")}
        />
      ) : (
        <GrigliaAlbum album={album.voci} />
      )}
      <PiedeElenco pagine={album} />
    </>
  );
}

/** Tutti gli artisti dell'altro, a pagine. */
function TuttiGliArtisti() {
  const { chiave } = useSfoglia();
  const artisti = usePagine(`${chiave}:artisti`, (da, quanti) =>
    ipc.lontanoArtisti(chiave, da, quanti),
  );
  return (
    <>
      {vuoto(artisti) ? (
        <StatoVuoto
          icona="i-artist"
          titolo={t("devices.browse.empty")}
          testo={t("devices.browse.empty.sub")}
        />
      ) : (
        artisti.voci.length > 0 && (
          <div className="griglia-artisti track-grid">
            {artisti.voci.map((a, i) => (
              <SchedaArtista key={a.name} artista={a} indice={i} />
            ))}
          </div>
        )
      )}
      <PiedeElenco pagine={artisti} />
    </>
  );
}

/** Tutti i brani dell'altro, in uno dei quattro ordini, a pagine. */
function TuttiIBrani({
  ordine,
  onOrdine,
}: {
  ordine: OrdineRemoto;
  onOrdine: (ordine: OrdineRemoto) => void;
}) {
  const sfoglia = useSfoglia();
  const { chiave } = sfoglia;
  const brani = usePagine(`${chiave}:brani:${ordine}`, (da, quanti) =>
    ipc.lontanoBrani(chiave, ordine, da, quanti),
  );
  const ordini = ordiniBrani();
  const etichetta = ordini.find(([o]) => o === ordine)?.[1] ?? t("sort.shelf");
  return (
    <>
      <div className="azioni">
        <button
          type="button"
          className="pillola btn-ghost"
          aria-label={t("header.sort", { nome: etichetta })}
          onClick={(e) =>
            apriMenu(
              e,
              sfoglia,
              { titolo: t("sort.heading") },
              ordini.map(([o, nome]) => ({
                etichetta: nome,
                azione: () => onOrdine(o),
                scelta: o === ordine,
              })),
            )
          }
        >
          <Icona nome="i-sort" dim={15} />
          <span>{etichetta}</span>
          <Icona nome="i-chev-d" dim={13} />
        </button>
      </div>
      {vuoto(brani) ? (
        <StatoVuoto
          icona="i-track"
          titolo={t("devices.browse.empty")}
          testo={t("devices.browse.empty.sub")}
        />
      ) : (
        <ElencoBrani brani={brani.voci} conAscolti={ordine === "ascoltati"} />
      )}
      <PiedeElenco pagine={brani} />
    </>
  );
}

/**
 * Sul computer, la freccia che chiude un livello. Sul telefono non c'è: lo fa
 * la freccia della barra, che passa dallo stesso Indietro di Android
 * (`indietro.ts`).
 */
function Torna({ onChiudi }: { onChiudi: () => void }) {
  if (MOBILE) return null;
  return (
    <div className="azioni">
      <button type="button" className="bottone minuto btn-ghost" onClick={onChiudi}>
        <Icona nome="i-chev-l" dim={14} />
        {t("devices.browse.back")}
      </button>
    </div>
  );
}

/** Un disco aperto: la testata grande, e i suoi brani col loro numero. */
function AlbumAperto({ album, onChiudi }: { album: Album; onChiudi: () => void }) {
  const sfoglia = useSfoglia();
  const { chiave } = sfoglia;
  const brani = usePagine(
    `${chiave}:album:${album.albumKey}`,
    () => ipc.lontanoBraniAlbum(chiave, album.albumKey),
    false,
  );
  const tutti = brani.voci;
  const scaricabili = tutti.filter((b) => b.segmento === null);
  const fai = gesti(sfoglia);
  // L'anno dal disco o dal suo primo brano: aperto da un ripiano, il disco sa
  // solo quel che sapeva il brano da cui viene.
  const anno = album.year ?? tutti[0]?.year ?? null;
  const sottotitolo = [
    nomeArtista(album.artist),
    anno === null ? null : String(anno),
    tutti.length > 0 ? brani_(tutti.length) : null,
    tutti.length > 0 ? durata(durataTotale(tutti)) : null,
  ]
    .filter((pezzo) => pezzo !== null)
    .join(" · ");
  return (
    <>
      <Torna onChiudi={onChiudi} />
      <TestataDettaglio
        conTitolo
        occhiello={t("page.album")}
        titolo={titoloAlbum(album.title)}
        sottotitolo={sottotitolo}
        copertine={[album.coverArtHash]}
        spento={tutti.length === 0}
        onRiproduci={() => fai.suona(inMano(tutti))}
        onCasuale={() => fai.casuale(inMano(tutti))}
        sotto={
          <div className="azioni">
            <button
              type="button"
              className="bottone minuto btn-ghost"
              disabled={tutti.length === 0}
              onClick={() => fai.accoda(inMano(tutti))}
            >
              <Icona nome="i-queue" dim={14} />
              {t("devices.browse.queueAll")}
            </button>
            <button
              type="button"
              className="bottone minuto btn-ghost"
              disabled={scaricabili.length === 0}
              title={
                tutti.length > 0 && scaricabili.length === 0 ? t("devices.download.cue") : undefined
              }
              onClick={() => fai.scarica(inMano(scaricabili))}
            >
              <Icona nome="i-import" dim={14} />
              {t("devices.download.album")}
            </button>
          </div>
        }
      />
      <ElencoBrani brani={tutti} nelDisco />
      <PiedeElenco pagine={brani} />
    </>
  );
}

/** Un artista aperto: la testata col mosaico, e i suoi dischi. */
function ArtistaAperto({ artista, onChiudi }: { artista: Artista; onChiudi: () => void }) {
  const sfoglia = useSfoglia();
  const { chiave } = sfoglia;
  const album = usePagine(
    `${chiave}:artista:${artista.name}`,
    () => ipc.lontanoAlbumArtista(chiave, artista.name),
    false,
  );
  const fai = gesti(sfoglia);
  const tutti = () => braniDei(chiave, album.voci);
  const copertine = album.voci.length > 0 ? copertineDegliAlbum(album.voci) : artista.covers;
  return (
    <>
      <Torna onChiudi={onChiudi} />
      <TestataDettaglio
        conTitolo
        rotonda
        occhiello={t("page.artist")}
        titolo={nomeArtista(artista.name)}
        sottotitolo={`${t("artists.albums", { n: artista.albums })} · ${brani_(artista.tracks)}`}
        copertine={copertine}
        spento={album.voci.length === 0}
        onRiproduci={() => fai.suona(tutti)}
        onCasuale={() => fai.casuale(tutti)}
        sotto={
          <div className="azioni">
            <button
              type="button"
              className="bottone minuto btn-ghost"
              disabled={album.voci.length === 0}
              onClick={() => fai.accoda(tutti)}
            >
              <Icona nome="i-queue" dim={14} />
              {t("devices.browse.queueAll")}
            </button>
            <button
              type="button"
              className="bottone minuto btn-ghost"
              disabled={album.voci.length === 0}
              onClick={() => fai.scarica(tutti)}
            >
              <Icona nome="i-import" dim={14} />
              {t("devices.download")}
            </button>
          </div>
        }
      />
      <GrigliaAlbum album={album.voci} conAnno />
      <PiedeElenco pagine={album} />
    </>
  );
}

/** I risultati di una ricerca fra i brani dell'altro. */
function Risultati({ testo }: { testo: string }) {
  const { chiave } = useSfoglia();
  const brani = usePagine(`${chiave}:cerca:${testo}`, (da, quanti) =>
    ipc.lontanoCerca(chiave, testo, da, quanti),
  );
  // «Caricamento…» finché il conteggio non risponde; se la domanda fallisce
  // (un dispositivo raggiunto via relay, una risposta che scade) non si scrive
  // niente: il conteggio è un di più, l'elenco sotto ha la sua riga d'errore, e
  // un «Caricamento…» che non finisce mai — annunciato dal lettore di schermo —
  // direbbe il falso (R13-7).
  const [quanti, setQuanti] = useState<number | "attesa" | "mancante">("attesa");
  useEffect(() => {
    let vivo = true;
    setQuanti("attesa");
    ipc
      .lontanoCercaConteggio(chiave, testo)
      .then((n) => {
        if (vivo) setQuanti(n);
      })
      .catch(() => {
        if (vivo) setQuanti("mancante");
      });
    return () => {
      vivo = false;
    };
  }, [chiave, testo]);
  const nessuno = !brani.caricando && brani.errore === null && brani.voci.length === 0;
  return (
    <>
      {/* Montata sempre, e cambia solo il testo: una regione di stato che nasce
          già piena spesso non viene annunciata. */}
      <p className="nota" role="status">
        {quanti === "attesa"
          ? t("common.loading")
          : quanti === "mancante"
            ? ""
            : t("page.search.results", { n: quanti })}
      </p>
      {nessuno ? (
        <StatoVuoto
          icona="i-search"
          titolo={t("devices.browse.noResults")}
          testo={t("devices.browse.noResults.sub")}
        />
      ) : (
        <ElencoBrani brani={brani.voci} />
      )}
      <PiedeElenco pagine={brani} />
    </>
  );
}

/** Una scheda o un livello: montato quando serve, nascosto quando è coperto. */
function Piano({ nascosto, children }: { nascosto: boolean; children: ReactNode }) {
  return (
    <div className="piano-sfoglia" hidden={nascosto}>
      {children}
    </div>
  );
}

/** Il nome e lo stato di un dispositivo, per le testate di `App`. */
export function useDispositivo(chiave: string | null) {
  // Il dispositivo per chiave, non l'archivio intero: `App` lo chiama anche con
  // nessun dispositivo aperto, e a ogni `lontano:stato` si ridisegnava per
  // niente (R13-11). La scelta si rifà con la chiave.
  const scegli = useMemo(() => sceltaDelDispositivo(chiave), [chiave]);
  const leggi = useCallback(() => scegli(archivioLontano.leggi()), [scegli]);
  return useSyncExternalStore(archivioLontano.iscrivi, leggi, leggi);
}

/** La riga sotto il nome: telefono o computer, se risponde, e se dal relay. */
export function sottotitoloDispositivo(
  dispositivo: {
    piattaforma: string;
    online: boolean;
    vistoMs: number | null;
    via: "diretto" | "relay" | null;
  } | null,
): string | undefined {
  if (dispositivo === null) return undefined;
  const tipo = t(eTelefono(dispositivo.piattaforma) ? "devices.kind.phone" : "devices.kind.computer");
  const stato = dispositivo.online
    ? t("devices.state.online")
    : dispositivo.vistoMs !== null
      ? t("devices.state.seen", { quando: dataOraBreve(dispositivo.vistoMs) })
      : t("devices.state.never");
  const via = dispositivo.online && dispositivo.via === "relay" ? ` · ${t("devices.via.relay")}` : "";
  return `${tipo} · ${stato}${via}`;
}

/**
 * La pagina. `chiave` è il dispositivo: chi la monta le dà `key={chiave}`, e
 * passare a un altro dispositivo la rifà da capo.
 */
export function Dispositivo({
  chiave,
  onErrore,
  onImpostazioni,
  onMenu,
}: {
  chiave: string;
  onErrore: (errore: unknown) => void;
  /** Porta alla scheda «Dispositivi» delle Impostazioni. */
  onImpostazioni: () => void;
  /** Apre il menù di `App`: il foglio dal basso sul telefono. */
  onMenu: (apertura: Apertura) => void;
}) {
  const dispositivo = useDispositivo(chiave);
  const [scheda, setScheda] = useState<SchedaSfoglia>("evidenza");
  // Le schede già aperte: restano montate, con le loro pagine.
  const [aperte, setAperte] = useState<ReadonlySet<SchedaSfoglia>>(() => new Set(["evidenza"]));
  const [ordine, setOrdine] = useState<OrdineRemoto>("titolo");
  const [testo, setTesto] = useState("");
  const [cercato, setCercato] = useState("");
  const [album, setAlbum] = useState<Album | null>(null);
  const [artista, setArtista] = useState<Artista | null>(null);
  const idCerca = useId();
  const online = dispositivo?.online === true;

  // Un dispositivo che non risponde si riprova adesso, aperta la sua pagina:
  // un PC acceso da poco non deve aspettare la pausa lunga del nodo.
  useEffect(() => {
    if (!online) ipc.lontanoCercaOra().catch(() => undefined);
  }, [online]);

  // Dove si era. Aprire un disco o un artista non smonta quel che c'è sotto
  // (resta nascosto, con le sue pagine), e tornando si ritrova il punto: una
  // pila, perché i livelli sono due, artista e disco. Le schede hanno ognuna
  // il suo punto.
  const radice = useRef<HTMLDivElement>(null);
  const posizioni = useRef<number[]>([]);
  const puntiDelleSchede = useRef(new Map<SchedaSfoglia, number>());
  const daRimettere = useRef<number | null>(null);
  const qui = () => scorrevoleDi(radice.current)?.scrollTop ?? 0;
  useLayoutEffect(() => {
    const dove = daRimettere.current;
    if (dove === null) return;
    daRimettere.current = null;
    const scorre = scorrevoleDi(radice.current);
    if (scorre !== null) scorre.scrollTop = dove;
  }, [album, artista, scheda, ordine]);

  const apriLivello = (apri: () => void) => {
    posizioni.current.push(qui());
    daRimettere.current = 0;
    apri();
  };
  const chiudiLivello = (chiudi: () => void) => {
    daRimettere.current = posizioni.current.pop() ?? 0;
    chiudi();
  };
  // Con un'identità sola: stanno nel valore del contesto, e un valore nuovo a
  // ogni disegno ridisegnerebbe ogni riga che lo legge (R13-11).
  const apriAlbum = useStabile((scelto: Album) => apriLivello(() => setAlbum(scelto)));
  const apriArtista = useStabile((scelto: Artista) => apriLivello(() => setArtista(scelto)));
  const sfoglia = useMemo<Sfoglia>(
    () => ({ chiave, onErrore, onMenu, apriAlbum, apriArtista }),
    [chiave, onErrore, onMenu, apriAlbum, apriArtista],
  );
  const chiudiAlbum = () => chiudiLivello(() => setAlbum(null));
  const chiudiArtista = () => chiudiLivello(() => setArtista(null));
  /** Fuori da ogni livello e dalla ricerca, sulla scheda di sotto dov'era. */
  const chiudiTutto = () => {
    const sotto = posizioni.current[0];
    if (sotto !== undefined) daRimettere.current = sotto;
    posizioni.current = [];
    setAlbum(null);
    setArtista(null);
    setTesto("");
    setCercato("");
  };
  /** Un'altra scheda (con `nuovo`, un altro ordine dei brani). */
  const scegli = (nuova: SchedaSfoglia, nuovo?: OrdineRemoto) => {
    // Il punto della scheda di adesso è quello sotto i livelli, se ce ne sono.
    puntiDelleSchede.current.set(scheda, posizioni.current[0] ?? qui());
    chiudiTutto();
    const altroOrdine = nuovo !== undefined && nuovo !== ordine;
    daRimettere.current = altroOrdine ? 0 : (puntiDelleSchede.current.get(nuova) ?? 0);
    if (nuovo !== undefined) setOrdine(nuovo);
    setScheda(nuova);
    setAperte((prima) => (prima.has(nuova) ? prima : new Set([...prima, nuova])));
  };
  const cambiaOrdine = (nuovo: OrdineRemoto) => {
    daRimettere.current = 0;
    setOrdine(nuovo);
  };

  // La domanda parte quando si smette di scrivere: ogni lettera è traffico.
  useEffect(() => {
    const pulito = testo.trim();
    const attesa = window.setTimeout(() => setCercato(pulito), ATTESA_RICERCA_MS);
    return () => window.clearTimeout(attesa);
  }, [testo]);

  // Indietro chiude prima il disco, poi l'artista, poi la ricerca; sul
  // telefono poi torna su «In evidenza», e solo dopo esce dalla pagina
  // (`passoIndietroTelefono`). Sul computer è Escape (e `Alt+←`, e il tasto
  // indietro del mouse) a fare lo stesso, come nella libreria — ma senza
  // cambiare scheda (R13-5). Con il dispositivo che non risponde la pagina è lo
  // stato vuoto, i livelli non si vedono, e il gesto non li chiude (R13-6):
  // `passoIndietroSfoglia` sceglie da quel che si vede.
  const passo = passoIndietroSfoglia({
    online,
    album: album !== null,
    artista: artista !== null,
    ricerca: testo !== "",
    altraScheda: scheda !== "evidenza",
    sulComputer: !MOBILE,
  });
  useEffect(() => {
    if (passo === null) return;
    return suIndietro(
      () => {
        if (passo === "album") chiudiAlbum();
        else if (passo === "artista") chiudiArtista();
        else if (passo === "ricerca") chiudiTutto();
        else scegli("evidenza");
        return true;
      },
      // Anche per Escape, sul computer: gli altri gestori della pila (la coda,
      // la terza colonna) sono del solo telefono, qui no.
      { sulComputer: true },
    );
    // Le funzioni qui sopra cambiano a ogni disegno, ma leggono solo ref,
    // setter stabili e lo stato elencato: rinnovare il gesto per loro non serve.
  }, [passo, album, artista, testo, scheda]);

  if (dispositivo === null || dispositivo.revocato) {
    return (
      <StatoVuoto
        icona="i-server"
        titolo={t("devices.browse.gone")}
        testo={t("devices.browse.gone.sub")}
        azione={{ etichetta: t("devices.browse.settings"), fai: onImpostazioni }}
      />
    );
  }

  if (!dispositivo.online) {
    return (
      <StatoVuoto
        icona={eTelefono(dispositivo.piattaforma) ? "i-telefono" : "i-computer"}
        titolo={t("devices.browse.offline", { nome: dispositivo.nome })}
        testo={t(
          eTelefono(dispositivo.piattaforma)
            ? "devices.browse.offline.sub.phone"
            : "devices.browse.offline.sub.computer",
        )}
        nota={
          dispositivo.vistoMs !== null
            ? t("devices.state.seen", { quando: dataOraBreve(dispositivo.vistoMs) })
            : undefined
        }
        azione={{
          etichetta: t("devices.browse.retryNow"),
          fai: () => {
            ipc.lontanoCercaOra().catch(onErrore);
          },
        }}
        altra={{ etichetta: t("devices.browse.settings"), fai: onImpostazioni }}
      />
    );
  }

  // I piani uno sopra l'altro: sotto le schede (o la ricerca), sopra
  // l'artista, in cima il disco. Quel che è coperto resta montato, nascosto.
  const livello = album !== null || artista !== null;
  const coperta = livello || cercato !== "";
  /** Una scheda, se è già stata aperta: visibile solo se è quella scelta. */
  const inScheda = (quale: SchedaSfoglia, dentro: () => ReactNode) =>
    aperte.has(quale) && <Piano nascosto={coperta || scheda !== quale}>{dentro()}</Piano>;
  return (
    <ContestoSfoglia.Provider value={sfoglia}>
      <div className="sfoglia" ref={radice}>
        <StrisciaScarico onErrore={onErrore} />
        <div className="barra-sfoglia">
          <Segmentato<SchedaSfoglia>
            etichetta={t("devices.browse.tabs")}
            scelta={scheda}
            voci={[
              { chiave: "evidenza", etichetta: t("devices.browse.highlights") },
              { chiave: "album", etichetta: t("nav.albums") },
              { chiave: "artisti", etichetta: t("nav.artists") },
              { chiave: "brani", etichetta: t("nav.tracks") },
            ]}
            onScegli={(nuova) => scegli(nuova)}
          />
          <input
            id={idCerca}
            type="search"
            className="campo field-input"
            placeholder={t("devices.browse.search", { nome: dispositivo.nome })}
            aria-label={t("devices.browse.search", { nome: dispositivo.nome })}
            value={testo}
            onChange={(e) => {
              // Si cerca fuori dai livelli: la ricerca sta sopra le schede.
              posizioni.current = [];
              setAlbum(null);
              setArtista(null);
              setTesto(e.target.value);
            }}
          />
        </div>
        {inScheda("evidenza", () => (
          <InEvidenza onTutti={(quale, nuovo) => scegli(quale, nuovo)} />
        ))}
        {inScheda("album", () => (
          <TuttiGliAlbum />
        ))}
        {inScheda("artisti", () => (
          <TuttiGliArtisti />
        ))}
        {inScheda("brani", () => (
          <TuttiIBrani ordine={ordine} onOrdine={cambiaOrdine} />
        ))}
        {/* I risultati restano montati sotto un disco o un artista aperti
            dal menù «Vai all'album», come le schede: tornando indietro si
            ritrovano con le loro pagine e il loro punto, e non si chiedono da
            capo all'altro dispositivo (R13-7). */}
        {cercato !== "" && (
          <Piano nascosto={livello}>
            <Risultati testo={cercato} />
          </Piano>
        )}
        {artista !== null && (
          <Piano nascosto={album !== null}>
            <ArtistaAperto artista={artista} onChiudi={chiudiArtista} />
          </Piano>
        )}
        {album !== null && (
          <Piano nascosto={false}>
            <AlbumAperto album={album} onChiudi={chiudiAlbum} />
          </Piano>
        )}
      </div>
    </ContestoSfoglia.Provider>
  );
}
