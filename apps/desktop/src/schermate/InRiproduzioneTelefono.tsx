/**
 * «In riproduzione» sul telefono: un foglio a tutto schermo.
 *
 * Sul computer la schermata è «quasi» intera (vedi `InRiproduzione.tsx`): la
 * barra laterale resta, e guardare la copertina e cambiare vista sono due
 * gesti che non si annullano. Sul telefono quel ragionamento non regge, perché
 * accanto non c'è niente: una schermata che lascia sotto le schede e il mini
 * lettore lascia tre righe di comandi che ripetono quel che c'è sopra di loro,
 * in uno schermo che è già stretto. Qui il foglio copre tutto, schede comprese,
 * e si toglie come si è messo: col dito.
 *
 * # Un componente suo, e non un ramo di quello del computer
 *
 * Quello del computer ha logica che qui non serve: lo spettro, le tre colonne
 * e il loro `--un-pannello-solo`, la coda aperta di serie accanto alla
 * copertina. E questo ha logica che lì non c'è: il trascinamento, la copertina
 * che si sfoglia, i pannelli che prendono il posto della copertina invece di
 * starle accanto. Due rami in un componente solo sarebbero due componenti con
 * gli hook dell'uno chiamati anche nell'altro. I pezzi veri — il cursore, il
 * trasporto, le stelle, le righe della coda, il testo — sono gli stessi, e
 * con loro le correzioni.
 *
 * # Com'è fatto, dall'alto
 *
 * - la testata: la freccia che chiude, «In riproduzione» col disco sotto, e ⋮
 *   per il menu del brano;
 * - il palco: la copertina, grande quanto ci sta, oppure il pannello aperto
 *   — la coda o il testo — al suo posto. Uno alla volta, e nessuno all'apertura:
 *   chi apre questa schermata è venuto a guardare la copertina;
 * - chi suona, col cuore accanto;
 * - il cursore, il trasporto, e in fondo i due tasti dei pannelli con le
 *   stelle in mezzo.
 *
 * Il fondo è la copertina sfocata, sotto un velo del colore della superficie:
 * la stessa ricetta del computer (lì `np-scrim`), cioè il testo sta sopra un
 * colore neutro e la tinta si vede intorno.
 *
 * # I gesti
 *
 * - **giù**, da qualunque punto che non abbia già un mestiere per il dito — il
 *   cursore, le stelle, i pannelli che scorrono: il foglio segue il dito, e al
 *   rilascio `esitoTrascinamento` dice se chiudere o tornare su. Una vibrazione
 *   quando si passa la soglia: lasciando adesso, si chiude;
 * - **di lato sulla copertina**: il brano dopo o quello prima, con la copertina
 *   che esce dal lato verso cui la si è spinta e la nuova che entra dall'altro.
 *
 * Ogni gesto ha il suo tasto: la freccia in cima chiude, il trasporto cambia
 * brano. Chi usa TalkBack, o semplicemente non striscia, non perde niente.
 *
 * # Come si chiude
 *
 * Scivolando giù, sempre: con la freccia, col trascinamento, con Indietro
 * (`useSuIndietro`, che viene prima della scala di `App`). Un pannello aperto
 * si chiude prima del foglio: Indietro sulla coda torna alla copertina.
 */
import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";

import { Copertina, Sfocata } from "../Copertina";
import { RigheCoda, useRigheCoda } from "../Coda";
import { useDito } from "../dito";
import { dopoLaTransizione } from "../foglio";
import { titoloAlbum } from "../formato";
import { elastico, esitoStrisciata, esitoTrascinamento } from "../gesti";
import { useSuIndietro } from "../indietro";
import { branoDaMostrare, gestoPermesso, idDiLibreria } from "../insieme";
import { ipc, type Brano, type StatoRiproduzione } from "../ipc";
import { t } from "../lingue";
import { Attribuzione } from "../parti/Attribuzione";
import { NomeArtista } from "../parti/NomeArtista";
import { Icona } from "../parti/Icone";
import { Scrubber } from "../parti/Scrubber";
import { Testo } from "../parti/Testo";
import { Trasporto } from "../parti/Trasporto";
import { Stelle } from "../Stelle";
import { vibra } from "../vibra";

/** Il pannello che prende il posto della copertina. */
type Pannello = "coda" | "testo";

/** Da che parte è andato il brano, strisciando. */
type Verso = "prossimo" | "precedente";

/** Quanto si può tirare il foglio verso l'alto: poco, e sempre meno. */
const ELASTICO_SU = 32;

