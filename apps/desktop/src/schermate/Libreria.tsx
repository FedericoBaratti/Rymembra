/**
 * Le pagine della libreria: la testata, la griglia dei dischi, il vuoto.
 *
 * Stavano dentro `App.tsx`, scritte in linea nei due rami di `testa()` e di
 * `corpo()`, e ne sono uscite per la stessa ragione di `Elenco.tsx`: lo Studio
 * disegna le pagine vere, e una pagina che vive dentro il componente della
 * finestra non si può disegnare altrove. Quel che sa della finestra — quale
 * album si sta guardando, cosa succede a un clic — arriva per prop; quel che
 * suona e quale copertina vola arrivano dai contesti, come a tutte le schede.
 */
import { useContext, useEffect, useRef } from "react";
import type { ReactNode, RefObject } from "react";

import { Copertina } from "../Copertina";
import { brani_, nomeArtista, numero, titoloAlbum } from "../formato";
import { useFuoco } from "../fuoco";
import type { Album, Avvio } from "../ipc";
import { t } from "../lingue";
import type { VistaLibreria } from "../pagina";
import { useVersione } from "../pagine";
import { Icona, type NomeIcona } from "../parti/Icone";
import { COPERTINA_IN_VOLO, Presenza, Volo } from "../presenza";
import type { SorgenteRada } from "../rada";
import { conIndice, suonoAlbum, useQuelCheSuona } from "../segnali";
import { useVirtuale, type Virtuale } from "../virtuale";

/**
 * Lo scorrevole di chi non ne ha uno: la griglia disegnata per intero.
 *
 * Gli hook si chiamano sempre, e `useVirtuale` vuole un riferimento: questo è
 * quello che non punta a niente, e con lui la finestra non si calcola mai.
 */
const SENZA_SCORREVOLE: RefObject<HTMLElement | null> = { current: null };

/**
 * La finestra di una griglia di schede, e il fuoco che ci gira dentro.
 *
 * In comune fra gli album e gli artisti: le due griglie si virtualizzano allo
 * stesso modo — per **righe** di schede, con le colonne contate dal foglio — e
 * hanno lo stesso fuoco a due assi. Senza `scorrevole` (lo Studio, che la
 * griglia la disegna intera su una tela) non c'è finestra: si disegna tutto.
 */
export function useGrigliaVirtuale(opzioni: {
  totale: number;
  griglia: RefObject<HTMLElement | null>;
  scorrevole: RefObject<HTMLElement | null> | undefined;
  /** Come si riconosce una scheda; il metro è la prima vera, non uno scheletro. */
  selettore: string;
  selettoreMisura: string;
  memoria?: boolean | undefined;
}) {
  const { totale, griglia, scorrevole, selettore, selettoreMisura, memoria } = opzioni;
  const finestra = useVirtuale({
    totale,
    contenitore: scorrevole ?? SENZA_SCORREVOLE,
    ancora: griglia,
    selettoreRiga: selettore,
    selettoreMisura,
    griglia: true,
    memoria,
  });
  const intera = scorrevole === undefined;
  const primo = intera ? 0 : finestra.primo;
  const ultimo = intera ? totale : finestra.ultimo;
  const fuoco = useFuoco({
    totale,
    ancora: griglia,
    finestra: intera ? { primo, ultimo, sopra: 0, sotto: 0 } : finestra,
    scorriA: finestra.scorriA,
    selettoreRiga: selettore,
    colonne: finestra.colonne,
  });
  /*
   * La scheda con `tabIndex={0}`: quella attiva, se è disegnata; se no la prima
   * in vista. Senza il ripiego, scorrendo con la rotella lontano dalla scheda
   * attiva nessuna scheda nel DOM sarebbe una fermata, e Tab salterebbe la
   * griglia intera — prima, quando ogni scheda era una fermata, non succedeva.
   * Il conto è di `useFuoco`, che lo fa uguale per la coda e per l'albero.
   */
  return { finestra, intera, primo, ultimo, fuoco, fermata: fuoco.fermata };
}

/**
 * Le due paglie di una griglia, e lo scarto che ciascuna si porta dietro.
 *
 * Una paglia in una griglia occupa una riga tutta sua (`grid-column: 1 / -1`),
 * e la griglia le mette dopo lo spazio fra due righe: la paglia è alta il passo
 * per le righe che sostituisce **meno** quello spazio, ed è così che la prima
 * scheda disegnata cade dove cadrebbe senza finestra. Senza righe da sostituire
 * non si disegna affatto: una paglia alta zero porterebbe comunque il suo
 * spazio, e la griglia scenderebbe di quello.
 */
export function pagliaDiGriglia(
  chiave: string,
  righe: number,
  finestra: Virtuale,
): ReactNode {
  if (righe <= 0) return null;
  return (
    <div
      key={chiave}
      className="paglia"
      role="presentation"
      style={{
        gridColumn: "1 / -1",
        height: Math.max(righe * finestra.altezza - finestra.scarto, 0),
      }}
    />
  );
}

