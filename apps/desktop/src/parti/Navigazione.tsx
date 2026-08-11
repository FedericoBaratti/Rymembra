/**
 * La colonna di sinistra: dove vado.
 *
 * # La regola che l'ha svuotata
 *
 * Qui dentro c'erano cinque blocchi — playlist, cartelle, importazione,
 * aspetto, libreria — di cui uno solo era navigazione. Gli altri quattro erano
 * configurazione, e stavano nella barra laterale per il motivo per cui ci
 * finisce quasi tutto: era il posto dove c'era spazio.
 *
 * Il costo non era estetico. Una barra che contiene sia «dove sono» sia «cosa
 * sto per cambiare» non ha uno stato attivo che significhi qualcosa: il pulsante
 * «Scansiona» era sempre lì accanto ad Album, alla stessa distanza dal dito, e
 * chi scorreva l'elenco delle playlist si trovava sotto una barra di
 * avanzamento. Ora **a sinistra c'è solo navigazione**, e tutto il resto sta in
 * una pagina che si apre come le altre.
 *
 * Ne resta una sola voce che non è una destinazione della libreria —
 * Impostazioni — ed è appuntata in fondo, sotto una riga sottile, perché quello
 * *è* il posto dove tutti la cercano.
 *
 * # Perché la richiusura è un bottone e non una soglia
 *
 * La barra si stringe a `--rail-w` e resta usabile: le icone bastano a
 * riconoscere quattro destinazioni. È l'utente a deciderlo, non la larghezza
 * della finestra, perché su un monitor grande c'è chi vuole comunque tutta la
 * larghezza per le colonne dell'elenco.
 */
import { useState } from "react";

import type { Playlist } from "../ipc";
import { Icona, type NomeIcona } from "./Icone";

/** Dove si può andare. */
export type Vista = "album" | "artisti" | "brani" | "preferiti" | "impostazioni";

/** Le quattro destinazioni della libreria, nell'ordine in cui si guardano. */
const DESTINAZIONI: readonly (readonly [Vista, string, NomeIcona])[] = [
  ["album", "Album", "i-album"],
  ["artisti", "Artisti", "i-artist"],
  ["brani", "Brani", "i-track"],
  ["preferiti", "Preferiti", "i-heart"],
];

