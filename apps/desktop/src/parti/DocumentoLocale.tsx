/**
 * Un documento pubblico letto dalla copia che viaggia con l'app: senza rete,
 * nella finestra (D-10d). Vedi `documentoLocale.ts`.
 *
 * Testo semplice e non Markdown impaginato: è lo stesso file che sta nel
 * repository, e un `<pre>` che va a capo lo legge chiunque — gli avvisi delle
 * terze parti sono più di un mebibyte di elenchi, e impaginarli costerebbe una
 * dipendenza per un testo che si scorre e si copia. La versione impaginata, e
 * più nuova, è a un tasto.
 */
import { Dialogo } from "../Dialogo";
import { t } from "../lingue";

export function DocumentoLocale({
  titolo,
  testo,
  onSito,
  onChiudi,
}: {
  titolo: string;
  testo: string;
  /** Apre la versione sul sito, nel browser. */
  onSito: () => void;
  onChiudi: () => void;
}) {
  return (
    <Dialogo titolo={titolo} classe="finestrella larga documento-locale glass-modal" onChiudi={onChiudi}>
      {/* `tabIndex` 0: si scorre anche con la tastiera, come una pagina. */}
      <pre className="testo-documento" tabIndex={0}>
        {testo}
      </pre>
      <div className="tasti-finestrella">
        <button type="button" className="bottone" onClick={onSito}>
          {t("settings.docs.online")}
        </button>
        <button type="button" className="bottone primario" onClick={onChiudi} data-fuoco-iniziale>
          {t("common.close")}
        </button>
      </div>
    </Dialogo>
  );
}