/**
 * La griglia dei dischi.
 *
 * Una sola per le due pagine che la mostrano — gli Album e la pagina di un
 * artista — che differiscono solo per la riga sotto il titolo: nella pagina
 * dell'artista l'artista è già scritto in cima, e ripeterlo su ogni scheda
 * sarebbe rumore, quindi lì si dice l'anno.
 *
 * # Per righe, e solo quelle che si vedono
 *
 * Novecento dischi erano novecento schede nel DOM, ognuna con la sua copertina:
 * qui se ne disegnano le righe in vista più il margine (`useGrigliaVirtuale`),
 * e i dischi arrivano come le righe dell'elenco dei brani — da una lista rada
 * (`rada`) con il totale noto, gli scheletri al posto di quelli che mancano —
 * oppure interi (`album`) dove sono pochi e già in mano.
 *
 * Una fermata di tabulazione per la griglia, e le frecce sui due assi: vedi
 * «Le griglie» in `fuoco.ts`.
 */
export function GrigliaAlbum({
  album,
  rada,
  sotto,
  onApri,
  onMenu,
  scorrevole,
  memoria,
}: {
  /** I dischi, quando sono tutti in mano. */
  album?: readonly Album[] | undefined;
  /** I dischi, quando arrivano a pagine dove si guarda. Vince su `album`. */
  rada?: SorgenteRada<Album> | undefined;
  /** La riga sotto il titolo: chi suona, o l'anno. */
  sotto: "artista" | "anno";
  onApri: (album: Album) => void;
  onMenu: (e: React.MouseEvent, chiave: string) => void;
  /** Lo scorrevole: senza, la griglia si disegna per intero. */
  scorrevole?: RefObject<HTMLDivElement | null> | undefined;
  /** Lo scorrimento si ricorda: solo sotto un `<Activity>`. */
  memoria?: boolean | undefined;
}) {
  const ora = useQuelCheSuona();
  const inVolo = useContext(Volo);
  const griglia = useRef<HTMLDivElement>(null);
  const versione = useVersione(rada);
  const totale = rada !== undefined ? rada.totale : (album?.length ?? 0);
  const { finestra, intera, primo, ultimo, fuoco, fermata } = useGrigliaVirtuale({
    totale,
    griglia,
    scorrevole,
    selettore: ".scheda",
    selettoreMisura: ".scheda:not(.finta)",
    memoria,
  });

  // Le pagine che la finestra guarda: vedi `ElencoBrani`.
  useEffect(() => {
    rada?.richiedi(primo, ultimo);
  }, [rada, primo, ultimo, versione]);

  // Gli scheletri alti quanto una scheda vera, appena se n'è misurata una.
  const altezzaScheda = finestra.misurata
    ? finestra.altezza - finestra.scarto
    : undefined;

  const schede: ReactNode[] = [];
  for (let i = primo; i < ultimo; i += 1) {
    const a = rada !== undefined ? rada.riga(i) : album?.[i];
    const chiave = rada !== undefined || a === undefined ? `s${i}` : a.albumKey;
    if (a === undefined) {
      // Un bottone anche lo scheletro, con la stessa chiave della scheda che
      // lo sostituirà: il nodo resta lo stesso, e il fuoco arrivato con Fine su
      // un disco non ancora arrivato ci resta quando il disco arriva.
      schede.push(
        <button
          key={chiave}
          type="button"
          className="scheda card finta"
          aria-busy="true"
          aria-label={t("list.loading")}
          data-indice={i}
          tabIndex={i === fermata ? 0 : -1}
          onFocus={() => fuoco.segna(i)}
          style={altezzaScheda === undefined ? undefined : { height: altezzaScheda }}
        >
          <div className="copertina skeleton" />
          <div className="riga-finta skeleton" />
          <div className="riga-finta corta skeleton" />
        </button>,
      );
      continue;
    }
    schede.push(
      <button
        key={chiave}
        type="button"
        className="scheda card"
        {...suonoAlbum(ora, a.albumKey)}
        style={conIndice(i)}
        data-indice={i}
        tabIndex={i === fermata ? 0 : -1}
        onFocus={() => fuoco.segna(i)}
        onClick={() => onApri(a)}
        onContextMenu={(e) => onMenu(e, a.albumKey)}
      >
        <Presenza
          parte="card-cover"
          nome={a.albumKey === inVolo ? COPERTINA_IN_VOLO : undefined}
          compagna="hero-art"
        >
          <Copertina
            hash={a.coverArtHash}
            titolo={titoloAlbum(a.title)}
            classe="copertina card-cover"
            misura="scheda"
            larghezza={finestra.larghezzaColonna}
            /* Il titolo è scritto sotto: un `alt` lo farebbe leggere due volte. */
            accanto
          />
        </Presenza>
        {/* `dir="auto"` su titolo e artista: un nome arabo o ebraico si legge
            da destra, e la direzione la decide il nome, non la pagina. */}
        <div className="titolo card-title" title={titoloAlbum(a.title)} dir="auto">
          {titoloAlbum(a.title)}
        </div>
        {sotto === "artista" ? (
          <div className="sotto card-subtitle" title={nomeArtista(a.artist)}>
            <bdi>{nomeArtista(a.artist)}</bdi>
            {a.totalTracks > 1 ? ` · ${numero(a.totalTracks)}` : ""}
          </div>
        ) : (
          <div className="sotto card-subtitle">
            {a.year ?? ""}
            {a.year && a.totalTracks > 1 ? " · " : ""}
            {a.totalTracks > 1 ? brani_(a.totalTracks) : ""}
          </div>
        )}
      </button>,
    );
  }

  return (
    <div
      className="griglia track-grid"
      ref={griglia}
      onKeyDown={fuoco.daTastiera}
      data-scorso={finestra.scorso || undefined}
    >
      {!intera && pagliaDiGriglia("sopra", finestra.sopra, finestra)}
      {schede}
      {!intera && pagliaDiGriglia("sotto", finestra.sotto, finestra)}
    </div>
  );
}

