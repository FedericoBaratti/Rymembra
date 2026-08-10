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
 */

/** Una linguetta. */
export type Voce<T extends string> = {
  /** Il valore che sceglie. */
  chiave: T;
  /** Cosa si legge. */
  etichetta: string;
  /** Un numero accanto all'etichetta, in mono. */
  conteggio?: number | undefined;
  /** Spenta, con la sua ragione nel titolo. */
  spenta?: string | undefined;
};

export function Segmentato<T extends string>({
  voci,
  scelta,
  onScegli,
  etichetta,
  classe,
}: {
  voci: readonly Voce<T>[];
  scelta: T;
  onScegli: (chiave: T) => void;
  /** Cosa sceglie questo gruppo, per chi non vede le linguette. */
  etichetta: string;
  /** Una classe in più, per le varianti di misura. */
  classe?: string | undefined;
}) {
  /**
   * Le frecce girano dentro il gruppo.
   *
   * `roving tabindex`: una sola linguetta è raggiungibile col tabulatore — quella
   * scelta — e dentro il gruppo ci si muove con le frecce. Senza, un segmentato
   * da cinque voci costerebbe cinque fermate di Tab per attraversarlo.
   */
  const daTastiera = (e: React.KeyboardEvent, indice: number) => {
    const passo = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    if (passo === 0) return;
    e.preventDefault();
    const quante = voci.length;
    // Il modulo con l'addizione: in JavaScript `-1 % 3` fa `-1`, non `2`.
    let prossimo = (indice + passo + quante) % quante;
    // Una linguetta spenta si scavalca invece di fermare il giro.
    for (let giri = 0; giri < quante && voci[prossimo]?.spenta; giri += 1) {
      prossimo = (prossimo + passo + quante) % quante;
    }
    const voce = voci[prossimo];
    if (voce && !voce.spenta) onScegli(voce.chiave);
  };

  return (
    <div
      className={classe ? `segmentato segmented ${classe}` : "segmentato segmented"}
      role="radiogroup"
      aria-label={etichetta}
    >
      {voci.map((voce, indice) => {
        const attiva = voce.chiave === scelta;
        return (
          <button
            key={voce.chiave}
            type="button"
            className="linguetta"
            role="radio"
            aria-checked={attiva}
            data-active={attiva || undefined}
            tabIndex={attiva ? 0 : -1}
            disabled={voce.spenta !== undefined}
            title={voce.spenta}
            onClick={() => onScegli(voce.chiave)}
            onKeyDown={(e) => daTastiera(e, indice)}
          >
            {voce.etichetta}
            {voce.conteggio !== undefined && (
              <span className="quante">{voce.conteggio}</span>
            )}
          </button>
        );
      })}
    </div>
  );
}
