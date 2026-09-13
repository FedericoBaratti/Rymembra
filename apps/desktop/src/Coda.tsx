/**
 * La coda: le righe, e il pannello che le conteneva.
 *
 * # Perché gli indici vengono dalla coda e non dalle righe
 *
 * `StatoRiproduzione.coda` sono identificativi; le righe si chiedono a parte con
 * `braniPerId`. Le due liste **non hanno la stessa lunghezza**: un brano tolto
 * dalla libreria mentre stava in coda non torna indietro — «gli identificativi
 * che non esistono più semplicemente non compaiono», dice `summaries_by_id`.
 *
 * Disegnare le righe ricevute e usarne la posizione vorrebbe dire mandare a
 * `coda_vai` un indice spostato di uno per ogni brano sparito, cioè far partire
 * una canzone diversa da quella cliccata. Perciò si cammina sulla coda — che è
 * la verità — e le righe si cercano per identificativo.
 *
 * # Perché il file è diviso in tre
 *
 * La coda ora si vede in due posti: nella terza colonna, che è la sua casa, e
 * nel pannello flottante, che torna quando la colonna è chiusa. Sono la stessa
 * lista con due cornici, e duplicarla vorrebbe dire correggere due volte ogni
 * difetto del riordino. Quindi: un gancio che tiene le righe, un elenco che le
 * disegna, e un pannello che è solo la cornice.
 */
import { useEffect, useState } from "react";

import { durata, nomeArtista, titoloAlbum } from "./formato";
import { ipc, type Brano, type StatoRiproduzione } from "./ipc";
import { Icona } from "./parti/Icone";
import { t, tSe } from "./lingue";

/**
 * Le righe della coda, per identificativo.
 *
 * La chiave è la coda unita con le virgole e non l'array: React confronta per
 * identità, e ogni evento `riproduzione:stato` porta un array nuovo con dentro
 * gli stessi numeri. Il nucleo ne manda quattro al secondo — senza questo, ogni
 * secondo partirebbero quattro richieste di righe identiche.
 */
export function useRigheCoda(
  coda: readonly number[],
  onErrore: (e: unknown) => void,
): Map<number, Brano> {
  const [righe, setRighe] = useState<Map<number, Brano>>(new Map());
  const chiave = coda.join(",");

  useEffect(() => {
    const ids = chiave.length > 0 ? chiave.split(",").map(Number) : [];
    if (ids.length === 0) {
      setRighe(new Map());
      return;
    }
    let annullato = false;
    ipc
      .braniPerId(ids)
      .then((trovate) => {
        if (annullato) return;
        setRighe(new Map(trovate.map((b) => [b.id, b])));
      })
      .catch(onErrore);
    return () => {
      annullato = true;
    };
  }, [chiave, onErrore]);

  return righe;
}

/**
 * L'elenco della coda, riordinabile.
 *
 * # Perché il riordino ha anche una scorciatoia
 *
 * Il trascinamento HTML5 non esiste per chi non usa il mouse: `dragstart` nasce
 * da un puntatore, e nessuna combinazione di tasti lo produce. Finché il
 * riordino era **solo** trascinabile, riordinare la coda era una funzione che
 * una parte degli utenti non aveva — non «scomoda», assente.
 *
 * `Alt`+`↑↓` sposta la riga a fuoco. Alt e non le frecce nude perché quelle
 * devono continuare a muovere il fuoco: sono due gesti diversi sullo stesso
 * tasto, e il modificatore è quel che li distingue.
 */
