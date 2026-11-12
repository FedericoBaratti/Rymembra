/**
 * «Torna alla copia di prima dell'aggiornamento», nella scheda della
 * manutenzione (Impostazioni › Libreria e dati).
 *
 * # Perché un gesto e non le istruzioni
 *
 * Prima di ogni migrazione Rymembra copia la libreria intera accanto al
 * database (`rymembra.db.schema-NNN.bak`), e la strada per tornarci era scritta
 * nei documenti: chiudere, rinominare, rimettere. Seguita alla lettera rovinava
 * la copia: all'uscita restano `rymembra.db-wal` e `-shm` del database nuovo, e
 * SQLite li applica a qualunque file porti quel nome. Il nucleo la rimette con
 * SQLite stesso (l'API di backup), dopo aver messo da parte la libreria di
 * adesso, e poi Rymembra si chiude: la copia rimessa non deve essere
 * riaggiornata da questa versione prima che chi l'ha chiesta installi quella di
 * prima.
 *
 * # Perché si disegna anche senza libreria
 *
 * Perché è il caso in cui serve di più: una migrazione fermata a metà lascia
 * la libreria chiusa, e la frase d'errore (`errors.db.migrationFailed`) manda
 * qui. La copia si chiede a un comando che non apre il database, e la scheda
 * che la ospita si disegna anche senza `avvio`.
 *
 * Sul telefono non c'è: Android non installa una versione più vecchia sopra
 * una più nuova senza disinstallare, e disinstallare porta via libreria e copia
 * insieme. Chi la monta lo sa (`MOBILE`).
 */
import { useEffect, useState } from "react";

import { ConfermaGesto, type Domanda } from "../ConfermaGesto";
import { dataOraBreve } from "../formato";
import { ipc, type CopiaDiPrima as Copia } from "../ipc";
import { t } from "../lingue";

export function CopiaDiPrima({ onErrore }: { onErrore: (e: unknown) => void }) {
  const [copia, setCopia] = useState<Copia | null>(null);
  const [domanda, setDomanda] = useState<Domanda | null>(null);
  const [inCorso, setInCorso] = useState(false);

  useEffect(() => {
    let vivo = true;
    ipc
      .manutenzioneCopiaDiPrima()
      .then((letta) => {
        if (vivo) setCopia(letta);
      })
      // Non sapere se c'è una copia non merita la fascia rossa: la scheda
      // semplicemente non offre il gesto.
      .catch(() => undefined);
    return () => {
      vivo = false;
    };
  }, []);

  if (copia === null) return null;
  const quando = dataOraBreve(copia.quandoMs);
  const chiedi = () =>
    setDomanda({
      titolo: t("settings.restoreCopy.title"),
      descrizione: [
        t("settings.restoreCopy.confirm", { messa: copia.messaDaParte, quando }),
        copia.versione < copia.versioneDiQuesta
          ? t("settings.restoreCopy.confirm.older")
          : "",
      ]
        .filter((parte) => parte !== "")
        .join(" "),
      conferma: t("settings.restoreCopy.do"),
      fai: () => {
        setInCorso(true);
        // Se riesce, Rymembra si chiude da sé: non c'è un «dopo» da disegnare.
        ipc
          .manutenzioneTornaAllaCopia()
          .catch(onErrore)
          .finally(() => setInCorso(false));
      },
    });

  return (
    <>
      <p className="nota">
        {t("settings.restoreCopy.what", { quando, nome: copia.nome })}
      </p>
      <div className="azioni">
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={inCorso}
          onClick={chiedi}
        >
          {t("settings.restoreCopy.button")}
        </button>
      </div>
      {domanda !== null && (
        <ConfermaGesto domanda={domanda} onChiudi={() => setDomanda(null)} />
      )}
    </>
  );
}
