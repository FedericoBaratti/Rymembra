/**
 * La cronologia d'ascolto, sotto la coda nella terza colonna.
 *
 * # Perché arriva così tardi
 *
 * `play_history` si scrive dal primo giorno e non la leggeva nessuno: la
 * linguetta accanto a «Coda» era disegnata spenta, con la sua ragione scritta
 * accanto — «la cronologia si registra, ma non c'è ancora un comando per
 * leggerla». Era un difetto piccolo finché quella tabella conteneva solo gli
 * ascolti fatti dentro Rymembra: su una libreria appena aperta, zero righe sotto
 * una linguetta che nessuno avrebbe premuto due volte.
 *
 * L'importazione di un account Spotify la riempie di anni, e da lì in poi una
 * schermata che non li mostra è la differenza fra aver importato e non averlo
 * fatto.
 *
 * # Perché si ricarica al cambio di brano e non a ogni evento
 *
 * Una riga di cronologia nasce quando un ascolto **finisce**, e un ascolto
 * finisce quando ne comincia un altro o quando la coda si ferma. `riproduzione:
 * stato` arriva quattro volte al secondo: ricaricare a ogni colpo vorrebbe dire
 * quattro query al secondo per una lista che cambia una volta ogni tre minuti.
 * Si guarda invece l'identificativo del brano corrente — quando cambia, quello
 * di prima è appena stato scritto.
 *
 * Resta un caso scoperto: l'ultimo brano di una coda che finisce e basta. La
 * sua riga compare alla riapertura della linguetta, ed è un ritardo che ho
 * scelto di accettare invece di aggiungere un evento apposta.
 */
import { useCallback, useEffect, useState } from "react";

import { data, dataOra, durata, nomeArtista, numero } from "../formato";
import { ipc, type StatoRiproduzione, type VoceCronologia } from "../ipc";
import { locale, t } from "../lingue";
import { Icona } from "./Icone";
import { fasciaQuando } from "./quandoCronologia";
import { useDati } from "../dati";

/** Quante righe per volta. Una schermata piena più un po' di margine. */
const PAGINA = 50;

/**
 * Quando, in una forma che si legge di sfuggita.
 *
 * Quale forma lo decide `fasciaQuando`, per giorni di calendario: con le
 * finestre mobili di prima un ascolto di ieri pomeriggio si leggeva come un
 * orario di oggi, e uno di venerdì scorso come «ven» di oggi. Qui si scrive
 * nella lingua di chi guarda.
 */
function quando(ms: number): string {
  const fascia = fasciaQuando(ms, Date.now());
  const ora = () =>
    new Date(ms).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" });
  switch (fascia.tipo) {
    case "futuro":
      return data(ms);
    case "ora":
      return t("history.now");
    case "minuti":
      return t("history.minutesAgo", { n: fascia.minuti });
    case "oggi":
      return ora();
    case "ieri":
      return t("history.yesterday", { ora: ora() });
    case "settimana":
      return new Date(ms).toLocaleDateString(locale(), { weekday: "short" });
    case "data":
      return new Date(ms).toLocaleDateString(locale(), {
        day: "numeric",
        month: "short",
        ...(fascia.anno ? { year: "2-digit" } : {}),
      });
  }
}

export function Cronologia({
  stato,
  onErrore,
}: {
  stato: StatoRiproduzione;
  onErrore: (e: unknown) => void;
}) {
  const letture = useDati().ipc;
  const [voci, setVoci] = useState<VoceCronologia[]>([]);
  const [totale, setTotale] = useState<number | null>(null);
  const [quante, setQuante] = useState(PAGINA);
  const [caricando, setCaricando] = useState(true);
  const corrente = stato.brano?.id ?? null;

  const comanda = (azione: Promise<void>) => {
    azione.catch(onErrore);
  };

  const carica = useCallback(
    (limite: number) => {
      let annullato = false;
      setCaricando(true);
      Promise.all([letture.cronologia(0, limite), letture.cronologiaConteggio()])
        .then(([pagina, quanti]) => {
          if (annullato) return;
          setVoci(pagina);
          setTotale(quanti);
        })
        .catch(onErrore)
        .finally(() => {
          if (!annullato) setCaricando(false);
        });
      return () => {
        annullato = true;
      };
    },
    [onErrore, letture],
  );

  // `corrente` fra le dipendenze: è il segnale che un ascolto si è chiuso.
  useEffect(() => carica(quante), [carica, quante, corrente]);

  if (caricando && voci.length === 0) {
    return <p className="vuota-coda empty-state">{t("history.loading")}</p>;
  }

  if (voci.length === 0) {
    return <p className="vuota-coda empty-state">{t("history.empty")}</p>;
  }

  return (
    <div className="cronologia">
      {/* `righe-coda` insieme a `righe-cronologia`: la prima porta la scatola
          che scorre — `.coda-in-colonna .righe-coda` è la regola che dà
          `flex: 1` e `overflow-y: auto` — e la seconda le tre differenze.
          Senza, l'elenco cresce oltre la colonna e le righe più vecchie
          restano sotto il bordo della finestra, irraggiungibili. */}
      <ol className="righe-coda righe-cronologia queue-list">
        {voci.map((voce) => (
          <li key={voce.id} className="riga-coda list-row">
            <span className="presa" aria-hidden="true">
              <Icona
                nome={voce.sorgente === "local" ? "i-play" : "i-import"}
                dim={13}
              />
            </span>
            <button
              type="button"
              className="salta"
              /* Fa partire quel brano da solo, non una coda: la cronologia è
                 un elenco di momenti, non una scaletta da rimettere. */
              onClick={() => comanda(ipc.suona([voce.brano.id], 0))}
              title={t(
                voce.sorgente === "local"
                  ? "history.entry.title"
                  : "history.entry.title.imported",
                { titolo: voce.brano.title, quando: dataOra(voce.quandoMs) },
              )}
            >
              <span className="nome">{voce.brano.title}</span>
              <span className="autore">
                {nomeArtista(voce.brano.artist)} · {durata(voce.msAscoltati)}
              </span>
            </button>
            <span className="durata">{quando(voce.quandoMs)}</span>
          </li>
        ))}
      </ol>

      {totale !== null && totale > voci.length && (
        <button
          type="button"
          className="bottone minuto btn-ghost altri-ascolti"
          disabled={caricando}
          onClick={() => setQuante((q) => q + PAGINA)}
        >
          {caricando
            ? t("common.loading")
            : t("history.more", {
                quanti: Math.min(PAGINA, totale - voci.length),
                totale: numero(totale),
              })}
        </button>
      )}
    </div>
  );
}
