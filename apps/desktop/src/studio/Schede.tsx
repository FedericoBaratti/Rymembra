/**
 * Le linguette di un pannello dello Studio.
 *
 * `role="tablist"` con le frecce che girano fra le linguette e un solo punto
 * di tabulazione, come `Segmentato`; a differenza di quello, una linguetta qui
 * cambia il pannello sotto, non un valore — ed è per questo che è un'altra
 * cosa.
 */
import { useRef, type KeyboardEvent, type ReactNode } from "react";

export function Schede<K extends string>({
  voci,
  scelta,
  onScegli,
  etichetta,
  coda,
}: {
  voci: readonly (readonly [K, string, number?])[];
  scelta: K | null;
  onScegli: (chiave: K) => void;
  etichetta: string;
  /** Quel che sta in fondo alla fila, a destra: un tasto, un conto. */
  coda?: ReactNode;
}) {
  const linguette = useRef<(HTMLButtonElement | null)[]>([]);
  const accesa = Math.max(
    0,
    voci.findIndex(([chiave]) => chiave === scelta),
  );
  const daTastiera = (e: KeyboardEvent, indice: number) => {
    const passo = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
    const salto = e.key === "Home" ? 0 : e.key === "End" ? voci.length - 1 : null;
    if (passo === 0 && salto === null) return;
    e.preventDefault();
    const prossima = salto ?? (indice + passo + voci.length) % voci.length;
    const voce = voci[prossima];
    if (voce === undefined) return;
    linguette.current[prossima]?.focus();
    onScegli(voce[0]);
  };
  return (
    <div className="schede-studio" role="tablist" aria-label={etichetta}>
      {voci.map(([chiave, nome, conto], i) => (
        <button
          key={chiave}
          ref={(n) => {
            linguette.current[i] = n;
          }}
          type="button"
          role="tab"
          className="scheda-studio"
          aria-selected={chiave === scelta}
          tabIndex={i === accesa ? 0 : -1}
          onKeyDown={(e) => daTastiera(e, i)}
          onClick={() => onScegli(chiave)}
        >
          {nome}
          {conto !== undefined && conto > 0 && <span className="conto-scheda">{conto}</span>}
        </button>
      ))}
      {coda}
    </div>
  );
}
