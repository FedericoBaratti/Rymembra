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
import type { StatoScarico } from "../ipc";
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
  // Sulla pagina stessa il toast è di troppo: ripeterebbe il riquadro di testa a
  // quattro centimetri di distanza.
  if (giaLì) return null;

  const inAttesa = stato?.conteggi.attesa ?? 0;
  const fatti = stato?.fatti ?? 0;
  const totale = fatti + inAttesa;
  const attiva = stato?.attiva ?? false;

  // Il rientro vince sulla barra: arriva quando la coda è già a zero, ed è la
  // notizia — non un avanzamento.
  if (rientro !== null && rientro.vociRimesse > 0) {
    return (
      <div className="toast toast-card" data-livello="esito" role="status">
        <Icona nome="i-repeat" dim={16} />
        <div className="dentro">
          <div className="cosa">
            {t("toast.back", { n: rientro.vociRimesse })}
          </div>
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

  // Niente coda, niente toast. Non si mostra «concluse»: la fine di una coda non
  // è una notizia da inseguire chi se n'è andato — il rientro sì, ed è quello di
  // sopra.
  if (inAttesa === 0) return null;

  return (
    <div className="toast toast-card" role="status">
      <Icona nome="i-list" dim={16} />
      <div className="dentro">
        <div className="cosa">
          {attiva
            ? t("toast.imports", {
                fatti: numero(fatti),
                totale: numero(totale),
              })
            : t("toast.imports.paused", { n: numero(inAttesa) })}
        </div>
        <div className="toast-progress">
          {/* In pausa la barra resta dov'è e cambia colore invece di sparire:
              una barra che sparisce dice «finito». */}
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
}
