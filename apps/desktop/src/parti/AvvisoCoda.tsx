/**
 * «Coda sostituita — Annulla».
 *
 * # Perché un annulla e non una conferma
 *
 * Perché il gesto che sostituisce la coda è quasi sempre voluto, e una domanda
 * a ogni doppio clic insegnerebbe a rispondere «sì» senza leggere — cioè a
 * perdere la coda comunque, con un clic in più. Il caso che ha fatto nascere
 * questo avviso è l'altro: un Invio su una cartella che ha buttato via una
 * coda da duecento brani, senza nessuna strada per riaverla.
 *
 * Quando l'avviso vale lo decide il nucleo (`riproduzione::coda`): una coda
 * costruita a mano, una coda lunga, o «Svuota». Una coda nata da un album che
 * lascia il posto a un altro album non manda niente, e qui non arriva niente.
 *
 * # Perché il numero
 *
 * L'evento porta il numero della sostituzione, e «Annulla» lo ripassa. Se nei
 * secondi in cui l'avviso è a schermo la coda è stata sostituita di nuovo, il
 * nucleo risponde `false` e non tocca niente: rimettere la coda di prima del
 * gesto **successivo** sarebbe un annulla che non annulla quel che dice.
 */
import { useEffect, useState } from "react";

import { ipc } from "../ipc";
import { useAscolto } from "../pagine";
import { Icona } from "./Icone";
import { t } from "../lingue";

/**
 * Quanto resta a schermo, in millisecondi.
 *
 * Dieci secondi, più dei sei dell'uscita spostata in `AvvisoAudio`: quello si
 * legge e basta, questo chiede un gesto, e chi si accorge di aver perso la coda
 * se ne accorge quando la musica cambia — un paio di secondi dopo il clic.
 */
const DURATA = 10_000;

/** Quel che arriva con `coda:sostituita`. */
interface CodaSostituita {
  numero: number;
  brani: number;
  svuotata: boolean;
}

export function AvvisoCoda({ onErrore }: { onErrore: (e: unknown) => void }) {
  const [avviso, setAvviso] = useState<CodaSostituita | null>(null);

  useAscolto<CodaSostituita>("coda:sostituita", setAvviso);

  // Un avviso nuovo riparte da dieci secondi: ogni evento è un oggetto nuovo,
  // quindi il conto si rifà anche quando uno ne sostituisce un altro a schermo.
  useEffect(() => {
    if (avviso === null) return;
    const conto = window.setTimeout(() => setAvviso(null), DURATA);
    return () => window.clearTimeout(conto);
  }, [avviso]);

  if (avviso === null) return null;

  const brani =
    avviso.brani === 1
      ? t("format.tracks.uno")
      : t("format.tracks", { n: avviso.brani });
  return (
    <div className="notizia avviso-coda toast-card" role="status">
      <Icona nome="i-check" dim={16} />
      <span>
        {avviso.svuotata
          ? t("queue.undo.cleared", { brani })
          : t("queue.undo.replaced", { brani })}
      </span>
      <button
        type="button"
        className="bottone minuto btn-ghost"
        title={t("queue.undo.aria")}
        onClick={() => {
          const numero = avviso.numero;
          setAvviso(null);
          ipc.codaRipristina(numero).catch(onErrore);
        }}
      >
        {t("queue.undo")}
      </button>
      <button
        type="button"
        className="tasto icon-btn"
        aria-label={t("common.close")}
        onClick={() => setAvviso(null)}
      >
        <Icona nome="i-x" dim={14} />
      </button>
    </div>
  );
}
