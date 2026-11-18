/**
 * La testata della scheda Cerca: il campo, e sotto i due posti in cui cercare.
 *
 * In libreria si cerca fra quel che si ha; in Esplora fra i cataloghi aperti
 * fuori (`schermate/Esplora.tsx`), che ha il suo campo e il suo tasto perché
 * ogni ricerca là costa delle richieste in rete. Due segmenti di una scheda
 * sola, e non due schede: sono la stessa domanda — «dov'è quella canzone» —
 * fatta in due posti.
 *
 * # Lo stesso componente per tre pagine
 *
 * Il campo vuoto (`cerca`), i risultati (`ricerca`) ed Esplora (`esplora`)
 * disegnano **questa** testata, e `testa()` in `App.tsx` la restituisce
 * diretta per tutte e tre. Alla prima lettera la pagina passa da `cerca` a
 * `ricerca`: con un componente diverso nello stesso posto React rifarebbe la
 * testata da capo, e il campo nuovo nascerebbe senza fuoco — chi scrive
 * dovrebbe ritoccarlo dopo ogni prima lettera. Su Esplora il campo della
 * libreria resta montato e nascosto per la stessa ragione.
 */
import type { ReactNode } from "react";

import { t } from "../lingue";
import type { SegmentoCerca } from "../schedeTelefono";
import { Icona } from "./Icone";
import { Segmentato } from "./Segmentato";

export function BarraCerca({
  query,
  onQuery,
  segmento,
  onSegmento,
  sottotitolo,
  sotto,
}: {
  query: string;
  onQuery: (testo: string) => void;
  segmento: SegmentoCerca;
  onSegmento: (segmento: SegmentoCerca) => void;
  /** Quanti risultati, sotto il campo, mentre si cerca. */
  sottotitolo?: string | undefined;
  /**
   * Una riga sotto i due segmenti: il filtro per dispositivo. Dentro la testata
   * e non accanto: la testata è la stessa per le tre pagine di Cerca (vedi il
   * commento in cima), e un'altra fila fuori da lei la rifarebbe da capo.
   */
  sotto?: ReactNode;
}) {
  return (
    <header className="barra-alta barra-cerca" data-grande>
      <div className="riga-barra">
        <div className="titoli-barra">
          <h1 className="titolo-barra">{t("nav.search")}</h1>
        </div>
      </div>
      <div className="campo-cerca" hidden={segmento === "esplora"}>
        <Icona nome="i-search" dim={20} />
        <input
          className="campo-barra"
          type="search"
          /* Il tasto della tastiera dice «Cerca» invece di «Invio»: la
             ricerca parte già mentre si scrive, ma chi lo preme si aspetta che
             la tastiera se ne vada — e se ne va, perché il campo perde il
             fuoco. */
          enterKeyHint="search"
          placeholder={t("search.placeholder")}
          value={query}
          onChange={(e) => onQuery(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") e.currentTarget.blur();
          }}
          spellCheck={false}
          autoCapitalize="none"
          autoCorrect="off"
          /* Lo stesso segno del campo del computer: è come la scorciatoia
             «/», il Giro e la chiusura col tasto Indietro lo trovano. Vedi
             `campoRicerca` in `tastiera.ts`. */
          data-cerca="1"
        />
        {query.length > 0 && (
          <button
            type="button"
            className="tasto-barra svuota-cerca"
            aria-label={t("search.clear")}
            onClick={() => onQuery("")}
          >
            <Icona nome="i-x" dim={18} />
          </button>
        )}
      </div>
      {sottotitolo !== undefined && segmento === "cerca" && (
        <p className="sotto-cerca" aria-live="polite">
          {sottotitolo}
        </p>
      )}
      <Segmentato<SegmentoCerca>
        classe="segmento-cerca"
        etichetta={t("search.segment.aria")}
        voci={[
          { chiave: "cerca", etichetta: t("search.segment.library") },
          { chiave: "esplora", etichetta: t("search.segment.explore") },
        ]}
        scelta={segmento}
        onScegli={onSegmento}
      />
      {sotto}
    </header>
  );
}
