/**
 * La tela: l'applicazione vera, con dati finti, dentro una finestra finta.
 *
 * # Perché un `<iframe>`
 *
 * La tela dello Studio 2.x era un riquadro dentro la pagina, e il foglio della
 * bozza ci arrivava riscrivendo `:root[data-skin]` in `.anteprima[data-skin]`.
 * Ne venivano tre bugie. Il foglio della skin **indossata** colpiva anche il
 * riquadro — e se la bozza era quella skin, una proprietà tolta nella bozza
 * restava lì, dipinta dall'altra. `position: fixed`, `100vw` e le query
 * misuravano la finestra dello Studio, non quella finta. E i `@keyframes` delle
 * due skin si pestavano i piedi.
 *
 * Un `<iframe>` vuoto è un documento suo: ci si copiano i fogli dell'app — non
 * quello della skin attiva — e ci si mette il foglio della bozza compilato coi
 * selettori della finestra vera (`Ambito::tela`). Dentro, `:root` è la radice
 * della tela, la finestra è larga quanto la larghezza scelta, e tutto si misura
 * come lo misurerà l'app.
 *
 * I componenti restano quelli dell'app, montati con un portale: React li
 * disegna nel documento della tela, e i loro gestori di eventi — che React
 * ascolta sulla radice di questo documento — non ricevono niente. È il primo
 * cancello contro un clic che fa partire un'azione vera; il secondo è qui
 * sotto, in cattura, per quel che il browser farebbe da sé.
 */
import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";

import { Chiedi } from "../Chiedi";
import { Copertina } from "../Copertina";
import { DatiContesto } from "../dati";
import { brani_, nomeArtista, titoloAlbum } from "../formato";
import { Impaginazione, type ContestoWidget } from "../Impaginazione";
import type { NodoScafale, ParteRegistro } from "../ipc";
import { t } from "../lingue";
import { Menu } from "../Menu";
import { ControlliFinestra } from "../parti/BarraTitolo";
import { PannelloEq } from "../parti/Equalizzatore";
import { Fascia } from "../parti/Fascia";
import { Icona, Simboli } from "../parti/Icone";
import { Intestazione } from "../parti/Intestazione";
import { PilaToast } from "../parti/PilaToast";
import { creaArchivioPosizione } from "../posizione";
import { Presenze, Volo } from "../presenza";
import { Primo } from "../Primo";
import { Artisti } from "../schermate/Artisti";
import { Cartelle } from "../schermate/Cartelle";
import { ElencoBrani, ElencoFinto, GrigliaFinta, Sentinella } from "../schermate/Elenco";
import { Esplora } from "../schermate/Esplora";
import { Home, testaHome } from "../schermate/Home";
import { Impostazioni } from "../schermate/Impostazioni";
import { InRiproduzione } from "../schermate/InRiproduzione";
import { GrigliaAlbum, StatoVuoto, testaLibreria } from "../schermate/Libreria";
import { Suona } from "../segnali";
import {
  ALBUM,
  ARTISTI,
  BRANI,
  CASA,
  ESPLORA,
  IN_ASCOLTO,
  SETTIMANA,
  contestoFinto,
  datiFinti,
  statoFinto,
} from "./mondo";
import { impostazioniFinte } from "./mondoImpostazioni";
import { ALTEZZA, LARGHEZZE, type Larghezza, type Scena } from "./scena";

/** Un comando che non fa niente. */
const niente = () => {
  /* apposta */
};

/** Le mappe vuote, una volta sola: identità stabile per i fornitori. */
const NESSUNA_PRESENZA = new Map();
const NESSUNA_SELEZIONE = new Set<number>();
const NESSUNA_PAGINA = { altre: false, sentinella: () => undefined };

/** Un riquadro sulla tela, nelle coordinate della tela (prima dello zoom). */
export interface Riquadro {
  x: number;
  y: number;
  w: number;
  h: number;
}

/** Gli eventi che sulla tela non devono fare niente. */
const FERMATI = [
  "click",
  "dblclick",
  "auxclick",
  "mousedown",
  "mouseup",
  "pointerdown",
  "pointerup",
  "contextmenu",
  "keyup",
  "keypress",
  "submit",
  "dragstart",
  "drop",
  "input",
  "change",
  "beforeinput",
] as const;

