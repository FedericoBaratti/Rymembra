/**
 * Il rapporto di un'importazione, riaperto.
 *
 * # Cosa si può riaprire, e perché conta
 *
 * `missingTracks` è la voce che pesa: sono brani che l'utente ha sul servizio e
 * non su questo disco, e sono **l'unica cosa che non può ricostruire dopo**.
 * Finché il rapporto moriva con la finestrella, chi chiudeva senza copiarli li
 * perdeva — e la finestrella si chiude da sé alla conferma, perché il gesto da
 * rendere facile era il link successivo.
 *
 * I quattro contatori per gradino si rileggono con lo stesso vocabolario della
 * scomposizione dell'anteprima (`PastigliaGradino`): il rapporto è quell'elenco
 * a cose fatte, e se parlasse una lingua sua sarebbe un secondo sistema da
 * imparare per una cosa che si guarda due volte.
 *
 * # Perché non è una finestrella
 *
 * Perché ci si arriva **dalla pagina**, e una modale sopra una pagina che mostra
 * la stessa importazione è un livello di troppo. Si apre in linea, sotto la
 * riga: la riga resta al suo posto, e chiudere non fa perdere il segno.
 */
import { useEffect, useState } from "react";

import { ipc, type EsitoImport } from "../ipc";
import { AvvisoErrore } from "./Avvisi";
import { Icona } from "./Icone";
import { nomeSorgente } from "./Importazioni";
import { PastigliaGradino, Tacche, fiduciaDi } from "./Incertezza";
import { numero } from "../formato";
import { t } from "../lingue";

/** Una riga etichetta/numero. */
function Voce({
  etichetta,
  quanti,
  sempre = false,
}: {
  etichetta: string;
  quanti: number;
  /**
   * Mostrala anche a zero.
   *
   * Vale per le due che **descrivono l'esito** — già in libreria, non trovati —
   * dove lo zero è la notizia. Per tutte le altre una riga fissa a zero è una
   * riga che si impara a non leggere.
   */
  sempre?: boolean;
}) {
  if (quanti === 0 && !sempre) return null;
  return (
    <div className="voce">
      <span>{etichetta}</span>
      <span className="stat-number">{numero(quanti)}</span>
    </div>
  );
}

export function Rapporto({
  sourceId,
  onChiudi,
}: {
  sourceId: string;
  onChiudi: () => void;
}) {
  const [esito, setEsito] = useState<EsitoImport | null>(null);
  const [errore, setErrore] = useState<unknown>(null);
  const [mancante, setMancante] = useState(false);

  useEffect(() => {
    let annullato = false;
    setEsito(null);
    setErrore(null);
    setMancante(false);
    ipc
      .importRapporto(sourceId)
      .then((r) => {
        if (annullato) return;
        // `null` non è un errore: è un'importazione più vecchia della
        // persistenza, o una di cui il rapporto è stato potato. Dirlo come
        // guasto manderebbe qualcuno a cercare una riparazione che non esiste.
        if (r === null) setMancante(true);
        else setEsito(r);
      })
      .catch((e: unknown) => {
        if (!annullato) setErrore(e);
      });
    return () => {
      annullato = true;
    };
  }, [sourceId]);

  return (
    <div className="rapporto-importazione">
      <div className="testata">
        <strong>{t("report.title")}</strong>
        <button
          type="button"
          className="tasto icon-btn"
          aria-label={t("report.close")}
          onClick={onChiudi}
        >
          <Icona nome="i-x" dim={12} />
        </button>
      </div>

      {errore !== null && <AvvisoErrore errore={errore} />}

      {mancante && (
        <p className="nota" data-livello="nota">
          {t("report.gone")}
        </p>
      )}

      {esito !== null && (
        <>
          <div className="nota provenienza">
            <Tacche
              fiducia={fiduciaDi(esito.source)}
              etichetta={t("report.provenance", {
                sorgente: nomeSorgente(esito.source),
              })}
            />
            <span>
              {t("report.summary", {
                genere: esito.kind,
                n: numero(esito.resolved),
                sorgente: nomeSorgente(esito.source),
              })}
            </span>
          </div>

          {esito.truncated !== null && (
            <p className="nota" data-livello="avviso">
              {t("report.truncated", {
                attesi: numero(esito.truncated.expected),
                letti: numero(esito.truncated.read),
                mancanti: numero(
                  esito.truncated.expected - esito.truncated.read,
                ),
              })}
            </p>
          )}

          <div className="voci">
            <Voce
              etichetta={t("report.matched")}
              quanti={esito.matched}
              sempre
            />
            <Voce
              etichetta={t("report.notFound")}
              quanti={esito.missing}
              sempre
            />
            <Voce
              etichetta={t("report.entries")}
              quanti={esito.playlistEntries}
            />
            <Voce
              etichetta={t("report.albumIds")}
              quanti={esito.spotifyAlbumIdsWritten}
            />
            <Voce etichetta={t("report.isrc")} quanti={esito.isrcWritten} />
            <Voce
              etichetta={t("report.restored")}
              quanti={esito.playlistRestored}
            />
          </div>

          <div className="scomposizione">
            {esito.matchedIsrc > 0 && (
              <span>
                <PastigliaGradino gradino="isrc" /> {esito.matchedIsrc}
              </span>
            )}
            {esito.matchedExact > 0 && (
              <span>
                <PastigliaGradino gradino="esatta" /> {esito.matchedExact}
              </span>
            )}
            {esito.matchedByTitle > 0 && (
              <span>
                <PastigliaGradino gradino="artistaTitolo" />{" "}
                {esito.matchedByTitle}
              </span>
            )}
            {esito.matchedStripped > 0 && (
              <span>
                <PastigliaGradino gradino="ripulito" /> {esito.matchedStripped}
              </span>
            )}
          </div>

          {/* L'elenco per nome, e selezionabile: è l'unica cosa del rapporto che
              non si può ricostruire da nessun'altra parte. Aperto di serie
              quando è corto, chiuso quando è lungo — seicento nomi aperti sono
              una pagina che si scorre per sbaglio. */}
          {esito.missingTracks.length > 0 && (
            <details open={esito.missingTracks.length <= 10}>
              <summary>
                {t("report.missing", { n: esito.missingTracks.length })}
              </summary>
              <ul className="mancanti">
                {esito.missingTracks.map((b) => (
                  <li key={`${b.position}-${b.title}`} className="list-row">
                    <span className="titolo">
                      {b.artist !== null ? `${b.artist} — ` : ""}
                      {b.title}
                    </span>
                    {b.album !== null && (
                      <span className="album">{b.album}</span>
                    )}
                  </li>
                ))}
              </ul>
            </details>
          )}
        </>
      )}

      {esito === null && !mancante && errore === null && (
        <div className="finte-voci" aria-label={t("report.loading")}>
          <div className="riga-finta skeleton" />
          <div className="riga-finta corta skeleton" />
          <div className="riga-finta skeleton" />
        </div>
      )}
    </div>
  );
}
