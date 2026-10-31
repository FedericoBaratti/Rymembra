/**
 * «Sicuro?», per i gesti che tolgono qualcosa e non hanno un «Annulla».
 *
 * # Perché una finestrella e non un «Annulla»
 *
 * Quel che si toglie da qui non torna con un clic: una skin disinstallata è un
 * file cancellato, un profilo dell'IA porta con sé la sua chiave nel
 * portachiavi, un account scollegato si ricollega solo ripassando dal consenso
 * di Google. Un «Annulla» di dieci secondi, come quello delle playlist
 * (`App.tsx`), vorrebbe un gettone del nucleo che rimetta le cose com'erano, e
 * per questi gesti il nucleo non ce l'ha. Resta la domanda prima.
 *
 * # Perché una sola, per gesti diversi
 *
 * Le altre conferme (`ConfermaEliminazione`, `ConfermaRimozioni`,
 * `ConfermaTrasloco`, `ConfermaSkin`) hanno un file ciascuna perché ognuna dice
 * qualcosa di suo — quanti brani stanno in rete, quali righe sparirebbero.
 * Queste cinque dicono tutte la stessa cosa con parole diverse: che cosa si
 * perde, e il tasto che lo fa. Cinque file uguali sarebbero cinque occasioni di
 * correggerne uno solo.
 *
 * Come nelle altre, il primario è «Annulla»: sopra un gesto distruttivo il
 * colore che invita a premere sta sul tasto che non toglie niente, ed è anche
 * quello che riceve il fuoco aprendo.
 */
import { useState } from "react";

import { Dialogo } from "./Dialogo";
import { t } from "./lingue";

/** Quel che serve a chiedere: lo scrive chi apre la domanda. */
export interface Domanda {
  titolo: string;
  /** Che cosa si perde, detto in una frase. */
  descrizione: string;
  /** Il tasto che conferma: il verbo del gesto, non «OK». */
  conferma: string;
  /**
   * Una scelta in più, dentro la stessa domanda: la frase della casella.
   *
   * Nasce per «Scollega» di un account del cloud, che può portarsi via anche
   * le sue cartelle o lasciarle in libreria ad aspettare. Una casella e non
   * due tasti di conferma: la domanda resta una sola — scollegare sì o no — e
   * la parte più grave è quella che si sceglie apposta. Per questo parte
   * **spenta**: il caso di serie è quello che non toglie niente in più.
   */
  casella?: string | undefined;
  /** Il gesto, quando la risposta è sì: con la casella, se c'era. */
  fai: (spuntata: boolean) => void;
}

export function ConfermaGesto({
  domanda,
  onChiudi,
}: {
  domanda: Domanda;
  onChiudi: () => void;
}) {
  const [spuntata, setSpuntata] = useState(false);
  return (
    <Dialogo
      titolo={domanda.titolo}
      descrizione={domanda.descrizione}
      ruolo="alertdialog"
      classe="finestrella stretta glass-modal"
      onChiudi={onChiudi}
    >
      {domanda.casella !== undefined && (
        <label className="scelta">
          <input
            type="checkbox"
            checked={spuntata}
            onChange={(e) => setSpuntata(e.target.checked)}
          />
          <span>{domanda.casella}</span>
        </label>
      )}
      <div className="tasti-finestrella">
        {/* Il marchio e non l'ordine: con la casella il primo focalizzabile
            sarebbe lei, e uno Spazio battuto per riflesso spunterebbe proprio
            la parte più grave del gesto. Vedi `fuocoIniziale` in `modale.ts`. */}
        <button
          type="button"
          className="bottone primario"
          onClick={onChiudi}
          data-fuoco-iniziale
        >
          {t("common.cancel")}
        </button>
        <button
          type="button"
          className="bottone"
          onClick={() => {
            onChiudi();
            domanda.fai(spuntata);
          }}
        >
          {domanda.conferma}
        </button>
      </div>
    </Dialogo>
  );
}
