/**
 * «Aggiungi a playlist», per una selezione di brani.
 *
 * Una finestrella e non un sottomenù: i sottomenù del menù contestuale vanno
 * inseguiti col puntatore e diventano illeggibili appena le playlist sono più
 * di cinque. Qui l'elenco scorre, e la creazione sta nello stesso posto della
 * scelta — perché «non ce n'è ancora una» è il caso più comune all'inizio.
 *
 * # «Crea» è un gesto solo
 *
 * Erano due chiamate, `playlistCrea` e poi `playlistAggiungi`: se la seconda
 * falliva restava una playlist vuota col nome appena scelto, e il nome —
 * ormai occupato — faceva fallire anche il tentativo successivo con «esiste
 * già». Adesso è un comando solo nel nucleo (`playlistCreaEAggiungi`), in una
 * transazione: o la playlist nasce coi suoi brani, o non nasce.
 *
 * # L'esito, in tutti e due i versi
 *
 * Un errore resta qui, con `role="alert"`: è la finestrella in cui si è
 * premuto, e chi usa un lettore di schermo deve sentirlo senza andarlo a
 * cercare. La riuscita invece va a chi l'ha aperta (`onFatto`), che la dice
 * nella fascia delle notizie — la finestrella si chiude, e un «fatto» detto
 * qui dentro non lo leggerebbe nessuno.
 */
import { useState } from "react";

import { Dialogo } from "./Dialogo";
import { brani_, numero } from "./formato";
import { ipc, testoErrore, type AggiuntaPlaylist, type Playlist } from "./ipc";
import { t } from "./lingue";

/**
 * La notizia di un'aggiunta riuscita: «Aggiunti 8 brani a «Serata» · 2 non
 * c'erano più».
 *
 * Qui e non in `App`, accanto alla finestrella che produce l'esito: chi legge
 * questo file sa cosa vuol dire `saltati`. La seconda metà compare solo quando
 * c'è — una scansione passata nel frattempo che ha tolto dei brani ancora
 * selezionati a schermo — ed è quel che distingue un'aggiunta riuscita a metà
 * da un guasto.
 */
export function fraseDellAggiunta(fatta: AggiuntaPlaylist): string {
  const nome = fatta.playlist.name;
  const detto =
    fatta.aggiunti > 0
      ? t("addTo.done", { n: fatta.aggiunti, brani: brani_(fatta.aggiunti), nome })
      : t("addTo.nothing", { nome });
  return fatta.saltati > 0
    ? `${detto} · ${t("addTo.missing", { n: fatta.saltati })}`
    : detto;
}

export function AggiungiAPlaylist({
  brani,
  playlist,
  onChiudi,
  onFatto,
}: {
  brani: number[];
  playlist: Playlist[];
  onChiudi: () => void;
  /** Com'è andata: la playlist, quanti sono entrati, quanti non c'erano più. */
  onFatto: (aggiunta: AggiuntaPlaylist) => void;
}) {
  const [nuova, setNuova] = useState("");
  const [errore, setErrore] = useState<string | null>(null);
  const [inCorso, setInCorso] = useState(false);

  // Le automatiche non compaiono: la loro appartenenza la decidono le regole, e
  // un brano aggiunto a mano sparirebbe al primo ricalcolo. Il nucleo lo
  // rifiuta con `library.playlistIsSmart`; qui si evita di proporlo.
  const scegliibili = playlist.filter((p) => !p.isSmart);

  const esegui = async (gesto: () => Promise<AggiuntaPlaylist>) => {
    setInCorso(true);
    setErrore(null);
    try {
      const fatta = await gesto();
      onFatto(fatta);
      onChiudi();
    } catch (e) {
      setErrore(testoErrore(e));
      setInCorso(false);
    }
  };

  const creaEAggiungi = () => {
    const nome = nuova.trim();
    if (nome.length === 0) return;
    void esegui(() => ipc.playlistCreaEAggiungi(nome, brani));
  };

  return (
    <Dialogo
      titolo={t("addTo.title", { quanti: brani_(brani.length) })}
      classe="finestrella stretta glass-modal"
      onChiudi={onChiudi}
      occupato={inCorso}
    >
      {errore && (
        <div className="errore" role="alert">
          {errore}
        </div>
      )}

      {scegliibili.length === 0 ? (
        <p>{t("addTo.empty")}</p>
      ) : (
        <div className="scelta-playlist">
          {scegliibili.map((p) => (
            <button
              key={p.id}
              type="button"
              disabled={inCorso}
              onClick={() => void esegui(() => ipc.playlistAggiungi(p.id, brani))}
            >
              <span className="nome">{p.name}</span>
              <span className="conteggio">{numero(p.tracks)}</span>
            </button>
          ))}
        </div>
      )}

      <form
        className="crea-playlist"
        onSubmit={(e) => {
          e.preventDefault();
          creaEAggiungi();
        }}
      >
        <input
          className="campo field-input"
          placeholder={t("addTo.newName")}
          aria-label={t("addTo.newName")}
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
    </Dialogo>
  );
}
