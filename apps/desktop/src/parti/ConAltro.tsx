/**
 * Una scheda col suo ⋮, sul telefono.
 *
 * Sul computer il menù di un disco, di un artista o di un brano della Home è
 * il tasto destro sulla scheda. Sul telefono il tasto destro è il tocco lungo,
 * che funziona (lo manda la WebView, e vibra: `vibra.ts`) ma non si vede:
 * chi non lo prova non sa che c'è. Il ⋮ lo dice, accanto al titolo, e fa la
 * stessa cosa.
 *
 * Un involucro e non un tasto dentro la scheda, perché la scheda è già un
 * `<button>`, e un tasto dentro un tasto non è HTML valido: i lettori di
 * schermo leggerebbero uno solo dei due, e il tocco andrebbe a tutti e due. Il
 * ⋮ sta quindi accanto, nello stesso involucro, e il foglio lo appoggia
 * sull'angolo della scheda.
 *
 * Sul computer non c'è niente: restituisce la scheda così com'è, e il DOM è
 * quello di sempre.
 */
import type { ReactNode } from "react";

import { MOBILE } from "../piattaforma";
import { Icona } from "./Icone";

export function ConAltro({
  etichetta,
  onMenu,
  children,
}: {
  /** Il nome del tasto: «Altro su …», col nome della scheda. */
  etichetta: string;
  onMenu: (e: React.MouseEvent) => void;
  /** La scheda. */
  children: ReactNode;
}) {
  if (!MOBILE) return children;
  return (
    <div className="scheda-guscio">
      {children}
      <button
        type="button"
        className="altro-scheda"
        aria-label={etichetta}
        /* Fuori dalla tabulazione: il fuoco gira fra le schede (`fuoco.ts`),
           e il menù da tastiera è il tasto del menù sulla scheda. */
        tabIndex={-1}
        onClick={onMenu}
      >
        <Icona nome="i-dots-v" dim={18} />
      </button>
    </div>
  );
}
