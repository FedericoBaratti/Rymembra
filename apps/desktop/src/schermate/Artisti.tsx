/**
 * Artisti: la vista che mancava.
 *
 * # Il ritratto è un mosaico, e non è un ripiego
 *
 * Nessuna immagine d'artista esisterà mai in Rymembra: non c'è rete, e un lettore
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
 *
 * Sotto quale lettera cade un artista lo decide `alfabeto.ts`, con
 * `Intl.Collator` nella lingua di chi legge: «Ørjan» sta sotto la O per un
 * italiano, e non sotto «#» insieme ai numeri.
 *
 * # Una finestra sulla griglia, e l'indice che la conosce
 *
 * Duemila artisti erano duemila schede nel DOM, ognuna con fino a quattro
 * copertine: la griglia si disegna per righe come quella degli album
 * (`useGrigliaVirtuale` in `Libreria.tsx`). L'indice alfabetico non può quindi
 * più cercare la scheda della R nel documento — quasi sempre non c'è — e non
 * ne ha bisogno: sa in che posizione sta il primo artista di ogni lettera, e
 * chiede alla finestra di scorrere lì. La lettera accesa è quella del primo
 * artista in vista, che la finestra sa senza osservare niente.
 */
import { useMemo, useRef } from "react";
import type { ReactNode, RefObject } from "react";

import { ALTRO, LETTERE, indiceAlfabetico } from "../alfabeto";
import { Copertina } from "../Copertina";
import type { Artista } from "../ipc";
import { Icona } from "../parti/Icone";
import { t, useLingua } from "../lingue";
import { brani_, nomeArtista } from "../formato";
import { conIndice } from "../segnali";
import { pagliaDiGriglia, useGrigliaVirtuale } from "./Libreria";

export function Artisti({
  artisti,
  onApri,
  onMenu,
  scorrevole,
  memoria,
}: {
  artisti: Artista[];
  onApri: (artista: Artista) => void;
  onMenu: (e: React.MouseEvent, artista: Artista) => void;
  /** Lo scorrevole: senza, la griglia si disegna per intero (lo Studio). */
  scorrevole?: RefObject<HTMLDivElement | null> | undefined;
  /** Lo scorrimento si ricorda: solo sotto un `<Activity>`. */
  memoria?: boolean | undefined;
}) {
  /*
   * La lettera di ogni artista, quali lettere hanno qualcuno sotto, e dove
   * comincia ognuna: è lì che salta l'indice. Nella lingua di chi legge, perché
   * «ö» è una «o» per un italiano e un'altra lettera per uno svedese — vedi
   * `alfabeto.ts`. Cambiando lingua l'indice si rifà.
   */
  const lingua = useLingua();
  const { lettere, piene, ancore } = useMemo(
    () => indiceAlfabetico(artisti.map((a) => a.sortName), lingua),
    [artisti, lingua],
  );

  const griglia = useRef<HTMLDivElement>(null);
  const { finestra, intera, primo, ultimo, fuoco, fermata } = useGrigliaVirtuale({
    totale: artisti.length,
    griglia,
    scorrevole,
    selettore: ".scheda-artista",
    selettoreMisura: ".scheda-artista",
    memoria,
  });

  /**
   * La lettera sotto cui si sta guardando: `aria-current` sulla sua voce
   * dell'indice, che per una skin è lo stato «corrente».
   *
   * È la lettera del primo artista in vista: la finestra lo sa già, e prima
   * serviva un `IntersectionObserver` sulle schede-ancora che con la finestra
   * non sarebbero più nel documento.
   */
  const corrente = intera ? null : (lettere[finestra.visibileDa] ?? null);

  const salta = (l: string) => {
    const indice = ancore.get(l);
    if (indice === undefined) return;
    // Scivola; col movimento ridotto — del sistema o dell'app — `scorriA` salta.
    finestra.scorriA(indice, "smooth");
  };

  if (artisti.length === 0) {
    return (
      <div className="vuoto empty-state">
        <span className="empty-icon" aria-hidden="true">
          <Icona nome="i-artist" dim={30} />
        </span>
        <h2>{t("artists.empty.title")}</h2>
        <p>{t("artists.empty.hint")}</p>
      </div>
    );
  }

  /*
   * Quanto è largo un tassello del mosaico, per `sizes`: la colonna intera con
   * una o due copertine — con due, ognuna è alta quanto il mosaico — e metà con
   * tre o quattro.
   */
  const colonna = finestra.larghezzaColonna;
  const schede: ReactNode[] = [];
  for (let i = primo; i < ultimo; i += 1) {
    const a = artisti[i];
    if (a === undefined) continue;
    const quante = Math.min(a.covers.length, 4);
    const tassello = colonna === null ? null : quante > 2 ? colonna / 2 : colonna;
    schede.push(
      <button
        key={a.name}
        type="button"
        className="scheda-artista artist-card"
        style={conIndice(i)}
        data-indice={i}
        tabIndex={i === fermata ? 0 : -1}
        onFocus={() => fuoco.segna(i)}
        onClick={() => onApri(a)}
        onContextMenu={(e) => onMenu(e, a)}
      >
        <span
          className="mosaico artist-mosaic"
          data-quante={quante}
          aria-hidden="true"
        >
          {a.covers.length === 0 ? (
            <span className="senza">
              <Icona nome="i-artist" dim={22} />
            </span>
          ) : (
            a.covers.map((hash) => (
              <Copertina
                key={hash}
                hash={hash}
                titolo={nomeArtista(a.name)}
                classe="tassello"
                misura="scheda"
                larghezza={tassello}
                accanto
              />
            ))
          )}
        </span>
        {/* `dir="auto"`: un nome in arabo o in ebraico si legge da destra, e
            la direzione la decide il nome, non la pagina. */}
        <span className="nome" title={nomeArtista(a.name)} dir="auto">
          {nomeArtista(a.name)}
        </span>
        <span className="quanti">
          {t("artists.counts", {
            album: t("artists.albums", { n: a.albums }),
            brani: brani_(a.tracks),
          })}
        </span>
      </button>,
    );
  }

  return (
    <div className="artisti">
      <div
        className="griglia-artisti track-grid"
        ref={griglia}
        onKeyDown={fuoco.daTastiera}
        data-scorso={finestra.scorso || undefined}
      >
        {!intera && pagliaDiGriglia("sopra", finestra.sopra, finestra)}
        {schede}
        {!intera && pagliaDiGriglia("sotto", finestra.sotto, finestra)}
      </div>

      <nav className="indice-alfabetico alpha-index" aria-label={t("artists.index.aria")}>
        {LETTERE.map((l) => (
          <button
            key={l}
            type="button"
            className="lettera alpha-letter"
            aria-current={corrente === l ? "location" : undefined}
            disabled={!piene.has(l)}
            aria-label={t("artists.index.letter", { lettera: l.toUpperCase() })}
            onClick={() => salta(l)}
          >
            {l.toUpperCase()}
          </button>
        ))}
        {piene.has(ALTRO) && (
          <button
            type="button"
            className="lettera alpha-letter"
            aria-current={corrente === ALTRO ? "location" : undefined}
            aria-label={t("artists.index.other")}
            onClick={() => salta(ALTRO)}
          >
            {ALTRO}
          </button>
        )}
      </nav>
    </div>
  );
}
