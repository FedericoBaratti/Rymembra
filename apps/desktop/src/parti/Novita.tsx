/**
 * La finestra delle Novità: due, tre, quattro schermate illustrate, una volta
 * sola dopo un aggiornamento.
 *
 * # Cosa c'è qui e cosa no
 *
 * I passi e i testi sono scritti a mano in `novita.ts` e nei file di lingua. Qui
 * c'è **quando** si apre (`NovitaAllAvvio`), **com'è fatta** (`Novita`) e le
 * scene (`Scena`: poche forme SVG, niente immagini da caricare, tutto in
 * `currentColor` e nei token della skin). Se la versione non ha novità, o le ha
 * già viste, o è un'installazione nuova, non si vede niente: la decisione è del
 * nucleo (`novita_da_mostrare`, la tabella di §11.3.2).
 *
 * # Un Dialogo modale
 *
 * Il fuoco resta dentro, Esc chiude, e il resto della finestra è `inert`
 * (`Dialogo`). `aria-modal` fa il resto: il giro guidato non parte mentre c'è
 * (`unModaleAperto`), e viceversa le Novità non si aprono sopra il giro, il primo
 * avvio o lo Studio. Chiudere in qualunque modo — l'ultimo passo, «Chiudi», Esc —
 * vuol dire «le ho viste»: distinguerli vorrebbe dire riproporle a chi le ha
 * già chiuse una volta.
 *
 * # Come si sfoglia
 *
 * Con i puntini, con Avanti e Indietro, con le frecce della tastiera. Sul
 * telefono anche col dito (`dito.ts`), a schermo intero: a sinistra si va
 * avanti, come si sfogliano le pagine.
 *
 * # Dopo di lei
 *
 * La proposta di partire con Windows (`PropostaSottofondo`) e il giro
 * guidato aspettano che le Novità si siano chiuse: `onInAttesa` dice a `App`
 * quando.
 */
import { useEffect, useRef, useState } from "react";

import { Dialogo } from "../Dialogo";
import { useDito } from "../dito";
import { unModaleAperto } from "../Giro";
import { ipc, type NovitaDaMostrare } from "../ipc";
import { t } from "../lingue";
import {
  VERSIONI_CON_NOVITA,
  comeSiApreNovita,
  mossaDaTasto,
  mossaDelDito,
  novitaDi,
  passoDopo,
  type Illustrazione,
  type Mossa,
  type PassoNovita,
} from "../novita";

/**
 * Le scene: una per tema, nello stesso riquadro di 240×140.
 *
 * Poche forme, tratto a due colori — `currentColor` e l'accento della skin — e
 * niente movimento: una finestra che si legge una volta non ha bisogno di
 * muoversi, e così non c'è niente da spegnere con «Riduci tutto».
 */
