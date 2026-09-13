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
 *
 * # Perché `raggiungibile`
 *
 * Perché cinque bottoni sono cinque fermate di tabulazione, e dentro un elenco di
 * 18.534 righe sono 92.670. Là la fila di stelle non è una fermata: si arriva
 * alla riga con Tab e alle stelle con ←→ (vedi `fuoco.ts`), quindi le cinque
 * passano a `tabIndex={-1}` — raggiungibili col fuoco programmatico, non con
 * Tab. Fuori dall'elenco — la fascia del lettore, la terza colonna — la fila è
 * un comando come gli altri e la fermata ce l'ha: per questo il valore di serie è
 * `true`, cioè il comportamento di prima per chi non dice niente.
 */
import { Icona } from "./parti/Icone";
import { t } from "./lingue";

const STELLE = [1, 2, 3, 4, 5] as const;

export function Stelle({
  valore,
  onVoto,
  raggiungibile = true,
}: {
  valore: number;
  onVoto: (stelle: number) => void;
  /** Le cinque stelle sono fermate di tabulazione. Vedi la nota del modulo. */
  raggiungibile?: boolean;
}) {
  return (
    <div
      className="stelle"
      role="group"
      aria-label={
        valore > 0 ? t("rating.label", { n: valore }) : t("rating.none")
      }
    >
      {STELLE.map((n) => (
        <button
          key={n}
          type="button"
          className="stella"
          tabIndex={raggiungibile ? undefined : -1}
          aria-pressed={n <= valore}
          aria-label={
            n === valore ? t("rating.clear") : t("rating.stars", { n })
          }
          onClick={() => onVoto(n === valore ? 0 : n)}
        >
          <Icona nome={n <= valore ? "i-star" : "i-star-o"} dim={13} />
        </button>
      ))}
    </div>
  );
}