/**
 * Il riquadro di quando non c'è niente: un'icona, cosa manca, e se si può,
 * il gesto che lo riempie.
 *
 * Uno per tutte le pagine, e non copiato in ognuna: le copie scritte a mano
 * erano sei, e la prima differenza fra due di loro sarebbe stata una skin che
 * veste lo stato vuoto degli album e non quello degli artisti.
 *
 * `nota` è la riga in più, più piccola, che qualche pagina ha da dire sotto la
 * spiegazione. `secondaria` lascia il gesto nel suo grigio: quando la pagina
 * vuota non è un guasto da riparare ma un posto dove si arriverà — la coda
 * delle importazioni — un tasto colorato lo farebbe sembrare un dovere.
 */
export function StatoVuoto({
  icona,
  titolo,
  testo,
  nota,
  azione,
}: {
  icona: NomeIcona;
  titolo: string;
  testo: React.ReactNode;
  nota?: React.ReactNode;
  azione?:
    | { etichetta: string; fai: () => void; secondaria?: boolean | undefined }
    | undefined;
}) {
  return (
    <div className="vuoto empty-state">
      <span className="empty-icon" aria-hidden="true">
        <Icona nome={icona} dim={30} />
      </span>
      <h2>{titolo}</h2>
      <p>{testo}</p>
      {nota !== undefined && <p className="nota">{nota}</p>}
      {azione && (
        <button
          type="button"
          className={azione.secondaria ? "bottone" : "bottone primario btn-accent"}
          onClick={azione.fai}
        >
          {azione.etichetta}
        </button>
      )}
    </div>
  );
}

/**
 * La testata di una vista della libreria, quando niente è aperto sopra.
 *
 * Le prop di `Intestazione`, non un componente che la avvolge: alla prima
 * lettera nella casella la pagina diventa quella della ricerca, che disegna
 * `Intestazione` diretta, e un componente in mezzo era un tipo diverso nello
 * stesso posto. React rifaceva la testata da capo, il campo nuovo nasceva senza
 * fuoco, e chi scriveva doveva ricliccarci dopo la prima lettera. La casella la
 * aggiunge chi disegna: `testa()` in `App.tsx`, e lo Studio.
 */
export function testaLibreria({
  vista,
  numeri,
  cartelle,
  ordinamento,
}: {
  vista: VistaLibreria;
  numeri: Avvio["numeri"] | undefined;
  /** Quante cartelle si sorvegliano. */
  cartelle: number;
  /** Solo sui Brani, l'unica vista che si ordina. */
  ordinamento?: { etichetta: string; onApri: (e: React.MouseEvent) => void } | undefined;
}) {
  const titoli: Record<VistaLibreria, [string, string]> = {
    album: [t("nav.albums"), t("page.library.inLibrary", { n: numeri?.albums ?? 0 })],
    artisti: [t("nav.artists"), t("page.library.artistsSub", { n: numeri?.artists ?? 0 })],
    brani: [t("nav.tracks"), t("page.library.inLibrary", { n: numeri?.tracks ?? 0 })],
    // Il sottotitolo conta le radici e non i brani, come il conteggio nella
    // barra: qui il numero dei brani lo saprebbe solo l'albero, che a questo
    // punto potrebbe non essere ancora stato costruito.
    cartelle: [t("nav.folders"), t("page.folders.sub", { n: cartelle })],
    preferiti: [t("nav.favorites"), t("page.library.liked", { n: numeri?.liked ?? 0 })],
  };
  const [titolo, sotto] = titoli[vista];
  return {
    occhiello: t("page.library"),
    titolo,
    sottotitolo: sotto,
    ...(vista === "brani" && ordinamento ? { ordinamento } : {}),
  };
}
