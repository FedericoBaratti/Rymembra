/**
 * «Aggiungi a playlist», per una selezione di brani.
 *
 * Una finestrella e non un sottomenù: i sottomenù del menù contestuale vanno
 * inseguiti col puntatore e diventano illeggibili appena le playlist sono più
 * di cinque. Qui l'elenco scorre, e la creazione sta nello stesso posto della
 * scelta — perché «non ce n'è ancora una» è il caso più comune all'inizio.
 */
import { useState } from "react";

import { brani_ } from "./formato";
import { ipc, testoErrore, type Playlist } from "./ipc";

export function AggiungiAPlaylist({
  brani,
  playlist,
  onChiudi,
  onFatto,
}: {
  brani: number[];
  playlist: Playlist[];
  onChiudi: () => void;
  onFatto: () => void;
}) {
  const [nuova, setNuova] = useState("");
  const [errore, setErrore] = useState<string | null>(null);
  const [inCorso, setInCorso] = useState(false);

  // Le automatiche non compaiono: la loro appartenenza la decidono le regole, e
  // un brano aggiunto a mano sparirebbe al primo ricalcolo. Il nucleo lo
  // rifiuta con `library.playlistIsSmart`; qui si evita di proporlo.
  const scegliibili = playlist.filter((p) => !p.isSmart);

  const aggiungi = async (id: number) => {
    setInCorso(true);
    try {
      await ipc.playlistAggiungi(id, brani);
      onFatto();
      onChiudi();
    } catch (e) {
      setErrore(testoErrore(e));
      setInCorso(false);
    }
  };

  const creaEAggiungi = async () => {
    const nome = nuova.trim();
    if (nome.length === 0) return;
    setInCorso(true);
    try {
      const creata = await ipc.playlistCrea(nome);
      await ipc.playlistAggiungi(creata.id, brani);
      onFatto();
      onChiudi();
    } catch (e) {
      setErrore(testoErrore(e));
      setInCorso(false);
    }
  };

  return (
    <div className="velo scuro" onClick={onChiudi}>
      <div
        className="finestrella stretta"
        role="dialog"
        aria-modal="true"
        aria-label="Aggiungi a playlist"
        onClick={(e) => e.stopPropagation()}
      >
        <h2>Aggiungi {brani_(brani.length)} a…</h2>

        {errore && <div className="errore">{errore}</div>}

        {scegliibili.length === 0 ? (
          <p>Non c&apos;è ancora nessuna playlist. Creane una qui sotto.</p>
        ) : (
          <div className="scelta-playlist">
            {scegliibili.map((p) => (
              <button
                key={p.id}
                type="button"
                disabled={inCorso}
                onClick={() => void aggiungi(p.id)}
              >
                <span className="nome">{p.name}</span>
                <span className="conteggio">{p.tracks}</span>
              </button>
            ))}
          </div>
        )}

        <form
          className="crea-playlist"
          onSubmit={(e) => {
            e.preventDefault();
            void creaEAggiungi();
          }}
        >
          <input
            className="campo"
            placeholder="Nuova playlist…"
            value={nuova}
            spellCheck={false}
            onChange={(e) => setNuova(e.target.value)}
          />
          <button
            type="submit"
            className="bottone primario"
            disabled={inCorso || nuova.trim().length === 0}
          >
            Crea
          </button>
        </form>

        <div className="tasti-finestrella">
          <button type="button" className="bottone" onClick={onChiudi}>
            Annulla
          </button>
        </div>
      </div>
    </div>
  );
}
