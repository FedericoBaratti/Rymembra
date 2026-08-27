/**
 * La lettura di un link, mentre dura.
 *
 * # Perché era muta, e cosa è cambiato
 *
 * Non esisteva nessuna famiglia `import:*`: la lettura di un link era una
 * chiamata bloccante silenziosa dietro il tasto «Guarda», e una playlist da
 * trecento brani sono duecento pagine dietro un `<p>Lettura in corso…</p>`. Con
 * `import:avanzamento` la barra è vera e porta le pagine.
 *
 * # Perché le pagine e non i secondi
 *
 * Perché i secondi li può contare la finestra, ma non vogliono dire niente: due
 * letture della stessa durata hanno numeri di pagine diversi, e l'unica domanda
 * che ci si fa aspettando è «sta andando avanti?». `pagine` può arrivare `null`
 * — il livello che risponde non sempre sa quante saranno — e allora la barra
 * diventa **indeterminata** invece di stimare: meglio dire «non lo so» che
 * disegnare una previsione.
 *
 * # Perché non c'è un tasto Annulla
 *
 * L'abort del lettore è cablato a «no». Un tasto che non annulla è peggio della
 * sua assenza, quindi al suo posto c'è la frase che dice cosa succede se si
 * chiude: la lettura continua di là, e la si ritrova fatta.
 */
import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";

import type { AvanzamentoImport } from "../ipc";
import { numero } from "../formato";
import { t } from "../lingue";

/** Le frasi di `nomeSorgente()`, al presente: la stessa scala, in corso d'opera. */
function staLeggendo(sorgente: string): string {
  switch (sorgente) {
    case "archive.org":
      return t("reading.archive");
    case "jamendo":
      return t("reading.jamendo");
    case "audius":
      return t("reading.audius");
    default:
      return t("reading.generic");
  }
}

/**
 * L'avanzamento della lettura in corso, o `null` quando non ce n'è una.
 *
 * Si sottoscrive **solo** mentre serve, e azzera quando `attivo` torna falso:
 * l'avanzamento della lettura di prima, lasciato in piedi, farebbe cominciare la
 * barra nuova dal punto in cui era arrivata quella vecchia.
 */
export function useLetturaLink(attivo: boolean): AvanzamentoImport | null {
  const [avanzamento, setAvanzamento] = useState<AvanzamentoImport | null>(
    null,
  );

  useEffect(() => {
    if (!attivo) {
      setAvanzamento(null);
      return;
    }
    const promessa = listen<AvanzamentoImport>("import:avanzamento", (evento) =>
      setAvanzamento(evento.payload),
    );
    return () => {
      void promessa.then((stop) => stop());
    };
  }, [attivo]);

  return avanzamento;
}

/**
 * Il segnaposto della scheda d'anteprima, della misura di quella vera.
 *
 * La stessa disciplina degli elenchi della libreria: un segnaposto della misura
 * di quel che sostituisce, così l'arrivo dei dati non fa saltare niente. Qui è
 * una scheda sola, ma la finestrella è larga 560 px e un salto di settanta pixel
 * a metà lettura sposta il tasto sotto il dito.
 */
export function LetturaLink({
  avanzamento,
}: {
  avanzamento: AvanzamentoImport | null;
}) {
  // Una pagina sola non è un avanzamento: la barra sarebbe già piena prima di
  // dire qualcosa.
  const indeterminata =
    avanzamento === null ||
    avanzamento.pagine === null ||
    avanzamento.pagine <= 1;
  const percento =
    avanzamento !== null &&
    avanzamento.pagine !== null &&
    avanzamento.pagine > 0
      ? Math.min(
          100,
          Math.round((avanzamento.pagina / avanzamento.pagine) * 100),
        )
      : 0;

  return (
    <div className="lettura-link">
      <div className="finta-anteprima" aria-hidden="true">
        <div className="copertina skeleton" />
        <div className="chi">
          <div className="riga-finta skeleton" />
          <div className="riga-finta corta skeleton" />
        </div>
      </div>

      <div
        className="avanzamento"
        role="progressbar"
        aria-label={t("reading.aria")}
        // Indeterminata: i tre `aria-value*` si **omettono** invece di mettere
        // zero. Uno zero dichiarato è un'informazione, e qui non ce l'abbiamo.
        {...(indeterminata
          ? {}
          : {
              "aria-valuenow": percento,
              "aria-valuemin": 0,
              "aria-valuemax": 100,
            })}
      >
        <div className="cosa">
          <span>
            {avanzamento === null
              ? t("reading.link")
              : staLeggendo(avanzamento.sorgente)}
          </span>
          {avanzamento !== null && (
            <span className="numeri">
              {avanzamento.pagine !== null
                ? t("reading.counts", {
                    n: numero(avanzamento.brani),
                    pagina: avanzamento.pagina,
                    pagine: avanzamento.pagine,
                  })
                : t("reading.counts.open", {
                    n: numero(avanzamento.brani),
                    pagina: avanzamento.pagina,
                  })}
            </span>
          )}
        </div>
        <div className="barra" data-indeterminata={indeterminata || undefined}>
          <div
            className="riempimento"
            style={indeterminata ? undefined : { width: `${percento}%` }}
          />
        </div>
      </div>

      <p className="nota">{t("reading.keepsGoing")}</p>
    </div>
  );
}