/**
 * Copia nel documento della tela i fogli dell'app, meno quello della skin
 * attiva: la tela veste la bozza, non la skin indossata dallo Studio.
 *
 * # Idempotente
 *
 * La tela vive sotto `<Activity>`, e tornando visibile — da «Prova
 * sull'app», per esempio — gli effetti ripartono. Fino alla 3.0 ogni ritorno
 * ricopiava tutti i fogli in coda a quelli già copiati: dopo dieci prove la
 * tela aveva dieci copie di `stile.css`, e il motore le ricalcolava tutte a
 * ogni cambio della bozza. Adesso `copie` ricorda quale nodo dell'app è già
 * dentro e con quale copia: si aggiunge solo quel che manca, si toglie quel
 * che l'app non ha più, e un foglio cambiato — il ricaricamento a caldo in
 * sviluppo, un foglio caricato più tardi con lo Studio — si riallinea.
 */
function copiaFogli(dentro: Document, copie: Map<Element, Element>): void {
  const adesso = new Set<Element>();
  for (const nodo of document.head.querySelectorAll("style, link[rel='stylesheet']")) {
    if (nodo.id === "skin-attiva") continue;
    adesso.add(nodo);
    const copia = copie.get(nodo);
    if (copia !== undefined && copia.isConnected && copia.ownerDocument === dentro) {
      if (nodo instanceof HTMLStyleElement && copia.textContent !== nodo.textContent) {
        copia.textContent = nodo.textContent;
      }
      continue;
    }
    const nuova = dentro.importNode(nodo, true);
    // Prima del foglio della bozza, se c'è già: la bozza deve vincere a pari
    // specificità, come nella finestra vera vince la skin.
    const bozza = dentro.getElementById("bozza");
    if (bozza === null) dentro.head.append(nuova);
    else dentro.head.insertBefore(nuova, bozza);
    copie.set(nodo, nuova);
  }
  for (const [nodo, copia] of copie) {
    if (adesso.has(nodo)) continue;
    copia.remove();
    copie.delete(nodo);
  }
}

/** Due riquadri uguali al pixel: la sonda non ridisegna per niente. */
function stessoRiquadro(a: Riquadro, b: Riquadro): boolean {
  return a.x === b.x && a.y === b.y && a.w === b.w && a.h === b.h;
}

/** La parte più interna sotto un elemento, se ce n'è una. */
function parteSotto(
  da: Element | null,
  parti: ReadonlyMap<string, ParteRegistro>,
): { nome: string; elemento: Element } | null {
  for (let e = da; e !== null; e = e.parentElement) {
    for (const classe of e.classList) {
      if (parti.has(classe)) return { nome: classe, elemento: e };
    }
  }
  return null;
}

function riquadroDi(e: Element): Riquadro {
  const r = e.getBoundingClientRect();
  return { x: r.left, y: r.top, w: r.width, h: r.height };
}

