/**
 * Riassegnare i tasti.
 *
 * # Perché si cattura la pressione invece di scrivere il nome
 *
 * Un campo in cui digitare `Ctrl+Shift+K` è un campo in cui si sbaglia: la
 * stessa combinazione si scrive in sei modi, e nessuno sa come chiamare il
 * tasto fra `Ctrl` e `Alt` sulla propria tastiera. Qui si preme il tasto e si
 * legge quel che è arrivato — la stessa funzione che poi lo riconoscerà,
 * `tastoDi`, quindi non esiste una combinazione che si può registrare e non
 * scattare.
 *
 * # Perché un conflitto si dice invece di rifiutarsi
 *
 * Assegnare Spazio a due comandi è un errore, ma il momento in cui lo si
 * commette è a metà di un lavoro: si sta spostando una scorciatoia da un
 * comando all'altro, e per un istante ce l'hanno tutti e due. Rifiutare la
 * seconda assegnazione obbligherebbe a fare i passi nell'ordine giusto senza
 * dirlo. Si accetta, si dichiara, e chi legge sistema. Nel frattempo
 * l'ascoltatore ha comunque una regola definita — vince il primo dell'elenco —
 * quindi non c'è nessun istante di comportamento imprevedibile.
 */
import { useState } from "react";

import { Icona } from "./Icone";
import { t } from "../lingue";
import {
  DI_SERIE,
  type Associazioni,
  type Comando,
  comandi,
  conflitti,
  serieMutabile,
  tastoDi,
  tastoScritto,
} from "../tastiera";
import { Trans } from "../lingue/Trans";

/** I tasti che non si lasciano assegnare, e perché. */
function vietati(): Record<string, string> {
  return {
    Escape: t("shortcuts.forbidden.escape"),
    Tab: t("shortcuts.forbidden.tab"),
    Enter: t("shortcuts.forbidden.enter"),
  };
}

export function Scorciatoie({
  associazioni,
  onCambia,
}: {
  associazioni: Associazioni;
  /** Le nuove associazioni, già intere: le scrive chi le riceve. */
  onCambia: (a: Associazioni) => void;
}) {
  /** Il comando che sta aspettando una pressione, o `null`. */
  const [inAscolto, setInAscolto] = useState<Comando | null>(null);
  /** Perché l'ultima pressione non è stata presa. */
  const [rifiutato, setRifiutato] = useState<string | null>(null);
  const contesi = conflitti(associazioni);

  /** Sostituisce le associazioni di un comando. */
  const scrivi = (comando: Comando, tasti: string[]) => {
    onCambia({ ...associazioni, [comando]: tasti });
  };

  const cattura = (comando: Comando, e: React.KeyboardEvent) => {
    e.preventDefault();
    // Ferma la propagazione, altrimenti la stessa pressione arriverebbe anche
    // all'ascoltatore globale: assegnare lo Spazio metterebbe in pausa.
    e.stopPropagation();
    // Escape non si assegna e non è nemmeno un rifiuto da spiegare: qui vuol
    // dire quel che vuole dire dappertutto, cioè «lascia stare».
    if (e.key === "Escape") {
      setInAscolto(null);
      setRifiutato(null);
      return;
    }
    const tasto = tastoDi(e);
    // `null` = è stato premuto solo un modificatore: la combinazione si sta
    // ancora componendo, e concludere adesso registrerebbe «Ctrl» da solo.
    if (tasto === null) return;
    const vietato = vietati()[e.key];
    if (vietato !== undefined) {
      setRifiutato(vietato);
      return;
    }
    setRifiutato(null);
    setInAscolto(null);
    // Il tasto **si aggiunge** invece di sostituire: «Cerca» ne ha due di
    // serie, e chi ne aggiunge uno non sta chiedendo di perdere l'altro.
    if (associazioni[comando].includes(tasto)) return;
    scrivi(comando, [...associazioni[comando], tasto]);
  };

  return (
    <>
      <p className="nota">
        <Trans
          k="shortcuts.intro"
          v={{
            assegna: <strong>{t("shortcuts.intro.assign")}</strong>,
            esc: <code>{t("keys.esc")}</code>,
          }}
        />
      </p>

      <ul className="cartelle">
        {comandi().map(({ chiave, titolo, spiegazione }) => (
          <li className="cartella scorciatoia" key={chiave}>
            <div className="che-cosa">
              <div className="etichetta">{titolo}</div>
              <div className="spiegazione">{spiegazione}</div>
            </div>
            <div className="tasti">
              {associazioni[chiave].map((tasto) => (
                <button
                  key={tasto}
                  type="button"
                  className="tasto-scorciatoia"
                  title={
                    contesi.has(tasto)
                      ? t("shortcuts.contested", { tasto: tastoScritto(tasto) })
                      : t("shortcuts.removeKey", { tasto: tastoScritto(tasto) })
                  }
                  data-conteso={contesi.has(tasto) || undefined}
                  onClick={() =>
                    scrivi(
                      chiave,
                      associazioni[chiave].filter((t) => t !== tasto),
                    )
                  }
                >
                  <kbd>{tastoScritto(tasto)}</kbd>
                  <Icona nome="i-x" dim={11} />
                </button>
              ))}
              {associazioni[chiave].length === 0 && (
                <span className="spiegazione">{t("shortcuts.none")}</span>
              )}
              <button
                type="button"
                className="bottone minuto btn-ghost"
                aria-label={t("shortcuts.assignTo", { comando: titolo })}
                data-inascolto={inAscolto === chiave || undefined}
                onKeyDown={(e) => {
                  if (inAscolto === chiave) cattura(chiave, e);
                }}
                onBlur={() => inAscolto === chiave && setInAscolto(null)}
                onClick={() => {
                  setRifiutato(null);
                  setInAscolto(inAscolto === chiave ? null : chiave);
                }}
              >
                {inAscolto === chiave
                  ? t("shortcuts.pressAKey")
                  : t("shortcuts.assign")}
              </button>
            </div>
          </li>
        ))}
      </ul>

      {rifiutato !== null && <p className="nota">{rifiutato}</p>}

      {contesi.size > 0 && (
        <p className="nota">
          <strong>{t("shortcuts.clash")}</strong>:{" "}
          {[...contesi.entries()]
            .map(
              ([tasto, quali]) =>
                `${tastoScritto(tasto)} (${quali
                  .map(
                    (c) => comandi().find((x) => x.chiave === c)?.titolo ?? c,
                  )
                  .join(", ")})`,
            )
            .join("; ")}
          {t("shortcuts.clash.tail")}
        </p>
      )}

      <div className="azioni">
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={comandi().every(
            ({ chiave }) =>
              associazioni[chiave].length === DI_SERIE[chiave].length &&
              associazioni[chiave].every((t, i) => t === DI_SERIE[chiave][i]),
          )}
          onClick={() => onCambia(serieMutabile())}
        >
          {t("shortcuts.reset")}
        </button>
      </div>
    </>
  );
}