/**
 * Quanto aspetta una copertina spinta via che arrivi quella dopo.
 *
 * Il brano dopo non arriva sempre: la coda può essere finita, e «precedente»
 * a brano avviato ricomincia questo invece di tornare indietro
 * (`precedente_ora` in `riproduzione/mod.rs`). Passato questo tempo senza un
 * brano nuovo, la copertina torna al suo posto: dice «qui non c'è altro», o
 * «da capo», col movimento stesso.
 */
const RITORNO_MS = 600;

/** Il segno «il dito è sopra», scritto sul nodo e non nello stato. */
function trascina(nodo: HTMLElement | null, si: boolean): void {
  // Sul nodo e non con `setState`: il rilascio toglie lo spostamento subito
  // (`removeProperty`), e se l'attributo che spegne la transizione restasse
  // lì fino al disegno dopo, per un fotogramma l'elemento tornerebbe al suo
  // posto di scatto invece di scivolarci.
  if (nodo === null) return;
  if (si) nodo.dataset["trascinando"] = "";
  else delete nodo.dataset["trascinando"];
}

/**
 * La copertina che si sfoglia.
 *
 * Due livelli: il guscio prende il dito e porta lo spostamento in una
 * variabile (`--sposta-x`), la faccia dentro lo segue. La faccia ha per chiave
 * il brano, quindi a ogni brano nuovo è un elemento nuovo, ed entra con
 * `@starting-style` dal lato giusto: da destra se si è andati avanti, da
 * sinistra se indietro, in dissolvenza se il brano è cambiato da sé o da un
 * tasto.
 */
function CopertinaNp({
  brano,
  trasporto,
  onErrore,
}: {
  brano: Brano;
  /** Si può cambiare brano: fuori da «Ascolta insieme», o da chi conduce. */
  trasporto: boolean;
  onErrore: (e: unknown) => void;
}) {
  const guscio = useRef<HTMLDivElement>(null);
  const [esce, setEsce] = useState<Verso | null>(null);
  const [entra, setEntra] = useState<Verso | "cambio" | null>(null);
  // Il brano è cambiato: quel che usciva è uscito, e il nuovo entra dal lato
  // opposto. Durante il disegno, come raccomanda React per uno stato che
  // segue una prop: non c'è il fotogramma col brano nuovo e l'uscita vecchia.
  const [di, setDi] = useState(brano.id);
  if (di !== brano.id) {
    setDi(brano.id);
    setEntra(esce ?? "cambio");
    setEsce(null);
  }
  // Prima della pittura della faccia nuova: lo spostamento lasciato dal dito
  // appartiene alla vecchia.
  useLayoutEffect(() => {
    guscio.current?.style.removeProperty("--sposta-x");
  }, [brano.id]);
  // Spinta via e non sostituita: torna. Vedi `RITORNO_MS`.
  useEffect(() => {
    if (esce === null) return;
    const attesa = window.setTimeout(() => {
      guscio.current?.style.removeProperty("--sposta-x");
      setEsce(null);
    }, RITORNO_MS);
    return () => window.clearTimeout(attesa);
  }, [esce]);

  const dito = useDito({
    assi: "x",
    spento: !trasporto,
    onParte: () => trascina(guscio.current, true),
    onMuovi: (dx) => guscio.current?.style.setProperty("--sposta-x", `${dx}px`),
    onFine: ({ dx, vx }) => {
      const nodo = guscio.current;
      trascina(nodo, false);
      const esito = esitoStrisciata({ dx, vx, larghezza: nodo?.offsetWidth ?? 1 });
      if (esito === null) {
        nodo?.style.removeProperty("--sposta-x");
        return;
      }
      // Lo spostamento resta: la regola dell'uscita la porta via da lì,
      // invece di farla ripartire dal centro.
      vibra("scatto");
      setEsce(esito);
      (esito === "prossimo" ? ipc.prossimo() : ipc.precedente()).catch((e: unknown) => {
        nodo?.style.removeProperty("--sposta-x");
        setEsce(null);
        onErrore(e);
      });
    },
    onAnnulla: () => {
      trascina(guscio.current, false);
      guscio.current?.style.removeProperty("--sposta-x");
    },
  });

  return (
    <div
      ref={guscio}
      className="copertina-np"
      data-esce={esce ?? undefined}
      onPointerDown={dito.onPointerDown}
    >
      <div key={brano.id} className="faccia-np" data-entra={entra ?? undefined}>
        <Copertina
          hash={brano.coverArtHash}
          titolo={titoloAlbum(brano.album)}
          classe="arte-np"
          piena
          accanto
        />
      </div>
    </div>
  );
}