export function Tela({
  css,
  id,
  tema,
  densita,
  movimento,
  shell,
  scena,
  larghezza,
  zoom,
  inPausa,
  parti,
  scelta,
  statoForzato,
  sonda,
  rigioca,
  onScegli,
}: {
  /** Il foglio della bozza, compilato per la tela. */
  css: string;
  id: string;
  /** Il tema mostrato: `dark` o `light`. */
  tema: "dark" | "light";
  densita: string;
  movimento: string;
  shell: NodoScafale | null;
  scena: Scena;
  larghezza: Larghezza;
  zoom: "adatta" | 1 | 2;
  /** Il trasporto finto: la copertina che gira gira davvero. */
  inPausa: boolean;
  parti: ReadonlyMap<string, ParteRegistro>;
  /** La parte scelta: si contornano tutte le sue occorrenze. */
  scelta: string | null;
  /** Lo stato forzato sulla parte scelta, o `null`. */
  statoForzato: string | null;
  sonda: boolean;
  rigioca: number;
  onScegli: (parte: string) => void;
}) {
  const cornice = useRef<HTMLDivElement>(null);
  const iframe = useRef<HTMLIFrameElement>(null);
  const [doc, setDoc] = useState<Document | null>(null);
  const [spazio, setSpazio] = useState({ w: 0, h: 0 });
  const [sotto, setSotto] = useState<{ nome: string; riquadro: Riquadro } | null>(null);
  const [contorni, setContorni] = useState<Riquadro[]>([]);
  const larga = LARGHEZZE[larghezza];

  // ── il documento della tela ────────────────────────────────────────────
  //
  // Riparte a ogni ritorno da `<Activity>` nascosta, e per questo è
  // idempotente: vedi `copiaFogli`. Il foglio della bozza si crea una volta.
  const copie = useRef(new Map<Element, Element>());
  useLayoutEffect(() => {
    const cornice = iframe.current;
    const dentro = cornice?.contentDocument;
    if (!dentro) return;
    if (dentro.getElementById("bozza") === null) {
      const bozza = dentro.createElement("style");
      bozza.id = "bozza";
      dentro.head.append(bozza);
    }
    copiaFogli(dentro, copie.current);
    dentro.body.classList.add("corpo-tela");
    setDoc(dentro);
  }, []);

  useEffect(() => {
    const foglio = doc?.getElementById("bozza");
    if (foglio) foglio.textContent = css;
  }, [doc, css]);

  // Gli attributi della radice: sono selettori, quindi contano.
  useEffect(() => {
    if (!doc) return;
    const radice = doc.documentElement;
    radice.dataset.skin = id;
    radice.dataset.theme = tema;
    radice.dataset.density = densita;
    radice.dataset.motion = movimento;
    radice.lang = document.documentElement.lang;
  }, [doc, id, tema, densita, movimento]);

  // Il movimento chiesto da chi guarda si rispecchia dalla finestra vera, e
  // **nei due sensi**: una tela che si muove per chi ha chiesto di non
  // muoversi non è un'anteprima onesta, e una che resta ferma dopo che l'ha
  // riacceso nemmeno. Fino alla 3.0 si copiava una volta, solo se acceso, e
  // non si toglieva più.
  useEffect(() => {
    if (!doc) return;
    const fuori = document.documentElement;
    const rispecchia = () => {
      const utente = fuori.dataset.motionUtente;
      if (utente === undefined) delete doc.documentElement.dataset.motionUtente;
      else doc.documentElement.dataset.motionUtente = utente;
    };
    rispecchia();
    const osserva = new MutationObserver(rispecchia);
    osserva.observe(fuori, { attributes: true, attributeFilter: ["data-motion-utente"] });
    return () => osserva.disconnect();
  }, [doc]);

  // ── lo zoom ─────────────────────────────────────────────────────────────
  useLayoutEffect(() => {
    const nodo = cornice.current;
    if (!nodo) return;
    const misura = () => setSpazio({ w: nodo.clientWidth, h: nodo.clientHeight });
    misura();
    const osserva = new ResizeObserver(misura);
    osserva.observe(nodo);
    return () => osserva.disconnect();
  }, []);
  const scala =
    zoom === "adatta"
      ? Math.max(0.2, Math.min(1, (spazio.w - 32) / larga, (spazio.h - 32) / ALTEZZA))
      : zoom;

  // ── il blocco degli eventi e la sonda ───────────────────────────────────
  const partiRef = useRef(parti);
  partiRef.current = parti;
  const sondaRef = useRef(sonda);
  sondaRef.current = sonda;
  const scegliRef = useRef(onScegli);
  scegliRef.current = onScegli;

  useEffect(() => {
    if (!doc) return;
    const ferma = (e: Event) => {
      if (e.type === "click" && sondaRef.current) {
        const trovata = parteSotto(e.target as Element | null, partiRef.current);
        if (trovata) scegliRef.current(trovata.nome);
      }
      e.preventDefault();
      e.stopPropagation();
    };
    // Il fuoco non entra: un campo della tela che lo prende lo ruba al campo
    // dello Studio in cui si sta scrivendo.
    const via = (e: FocusEvent) => {
      if (e.target instanceof HTMLElement) e.target.blur();
    };
    // Solo quando cambiano la parte o il suo riquadro: `pointermove` arriva
    // decine di volte al secondo, e un oggetto nuovo a ogni arrivo era un
    // disegno nuovo della tela — e, prima che `Mondo` fosse memorizzato,
    // dell'intera applicazione finta che c'è dentro.
    const muovi = (e: PointerEvent) => {
      if (!sondaRef.current) return;
      const trovata = parteSotto(e.target as Element | null, partiRef.current);
      const prossimo = trovata ? { nome: trovata.nome, riquadro: riquadroDi(trovata.elemento) } : null;
      setSotto((prima) =>
        prima === null || prossimo === null
          ? prima === prossimo
            ? prima
            : prossimo
          : prima.nome === prossimo.nome && stessoRiquadro(prima.riquadro, prossimo.riquadro)
            ? prima
            : prossimo,
      );
    };
    const esci = () => setSotto(null);
    // I tasti premuti mentre il fuoco è dentro la tela — ci finisce con un
    // clic, anche se il clic non fa niente — non fanno niente lì dentro, e
    // fino alla 3.0 finivano lì: le scorciatoie dello Studio, che ascoltano
    // sulla finestra, non li ricevevano, e il Tab non usciva più. Una trappola
    // di tastiera. Adesso il fuoco torna sulla cornice, fuori dalla tela, e il
    // tasto si rimanda da lì: Ctrl+Z, Esc e le altre arrivano dove devono, e
    // il prossimo Tab riparte dalla cornice nell'ordine della finestra.
    const tasto = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      const fuori = cornice.current;
      if (fuori === null) return;
      fuori.focus({ preventScroll: true });
      if (e.key === "Tab") return;
      fuori.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: e.key,
          code: e.code,
          ctrlKey: e.ctrlKey,
          shiftKey: e.shiftKey,
          altKey: e.altKey,
          metaKey: e.metaKey,
          repeat: e.repeat,
          bubbles: true,
          cancelable: true,
        }),
      );
    };
    for (const tipo of FERMATI) doc.addEventListener(tipo, ferma, true);
    doc.addEventListener("keydown", tasto, true);
    doc.addEventListener("focusin", via, true);
    doc.addEventListener("pointermove", muovi, true);
    doc.addEventListener("pointerleave", esci, true);
    return () => {
      for (const tipo of FERMATI) doc.removeEventListener(tipo, ferma, true);
      doc.removeEventListener("keydown", tasto, true);
      doc.removeEventListener("focusin", via, true);
      doc.removeEventListener("pointermove", muovi, true);
      doc.removeEventListener("pointerleave", esci, true);
    };
  }, [doc]);

  // ── i contorni e lo stato forzato della parte scelta ─────────────────────
  const rimisura = useCallback(() => {
    if (!doc || scelta === null) {
      setContorni([]);
      return;
    }
    const trovate = doc.querySelectorAll(`.${CSS.escape(scelta)}`);
    setContorni(
      [...trovate]
        .map(riquadroDi)
        .filter((r) => r.w > 0 && r.h > 0)
        .slice(0, 60),
    );
  }, [doc, scelta]);

  useEffect(() => {
    if (!doc) return;
    let fotogramma = 0;
    const presto = () => {
      if (fotogramma !== 0) return;
      fotogramma = requestAnimationFrame(() => {
        fotogramma = 0;
        rimisura();
      });
    };
    // Lo stato forzato si scrive su ogni occorrenza, anche su quelle che
    // nascono dopo — una riga di un elenco virtuale che entra scorrendo.
    const forza = () => {
      if (scelta === null) return;
      for (const e of doc.querySelectorAll(`.${CSS.escape(scelta)}`)) {
        if (statoForzato === null) e.removeAttribute("data-forza");
        else if (e.getAttribute("data-forza") !== statoForzato) e.setAttribute("data-forza", statoForzato);
      }
    };
    forza();
    presto();
    const osserva = new MutationObserver(() => {
      forza();
      presto();
    });
    osserva.observe(doc.body, { childList: true, subtree: true });
    const ridimensiona = new ResizeObserver(presto);
    ridimensiona.observe(doc.body);
    doc.addEventListener("scroll", presto, true);
    return () => {
      osserva.disconnect();
      ridimensiona.disconnect();
      doc.removeEventListener("scroll", presto, true);
      cancelAnimationFrame(fotogramma);
      for (const e of doc.querySelectorAll("[data-forza]")) e.removeAttribute("data-forza");
    };
  }, [doc, scelta, statoForzato, rimisura, css, scena, larghezza]);

  // ── il trasporto finto ───────────────────────────────────────────────────
  const posizione = useMemo(() => creaArchivioPosizione(96_000), []);
  useEffect(() => {
    if (inPausa) return;
    let ultimo = performance.now();
    let fotogramma = requestAnimationFrame(function passo(adesso) {
      const trascorso = adesso - ultimo;
      ultimo = adesso;
      const prossima = (posizione.leggi() + trascorso) % IN_ASCOLTO.durationMs;
      posizione.pubblica(Math.round(prossima / 50) * 50);
      fotogramma = requestAnimationFrame(passo);
    });
    return () => cancelAnimationFrame(fotogramma);
  }, [inPausa, posizione]);

  const dati = useMemo(
    () => datiFinti(posizione, () => ({ larghezza: larga, altezza: ALTEZZA })),
    [posizione, larga],
  );
  const conBrano = scena.pagina !== "vuoto";
  const stato = useMemo(() => statoFinto(conBrano, inPausa), [conBrano, inPausa]);
  const quelCheSuona = useMemo(
    () => ({
      brano: conBrano ? IN_ASCOLTO.id : null,
      album: conBrano ? IN_ASCOLTO.albumKey : null,
      inPausa,
    }),
    [conBrano, inPausa],
  );
  const contesto = useMemo(
    () => contestoFinto(scena, larghezza, stato),
    [scena, larghezza, stato],
  );

  // Il portale, memorizzato: la tela si ridisegna per la sonda, i contorni,
  // lo zoom — cose che stanno **fuori** dall'applicazione finta — e senza
  // questo ogni suo disegno ridisegnava anche quella, pagina intera.
  const portale = useMemo(
    () =>
      doc === null
        ? null
        : createPortal(
            <DatiContesto value={dati}>
              {/* Le icone sono `<use href="#i-…">`, e un riferimento così cerca il
                  simbolo nel proprio documento: senza un foglio dei simboli
                  anche qui, ogni icona della tela sarebbe un riquadro vuoto. */}
              <Simboli />
              <Presenze value={NESSUNA_PRESENZA}>
                <Volo value={null}>
                  <Suona value={quelCheSuona}>
                    <Mondo key={rigioca} shell={shell} scena={scena} contesto={contesto} />
                  </Suona>
                </Volo>
              </Presenze>
            </DatiContesto>,
            doc.body,
          ),
    [doc, dati, quelCheSuona, rigioca, shell, scena, contesto],
  );

  return (
    // `tabIndex={-1}`: la cornice prende il fuoco che esce dalla tela, e
    // solo quello. Vedi `tasto` qui sopra.
    <div className="cornice-tela" ref={cornice} tabIndex={-1}>
      <div
        className="tela"
        style={{ width: larga * scala, height: ALTEZZA * scala }}
      >
        <iframe
          ref={iframe}
          className="finestra-tela"
          title={t("studio.canvas.title")}
          tabIndex={-1}
          style={{ width: larga, height: ALTEZZA, transform: `scale(${scala})` }}
        />
        <div className="contorni" aria-hidden="true">
          {contorni.map((r, i) => (
            <span
              key={i}
              className="contorno"
              data-scelto
              style={{ left: r.x * scala, top: r.y * scala, width: r.w * scala, height: r.h * scala }}
            />
          ))}
          {sonda && sotto !== null && sotto.nome !== scelta && (
            <span
              className="contorno"
              style={{
                left: sotto.riquadro.x * scala,
                top: sotto.riquadro.y * scala,
                width: sotto.riquadro.w * scala,
                height: sotto.riquadro.h * scala,
              }}
            >
              <span className="nome-sonda">{sotto.nome}</span>
            </span>
          )}
        </div>
      </div>
      {portale}
    </div>
  );
}

