/**
 * Portare dentro una playlist da un file M3U, PLS o XSPF.
 *
 * # Perché c'è un piano prima
 *
 * Perché reimportare **sostituisce**: una playlist con lo stesso nome viene
 * svuotata e riscritta, che è l'unica delle tre scelte possibili — accodare,
 * rinominare, sostituire — che si può fare due volte senza pentirsene. Ma
 * cancella, e quel che cancella va detto prima di farlo, non dopo.
 *
 * # I mancanti si dicono con il percorso
 *
 * Un brano di un M3U che qui non c'è **non** finisce nella coda, al contrario
 * di quel che succede importando un archivio o un link. La differenza è nella
 * domanda: quelli sono elenchi di canzoni che l'utente vuole avere, un file M3U
 * è la fotografia di una libreria che qualcuno aveva già. Andare a cercare nei
 * cataloghi un file che esiste sul computer di chi ha esportato la playlist è
 * una risposta a una domanda che nessuno ha fatto — e il percorso, che si
 * mostra, è l'unica cosa utile per andarselo a prendere dov'è davvero.
 */
import { useEffect, useState } from "react";

import { useFinestrella } from "./finestrella";
import { ipc, testoErrore, type EsitoFilePlaylist } from "./ipc";
import { Avviso } from "./parti/Avvisi";
import { Icona } from "./parti/Icone";
import { numero } from "./formato";
import { t } from "./lingue";
import { Trans } from "./lingue/Trans";

export function ImportaPlaylist({
  percorso,
  onChiudi,
  onImportato,
  onErrore,
}: {
  percorso: string;
  onChiudi: () => void;
  onImportato: (esito: EsitoFilePlaylist) => void;
  onErrore: (e: unknown) => void;
}) {
  const [rapporto, setRapporto] = useState<EsitoFilePlaylist | null>(null);
  const [nome, setNome] = useState("");
  const [errore, setErrore] = useState<string | null>(null);
  const [inCorso, setInCorso] = useState(true);
  // Si apre con tutti i comandi spenti, perché il piano non è ancora arrivato:
  // il fuoco va sulla radice, che è l'unico posto che c'è in quell'istante, e
  // l'Escape funziona anche durante la lettura del file.
  const finestrella = useFinestrella<HTMLDivElement>(onChiudi);

  useEffect(() => {
    let annullato = false;
    setInCorso(true);
    ipc
      .playlistFilePiano(percorso)
      .then((piano) => {
        if (annullato) return;
        setRapporto(piano.rapporto);
        setNome(piano.nome);
      })
      .catch((e: unknown) => {
        if (!annullato) setErrore(testoErrore(e));
      })
      .finally(() => {
        if (!annullato) setInCorso(false);
      });
    return () => {
      annullato = true;
    };
  }, [percorso]);

  const conferma = () => {
    setInCorso(true);
    setErrore(null);
    ipc
      .playlistFileImporta(percorso, nome)
      .then((esito) => {
        onImportato(esito);
        onChiudi();
      })
      .catch((e: unknown) => {
        setErrore(testoErrore(e));
        setInCorso(false);
        onErrore(e);
      });
  };

  const vuota = rapporto !== null && rapporto.matchedByPath + rapporto.matchedByTags === 0;

  return (
    <div className="velo scuro" onClick={inCorso ? undefined : onChiudi}>
      <div
        ref={finestrella}
        className="finestrella glass-modal"
        role="dialog"
        aria-modal="true"
        aria-label={t("plfile.aria")}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{t("plfile.title")}</h2>
        <div className="percorso">{percorso}</div>

        {errore && <div className="errore">{errore}</div>}

        {rapporto === null ? (
          <p className="nota">
            {inCorso ? t("plfile.reading") : t("plfile.nothing")}
          </p>
        ) : (
          <>
            <label>
              {t("plfile.name")}
              <input
                type="text"
                value={nome}
                onChange={(e) => setNome(e.target.value)}
              />
            </label>

            <dl className="numeri">
              <div>
                <dt>{t("plfile.rows")}</dt>
                <dd className="stat-number">{numero(rapporto.entries)}</dd>
              </div>
              <div>
                <dt>{t("plfile.byPath")}</dt>
                <dd className="stat-number">
                  {numero(rapporto.matchedByPath)}
                </dd>
              </div>
              <div>
                <dt>{t("plfile.byTags")}</dt>
                <dd className="stat-number">
                  {numero(rapporto.matchedByTags)}
                </dd>
              </div>
              <div>
                <dt>{t("plfile.notInLib")}</dt>
                <dd className="stat-number">
                  {numero(rapporto.missing.length)}
                </dd>
              </div>
            </dl>

            {/* Avviso: qualcosa viene distrutto, e un gesto lo evita. Il gesto
                è cambiare il nome nel campo qui sopra, quindi non c'è un tasto
                da mettere dentro il riquadro — la frase indica dove. */}
            {rapporto.replaced && (
              <Avviso livello="avviso">
                <Trans
                  k="plfile.replaced"
                  v={{
                    nome: <strong>{nome}</strong>,
                    sostituita: <strong>{t("plfile.replaced.word")}</strong>,
                  }}
                />
              </Avviso>
            )}

            {/* Blocco: importare produrrebbe una playlist vuota, e il primario è
                spento. Era ambra accanto all'altro, e i due dicevano cose
                opposte con lo stesso colore — «attento» e «non si può». */}
            {vuota && <Avviso livello="blocco">{t("plfile.empty")}</Avviso>}

            {/* Nota: righe che non si sono capite sono un fatto del file, non
                un guasto di Aether, e non c'è niente da fare. */}
            {rapporto.unreadable > 0 && (
              <Avviso livello="nota">
                {t("plfile.unreadable", { n: rapporto.unreadable })}
              </Avviso>
            )}

            {rapporto.missing.length > 0 && (
              <details className="mancanti-account">
                <summary>
                  {t("plfile.missing", { n: rapporto.missing.length })}
                </summary>
                <ul>
                  {rapporto.missing.slice(0, 50).map((m) => (
                    <li key={m.path}>
                      <span className="nome">
                        {m.title ?? m.path.split(/[\\/]/).pop()}
                      </span>
                      {m.artist !== null && (
                        <span className="autore">{m.artist}</span>
                      )}
                      {/* Il percorso e non il titolo: è l'unica cosa che serve
                          per andarselo a prendere dov'è davvero. */}
                      <span className="percorso" title={m.path}>
                        {m.path}
                      </span>
                    </li>
                  ))}
                </ul>
                {rapporto.missing.length > 50 && (
                  <p className="nota">
                    {t("plfile.andMore", { n: rapporto.missing.length - 50 })}
                  </p>
                )}
              </details>
            )}
          </>
        )}

        <div className="tasti-finestrella">
          <button
            type="button"
            className="bottone btn-ghost"
            disabled={inCorso}
            onClick={onChiudi}
          >
            {t("common.cancel")}
          </button>
          <button
            type="button"
            className="bottone primario btn-accent"
            // `vuota`: la playlist verrebbe creata e sarebbe vuota, cioè un
            // successo apparente. Il riquadro qui sopra è un blocco e questo
            // tasto deve dirlo anche lui, o il blocco è solo un colore.
            disabled={
              inCorso || rapporto === null || nome.trim().length === 0 || vuota
            }
            onClick={conferma}
          >
            <Icona nome="i-import" dim={15} />
            {inCorso ? t("plfile.working") : t("legacy.import")}
          </button>
        </div>
      </div>
    </div>
  );
}