export function Navigazione({
  vista,
  playlistAperta,
  inLibreria,
  conteggi,
  playlist,
  larga = true,
  inFondo = false,
  onVista,
  onPlaylist,
  onMenuPlaylist,
  onNuovaPlaylist,
  onNuovaSmart,
  onImportaFile,
}: {
  vista: Vista;
  playlistAperta: number | null;
  /** Falso mentre si cerca o si guarda un album: nessuna voce è «quella lì». */
  inLibreria: boolean;
  conteggi: Partial<Record<Vista, number>>;
  playlist: Playlist[];
  /**
   * Comincia larga, con le etichette accanto alle icone.
   *
   * È solo lo stato **iniziale**: il bottone di richiusura continua a
   * possederlo, e la skin dice da dove si parte invece di decidere per sempre.
   */
  larga?: boolean;
  /** In fondo alla finestra invece che di lato: niente marchio, niente playlist. */
  inFondo?: boolean;
  onVista: (vista: Vista) => void;
  onPlaylist: (p: Playlist) => void;
  onMenuPlaylist: (e: React.MouseEvent, p: Playlist) => void;
  onNuovaPlaylist: () => void;
  /** Apre l'editor delle regole per una playlist che si aggiorna da sé. */
  onNuovaSmart: () => void;
  /** Sceglie un file M3U, PLS o XSPF da portare dentro. */
  onImportaFile: () => void;
}) {
  const [stretta, setStretta] = useState(!larga);

  return (
    // Niente `app-shell`: quella classe dice «il contenitore di tutta la
    // finestra», e la porta la zona radice dello scafale. Averla qui e là
    // voleva dire che una skin che la ridipingeva colorava due cose.
    <nav
      className={inFondo ? "navigazione bottom-nav" : "navigazione"}
      data-stretta={(!inFondo && stretta) || undefined}
      data-fondo={inFondo || undefined}
      aria-label="Navigazione"
    >
      {/* In fondo il marchio non ci sta e non serve: la finestra è già aperta,
          e chi la guarda sa in quale programma si trova. */}
      <div className="marchio" hidden={inFondo}>
        <span className="anello" aria-hidden="true">
          <Icona nome="i-mark" dim={20} />
        </span>
        <span className="nome">Aether</span>
        <button
          type="button"
          className="tasto icon-btn richiudi"
          aria-label={stretta ? "Allarga la barra" : "Restringi la barra"}
          aria-expanded={!stretta}
          title={stretta ? "Allarga" : "Restringi"}
          onClick={() => setStretta((prima) => !prima)}
        >
          <Icona nome={stretta ? "i-chev-r" : "i-chev-l"} dim={15} />
        </button>
      </div>

      <div className="destinazioni">
        {DESTINAZIONI.map(([chiave, etichetta, icona]) => {
          const qui = inLibreria && vista === chiave && playlistAperta === null;
          return (
            <button
              key={chiave}
              type="button"
              className="voce nav-pill"
              aria-current={qui ? "page" : undefined}
              data-active={qui || undefined}
              title={stretta ? etichetta : undefined}
              onClick={() => onVista(chiave)}
            >
              <Icona nome={icona} dim={19} />
              <span className="etichetta">{etichetta}</span>
              <span className="conteggio">{conteggi[chiave] ?? "—"}</span>
            </button>
          );
        })}
      </div>

      <div className="gruppo" hidden={inFondo}>
        <div className="titolo-gruppo">
          <span>Playlist</span>
          {/* Tre tasti e non un menù: sono tre cose che si fanno di rado ma
              che, quando si fanno, si sanno già — e un menù a tendina per tre
              voci è un clic in più per ognuna delle tre. L'ordine è quello
              della frequenza. */}
          <button
            type="button"
            className="tasto icon-btn"
            aria-label="Importa una playlist da file"
            title="Da un file M3U, PLS o XSPF…"
            onClick={onImportaFile}
          >
            <Icona nome="i-import" dim={14} />
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-label="Nuova playlist intelligente"
            title="Nuova playlist intelligente: si riempie da sé"
            onClick={onNuovaSmart}
          >
            <Icona nome="i-settings" dim={14} />
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-label="Nuova playlist"
            title="Nuova playlist"
            onClick={onNuovaPlaylist}
          >
            <Icona nome="i-plus" dim={14} />
          </button>
        </div>
        {playlist.length === 0 ? (
          <p className="niente">Nessuna playlist.</p>
        ) : (
          playlist.map((p) => {
            const qui = playlistAperta === p.id && inLibreria;
            return (
              <button
                key={p.id}
                type="button"
                className="voce nav-pill playlist"
                aria-current={qui ? "page" : undefined}
                data-active={qui || undefined}
                title={p.isSmart ? `${p.name} — automatica` : p.name}
                onClick={() => onPlaylist(p)}
                onContextMenu={(e) => onMenuPlaylist(e, p)}
              >
                {/* L'ingranaggio era un glifo che su alcuni caratteri di sistema
                    non esisteva e si vedeva come un rettangolo vuoto. */}
                <Icona nome={p.isSmart ? "i-settings" : "i-list"} dim={16} />
                <span className="etichetta">{p.name}</span>
                <span className="conteggio">{p.tracks}</span>
              </button>
            );
          })
        )}
      </div>

      {/* Lo spaziatore spinge Impostazioni in fondo. Non è una svista di
          allineamento: è la posizione in cui la si cerca. */}
      <div className="spinta" hidden={inFondo} />

      <div className="fondo">
        <button
          type="button"
          className="voce nav-pill"
          aria-current={vista === "impostazioni" ? "page" : undefined}
          data-active={vista === "impostazioni" || undefined}
          title={stretta ? "Impostazioni" : undefined}
          onClick={() => onVista("impostazioni")}
        >
          <Icona nome="i-settings" dim={19} />
          <span className="etichetta">Impostazioni</span>
        </button>
      </div>
    </nav>
  );
}
