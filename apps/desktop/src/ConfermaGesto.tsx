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
import { Dialogo } from "./Dialogo";
import { t } from "./lingue";

/** Quel che serve a chiedere: lo scrive chi apre la domanda. */
export interface Domanda {
  titolo: string;
  /** Che cosa si perde, detto in una frase. */
  descrizione: string;
  /** Il tasto che conferma: il verbo del gesto, non «OK». */
  conferma: string;
  /** Il gesto, quando la risposta è sì. */
  fai: () => void;
}

export function ConfermaGesto({
  domanda,
  onChiudi,
}: {
  domanda: Domanda;
  onChiudi: () => void;
}) {
  return (
    <Dialogo
      titolo={domanda.titolo}
      descrizione={domanda.descrizione}
      ruolo="alertdialog"
      classe="finestrella stretta glass-modal"
      onChiudi={onChiudi}
    >
      <div className="tasti-finestrella">
        <button type="button" className="bottone primario" onClick={onChiudi}>
          {t("common.cancel")}
        </button>
        <button
          type="button"
          className="bottone"
          onClick={() => {
            onChiudi();
            domanda.fai();
          }}
        >
          {domanda.conferma}
        </button>
      </div>
    </Dialogo>
  );
}
