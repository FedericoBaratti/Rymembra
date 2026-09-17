/**
 * Il gruppo di linguette: due o tre viste dello stesso contenuto.
 *
 * # Perché non è una voce di navigazione
 *
 * Sembra un `nav-pill` e non lo è, ed è il motivo per cui `segmented` è una
 * parte a sé nel registro. Un `nav-pill` porta **altrove**: cambia la pagina,
 * e la pagina di prima smette di esistere. Una linguetta cambia il **taglio**
 * dello stesso contenuto — coda o cronologia, chiaro o scuro, token o parti — e
 * quel che c'era intorno resta dov'è.
 *
 * Sono due cose diverse anche per chi le disegna: chi fa una skin vuole poter
 * dare alla navigazione il peso di una navigazione e alle linguette quello di un
 * interruttore, e con una classe sola dovrebbe scegliere quale delle due
 * sacrificare.
 *
 * # Perché `radiogroup` e non un gruppo di bottoni
 *
 * Sono opzioni che si escludono: uno screen reader deve poter dire «2 di 3», e
 * le frecce devono muoversi fra le linguette senza uscire dal gruppo. Un elenco
 * di `<button>` non lo comunica, e la differenza si sente solo quando non si
 * guarda lo schermo.
 *
 * # Il fuoco si muove, non solo la scelta
 *
 * Per un po' le frecce cambiavano la scelta e **lasciavano il fuoco dov'era**.
 * Il difetto non era estetico: la linguetta appena accesa nasce con
 * `tabIndex={-1}` e quella di prima tiene l'anello, quindi la freccia successiva
 * partiva di nuovo dalla stessa posizione e il gruppo avanzava di un passo e si
 * bloccava. Adesso ogni linguetta tiene il suo riferimento e il giro la
 * focalizza: il fuoco *è* il cursore del gruppo, e la scelta lo segue.
 *
 * Da qui viene anche la separazione fra le due cose. `fuoco` è la linguetta su
 * cui si è, `scelta` quella accesa, e per quasi tutto il tempo coincidono; non
 * coincidono quando il giro passa su una linguetta spenta, che si può
 * raggiungere ma non accendere. Quando il gruppo perde il fuoco, `fuoco` torna
 * a essere la scelta: l'unica fermata di tabulazione che il gruppo offre deve
 * essere quella accesa, altrimenti rientrando con Tab si tornerebbe su una
 * linguetta qualunque.
 *
 * # Perché le spente sono `aria-disabled` e non `disabled`
 *
 * Perché la ragione per cui una linguetta è spenta è la sola cosa che chi la
 * guarda vuole sapere, e un `<button disabled>` non prende il fuoco: il
 * `title` che la spiegava non compariva mai per chi naviga da tastiera, cioè
 * proprio per chi non può provare a premerla per scoprirlo. Con
 * `aria-disabled` la linguetta resta raggiungibile, il guscio `.con-ragione`
 * accende la pastiglia al fuoco come al passaggio, e `aria-describedby` la
 * fa leggere. Il prezzo è visibile e voluto: una linguetta spenta adesso
 * prende l'anello di fuoco.
 */
import { Fragment, useId, useRef, useState } from "react";

/** Una linguetta. */
export type Voce<T extends string> = {
  /** Il valore che sceglie. */
  chiave: T;
  /** Cosa si legge. */
  etichetta: string;
  /** Un numero accanto all'etichetta, in mono. */
  conteggio?: number | undefined;
  /** Spenta, con la sua ragione nella pastiglia. */
  spenta?: string | undefined;
};

