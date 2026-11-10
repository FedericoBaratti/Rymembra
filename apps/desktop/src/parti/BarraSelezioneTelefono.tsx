/**
 * La barra della selezione sul telefono.
 *
 * Sul computer è una striscia di parole (`BarraSelezione.tsx`): quanti,
 * «Seleziona tutto», «Riproduci», «Riproduci dopo», «In coda», «Aggiungi a
 * playlist», la ×. Su uno schermo da quattrocento pixel quelle parole non
 * stanno in fila. Qui è una scheda in due file, nel posto del mini lettore:
 *
 * - sopra, la × che chiude, quanti sono, e «Seleziona tutto»;
 * - sotto, cinque tasti con l'icona sopra e una parola corta sotto: riproduci,
 *   dopo, in coda, playlist, e ⋮ per il resto del menù (la radio, i dischi,
 *   togliere), che sul computer sta nel tasto destro.
 *
 * # Perché tutto in basso, e niente in cima
 *
 * Molte app di Android mettono quanti e la × in una barra in cima, al posto
 * della testata. Qui la testata è di altezze diverse, con il titolo grande e
 * le pillole della Libreria, e sostituirla con una barra più bassa faceva
 * salire l'elenco intero di un dito e mezzo nel momento del tocco lungo: la
 * riga appena scelta scappava da sotto il dito, e il tocco dopo cadeva su
 * quella sbagliata. In basso la pagina sopra non si muove, e tutti i comandi
 * della selezione stanno dove arriva il pollice.
 *
 * Prende il posto del mini lettore per la stessa ragione detta sul computer:
 * chi ha appena scelto quaranta brani non sta cercando il tasto pausa.
 */
import { memo } from "react";

import { brani_ } from "../formato";
import { gestoPermesso } from "../insieme";
import { t, useLingua } from "../lingue";
import { useLettore } from "../riproduzione";
import { Icona, type NomeIcona } from "./Icone";

/** Un tasto della barra: l'icona sopra, la parola sotto. */
function Azione({
  icona,
  etichetta,
  spenta,
  onClick,
}: {
  icona: NomeIcona;
  etichetta: string;
  spenta?: boolean | undefined;
  onClick: (e: React.MouseEvent<HTMLButtonElement>) => void;
}) {
  return (
    <button type="button" className="azione-selezione" disabled={spenta} onClick={onClick}>
      <Icona nome={icona} dim={22} />
      <span className="etichetta-azione">{etichetta}</span>
    </button>
  );
}

export const BarraSelezioneTelefono = memo(function BarraSelezioneTelefono({
  quanti,
  tuttiSelezionati,
  onTuttiOAnnulla,
  onChiudi,
  onRiproduci,
  onDopo,
  onAccoda,
  onPlaylist,
  onAltro,
}: {
  quanti: number;
  /** È già scelto tutto l'elenco: il tasto dice «Deseleziona tutto». */
  tuttiSelezionati: boolean;
  onTuttiOAnnulla: () => void;
  onChiudi: () => void;
  onRiproduci: () => void;
  onDopo: () => void;
  onAccoda: () => void;
  onPlaylist: () => void;
  /** Il menù di tutti i brani scelti. */
  onAltro: (e: React.MouseEvent) => void;
}) {
  // Sotto `memo`: vedi «I widget sotto `memo`» in `Impaginazione.tsx`.
  useLingua();
  // Come sul computer: in una sessione in cui chi ospita non ha dato la coda
  // a questo telefono, i tre gesti che la toccano si spengono.
  const codaLibera = useLettore((s) => gestoPermesso(s.insieme, "coda"));
  return (
    <div className="barra-selezione-telefono" role="toolbar" aria-label={t("selection.aria")}>
      <div className="testa-selezione">
        <button
          type="button"
          className="chiudi-selezione"
          aria-label={t("selection.clear")}
          onClick={onChiudi}
        >
          <Icona nome="i-x" dim={20} />
        </button>
        <span className="quanti-selezione">{brani_(quanti)}</span>
        <button type="button" className="tutti-selezione" onClick={onTuttiOAnnulla}>
          {tuttiSelezionati ? t("selection.deselectAll") : t("selection.selectAll")}
        </button>
      </div>
      <div className="azioni-selezione">
        <Azione
          icona="i-play"
          etichetta={t("selection.bar.play")}
          spenta={!codaLibera}
          onClick={onRiproduci}
        />
        <Azione
          icona="i-dopo"
          etichetta={t("selection.bar.next")}
          spenta={!codaLibera}
          onClick={onDopo}
        />
        <Azione
          icona="i-queue"
          etichetta={t("selection.bar.queue")}
          spenta={!codaLibera}
          onClick={onAccoda}
        />
        <Azione icona="i-plus" etichetta={t("selection.bar.playlist")} onClick={onPlaylist} />
        <Azione icona="i-dots-v" etichetta={t("selection.bar.more")} onClick={onAltro} />
      </div>
    </div>
  );
});