function Scena({ nome }: { nome: Illustrazione }) {
  return (
    <svg
      className="novita-scena"
      viewBox="0 0 240 140"
      aria-hidden="true"
      focusable="false"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {nome === "libreria" && (
        <>
          {/* Il telefono e il computer, e in mezzo la libreria che li tiene
              insieme: l'ultima riga è tratteggiata e spenta, il dispositivo che
              non c'è. */}
          <rect x="16" y="36" width="44" height="76" rx="8" />
          <path d="M28 54h20M28 68h20M28 82h20" />
          <rect x="172" y="44" width="54" height="38" rx="4" />
          <path d="M164 94h70M184 58h30M184 68h18" />
          <rect className="accento" x="88" y="30" width="64" height="80" rx="10" />
          <path className="accento" d="M102 52h36M102 68h36" />
          <path d="M102 86h36" strokeDasharray="3 6" opacity="0.4" />
          <path d="M60 74h28M152 74h20" strokeDasharray="2 6" />
        </>
      )}
      {nome === "sottofondo" && (
        <>
          {/* La finestra che si chiude, e l'icona che resta nel vassoio con la
              musica che continua. */}
          <rect x="22" y="22" width="118" height="78" rx="8" strokeDasharray="4 6" opacity="0.55" />
          <path d="M120 34l10 10m0-10l-10 10" opacity="0.55" />
          <rect x="14" y="112" width="212" height="16" rx="6" />
          <rect className="accento" x="186" y="115" width="24" height="10" rx="3" />
          <path className="accento" d="M164 50v26a8 8 0 1 1-4-7V52l22-5v22" />
          <path d="M198 96c4-3 4-9 0-12M206 100c7-6 7-16 0-22" opacity="0.6" />
        </>
      )}
      {nome === "auto" && (
        <>
          {/* Lo schermo dell'auto, con la copertina e il tasto di riproduzione. */}
          <rect x="30" y="24" width="180" height="92" rx="12" />
          <rect className="accento" x="46" y="40" width="60" height="60" rx="8" />
          <path className="accento" d="M62 82V58l24 12-24 12z" />
          <path d="M122 50h70M122 66h50M122 82h60" />
          <path d="M16 126h208" opacity="0.5" />
        </>
      )}
      {nome === "aggiornamenti" && (
        <>
          {/* Una nuvola che scende, la barra che si riempie, e i passi. */}
          <path d="M72 66a22 22 0 0 1 4-43 28 28 0 0 1 52 8 20 20 0 0 1 2 39" />
          <path className="accento" d="M104 48v36m-14-14l14 14 14-14" />
          <rect x="40" y="98" width="160" height="10" rx="5" />
          <rect className="accento" x="40" y="98" width="104" height="10" rx="5" fill="currentColor" />
          <circle cx="48" cy="126" r="4" fill="currentColor" />
          <circle cx="96" cy="126" r="4" fill="currentColor" />
          <circle className="accento" cx="144" cy="126" r="4" fill="currentColor" />
          <circle cx="192" cy="126" r="4" />
        </>
      )}
    </svg>
  );
}

/** La finestra, con i passi di una versione. */
function Novita({
  versione,
  passi,
  onChiudi,
}: {
  versione: string;
  passi: readonly PassoNovita[];
  onChiudi: () => void;
}) {
  const [n, setN] = useState(0);
  const passo = passi[n];
  const ultimo = n === passi.length - 1;
  const vai = (mossa: Mossa) => setN((prima) => passoDopo(prima, passi.length, mossa));

  const corpo = useRef<HTMLDivElement>(null);
  // Col dito, sul telefono: a sinistra avanti, a destra indietro. I tasti e i
  // puntini hanno già il loro mestiere, e non iniziano un gesto.
  const dito = useDito({
    assi: "x",
    ignora: "button",
    onFine: ({ dx, vx }) => {
      const mossa = mossaDelDito({
        dx,
        vx,
        larghezza: corpo.current?.offsetWidth ?? 0,
      });
      if (mossa !== null) vai(mossa);
    },
  });

  if (passo === undefined) return null;
  return (
    <Dialogo
      titolo={t("whatsNew.title", { v: versione })}
      classe="finestrella novita glass-modal"
      onChiudi={onChiudi}
    >
      {/* Le frecce si ascoltano su tutto il contenuto e non solo sulla scena: il
          fuoco comincia sul tasto «Avanti», che sta in fondo. `display: contents`
          (`finestrelle.css`) perché il contenitore non sia una scatola in più. */}
      <div
        className="novita-tastiera"
        onKeyDown={(e) => {
          const mossa = mossaDaTasto(e.key);
          if (mossa === null) return;
          e.preventDefault();
          vai(mossa);
        }}
      >
        <div ref={corpo} className="novita-corpo" {...dito}>
          {/* Un passo nuovo è un elemento nuovo (`key`), che entra con una
              dissolvenza sua: vedi `novita-entra` in `finestrelle.css`. */}
          <div className="novita-passo" key={n}>
            <Scena nome={passo.illustrazione} />
            <h3>{t(passo.titoloKey)}</h3>
            <p>{t(passo.testoKey)}</p>
          </div>
        </div>
        <ol className="novita-puntini" aria-label={t("whatsNew.steps")}>
          {passi.map((_, i) => (
            <li key={i}>
              <button
                type="button"
                className="novita-puntino"
                data-stato={i === n ? "corrente" : i < n ? "fatto" : "dopo"}
                aria-label={t("whatsNew.step", { n: i + 1, tot: passi.length })}
                {...(i === n ? { "aria-current": "step" as const } : {})}
                onClick={() => setN(i)}
              />
            </li>
          ))}
        </ol>
        <div className="tasti-finestrella">
          {!ultimo && (
            <button type="button" className="bottone" onClick={onChiudi}>
              {t("whatsNew.close")}
            </button>
          )}
          {n > 0 && (
            <button type="button" className="bottone" onClick={() => vai("indietro")}>
              {t("whatsNew.back")}
            </button>
          )}
          <button
            type="button"
            className="bottone primario"
            data-fuoco-iniziale
            onClick={() => (ultimo ? onChiudi() : vai("avanti"))}
          >
            {ultimo ? t("whatsNew.close") : t("whatsNew.next")}
          </button>
        </div>
      </div>
    </Dialogo>
  );
}

