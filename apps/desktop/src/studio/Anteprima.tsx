/**
 * L'anteprima, e la sonda.
 *
 * # Come il foglio arriva qui dentro
 *
 * Il compilatore scrive `:root[data-skin='<id>'] .<parte>`, e `:root` è
 * l'elemento `<html>`: una regola così non può colpire un nodo annidato. Quindi
 * il prefisso si riscrive — `:root[data-skin='x']` diventa `.anteprima[data-skin='x']`
 * — con una sostituzione testuale.
 *
 * Sostituire testo dentro del CSS è di solito una pessima idea. Qui regge per
 * una ragione precisa: **quel CSS lo scrive una macchina**, e il prefisso è
 * esattamente quello che `selettore()` in `compile.rs` produce, carattere per
 * carattere. Non c'è un autore che possa scriverlo diversamente, perché non c'è
 * un autore.
 *
 * # Perché l'anteprima vera sta altrove
 *
 * «L'anteprima è l'applicazione, non un riquadro» — e infatti l'anteprima vera
 * esiste ed è in Impostazioni: passare sopra la scheda di una skin chiama
 * `skin(id)` e ridipinge **la finestra intera**, uscendone la revoca. Quel
 * comando esisteva già nell'IPC e faceva esattamente questo, senza che nessuno
 * lo chiamasse con un argomento.
 *
 * Questa miniatura serve a un'altra cosa: alla sonda. Per sapere come si chiama
 * la superficie che si sta guardando bisogna poterci passare sopra il puntatore
 * **senza** che il passaggio faccia qualcos'altro, e sull'applicazione vera
 * ogni superficie è anche un bersaglio. Qui è ferma, e il puntatore serve solo
 * a chiedere «questa cosa come si chiama».
 *
 * # Cosa c'è dentro
 *
 * Non più un markup finto. Qui dentro va **l'applicazione**, resa dallo stesso
 * `<Impaginazione>` della finestra vera con dati inventati: le cinque scene sono
 * cinque stati del mondo, non cinque disegni. La classe di bug «il finto è
 * andato alla deriva» non ha più un posto in cui succedere, perché non c'è più
 * un finto da tenere allineato — solo dei dati.
 *
 * # La sonda
 *
 * Cammina dall'elemento sotto il puntatore verso l'alto e confronta ogni classe
 * con i nomi del registro. Non servono attributi in più, e il motivo è la regola
 * cinque del markup: `className="semantica parte"`. Il nome della parte **è** una
 * classe, quindi la sonda legge il contratto direttamente dal DOM.
 *
 * Quel che non è una parte non si illumina, ed è il punto: il problema vero di
 * chi scrive una skin non è scegliere un colore, è sapere come si chiama la cosa
 * che sta guardando.
 *
 * Col renderer vero ce n'è una seconda, sullo stesso movimento: ogni posto porta
 * anche il suo `data-nodo`, quindi `closest()` dà l'indirizzo del nodo dello
 * scafale senza una riga di stato in più.
 */
import { useEffect, useMemo, useRef, useState } from "react";

import type { ParteRegistro } from "../ipc";

/**
 * La finestra vera, di cui la miniatura è una riduzione.
 *
 * Serve a due cose. Alla lettura della scala: «1280×820 · 78%» dice che quel
 * che si guarda ha le proporzioni della finestra e non misure inventate, e
 * quindi che una superficie sottile qui è sottile davvero. E al riquadro
 * stesso, che da qui prende le proporzioni da tenere mentre si prende tutto lo
 * spazio che c'è — la misura sta scritta in un posto solo, e il foglio la
 * riceve invece di ripeterla.
 */
export const MISURA = { larghezza: 1280, altezza: 820 } as const;

/** Il selettore che il compilatore emette, e quello che serve qui. */
function perLAnteprima(css: string, id: string): string {
  return css.replaceAll(`:root[data-skin='${id}']`, `.anteprima[data-skin='${id}']`);
}

/** Dove sta, dentro il riquadro, la superficie sotto il puntatore. */
type Riquadro = { x: number; y: number; w: number; h: number };

/**
 * La catena vuota, una volta sola.
 *
 * `setSotto([])` con un array nuovo cambia l'identità dello stato anche quando
 * il contenuto è lo stesso, quindi ridisegna; e siccome la sonda si azzera
 * dentro un effetto, ridisegnare rifà l'effetto. Un array condiviso rompe il
 * giro: `Object.is` lo riconosce e React non ridisegna.
 */
const NESSUNA: string[] = [];

