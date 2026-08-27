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
import { t } from "./lingue";

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
        className="finestrella stretta glass-modal"
        role="dialog"
        aria-modal="true"
        aria-label={t("addTo.aria")}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{t("addTo.title", { quanti: brani_(brani.length) })}</h2>

        {errore && <div className="errore">{errore}</div>}

        {scegliibili.length === 0 ? (
          <p>{t("addTo.empty")}</p>
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
            className="campo field-input"
            placeholder={t("addTo.newName")}
            value={nuova}
            spellCheck={false}
            onChange={(e) => setNuova(e.target.value)}
          />
          <button
            type="submit"
            className="bottone primario"
            disabled={inCorso || nuova.trim().length === 0}
          >
            {t("addTo.create")}
          </button>
        </form>

        <div className="tasti-finestrella">
          <button type="button" className="bottone" onClick={onChiudi}>
            {t("common.cancel")}
          </button>
        </div>
      </div>
    </div>
  );
}
