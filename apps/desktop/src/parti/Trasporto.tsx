/**
 * I comandi di riproduzione: casuale, precedente, riproduci, successivo, ripeti.
 *
 * # Perché una taglia e non cinque proprietà
 *
 * Le tre copie da cui viene questo componente differivano per tre cose insieme
 * — la misura delle icone, la classe del contenitore e i suggerimenti — e le
 * tre cose non variavano indipendentemente: erano tre configurazioni intere,
 * non otto combinazioni. Una parola chiusa le nomina; cinque booleani
 * lascerebbero scrivere le cinque combinazioni che non esistono.
 *
 * È anche la forma che questa scelta avrà quando arriverà nel registro dei
 * widget: là è un `OptionKind::Word`, cioè un segmentato nello Studio.
 */
import { ipc, type StatoRiproduzione } from "../ipc";
import { Icona } from "./Icone";
import { t } from "../lingue";

/**
 * Come si legge un modo di ripetizione, per chi usa uno screen reader.
 *
 * Una funzione e non una costante: una tabella costruita all'apertura del
 * modulo si porterebbe dietro la lingua di quel momento, e cambiarla dalle
 * impostazioni lascerebbe queste tre etichette indietro. Vale per ogni tabella
 * di testi di questo albero.
 */
function ripetizione(): Record<StatoRiproduzione["ripeti"], string> {
  return {
    off: t("player.repeat.off"),
    one: t("player.repeat.one"),
    all: t("player.repeat.all"),
  };
}

/** Dove sta il trasporto, e quindi quanto è grande. */
export type TagliaTrasporto = "barra" | "colonna" | "grande";

/**
 * Le tre configurazioni, per esteso.
 *
 * `classe` conta quanto le misure: ogni regola che disegna questi tasti è
 * agganciata al genitore (`.lettore .tasti`, `.colonna .trasporto`,
 * `.in-riproduzione .trasporto`), quindi il nome del contenitore non è
 * decorativo — è metà del selettore.
 */
const MISURE = {
  barra: {
    icona: 17,
    play: 20,
    classe: "tasti",
    titoli: true,
    scorciatoia: false,
  },
  colonna: {
    icona: 17,
    play: 20,
    classe: "trasporto np-transport",
    titoli: true,
    // Il nome del tasto è una parola come le altre: «Space» non si legge in
    // italiano più di quanto «Spazio» si legga in inglese.
    scorciatoia: true,
  },
  grande: {
    icona: 18,
    play: 24,
    classe: "trasporto np-transport",
    titoli: false,
    scorciatoia: false,
  },
} as const;

export function Trasporto({
  stato,
  taglia,
  conMescola = true,
  conRipeti = true,
  onErrore,
}: {
  stato: StatoRiproduzione;
  taglia: TagliaTrasporto;
  /** Il tasto che mescola la coda. */
  conMescola?: boolean;
  /** Il tasto che ripete, nei suoi tre stati. */
  conRipeti?: boolean;
  onErrore: (e: unknown) => void;
}) {
  const { icona, play, classe, titoli, scorciatoia } = MISURE[taglia];

  const comanda = (azione: Promise<void>) => {
    azione.catch(onErrore);
  };

  /** Un suggerimento, dove la taglia ne vuole. */
  const forse = (testo: string) => (titoli ? testo : undefined);

  return (
    <div className={classe}>
      {/* Le due modalità si possono togliere; i tre tasti in mezzo no. Non è
          una scelta di questo file: è `Essential` nel registro dei widget, e
          qui si vede solo l'assenza della manopola corrispondente. */}
      {conMescola && (
        <button
          type="button"
          className="tasto icon-btn"
          aria-pressed={stato.shuffle}
          aria-label={t("player.shuffle")}
          title={forse(t("player.shuffle"))}
          onClick={() => comanda(ipc.mescola())}
        >
          <Icona nome="i-shuffle" dim={icona} />
        </button>
      )}
      <button
        type="button"
        className="tasto icon-btn"
        aria-label={t("player.previous")}
        title={forse(t("player.previous.short"))}
        onClick={() => comanda(ipc.precedente())}
      >
        <Icona nome="i-prev" dim={icona} />
      </button>
      <button
        type="button"
        className="tasto grande play-btn-primary"
        aria-label={stato.inPausa ? t("player.resume") : t("player.pause")}
        title={forse(
          `${stato.inPausa ? t("player.resume") : t("player.pause.short")}${
            scorciatoia ? ` · ${t("keys.space")}` : ""
          }`,
        )}
        onClick={() => comanda(ipc.alterna())}
      >
        <Icona nome={stato.inPausa ? "i-play" : "i-pause"} dim={play} />
      </button>
      <button
        type="button"
        className="tasto icon-btn"
        aria-label={t("player.next")}
        title={forse(t("player.next.short"))}
        onClick={() => comanda(ipc.prossimo())}
      >
        <Icona nome="i-next" dim={icona} />
      </button>
      {conRipeti && (
        <button
          type="button"
          className="tasto icon-btn"
          aria-pressed={stato.ripeti !== "off"}
          aria-label={ripetizione()[stato.ripeti]}
          title={forse(ripetizione()[stato.ripeti])}
          onClick={() => comanda(ipc.ripeti())}
        >
          <Icona nome="i-repeat" dim={icona} />
          {/* Il «1» resta: le tre modalità hanno un'icona sola, e senza questo
              segno «ripeti tutto» e «ripeti questo» sarebbero indistinguibili a
              occhio — la differenza starebbe solo nell'etichetta letta da uno
              screen reader. */}
          {stato.ripeti === "one" && <span className="modo">1</span>}
        </button>
      )}
    </div>
  );
}
