/**
 * L'avanzamento della coda, in basso a destra, da qualunque pagina.
 *
 * # Perché è identico a quello della scansione
 *
 * Quello della scansione sta a venti righe di distanza in `App.tsx`, col
 * commento che spiega tutto: nessuno resta a guardare una barra per venti
 * secondi, si torna alla libreria, e l'avanzamento va in basso a destra invece
 * di sparire — sparire farebbe credere che sia finita, o peggio che il cambio di
 * schermata l'abbia annullata.
 *
 * Ogni parola di quel commento vale **di più** per un'operazione che dura
 * sessanta volte tanto e sopravvive al riavvio. La superficie c'era, la ragione
 * era già scritta: quel che mancava era che questo flusso la usasse. Due toast
 * che si comportano in modo diverso sono due sistemi, e chi ha imparato a
 * leggere quello della scansione non deve imparare niente di nuovo.
 *
 * # Le tre cose che dice, e le due che non dice
 *
 * Dice il conteggio, la barra e una porta. Non dice **quale brano** sta
 * scendendo: i fili sono tre, il titolo cambia più volte al secondo, e un titolo
 * che salta in un angolo dello schermo è sfarfallio. Non dice **quanto manca in
 * minuti**: su tre fili e una rete che non conosciamo sarebbe una promessa.
 */
import { useRef } from "react";

import type { StatoScarico } from "../ipc";
import { Annuncio } from "./Annuncio";
import { Icona } from "./Icone";
import type { Rientro } from "./Importazioni";
import { numero } from "../formato";
import { t } from "../lingue";

export function ToastImportazioni({
  stato,
  rientro,
  giaLì,
  onApri,
  onChiudiRientro,
}: {
  stato: StatoScarico | null;
  rientro: Rientro | null;
  /** La pagina delle importazioni è già aperta. */
  giaLì: boolean;
  onApri: () => void;
  onChiudiRientro: () => void;
}) {
  const inAttesa = stato?.conteggi.attesa ?? 0;
  const fatti = stato?.fatti ?? 0;
  const totale = fatti + inAttesa;
  const attiva = stato?.attiva ?? false;
  const rientrate = rientro !== null && rientro.vociRimesse > 0 ? rientro.vociRimesse : 0;

  /*
   * Quel che si dice a chi ascolta lo schermo: i **passaggi**, non il conto.
   *
   * Il riquadro era un `role="status"` col conteggio dentro, cioè la stessa
   * cosa che `PilaToast` toglie alla scansione: «12 di 40», «13 di 40»… per
   * un'ora. E nasceva già pieno, quindi spesso non diceva neanche il primo.
   * Adesso parla una regione sempre montata (`Annuncio.tsx`), e la frase si
   * rifà solo quando la coda cambia fase — parte, si ferma, finisce con un
   * rientro — col conto di quel momento. Sulla pagina delle importazioni tace:
   * là la coda è scritta per intero.
   */
  const fase = giaLì
    ? null
    : rientrate > 0
      ? `rientro:${rientrate}`
      : inAttesa === 0
        ? null
        : attiva
          ? "attiva"
          : `pausa:${inAttesa}`;
  const passaggio = useRef<{ fase: string | null; frase: string | null }>({
    fase: null,
    frase: null,
  });
  if (passaggio.current.fase !== fase) {
    passaggio.current = {
      fase,
      frase:
        fase === null
          ? null
          : rientrate > 0
            ? t("toast.back", { n: rientrate })
            : attiva
              ? t("toast.imports", { fatti: numero(fatti), totale: numero(totale) })
              : t("toast.imports.paused", { n: inAttesa }),
    };
  }
  /** Il riquadro che si vede, o niente. */
  const riquadro = () => {
    // Sulla pagina stessa il toast è di troppo: ripeterebbe il riquadro di
    // testa a quattro centimetri di distanza.
    if (giaLì) return null;

    // Il rientro vince sulla barra: arriva quando la coda è già a zero, ed è la
    // notizia — non un avanzamento.
    if (rientrate > 0) {
      return (
        <div className="toast toast-card" data-livello="esito">
          <Icona nome="i-repeat" dim={16} />
          <div className="dentro">
            <div className="cosa">{t("toast.back", { n: rientrate })}</div>
          </div>
          <button
            type="button"
            className="bottone minuto btn-ghost"
            onClick={onApri}
          >
            {t("toast.open")}
          </button>
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("common.close")}
            onClick={onChiudiRientro}
          >
            <Icona nome="i-x" dim={12} />
          </button>
        </div>
      );
    }

    // Niente coda, niente toast. Non si mostra «concluse»: la fine di una coda
    // non è una notizia da inseguire chi se n'è andato — il rientro sì, ed è
    // quello di sopra.
    if (inAttesa === 0) return null;

    return (
      <div className="toast toast-card">
        <Icona nome="i-list" dim={16} />
        <div className="dentro">
          <div className="cosa">
            {attiva
              ? t("toast.imports", {
                  fatti: numero(fatti),
                  totale: numero(totale),
                })
              : t("toast.imports.paused", { n: inAttesa })}
          </div>
          <div className="toast-progress">
            {/* In pausa la barra resta dov'è e cambia colore invece di
                sparire: una barra che sparisce dice «finito». */}
            <span
              data-ferma={!attiva || undefined}
              style={{
                width:
                  totale > 0 ? `${Math.round((fatti / totale) * 100)}%` : "0%",
              }}
            />
          </div>
        </div>
        <button
          type="button"
          className="bottone minuto btn-ghost"
          onClick={onApri}
        >
          {t("toast.open")}
        </button>
      </div>
    );
  };

  // Un frammento solo, con la regione sempre al primo posto: cambiare forma
  // fra un ramo e l'altro la smonterebbe e la rimonterebbe, che è proprio il
  // «nasce già piena» che la regione esiste per evitare.
  return (
    <>
      <Annuncio testo={passaggio.current.frase} />
      {riquadro()}
    </>
  );
}