/**
 * La finestra finta: lo scafale, la pagina della scena, e quel che le sta sopra.
 *
 * Memorizzata: le sue tre prop cambiano solo quando cambia la scena o
 * l'impaginazione della bozza, e tutto il resto della tela — la sonda che
 * segue il puntatore, i contorni, lo zoom — non la riguarda.
 */
const Mondo = memo(function Mondo({
  shell,
  scena,
  contesto,
}: {
  shell: NodoScafale | null;
  scena: Scena;
  contesto: ContestoWidget;
}) {
  const scorrevole = useRef<HTMLDivElement>(null);
  const sopra = new Set(scena.sovrapposizioni);
  const fascia = sopra.has("notifica") ? (
    <Fascia
      notizia={t("studio.canvas.sampleNotice")}
      onChiudiNotizia={niente}
      messaggio={null}
      onChiudiErrore={niente}
    />
  ) : null;
  const { intestazione, corpo } = pagina(scena, contesto, scorrevole);

  return (
    <div className="finestra-app">
      <Impaginazione
        albero={shell}
        contesto={contesto}
        slot={{
          intestazione,
          contenuto: contesto.grande ? (
            <InRiproduzione
              stato={contesto.stato}
              onChiudi={niente}
              onPreferito={niente}
              onVoto={niente}
              onErrore={niente}
              avvisi={fascia}
              testoAperto={scena.pagina === "testo"}
            />
          ) : (
            <div className="dentro page-content" ref={scorrevole}>
              {fascia}
              {corpo}
            </div>
          ),
        }}
      />
      <ControlliFinestra ingrandita={false} />
      {sopra.has("notifica") && (
        <PilaToast scansione={{ fatti: 412, totale: 1464 }} onApriScansione={niente} />
      )}
      {sopra.has("menu") && (
        <Menu
          apertura={{
            x: 420,
            y: 240,
            voci: [
              { etichetta: t("menu.open"), azione: niente },
              { etichetta: t("menu.goToAlbum"), azione: niente },
              { etichetta: t("menu.goToArtist"), azione: niente },
              { etichetta: t("menu.showInFolder"), azione: niente },
            ],
          }}
          onChiudi={niente}
        />
      )}
      {sopra.has("eq") && (
        <PannelloEq attivo guadagni={[3, 2, 1, 0, -1, -1, 0, 1, 2, 3]} onChiudi={niente} onErrore={niente} />
      )}
      {sopra.has("finestrella") && (
        <Chiedi
          titolo={t("dialog.newPlaylist")}
          etichetta={t("rules.name")}
          iniziale={t("studio.canvas.samplePlaylist")}
          conferma={t("addTo.create")}
          onChiudi={niente}
          onRispondi={niente}
        />
      )}
      {sopra.has("giro") && <GiroFermo />}
      {sopra.has("primo") && (
        <Primo
          onCartelle={() => Promise.resolve()}
          onScegliCartella={() => Promise.resolve(null)}
          scansione={null}
          brani={0}
          esito={null}
          lasciate={[]}
          avvisi={null}
          onAscolta={niente}
          onSalta={niente}
        />
      )}
    </div>
  );
});