export function Anteprima({
  css,
  id,
  parti,
  sondaAccesa,
  scelta,
  tema,
  densita,
  movimento,
  onScegli,
  onNodo,
  onLarghezza,
  children,
}: {
  css: string;
  id: string;
  /** Il registro: quel che la sonda riconosce, e come si chiama il suo gruppo. */
  parti: ReadonlyMap<string, ParteRegistro>;
  sondaAccesa: boolean;
  scelta: string | null;
  /** Quale variante mostrare. Cambia l'anteprima, non la finestra. */
  tema?: "light" | undefined;
  /**
   * Le due manopole che vivono in un attributo, non nel foglio.
   *
   * `applicaSkin` le scrive sulla radice della finestra; qui vanno sul riquadro,
   * perché le regole che le leggono (`[data-density='…']`, il blocco di
   * `prefers-reduced-motion`) sono scritte sull'attributo nudo e le variabili si
   * ereditano. Senza, si potrebbe girare la densità e non vederla finché la skin
   * non è installata — che è il difetto che tutta questa vista esiste per non
   * avere.
   */
  densita?: string | undefined;
  movimento?: string | undefined;
  onScegli: (parte: string) => void;
  /**
   * L'indirizzo del nodo sotto il puntatore, quando ce n'è uno.
   *
   * È la seconda briciola di pane: `.app-shell › .player-shell` in *Ispeziona*,
   * `riga › colonna › lettore` in *Impagina*. Sono due catene sullo stesso
   * `pointermove`, perché ogni posto porta **sia** la sua classe di parte
   * **sia** il suo `data-nodo`.
   */
  onNodo?: ((indirizzo: string | null) => void) | undefined;
  /**
   * Quanto è larga la miniatura adesso, in pixel, bordo compreso.
   *
   * Non la scala già fatta percentuale: la percentuale è una lettura, e chi
   * disegna la testata ha bisogno della misura vera per finire dove finisce il
   * riquadro. Una percentuale arrotondata sbaglierebbe di qualche pixel, e si
   * vedrebbe — l'uno per cento di 1280 è tredici pixel.
   */
  onLarghezza?: ((pixel: number) => void) | undefined;
  /**
   * Cosa mostrare dentro: l'applicazione, con dati finti.
   */
  children: React.ReactNode;
}) {
  const riquadro = useRef<HTMLDivElement>(null);
  const [sotto, setSotto] = useState<string[]>([]);
  const [dove, setDove] = useState<Riquadro | null>(null);
  /**
   * La classe più interna che **non** è una parte.
   *
   * È l'informazione che prima si perdeva: la catena si fermava e non si sapeva
   * se perché il puntatore era sul bordo o perché quella superficie non è ancora
   * nel registro. Vederla scritta è, come dice il disegno, il modo più rapido
   * per scoprire che una superficie manca.
   */
  const [nonParte, setNonParte] = useState<string | null>(null);

  const foglio = useMemo(() => perLAnteprima(css, id), [css, id]);

  // La catena di parti sotto il puntatore, dalla più esterna alla più interna.
  useEffect(() => {
    const nodo = riquadro.current;
    if (!nodo || !sondaAccesa) {
      setSotto(NESSUNA);
      setDove(null);
      setNonParte(null);
      return;
    }
    const muovi = (e: PointerEvent) => {
      const bersaglio = e.target;
      if (!(bersaglio instanceof Element)) return;
      const catena: string[] = [];
      let interna: Element | null = null;
      for (let q: Element | null = bersaglio; q && q !== nodo; q = q.parentElement) {
        for (const classe of q.classList) {
          if (parti.has(classe)) {
            catena.unshift(classe);
            interna ??= q;
          }
        }
      }
      setSotto(catena);

      // La seconda catena, sullo stesso movimento: `closest()` risale al nodo
      // dello scafale che contiene il puntatore. Le due non si intralciano —
      // una legge le classi, l'altra un attributo — perché ogni posto porta
      // entrambe le cose.
      onNodo?.(bersaglio.closest("[data-nodo]")?.getAttribute("data-nodo") ?? null);

      // Il bersaglio ha delle classi e nessuna è del registro: è una superficie
      // che la skin non può ridipingere, e va detto invece che taciuto.
      const sue = [...bersaglio.classList];
      const riconosciuta = sue.some((c) => parti.has(c));
      setNonParte(riconosciuta ? null : (sue[0] ?? null));

      if (interna === null) {
        setDove(null);
        return;
      }
      const suo = interna.getBoundingClientRect();
      const mio = nodo.getBoundingClientRect();
      setDove({
        x: suo.left - mio.left,
        y: suo.top - mio.top,
        w: suo.width,
        h: suo.height,
      });
    };
    const esci = () => {
      setSotto(NESSUNA);
      setDove(null);
      setNonParte(null);
      onNodo?.(null);
    };
    nodo.addEventListener("pointermove", muovi);
    nodo.addEventListener("pointerleave", esci);
    return () => {
      nodo.removeEventListener("pointermove", muovi);
      nodo.removeEventListener("pointerleave", esci);
    };
  }, [sondaAccesa, parti, onNodo]);

  // Quanto è largo: si misura, non si scrive. Quanto spazio prendersi lo decide
  // il foglio, che è il posto giusto per deciderlo; qui si legge il risultato,
  // e la lettura resta vera da sé a ogni misura della finestra.
  useEffect(() => {
    const nodo = riquadro.current;
    if (!nodo || !onLarghezza) return;
    const osserva = new ResizeObserver(([voce]) => {
      // Il bordo compreso, perché è la larghezza che la testata deve copiare:
      // `contentRect` lascerebbe fuori i due pixel della cornice.
      onLarghezza(voce?.borderBoxSize?.[0]?.inlineSize ?? nodo.getBoundingClientRect().width);
    });
    osserva.observe(nodo);
    return () => osserva.disconnect();
  }, [onLarghezza]);

  const piuInterna = sotto[sotto.length - 1] ?? null;
  const definizione = piuInterna === null ? undefined : parti.get(piuInterna);

  return (
    <div className="anteprima-guscio">
      {/* Il foglio della skin in prova, con i selettori riscritti. */}
      <style>{foglio}</style>

      {/*
       * Lo spazio in cui il riquadro sta.
       *
       * Il riquadro non ha più una misura scritta a mano: si prende quel che
       * c'è qui dentro, tenendo le proporzioni della finestra vera. Le due
       * misure scendono da `MISURA` come variabili invece di stare scritte nel
       * foglio, perché la finestra vera è dichiarata una volta sola — qui
       * sopra — e ripeterla nel CSS sarebbe il modo di farle divergere.
       *
       * Dove questo elemento non serve — l'anteprima ferma della vista
       * Documento, che ha una misura sua — il foglio lo mette a
       * `display: contents` e sparisce dal calcolo.
       */}
      <div
        className="anteprima-spazio"
        style={
          {
            "--misura-l": String(MISURA.larghezza),
            "--misura-a": String(MISURA.altezza),
          } as React.CSSProperties
        }
      >
        <div
          ref={riquadro}
          className="anteprima"
          data-skin={id}
          data-theme={tema}
          data-density={densita}
          data-motion={movimento}
          data-sonda={sondaAccesa || undefined}
          style={{ "--parte-scelta": scelta ?? "" } as React.CSSProperties}
          onClick={() => piuInterna !== null && onScegli(piuInterna)}
        >
          {children}

          {/* Il contorno e l'etichetta stanno **sopra** la scena e non addosso
              all'elemento: un fondo messo sulla parte coprirebbe proprio la cosa
              che si sta guardando, cioè come la skin l'ha dipinta. */}
          {dove !== null && definizione && (
            <div
              className="alone"
              style={{ left: dove.x, top: dove.y, width: dove.w, height: dove.h }}
              aria-hidden="true"
            >
              <span className="targhetta" data-sotto={dove.y < 18 || undefined}>
                <code>.{definizione.name}</code>
                <span className="gruppo">{definizione.group}</span>
                {definizione.layers && <span className="strato">::after</span>}
              </span>
            </div>
          )}
        </div>
      </div>

      <div className="briciole">
        {sotto.length === 0 && nonParte === null ? (
          <span className="niente">
            {sondaAccesa
              ? "Passa sopra una superficie per sapere come si chiama."
              : "Sonda spenta · I"}
          </span>
        ) : (
          <>
            {sotto.map((parte, i) => (
              <span key={`${i}-${parte}`} className="briciola">
                {i > 0 && <span className="freccia">›</span>}
                <code>.{parte}</code>
              </span>
            ))}
            {nonParte !== null && (
              <span className="briciola mancante">
                {sotto.length > 0 && <span className="freccia">›</span>}
                <code>.{nonParte}</code>
                <span className="richiesta">richiesta al registro</span>
              </span>
            )}
          </>
        )}
      </div>
    </div>
  );
}
