/**
 * Artisti: la vista che mancava.
 *
 * # Il ritratto è un mosaico, e non è un ripiego
 *
 * Nessuna immagine d'artista esisterà mai in Aether: non c'è rete, e un lettore
 * locale che va a prendersi i ritratti su internet è un lettore locale che
 * chiama casa. Quindi il riquadro mostra fino a quattro copertine dei suoi
 * dischi, in un mosaico due per due.
 *
 * Non è un segnaposto in attesa della cosa vera. Le copertine sono dati che
 * esistono davvero, e dicono qualcosa che una fotografia promozionale non dice:
 * si riconosce un artista dai suoi album prima che dalla sua faccia, e chi ne ha
 * uno solo lo vede a tutto riquadro invece che in un quarto.
 *
 * # Perché un indice alfabetico e non le pagine
 *
 * Duecento artisti su tre colonne sono settanta righe: si scorre. Con le pagine
 * bisognerebbe sapere che «Radiohead» sta a pagina sette, che è
 * un'informazione che nessuno ha; con l'indice si salta alla R, che è come si
 * cerca in un elenco alfabetico da sempre.
 *
 * Le lettere senza artisti restano visibili e spente: un indice a buchi dice
 * quanto è grande la libreria, uno che mostra solo le lettere piene sembra un
 * indice completo e fa cercare la Q che non c'è.
 */
import { useMemo } from "react";

import { Copertina } from "../Copertina";
import type { Artista } from "../ipc";
import { Icona } from "../parti/Icone";

/** L'alfabeto dell'indice, più il cestino di chi non comincia per lettera. */
const LETTERE = "abcdefghijklmnopqrstuvwxyz".split("");

/** La lettera sotto cui sta un artista: la prima della chiave d'ordinamento. */
function lettera(artista: Artista): string {
  const prima = artista.sortName.slice(0, 1);
  return LETTERE.includes(prima) ? prima : "#";
}

export function Artisti({
  artisti,
  onApri,
  onMenu,
}: {
  artisti: Artista[];
  onApri: (artista: Artista) => void;
  onMenu: (e: React.MouseEvent, artista: Artista) => void;
}) {
  /** Quali lettere hanno qualcuno sotto. */
  const piene = useMemo(
    () => new Set(artisti.map(lettera)),
    [artisti],
  );

  /** Il primo artista di ogni lettera: è lì che salta l'indice. */
  const ancore = useMemo(() => {
    const trovate = new Map<string, string>();
    for (const a of artisti) {
      const l = lettera(a);
      if (!trovate.has(l)) trovate.set(l, a.name);
    }
    return trovate;
  }, [artisti]);

  const salta = (l: string) => {
    const nome = ancore.get(l);
    if (nome === undefined) return;
    document
      .getElementById(`artista-${encodeURIComponent(nome)}`)
      ?.scrollIntoView({ block: "start", behavior: "smooth" });
  };

  if (artisti.length === 0) {
    return (
      <div className="vuoto empty-state">
        <span className="empty-icon" aria-hidden="true">
          <Icona nome="i-artist" dim={30} />
        </span>
        <h2>Nessun artista</h2>
        <p>Gli artisti nascono dai brani: appena c&apos;è una scansione, ci sono.</p>
      </div>
    );
  }

  return (
    <div className="artisti">
      <div className="griglia-artisti track-grid">
        {artisti.map((a) => (
          <button
            key={a.name}
            id={`artista-${encodeURIComponent(a.name)}`}
            type="button"
            className="scheda-artista list-row"
            onClick={() => onApri(a)}
            onContextMenu={(e) => onMenu(e, a)}
          >
            <span
              className="mosaico"
              data-quante={Math.min(a.covers.length, 4)}
              aria-hidden="true"
            >
              {a.covers.length === 0 ? (
                <span className="senza">
                  <Icona nome="i-artist" dim={22} />
                </span>
              ) : (
                a.covers.map((hash) => (
                  <Copertina key={hash} hash={hash} titolo={a.name} classe="tassello" />
                ))
              )}
            </span>
            <span className="nome" title={a.name}>
              {a.name}
            </span>
            <span className="quanti">
              {a.albums === 1 ? "1 album" : `${a.albums} album`} ·{" "}
              {a.tracks === 1 ? "1 brano" : `${a.tracks} brani`}
            </span>
          </button>
        ))}
      </div>

      <nav className="indice-alfabetico" aria-label="Salta a una lettera">
        {LETTERE.map((l) => (
          <button
            key={l}
            type="button"
            className="lettera"
            disabled={!piene.has(l)}
            aria-label={`Salta agli artisti con la ${l.toUpperCase()}`}
            onClick={() => salta(l)}
          >
            {l.toUpperCase()}
          </button>
        ))}
        {piene.has("#") && (
          <button
            type="button"
            className="lettera"
            aria-label="Salta agli artisti che non cominciano per lettera"
            onClick={() => salta("#")}
          >
            #
          </button>
        )}
      </nav>
    </div>
  );
}