export function Segmentato<T extends string>({
  voci,
  scelta,
  onScegli,
  etichetta,
  classe,
  verticale = false,
}: {
  voci: readonly Voce<T>[];
  scelta: T;
  onScegli: (chiave: T) => void;
  /** Cosa sceglie questo gruppo, per chi non vede le linguette. */
  etichetta: string;
  /** Una classe in più, per le varianti di misura. */
  classe?: string | undefined;
  /**
   * Le linguette sono impilate invece che in fila.
   *
   * Serve solo a `aria-orientation`, che deve dire la verità: le frecce
   * funzionano su entrambi gli assi in ogni caso — è il rimedio al difetto per
   * cui un segmentato impilato rispondeva alle sole ←→ — ma annunciare
   * «orizzontale» a chi vede una colonna gli fa cercare i tasti sbagliati.
   * Di serie è falso perché oggi nel foglio ogni segmentato è una fila; chi lo
   * impila lo dichiara qui.
   */
  verticale?: boolean | undefined;
}) {
  const linguette = useRef<(HTMLButtonElement | null)[]>([]);
  const idBase = useId();
  /**
   * Dove si è, quando si è dentro.
   *
   * `null` vuol dire «fuori»: allora la fermata di tabulazione è la linguetta
   * accesa. Tenerlo come indice e non come riferimento al nodo serve al caso in
   * cui le voci cambino sotto il fuoco — il conteggio della coda che si svuota,
   * una linguetta che si spegne — senza lasciare un riferimento appeso a un
   * nodo smontato.
   */
  const [fuoco, setFuoco] = useState<number | null>(null);
  const accesa = voci.findIndex((voce) => voce.chiave === scelta);
  const raggiungibile = fuoco ?? (accesa === -1 ? 0 : accesa);

  /** Va su una linguetta: la focalizza, e la accende se si può. */
  const vaiA = (quale: number) => {
    const voce = voci[quale];
    if (!voce) return;
    setFuoco(quale);
    // Il fuoco **prima** della scelta: `onScegli` ridisegna il gruppo, e
    // focalizzare dopo vorrebbe dire cercare un nodo che in quell'istante React
    // ha appena sostituito.
    linguette.current[quale]?.focus();
    if (voce.spenta === undefined) onScegli(voce.chiave);
  };

  /**
   * Le frecce girano dentro il gruppo.
   *
   * `roving tabindex`: una sola linguetta è raggiungibile col tabulatore —
   * quella accesa, o quella su cui si è — e dentro il gruppo ci si muove con le
   * frecce. Senza, un segmentato da cinque voci costerebbe cinque fermate di
   * Tab per attraversarlo.
   *
   * Entrambi gli assi, e non per abbondanza: `role="radiogroup"` non dice da
   * che parte stanno le linguette, e un segmentato impilato — una riga di
   * proprietà stretta, una colonna dell'Ispettore — rispondeva alle sole ←→,
   * cioè ai due tasti che in una colonna non vuol premere nessuno. Home ed End
   * saltano ai capi, che su cinque voci è il gesto che evita quattro frecce.
   *
   * Le spente **non si scavalcano** più. Prima il giro le saltava, ed era
   * coerente con `disabled`; adesso che la loro ragione si legge solo
   * arrivandoci sopra, saltarle vorrebbe dire nasconderla di nuovo.
   */
  const daTastiera = (e: React.KeyboardEvent, indice: number) => {
    const quante = voci.length;
    const passo =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    let prossimo: number;
    // Il modulo con l'addizione: in JavaScript `-1 % 3` fa `-1`, non `2`.
    if (passo !== 0) prossimo = (indice + passo + quante) % quante;
    else if (e.key === "Home") prossimo = 0;
    else if (e.key === "End") prossimo = quante - 1;
    else return;
    // Fermato, non solo impedito: ←→ sono anche «avanti/indietro di cinque
    // secondi» per le scorciatoie appese a `window`, e scorrere le linguette
    // del tema spostava il brano. È la regola di `fuoco.ts`: il tasto lo
    // consuma chi ha il fuoco.
    e.preventDefault();
    e.stopPropagation();
    vaiA(prossimo);
  };

  return (
    <div
      className={classe ? `segmentato segmented ${classe}` : "segmentato segmented"}
      role="radiogroup"
      aria-label={etichetta}
      aria-orientation={verticale ? "vertical" : "horizontal"}
      /* Uscendo dal gruppo la fermata di tabulazione torna a essere la linguetta
         accesa. `relatedTarget` nullo vuol dire che il fuoco è andato fuori
         dalla finestra: anche quello è «fuori». */
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) setFuoco(null);
      }}
    >
      {voci.map((voce, indice) => {
        const attiva = voce.chiave === scelta;
        const idRagione = `${idBase}-${indice}`;
        const linguetta = (
          <button
            ref={(nodo) => {
              linguette.current[indice] = nodo;
            }}
            type="button"
            className="linguetta"
            role="radio"
            aria-checked={attiva}
            data-active={attiva || undefined}
            tabIndex={indice === raggiungibile ? 0 : -1}
            /* Spenta ma raggiungibile: vedi la nota del modulo. */
            aria-disabled={voce.spenta !== undefined || undefined}
            aria-describedby={voce.spenta !== undefined ? idRagione : undefined}
            onFocus={() => setFuoco(indice)}
            onClick={() => {
              if (voce.spenta === undefined) onScegli(voce.chiave);
            }}
            onKeyDown={(e) => daTastiera(e, indice)}
          >
            {voce.etichetta}
            {voce.conteggio !== undefined && (
              <span className="quante">{voce.conteggio}</span>
            )}
          </button>
        );
        return (
          <Fragment key={voce.chiave}>
            {voce.spenta === undefined ? (
              linguetta
            ) : (
              /* Il guscio esiste per la pastiglia: le dà l'antenato posizionato
                 da cui pendere e i due gesti — passaggio e fuoco — che la
                 alzano. Le regole stanno nel foglio accanto a `.tooltip-pill`. */
              <span className="con-ragione">
                {linguetta}
                <span className="tooltip-pill" id={idRagione} role="tooltip">
                  {voce.spenta}
                </span>
              </span>
            )}
          </Fragment>
        );
      })}
    </div>
  );
}