/** La coda, al posto della copertina. */
function PannelloCoda({
  stato,
  onErrore,
  onChiudi,
  onMenu,
}: {
  stato: StatoRiproduzione;
  onErrore: (e: unknown) => void;
  onChiudi: () => void;
  onMenu: (e: React.MouseEvent, indice: number, brano: Brano) => void;
}) {
  // Indietro chiude il pannello e non il foglio: montato dopo il foglio, il
  // suo gestore sta sopra nella pila.
  useSuIndietro(onChiudi);
  // Qui e non nel foglio: le righe si chiedono quando la coda si guarda, non
  // a ogni apertura di «In riproduzione».
  const righe = useRigheCoda(stato.coda, onErrore, stato.insieme?.revisione ?? null);
  const svuota = gestoPermesso(stato.insieme, "svuota");
  return (
    <aside className="coda-np pannello-np" aria-label={t("queue.panel.aria")}>
      <header>
        <span className="occhiello">{t("np.queued", { n: stato.coda.length })}</span>
        <button
          type="button"
          className="bottone minuto btn-ghost"
          disabled={stato.coda.length === 0 || !svuota}
          onClick={() => ipc.codaSvuota().catch(onErrore)}
        >
          {t("queue.clear")}
        </button>
      </header>
      <RigheCoda stato={stato} righe={righe} onErrore={onErrore} compatta onMenu={onMenu} />
    </aside>
  );
}

/** Il testo, al posto della copertina. */
function PannelloTesto({
  brano,
  onErrore,
  onChiudi,
}: {
  brano: Brano;
  onErrore: (e: unknown) => void;
  onChiudi: () => void;
}) {
  useSuIndietro(onChiudi);
  return (
    <div className="pannello-np">
      <Testo brano={brano} onErrore={onErrore} />
    </div>
  );
}