/**
 * Chiede al nucleo se mostrare le Novità, e le apre quando si può.
 *
 * Chiede **subito**, all'apertura dell'app, anche se poi dovrà aspettare: in
 * un'installazione nuova il nucleo registra la versione nella stessa risposta, e
 * se si aspettasse la fine del primo avvio la libreria sarebbe già piena — cioè
 * quel che distingue un aggiornamento da un'installazione nuova sarebbe
 * sparito. Un guasto nella domanda non si mostra: una finestra di novità che
 * non si riesce a decidere non si apre, e non tiene ferma né la proposta né il
 * giro.
 */
export function NovitaAllAvvio({
  avviato,
  primoVisibile,
  studioVisibile,
  giroAperto,
  onInAttesa,
}: {
  /** L'applicazione ha i suoi dati. */
  avviato: boolean;
  primoVisibile: boolean;
  studioVisibile: boolean;
  giroAperto: boolean;
  /**
   * Le Novità sono ancora da mostrare — non lo sa ancora, o è da aprire, o è
   * aperta — e la proposta del sottofondo e il giro aspettano.
   */
  onInAttesa: (inAttesa: boolean) => void;
}) {
  const [risposta, setRisposta] = useState<NovitaDaMostrare | null>(null);
  /** Le Novità sono state chiuse in questa sessione. */
  const [chiuse, setChiuse] = useState(false);
  const [aperta, setAperta] = useState(false);

  useEffect(() => {
    let vivo = true;
    ipc
      .novitaDaMostrare(VERSIONI_CON_NOVITA)
      .then((letta) => {
        if (!vivo) return;
        setRisposta(letta);
        if (!letta.mostra) onInAttesa(false);
      })
      .catch(() => {
        // Non si decide: non si apre, e niente aspetta.
        if (vivo) onInAttesa(false);
      });
    return () => {
      vivo = false;
    };
    // `onInAttesa` è un `useState` di `App`: stabile, e la domanda si fa una volta.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const daMostrare = risposta?.mostra === true && !chiuse;
  useEffect(() => {
    const come = comeSiApreNovita({
      daMostrare,
      aperta,
      avviato,
      primoVisibile,
      studioVisibile,
      giroAperto,
      modaleAperto: unModaleAperto(),
    });
    if (come === "apri") {
      setAperta(true);
      return;
    }
    if (come !== "aspetta") return;
    // Un altro dialogo aspetta una risposta — la copia del Store che propone la
    // libreria di prima —: si guarda il documento finché non si chiude, come fa
    // il giro. Un modale non è uno stato di questo componente, e la sua
    // chiusura non rifa questo effetto.
    const osserva = new MutationObserver(() => {
      if (unModaleAperto()) return;
      osserva.disconnect();
      setAperta(true);
    });
    osserva.observe(document.body, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ["aria-modal", "role", "hidden"],
    });
    return () => osserva.disconnect();
  }, [daMostrare, aperta, avviato, primoVisibile, studioVisibile, giroAperto]);

  if (!aperta || risposta === null) return null;
  const passi = novitaDi(risposta.versione);
  if (passi === null) return null;

  const chiudi = () => {
    setAperta(false);
    setChiuse(true);
    onInAttesa(false);
    // Una scrittura che non va non si mostra: vuol dire soltanto che le Novità si
    // rivedono al prossimo avvio, come il giro (`giroFatto`).
    ipc.novitaViste(risposta.versione).catch(() => undefined);
  };
  return <Novita versione={risposta.versione} passi={passi} onChiudi={chiudi} />;
}
