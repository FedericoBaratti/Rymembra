/**
 * Le cinque stelle di un brano.
 *
 * # Perché cliccare la stella già accesa azzera
 *
 * Senza, non ci sarebbe modo di togliere un voto: le stelle vanno da uno a
 * cinque e lo zero — «non l'ho votato» — non ha una stella sua. Dare al clic
 * sul valore corrente il significato di «toglilo» è la convenzione che tutti i
 * lettori usano, ed è l'unica che non richiede un sesto bersaglio.
 *
 * # Perché due disegni e non uno solo colorato
 *
 * `★` e `☆` erano due glifi di caratteri diversi su macchine diverse, e a
 * seconda del carattere di sistema la stella piena e quella vuota avevano
 * larghezze diverse: cambiare voto faceva saltare la riga di qualche pixel. Le
 * due icone hanno lo stesso `viewBox`, quindi la fila non si muove più.
 */
import { Icona } from "./parti/Icone";

const STELLE = [1, 2, 3, 4, 5] as const;

export function Stelle({
  valore,
  onVoto,
}: {
  valore: number;
  onVoto: (stelle: number) => void;
}) {
  return (
    <div
      className="stelle"
      role="group"
      aria-label={valore > 0 ? `Valutazione: ${valore} su 5` : "Non valutato"}
    >
      {STELLE.map((n) => (
        <button
          key={n}
          type="button"
          className="stella"
          aria-pressed={n <= valore}
          aria-label={n === valore ? "Togli la valutazione" : `${n} stelle`}
          onClick={() => onVoto(n === valore ? 0 : n)}
        >
          <Icona nome={n <= valore ? "i-star" : "i-star-o"} dim={13} />
        </button>
      ))}
    </div>
  );
}
