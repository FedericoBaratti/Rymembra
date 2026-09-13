/**
 * Il primo avvio: dalla prima apertura alla prima nota.
 *
 * # Cosa c'era prima
 *
 * Due stati vuoti. Il primo diceva «nessuna cartella sorvegliata» e apriva un
 * dialogo di sistema; il secondo, dopo, diceva «manca una scansione» e ne
 * chiedeva una. Erano scritti bene ed erano il problema lo stesso: fra il
 * doppio clic sull'installer e la prima nota c'erano quattro o cinque gesti, e
 * il primo era andare a cercare in un dialogo una cartella che il sistema
 * dichiara da sé.
 *
 * # Le tre cose che fa
 *
 * 1. **Guarda invece di chiedere.** Le cartelle musicali del sistema, contate
 *    una per una, proposte **già spuntate**. Il gesto diventa «Continua».
 * 2. **Non lascia mai una stanza vuota.** Durante la scansione c'è
 *    l'avanzamento, e appena la libreria ha qualcosa dentro il pulsante grande
 *    smette di dire «Continua» e dice «Ascolta».
 * 3. **Si può saltare.** Chi la musica la tiene su un NAS che stasera è spento
 *    non deve restare chiuso qui: «Lo faccio dopo» riporta agli stati vuoti di
 *    sempre, che infatti non sono stati tolti.
 *
 * # Perché non parte da solo
 *
 * Perché la musica che comincia senza che nessuno l'abbia chiesta è
 * esattamente ciò che `playback.rs` argomenta di non fare per l'autoplay, e la
 * ragione non cambia perché è il primo avvio. Il pulsante «Ascolta» costa un
 * clic e lo rende una risposta a un gesto invece di una sorpresa — e resta
 * l'unico clic fra l'installazione e il suono.
 */
import { useCallback, useEffect, useState } from "react";

import { useFinestrella } from "./finestrella";
import { ipc, type CartellaCandidata } from "./ipc";
import { t } from "./lingue";
import { Icona } from "./parti/Icone";

/** Quel che il primo avvio chiede alla finestra di fare per lui. */
export interface PrimoProps {
  /** Scrive le cartelle e avvia la scansione. */
  onCartelle: (cartelle: string[]) => Promise<void>;
  /** Apre il dialogo di sistema e restituisce la cartella scelta. */
  onScegliCartella: () => Promise<string | null>;
  /** L'avanzamento della scansione, o `null` se non ne sta girando nessuna. */
  scansione: { fatti: number; totale: number } | null;
  /** Quanti brani ci sono in libreria adesso. */
  brani: number;
  /** Fa partire la musica e chiude. */
  onAscolta: () => void;
  /** Chiude e basta. */
  onSalta: () => void;
}

/** Come si scrive il numero di brani di una cartella. */
function quantiBrani(cartella: CartellaCandidata): string {
  return cartella.troncato
    ? t("primo.braniOltre", { n: cartella.brani })
    : t("primo.brani", { n: cartella.brani });
}