/** La testata e il corpo della pagina di una scena. */
function pagina(
  scena: Scena,
  contesto: ContestoWidget,
  scorrevole: React.RefObject<HTMLDivElement | null>,
): { intestazione: ReactNode; corpo: ReactNode } {
  const elenco = (righe: typeof BRANI, numeroTraccia = false) => (
    <>
      <ElencoBrani
        righe={[...righe]}
        scorrevole={scorrevole}
        inAscolto={contesto.stato.brano?.id ?? null}
        inPausa={contesto.stato.inPausa}
        suonabile
        selezione={contesto.selezionati.length > 0 ? new Set(contesto.selezionati) : NESSUNA_SELEZIONE}
        onSuona={niente}
        onPreferito={niente}
        onVoto={niente}
        onMenu={niente}
        onSeleziona={niente}
        onAlternaSelezione={niente}
        numeroTraccia={numeroTraccia}
      />
      <Sentinella pagine={NESSUNA_PAGINA} />
    </>
  );
  const testaDi = (vista: "album" | "artisti" | "brani" | "cartelle") => (
    <Intestazione
      {...testaLibreria({
        vista,
        numeri: { albums: ALBUM.length, artists: ARTISTI.length, tracks: BRANI.length, liked: 9, durationMs: 0 },
        cartelle: 1,
        ordinamento: { etichetta: t("sort.shelf"), onApri: niente },
      })}
      query=""
      onQuery={niente}
    />
  );
  switch (scena.pagina) {
    case "home":
      return {
        intestazione: <Intestazione {...testaHome()} query="" onQuery={niente} />,
        corpo: (
          <Home
            casa={CASA}
            settimana={[...SETTIMANA]}
            onSuona={niente}
            onApriRaccolta={niente}
            onRiprendi={niente}
            onMenu={niente}
            onApriAlbum={niente}
            onMenuAlbum={niente}
          />
        ),
      };
    case "album":
      return {
        intestazione: testaDi("album"),
        corpo: <GrigliaAlbum album={ALBUM} sotto="artista" onApri={niente} onMenu={niente} />,
      };
    case "disco": {
      const disco = ALBUM[0]!;
      return {
        intestazione: (
          <Intestazione
            occhiello={t("page.album")}
            titolo={titoloAlbum(disco.title)}
            sottotitolo={`${nomeArtista(disco.artist)} · ${disco.year ?? ""} · ${brani_(disco.totalTracks)}`}
            copertina={<Copertina hash={disco.coverArtHash} titolo={disco.title} classe="hero-art" piena />}
            query=""
            onQuery={niente}
            azioni={
              <>
                <button type="button" className="pillola btn-ghost back-button">
                  <Icona nome="i-chev-l" dim={14} />
                  {t("page.album")}
                </button>
                <button type="button" className="pillola btn-accent">
                  <Icona nome="i-play" dim={14} />
                  {t("action.play")}
                </button>
              </>
            }
          />
        ),
        corpo: elenco(BRANI.filter((b) => b.albumKey === disco.albumKey), true),
      };
    }
    case "brani":
      return { intestazione: testaDi("brani"), corpo: elenco(BRANI) };
    case "artisti":
      return {
        intestazione: testaDi("artisti"),
        corpo: <Artisti artisti={[...ARTISTI]} onApri={niente} onMenu={niente} />,
      };
    case "cartelle":
      return {
        intestazione: testaDi("cartelle"),
        corpo: <Cartelle radici={["D:/Musica"]} scorrevole={scorrevole} onMenu={niente} onErrore={niente} />,
      };
    case "esplora":
      return {
        intestazione: (
          <Intestazione
            occhiello={t("page.explore.eyebrow")}
            titolo={t("page.explore.title")}
            sottotitolo={t("page.explore.sub")}
          />
        ),
        corpo: (
          <Esplora
            stato={ESPLORA}
            onStato={niente}
            onErrore={niente}
            onNotizia={niente}
            onSuona={niente}
            onAccoda={niente}
            onLibreriaCambiata={niente}
          />
        ),
      };
    case "impostazioni":
      return {
        intestazione: (
          <Intestazione titolo={t("page.settings.title")} sottotitolo={t("page.settings.sub")} />
        ),
        corpo: <Impostazioni {...impostazioniFinte()} />,
      };
    case "caricamento":
      return { intestazione: testaDi("album"), corpo: <GrigliaFinta /> };
    case "vuoto":
      return {
        intestazione: testaDi("brani"),
        corpo: (
          <StatoVuoto
            icona="i-scan"
            titolo={t("empty.library.title")}
            testo={t("empty.library.body")}
            azione={{ etichetta: t("empty.library.cta"), fai: niente }}
          />
        ),
      };
    case "schermo":
    case "testo":
      // Il contenuto è `InRiproduzione`, messo da `Mondo` quando `grande`.
      return { intestazione: null, corpo: <ElencoFinto /> };
  }
}

