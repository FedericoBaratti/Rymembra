/**
 * «Quale testo è di questo brano?»: le voci del catalogo, da scegliere a mano.
 *
 * # Perché esiste
 *
 * Perché la scelta automatica è prudente apposta — meglio nessun testo che
 * quello di un'altra canzone — e la prudenza a volte sbaglia in tutti e due i
 * versi: scarta la voce giusta, o ne prende una che le somiglia. Chi ascolta le
 * distingue a colpo d'occhio dalle prime due righe; il programma no. Fino a qui
 * l'unico rimedio era sincronizzare il testo da capo.
 *
 * # Cosa mostra
 *
 * Quel che il nucleo restituisce, nell'ordine in cui lo restituisce — prima le
 * voci con i tempi, poi la durata più vicina — con quel che serve a
 * riconoscerle: titolo, artista, album, durata, se hanno i tempi, e le prime
 * righe. Nessun veto: la finestrella esiste per chi ne sa più dei veti.
 *
 * «Nessuno di questi» c'è solo quando il testo che si ha è arrivato dal
 * catalogo: è l'unico che si può scartare, perché quello di un file o di chi
 * ascolta tornerebbe alla prossima apertura.
 */
import { useEffect, useState } from "react";

import { useFinestrella } from "../finestrella";
import { durata } from "../formato";
import {
  ipc,
  testoErrore,
  type Brano,
  type CandidatiTesto,
  type TestoBrano,
} from "../ipc";
import { t } from "../lingue";

export function SceltaTesto({
  brano,
  rifiutabile,
  onChiudi,
  onScelto,
}: {
  brano: Brano;
  /** Il testo che si ha è del catalogo, e si può scartare. */
  rifiutabile: boolean;
  onChiudi: () => void;
  /** Il testo da mostrare adesso, dopo la scelta o lo scarto. */
  onScelto: (testo: TestoBrano) => void;
}) {
  const finestrella = useFinestrella<HTMLDivElement>(onChiudi);
  const [elenco, setElenco] = useState<CandidatiTesto | null>(null);
  const [errore, setErrore] = useState<string | null>(null);
  const [inCorso, setInCorso] = useState(false);

  useEffect(() => {
    let annullato = false;
    ipc
      .testoCandidati(brano.id)
      .then((trovate) => {
        if (!annullato) setElenco(trovate);
      })
      .catch((e: unknown) => {
        if (annullato) return;
        setErrore(testoErrore(e));
        setElenco({ rete: true, voci: [] });
      });
    return () => {
      annullato = true;
    };
  }, [brano.id]);

  const fai = (gesto: Promise<TestoBrano>) => {
    setInCorso(true);
    setErrore(null);
    gesto.then(onScelto).catch((e: unknown) => {
      setErrore(testoErrore(e));
      setInCorso(false);
    });
  };

  return (
    <div className="velo scuro" onClick={onChiudi}>
      <div
        ref={finestrella}
        className="finestrella glass-modal"
        role="dialog"
        aria-modal="true"
        aria-label={t("np.lyrics.choose.aria")}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{t("np.lyrics.choose.title")}</h2>

        {errore && <div className="errore">{errore}</div>}

        {elenco === null ? (
          <p aria-live="polite">{t("np.lyrics.choose.loading")}</p>
        ) : elenco.voci.length === 0 ? (
          /* Due vuoti diversi: nessuna voce, oppure nessuna domanda. Il secondo
             non è una risposta del catalogo, e dirlo com'era — «il catalogo non
             ha voci per questo brano» — mandava a cercare il guasto nel
             catalogo invece che nell'interruttore che lo spegne. */
          errore === null && (
            <p>
              {elenco.rete
                ? t("np.lyrics.choose.none")
                : t("np.lyrics.choose.offline")}
            </p>
          )
        ) : (
          <div className="scelta-testo">
            {elenco.voci.map((voce) => (
              <button
                key={voce.id}
                type="button"
                disabled={inCorso}
                onClick={() => fai(ipc.testoScegli(brano.id, voce.id))}
              >
                <span className="chi">
                  {voce.titolo} · {voce.artista}
                </span>
                <span className="dati">
                  {[
                    voce.album,
                    voce.durataMs !== null ? durata(voce.durataMs) : null,
                    voce.strumentale
                      ? t("np.lyrics.choose.instrumental")
                      : voce.sincronizzato
                        ? t("np.lyrics.choose.synced")
                        : t("np.lyrics.choose.plain"),
                  ]
                    .filter((pezzo) => pezzo !== null && pezzo !== "")
                    .join(" · ")}
                </span>
                {voce.anteprima !== null && (
                  <span className="anteprima-testo">{voce.anteprima}</span>
                )}
              </button>
            ))}
          </div>
        )}

        <div className="tasti-finestrella">
          {rifiutabile && (
            <button
              type="button"
              className="bottone"
              title={t("np.lyrics.reject.hint")}
              disabled={inCorso}
              onClick={() => fai(ipc.testoRifiuta(brano.id))}
            >
              {t("np.lyrics.reject")}
            </button>
          )}
          <button type="button" className="bottone" onClick={onChiudi}>
            {t("common.cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}
