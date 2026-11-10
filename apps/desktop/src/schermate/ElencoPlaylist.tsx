/**
 * Le playlist, come pagina: la prima pillola della Libreria del telefono.
 *
 * Sul computer le playlist stanno nella barra laterale, sempre in vista, con
 * tre tasti per farne di nuove. Sul telefono la barra laterale non c'è, e
 * l'unico posto in cui stavano era il cassetto — cioè due tocchi e una
 * navigazione nascosta per arrivare alla cosa che chi ascolta apre più spesso
 * dopo la Home. Qui sono un elenco come gli altri: una riga per playlist, col
 * nome, quanti brani e quanto dura; il tocco la apre, il tocco lungo e il tasto
 * ⋮ aprono il suo menu (`vociPlaylist`, lo stesso del computer).
 *
 * Le tre strade per farne una nuova — vuota, intelligente, da un file — stanno
 * in testa all'elenco e non dietro un «+»: quando non ce n'è nessuna sono
 * l'unica cosa da fare qui, e quando ce ne sono restano a un tocco.
 */
import { brani_, durata } from "../formato";
import type { Playlist } from "../ipc";
import { t } from "../lingue";
import { Icona, type NomeIcona } from "../parti/Icone";

function Nuova({
  icona,
  etichetta,
  onClick,
}: {
  icona: NomeIcona;
  etichetta: string;
  onClick: () => void;
}) {
  return (
    <button type="button" className="nuova-playlist" onClick={onClick}>
      <Icona nome={icona} dim={18} />
      <span>{etichetta}</span>
    </button>
  );
}

export function ElencoPlaylist({
  playlist,
  onApri,
  onMenu,
  onNuova,
  onNuovaSmart,
  onImportaFile,
}: {
  playlist: readonly Playlist[];
  onApri: (p: Playlist) => void;
  onMenu: (e: React.MouseEvent, p: Playlist) => void;
  onNuova: () => void;
  onNuovaSmart: () => void;
  onImportaFile: () => void;
}) {
  return (
    <div className="elenco-playlist">
      <div className="nuove-playlist">
        <Nuova icona="i-plus" etichetta={t("playlist.new.short")} onClick={onNuova} />
        <Nuova icona="i-settings" etichetta={t("playlist.newSmart.short")} onClick={onNuovaSmart} />
        <Nuova icona="i-import" etichetta={t("playlist.importFile.short")} onClick={onImportaFile} />
      </div>
      {playlist.length === 0 ? (
        <div className="vuoto empty-state">
          <span className="empty-icon" aria-hidden="true">
            <Icona nome="i-list" dim={30} />
          </span>
          <h2>{t("playlist.empty.title")}</h2>
          <p>{t("playlist.empty.body")}</p>
        </div>
      ) : (
        <ul className="righe-playlist">
          {playlist.map((p) => (
            <li key={p.id} className="riga-playlist">
              <button
                type="button"
                className="apri-playlist"
                onClick={() => onApri(p)}
                onContextMenu={(e) => onMenu(e, p)}
              >
                <span className="tessera-playlist" aria-hidden="true">
                  <Icona nome={p.isSmart ? "i-settings" : "i-list"} dim={22} />
                </span>
                <span className="testi-playlist">
                  {/* `dir="auto"`: il nome di una playlist può essere scritto da destra. */}
                  <span className="nome-playlist" dir="auto">
                    {p.name}
                  </span>
                  <span className="sotto-playlist">
                    {p.isSmart ? `${t("page.playlist.smart")} · ` : ""}
                    {brani_(p.tracks)}
                    {p.durationMs > 0 ? ` · ${durata(p.durationMs)}` : ""}
                  </span>
                </span>
              </button>
              <button
                type="button"
                className="tasto-altro"
                aria-label={t("row.more", { titolo: p.name })}
                onClick={(e) => onMenu(e, p)}
              >
                <Icona nome="i-dots-v" dim={20} />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
