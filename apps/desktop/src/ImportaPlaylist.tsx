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
 * Un brano di un M3U che qui non c'è **non** finisce nella coda di
 * scaricamento, al contrario di quel che succede importando da Spotify. La
 * differenza è nella domanda: un link di Spotify è un elenco di canzoni che
 * l'utente vuole avere, un file M3U è la fotografia di una libreria che
 * qualcuno aveva già. Andare a cercare su YouTube un file che esiste sul
 * computer di chi ha esportato la playlist è una risposta a una domanda che
 * nessuno ha fatto — e il percorso, che si mostra, è l'unica cosa utile per
 * andarselo a prendere dov'è davvero.
 */
import { useEffect, useState } from "react";

import { ipc, testoErrore, type EsitoFilePlaylist } from "./ipc";
import { Icona } from "./parti/Icone";

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
        className="finestrella"
        role="dialog"
        aria-modal="true"
        aria-label="Importa una playlist da file"
        onClick={(e) => e.stopPropagation()}
      >
        <h2>Importa una playlist</h2>
        <div className="percorso">{percorso}</div>

        {errore && <div className="errore">{errore}</div>}

        {rapporto === null ? (
          <p className="nota">{inCorso ? "Leggo il file…" : "Niente da leggere."}</p>
        ) : (
          <>
            <label>
              Nome della playlist
              <input
                type="text"
                value={nome}
                onChange={(e) => setNome(e.target.value)}
              />
            </label>

            <dl className="numeri">
              <div>
                <dt>Righe nel file</dt>
                <dd className="stat-number">
                  {rapporto.entries.toLocaleString("it")}
                </dd>
              </div>
              <div>
                <dt>Trovate dal percorso</dt>
                <dd className="stat-number">
                  {rapporto.matchedByPath.toLocaleString("it")}
                </dd>
              </div>
              <div>
                <dt>Trovate dai tag</dt>
                <dd className="stat-number">
                  {rapporto.matchedByTags.toLocaleString("it")}
                </dd>
              </div>
              <div>
                <dt>Non in libreria</dt>
                <dd className="stat-number">
                  {rapporto.missing.length.toLocaleString("it")}
                </dd>
              </div>
            </dl>

            {rapporto.replaced && (
              <div className="avviso-monco">
                Una playlist chiamata <strong>{nome}</strong> c&apos;è già, e
                verrà <strong>sostituita</strong>: i suoi brani di adesso
                spariscono e restano quelli di questo file. Cambia il nome qui
                sopra se vuoi tenerle tutte e due.
              </div>
            )}

            {vuota && (
              <div className="avviso-monco">
                Nessuna riga di questo file corrisponde a un brano in libreria.
                Se la playlist viene da un altro computer è normale: i percorsi
                non esistono qui, e senza titolo e interprete scritti nel file
                non c&apos;è altro con cui cercare.
              </div>
            )}

            {rapporto.unreadable > 0 && (
              <p className="nota">
                {rapporto.unreadable}{" "}
                {rapporto.unreadable === 1 ? "riga" : "righe"} del file non si
                {rapporto.unreadable === 1 ? " è capita" : " sono capite"}. Le
                altre ci sono tutte.
              </p>
            )}

            {rapporto.missing.length > 0 && (
              <details className="mancanti-account">
                <summary>
                  {rapporto.missing.length.toLocaleString("it")}{" "}
                  {rapporto.missing.length === 1 ? "brano" : "brani"} che non hai
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
                    …e altri {(rapporto.missing.length - 50).toLocaleString("it")}.
                  </p>
                )}
              </details>
            )}
          </>
        )}

        <div className="in-fondo">
          <button
            type="button"
            className="bottone btn-ghost"
            disabled={inCorso}
            onClick={onChiudi}
          >
            Annulla
          </button>
          <button
            type="button"
            className="bottone primario btn-accent"
            disabled={inCorso || rapporto === null || nome.trim().length === 0}
            onClick={conferma}
          >
            <Icona nome="i-import" dim={15} />
            {inCorso ? "Un momento…" : "Importa"}
          </button>
        </div>
      </div>
    </div>
  );
}