/**
 * Il giro guidato, fermo su un passo.
 *
 * Non è `Giro`: quello guida l'applicazione — cambia pagina, cerca le sue
 * ancore nel documento, avanza sui suoi tasti — e sulla tela non c'è niente da
 * guidare. Qui c'è il suo disegno, con le sue due classi: il velo col buco e
 * il fumetto, dove una skin li ridipinge.
 */
function GiroFermo() {
  return (
    <>
      <div className="giro-velo tour-veil" aria-hidden="true">
        <div className="giro-buco" style={{ top: 64, left: 12, width: 216, height: 250 }} />
      </div>
      <div className="giro-fumetto tour-tooltip" style={{ top: 96, left: 244 }}>
        <div className="giro-testa">
          <h2>{t("tour.step.navigazione.title")}</h2>
          <span className="giro-passi">{t("tour.progress", { n: 2, totale: 10 })}</span>
        </div>
        <p>{t("tour.step.navigazione.body")}</p>
        <div className="giro-tasti">
          <button type="button" className="bottone minuto btn-ghost">
            {t("tour.skip")}
          </button>
          <button type="button" className="bottone">
            {t("tour.back")}
          </button>
          <button type="button" className="bottone primario btn-accent">
            {t("tour.next")}
          </button>
        </div>
      </div>
    </>
  );
}