export function RigheCoda({
  stato,
  righe,
  onErrore,
  compatta,
}: {
  stato: StatoRiproduzione;
  righe: Map<number, Brano>;
  onErrore: (e: unknown) => void;
  /** Nella terza colonna: niente durata, la larghezza non c'è. */
  compatta?: boolean | undefined;
}) {
  const [trascinato, setTrascinato] = useState<number | null>(null);
  /**
   * Su quale riga cadrebbe il rilascio adesso.
   *
   * Il trascinamento c'era e non lo diceva: si lasciava andare e si scopriva
   * dopo dov'era finita la riga. È lo stesso segno che ora porta l'elenco di
   * una playlist.
   */
  const [mirato, setMirato] = useState<number | null>(null);

  const comanda = (azione: Promise<void>) => {
    azione.catch(onErrore);
  };

  const posa = () => {
    setTrascinato(null);
    setMirato(null);
  };

  const lascia = (a: number) => {
    if (trascinato === null || trascinato === a) {
      posa();
      return;
    }
    comanda(ipc.codaRiordina(trascinato, a));
    posa();
  };

  const daTastiera = (e: React.KeyboardEvent, indice: number) => {
    if (!e.altKey) return;
    const passo = e.key === "ArrowDown" ? 1 : e.key === "ArrowUp" ? -1 : 0;
    if (passo === 0) return;
    const a = indice + passo;
    if (a < 0 || a >= stato.coda.length) return;
    e.preventDefault();
    comanda(ipc.codaRiordina(indice, a));
    // Il fuoco segue la riga spostata invece di restare sulla posizione: chi
    // tiene premuto Alt e freccia sta spostando **una** canzone, e lasciare il
    // fuoco fermo farebbe scendere quella dopo al colpo successivo.
    const lista = e.currentTarget.parentElement?.parentElement;
    const arrivo = lista?.children[a]?.querySelector<HTMLElement>(".salta");
    window.requestAnimationFrame(() => arrivo?.focus());
  };

  if (stato.coda.length === 0) {
    return <p className="vuota-coda empty-state">{t("queue.empty")}</p>;
  }

  return (
    <ol className="righe-coda queue-list" data-compatta={compatta || undefined}>
      {stato.coda.map((id, indice) => {
        const brano = righe.get(id);
        const inAscolto = indice === stato.posizioneCoda;
        /* Il perché sta solo sul brano **subito dopo** quello in ascolto, ed è
           l'unico che ce l'ha: il nucleo spiega la scelta che ha appena fatto,
           non tutta la coda. Le righe più in là le hai messe tu, o le sceglierà
           quando ci arriverà — e una frase su una scelta non ancora presa
           sarebbe inventata. */
        const codice =
          stato.posizioneCoda !== null &&
          indice === stato.posizioneCoda + 1
            ? stato.motivoProssimo
            : null;
        /* `tSe` e non `t`: il codice arriva dal nucleo, quindi TypeScript non
           può verificarlo. Il ripiego è la stringa vuota, e una stringa vuota
           non disegna niente — un motivo che l'interfaccia non conosce si tace
           invece di mostrare la propria chiave. */
        const perche = codice === null ? "" : tSe(`queue.why.${codice}`, "");
        return (
          <li
            /* L'indice fa parte della chiave: la stessa canzone può stare due
               volte nella stessa coda, e l'identificativo da solo non la
               distinguerebbe. */
            key={`${indice}-${id}`}
            className="riga-coda list-row"
            aria-current={inAscolto}
            data-active={inAscolto || undefined}
            data-sopra={(mirato === indice && trascinato !== indice) || undefined}
            draggable
            onDragStart={() => setTrascinato(indice)}
            onDragOver={(e) => {
              e.preventDefault();
              setMirato(indice);
            }}
            onDrop={() => lascia(indice)}
            onDragEnd={posa}
            onDoubleClick={() => comanda(ipc.codaVai(indice))}
          >
            <span className="presa" aria-hidden="true">
              <Icona nome={inAscolto ? "i-play" : "i-grip"} dim={13} />
            </span>
            <button
              type="button"
              className="salta"
              onClick={() => comanda(ipc.codaVai(indice))}
              onKeyDown={(e) => daTastiera(e, indice)}
              /* Un brano sparito dalla libreria non si può far partire, ma si
                 deve poter togliere: il tasto si spegne, la riga no. */
              disabled={!brano}
            >
              <span className="nome">
                {brano ? brano.title : t("queue.goneTrack")}
              </span>
              <span className="autore">
                {brano
                  ? `${nomeArtista(brano.artist)} · ${titoloAlbum(brano.album)}`
                  : `id ${id}`}
              </span>
              {perche !== "" && !compatta && (
                <span className="perche">{perche}</span>
              )}
            </button>
            {!compatta && (
              <span className="durata">
                {brano ? durata(brano.durationMs) : "—"}
              </span>
            )}
            <button
              type="button"
              className="tasto icon-btn"
              /* Col titolo dentro, come fa la × dell'elenco. «Togli dalla coda»
                 era la stessa frase su tutte le righe: chi guarda sa quale × ha
                 sotto il dito, chi ascolta sentiva cinquanta volte la stessa
                 cosa senza sapere quale brano stava per perdere. Il brano non più
                 in libreria si nomina come si nomina nella riga — non c'è altro
                 nome da dargli. */
              aria-label={t("queue.remove.aria", {
                titolo: brano ? brano.title : t("queue.goneTrack"),
              })}
              /* Il `title` resta la frase corta: al passaggio del mouse il titolo
                 del brano è già scritto accanto, a due centimetri dal puntatore. */
              title={t("queue.remove")}
              onClick={() => comanda(ipc.codaTogli(indice))}
            >
              <Icona nome="i-x" dim={14} />
            </button>
          </li>
        );
      })}
    </ol>
  );
}

/**
 * Il pannello flottante della coda.
 *
 * Torna a servire solo quando la terza colonna è chiusa: con la colonna aperta
 * la coda è già visibile, e aprirne una seconda copia sopra sarebbe la stessa
 * lista due volte nella stessa finestra.
 */
export function Coda({
  stato,
  onChiudi,
  onErrore,
}: {
  stato: StatoRiproduzione;
  onChiudi: () => void;
  onErrore: (e: unknown) => void;
}) {
  const righe = useRigheCoda(stato.coda, onErrore);

  return (
    <aside
      className="pannello-coda glass-modal"
      // L'ancora del giro guidato: il ripiego del passo della colonna, per chi
      // la colonna la tiene chiusa.
      data-giro="coda"
      aria-label={t("queue.panel.aria")}
    >
      <header>
        <h2>{t("queue.panel.title")}</h2>
        <span className="conteggio">{stato.coda.length}</span>
        <button
          type="button"
          className="bottone minuto btn-ghost"
          disabled={stato.coda.length === 0}
          onClick={() => {
            ipc.codaSvuota().catch(onErrore);
          }}
        >
          {t("queue.clear")}
        </button>
        <button
          type="button"
          className="tasto icon-btn"
          aria-label={t("queue.close")}
          onClick={onChiudi}
        >
          <Icona nome="i-x" dim={15} />
        </button>
      </header>
      <RigheCoda stato={stato} righe={righe} onErrore={onErrore} />
    </aside>
  );
}