export function InRiproduzioneTelefono({
  stato,
  onChiudi,
  onPreferito,
  onVoto,
  onMenu,
  onMenuCoda,
  onArtista,
  onErrore,
  avvisi,
}: {
  stato: StatoRiproduzione;
  /** Il foglio è sceso: lo si smonta. */
  onChiudi: () => void;
  onPreferito: (brano: Brano) => void;
  onVoto: (brano: Brano, stelle: number) => void;
  /** Il menu del brano, dal ⋮ in cima. */
  onMenu: (e: React.MouseEvent, brano: Brano) => void;
  /** Il menu di una riga della coda: vedi `onMenu` in `RigheCoda`. */
  onMenuCoda: (e: React.MouseEvent, indice: number, brano: Brano) => void;
  /** Il nome dell'artista apre la sua pagina; Indietro torna qui. */
  onArtista: (brano: Brano) => void;
  onErrore: (e: unknown) => void;
  /** Le notizie da mostrare sopra il foglio: vedi `InRiproduzione`. */
  avvisi: ReactNode;
}) {
  const foglio = useRef<HTMLElement>(null);
  const [chiudendo, setChiudendo] = useState(false);
  const [pannello, setPannello] = useState<Pannello | null>(null);
  const ultimaChiusura = useRef(onChiudi);
  ultimaChiusura.current = onChiudi;

  const chiudi = useCallback(() => setChiudendo(true), []);
  useSuIndietro(chiudi);
  const chiudiPannello = useCallback(() => setPannello(null), []);

  // La discesa è cominciata: si smonta quando è finita.
  useEffect(() => {
    if (!chiudendo) return;
    return dopoLaTransizione(foglio.current, () => ultimaChiusura.current());
  }, [chiudendo]);

  // Il fuoco entra, e alla chiusura torna dov'era: vedi «Il fuoco» in
  // `InRiproduzione.tsx`. Qui la pagina sotto è `inert` tutta, schede
  // comprese (`Impaginazione`, `inerte`).
  useEffect(() => {
    const chiAveva = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    foglio.current?.focus({ preventScroll: true });
    return () => {
      if (chiAveva !== null && chiAveva !== document.body && chiAveva.isConnected) {
        chiAveva.focus({ preventScroll: true });
      }
    };
  }, []);

  const brano = stato.brano === null ? null : branoDaMostrare(stato.brano, stato.insieme, t);

  // Il brano è finito mentre il foglio era aperto, e non ne viene un altro:
  // davanti a un fondo vuoto non si resta.
  useEffect(() => {
    if (brano === null) onChiudi();
  }, [brano, onChiudi]);

  const oltre = useRef(false);
  const dito = useDito({
    assi: "y",
    // Dove il dito ha già un mestiere: il cursore della posizione, le stelle,
    // e i pannelli, che scorrono.
    ignora: "input, .stelle, .pannello-np",
    spento: chiudendo,
    onParte: () => {
      oltre.current = false;
      trascina(foglio.current, true);
    },
    onMuovi: (_dx, dy) => {
      const nodo = foglio.current;
      if (nodo === null) return;
      nodo.style.setProperty("--trascina-y", `${dy >= 0 ? dy : elastico(dy, ELASTICO_SU)}px`);
      const passato = dy > nodo.offsetHeight / 4;
      if (passato !== oltre.current) {
        oltre.current = passato;
        if (passato) vibra("scatto");
      }
    },
    onFine: ({ dy, vy }) => {
      const nodo = foglio.current;
      trascina(nodo, false);
      if (esitoTrascinamento({ dy, vy, altezza: nodo?.offsetHeight ?? 0 }) === "chiudi") {
        chiudi();
      } else {
        nodo?.style.removeProperty("--trascina-y");
      }
    },
    onAnnulla: () => {
      trascina(foglio.current, false);
      foglio.current?.style.removeProperty("--trascina-y");
    },
  });

  if (brano === null) return null;

  // Cuore, stelle, menu e testo sono della libreria di questo telefono: una
  // voce di «Ascolta insieme» non ha una riga su cui scriverli. Vedi `Giudizio`.
  const diLibreria = idDiLibreria(brano.id);
  const mostrato = pannello === "testo" && !diLibreria ? null : pannello;
  const alterna = (quale: Pannello) => setPannello((prima) => (prima === quale ? null : quale));

  return (
    <section
      ref={foglio}
      /* Raggiungibile dal codice e non dal tabulatore: è dove entra il fuoco. */
      tabIndex={-1}
      className="foglio-np"
      role="dialog"
      aria-modal="true"
      aria-label={t("np.nowPlaying")}
      data-chiudendo={chiudendo || undefined}
      data-pannello={mostrato ?? undefined}
      onPointerDown={dito.onPointerDown}
    >
      <div className="sfondo-np" aria-hidden="true">
        <Sfocata hash={brano.coverArtHash} classe="tinta-np" />
      </div>

      <header className="testa-np">
        <button type="button" className="tasto-np" aria-label={t("common.close")} onClick={chiudi}>
          <Icona nome="i-chev-d" dim={26} />
        </button>
        <div className="titoli-testa-np">
          <span className="occhiello-np">{t("np.nowPlaying")}</span>
          <span className="album-testa-np" dir="auto">
            {titoloAlbum(brano.album)}
          </span>
        </div>
        {diLibreria && (
          <button
            type="button"
            className="tasto-np"
            aria-label={t("row.more", { titolo: brano.title })}
            onClick={(e) => onMenu(e, brano)}
          >
            <Icona nome="i-dots-v" dim={22} />
          </button>
        )}
      </header>

      <div className="avvisi-foglio-np">{avvisi}</div>

      <div className="palco-np">
        {mostrato === "coda" ? (
          <PannelloCoda
            stato={stato}
            onErrore={onErrore}
            onChiudi={chiudiPannello}
            onMenu={onMenuCoda}
          />
        ) : mostrato === "testo" ? (
          <PannelloTesto brano={brano} onErrore={onErrore} onChiudi={chiudiPannello} />
        ) : (
          <CopertinaNp
            brano={brano}
            trasporto={gestoPermesso(stato.insieme, "trasporto")}
            onErrore={onErrore}
          />
        )}
      </div>

      <div className="info-np">
        <div className="chi-np">
          <h1 className="titolo-np" dir="auto">
            {brano.title}
          </h1>
          <p className="artista-np" dir="auto">
            <NomeArtista brano={brano} onApri={onArtista} />
          </p>
          <Attribuzione brano={brano} onErrore={onErrore} />
        </div>
        {diLibreria && (
          <button
            type="button"
            className="tasto-np"
            aria-pressed={brano.liked}
            aria-label={t("track.favorite")}
            onClick={() => onPreferito(brano)}
          >
            <Icona nome={brano.liked ? "i-heart-f" : "i-heart"} dim={26} />
          </button>
        )}
      </div>

      <div className="cursore-np">
        <Scrubber stato={stato} onErrore={onErrore} />
      </div>

      <Trasporto stato={stato} taglia="telefono" onErrore={onErrore} />

      <footer className="piede-np">
        <button
          type="button"
          className="tasto-np linguetta-np"
          aria-pressed={mostrato === "coda"}
          aria-label={t("np.queue.toggle")}
          onClick={() => alterna("coda")}
        >
          <Icona nome="i-queue" dim={22} />
        </button>
        {diLibreria && <Stelle valore={brano.rating} onVoto={(stelle) => onVoto(brano, stelle)} />}
        {diLibreria && (
          <button
            type="button"
            className="tasto-np linguetta-np"
            aria-pressed={mostrato === "testo"}
            aria-label={t("np.lyrics.toggle")}
            onClick={() => alterna("testo")}
          >
            <Icona nome="i-text" dim={22} />
          </button>
        )}
      </footer>
    </section>
  );
}