/** La schermata di benvenuto. */
export function Primo({
  onCartelle,
  onScegliCartella,
  scansione,
  brani,
  onAscolta,
  onSalta,
}: PrimoProps) {
  const [candidate, setCandidate] = useState<CartellaCandidata[] | null>(null);
  const [scelte, setScelte] = useState<Set<string>>(new Set());
  const [inCorso, setInCorso] = useState(false);
  /*
   * Anche qui, e per il motivo più forte di tutti: questa non è una finestrella
   * sopra una pagina, è una schermata piena — e sotto, montata e raggiungibile
   * col tabulatore, c'è tutta l'applicazione (`App.tsx` disegna
   * l'`Impaginazione` subito dopo). Il Tab uscito da qui finiva su una libreria
   * vuota che non si vede nemmeno, con lo sfondo opaco di `.primo` sopra.
   *
   * E non è una trappola vera: la via d'uscita c'è ed è dichiarata, ed è
   * «Lo faccio dopo» — cioè `onSalta`, che è quel che Escape deve fare qui. Chi
   * tiene la musica su un NAS spento stasera non resta chiuso dentro.
   *
   * Il `data-fuoco-iniziale` sulla radice dice all'hook di fermare il fuoco lì
   * invece di darlo a un comando: nell'istante in cui questa schermata si apre
   * le cartelle non sono ancora state contate, quindi «Continua» è spento, e i
   * due che restano aprono un dialogo di sistema o saltano il primo avvio. Un
   * Invio battuto per caso non deve fare nessuna delle due cose.
   */
  const finestrella = useFinestrella<HTMLDivElement>(onSalta);

  // Si guarda una volta sola, all'apertura. La ricerca ha una scadenza dalla
  // parte del nucleo: se una cartella sincronizzata non risponde, torna un
  // elenco vuoto invece di lasciare questa schermata a girare per sempre.
  useEffect(() => {
    let vivo = true;
    void ipc
      .cartelleCandidate()
      .then((trovate) => {
        if (!vivo) return;
        setCandidate(trovate);
        // Già spuntate: è tutto il punto. Chi non le vuole le toglie, che è un
        // gesto in meno di chi doveva aggiungerle.
        setScelte(new Set(trovate.map((c) => c.percorso)));
      })
      .catch(() => {
        // Un guasto qui non è un motivo per bloccare il primo avvio: si
        // ricade sull'elenco vuoto, che ha già il suo testo e il suo pulsante.
        if (vivo) setCandidate([]);
      });
    return () => {
      vivo = false;
    };
  }, []);

  const spunta = useCallback((percorso: string) => {
    setScelte((prima) => {
      const dopo = new Set(prima);
      if (dopo.has(percorso)) dopo.delete(percorso);
      else dopo.add(percorso);
      return dopo;
    });
  }, []);

  const aggiungi = useCallback(async () => {
    const scelta = await onScegliCartella();
    if (scelta === null) return;
    setCandidate((prima) => {
      const elenco = prima ?? [];
      if (elenco.some((c) => c.percorso === scelta)) return elenco;
      return [
        ...elenco,
        { percorso: scelta, brani: 0, troncato: false, parziale: false },
      ];
    });
    setScelte((prima) => new Set(prima).add(scelta));
  }, [onScegliCartella]);

  const continua = useCallback(() => {
    if (scelte.size === 0) return;
    setInCorso(true);
    void onCartelle([...scelte]).finally(() => setInCorso(false));
  }, [onCartelle, scelte]);

  const cercando = candidate === null;
  const scansionando = scansione !== null;
  // Appena c'è qualcosa da sentire il pulsante cambia mestiere, anche se la
  // scansione non è finita: è la differenza fra aspettare e cominciare.
  const puoAscoltare = brani > 0;

  return (
    <div
      ref={finestrella}
      className="primo"
      role="dialog"
      aria-modal="true"
      aria-labelledby="primo-titolo"
      data-fuoco-iniziale
    >
      <div className="primo-scheda">
        <p className="occhiello">{t("primo.eyebrow")}</p>
        <h1 id="primo-titolo">{t("primo.title")}</h1>

        {cercando ? (
          <p className="primo-attesa" role="status">
            {t("primo.cercando")}
          </p>
        ) : candidate.length === 0 ? (
          <>
            <h2 className="primo-sotto">{t("primo.nessuna.title")}</h2>
            <p>{t("primo.nessuna.body")}</p>
          </>
        ) : (
          <>
            <p>{t("primo.body")}</p>
            <ul className="primo-cartelle">
              {candidate.map((cartella) => (
                <li key={cartella.percorso}>
                  <label className="primo-cartella">
                    <input
                      type="checkbox"
                      checked={scelte.has(cartella.percorso)}
                      onChange={() => spunta(cartella.percorso)}
                      disabled={scansionando || inCorso}
                    />
                    <span className="primo-percorso">{cartella.percorso}</span>
                    <span className="primo-conto">{quantiBrani(cartella)}</span>
                  </label>
                  {cartella.parziale && (
                    <p className="primo-avviso">{t("primo.parziale")}</p>
                  )}
                </li>
              ))}
            </ul>
          </>
        )}

        {scansionando && (
          <p className="primo-attesa" role="status">
            {t("primo.scansione")}
            {scansione.totale > 0 && ` ${scansione.fatti}/${scansione.totale}`}
          </p>
        )}

        {brani > 0 && (
          <p className="primo-pronto" role="status">
            {t("primo.pronto", { n: brani })}
          </p>
        )}

        <div className="primo-tasti">
          {puoAscoltare ? (
            <button
              type="button"
              className="bottone primario btn-accent"
              onClick={onAscolta}
            >
              <Icona nome="i-play" dim={16} />
              {t("primo.ascolta")}
            </button>
          ) : (
            <button
              type="button"
              className="bottone primario btn-accent"
              onClick={continua}
              disabled={cercando || scelte.size === 0 || scansionando || inCorso}
            >
              {t("primo.continua")}
            </button>
          )}
          <button
            type="button"
            className="bottone"
            onClick={() => void aggiungi()}
            disabled={scansionando || inCorso}
          >
            {t("primo.altra")}
          </button>
          <button type="button" className="bottone minuto" onClick={onSalta}>
            {t("primo.salta")}
          </button>
        </div>
      </div>
    </div>
  );
}
