/**
 * «Resta attiva in sottofondo», sul telefono: nella scheda «Dispositivi», sotto
 * «Raggiungibile da internet».
 *
 * L'interruttore accende un servizio in primo piano (`Presenza.kt`) che tiene
 * vivo il processo — e con lui il collegamento con i PC — anche con l'app
 * tolta dalle recenti. È spento di serie, perché costa batteria e perché è una
 * notifica che resta: la spiegazione lo dice, con la ragione e il prezzo.
 *
 * Sotto, con il sottofondo acceso, il risparmio batteria. Un servizio in primo
 * piano **non** esenta dal Doze: con lo schermo spento e il telefono fermo
 * Android sospende la rete anche a lui, e i PC non trovano il telefono. Il
 * rimedio è togliere Rymembra dal risparmio batteria, e non lo si chiede con
 * la richiesta diretta (la policy di Google Play la riserva ad altri): il
 * pulsante apre la pagina di sistema, dove lo si sceglie. Lo stato si rilegge
 * quando l'app torna in vista, perché lo si cambia fuori di qui.
 *
 * «Parti con l'auto» sta sotto di lui: sono due righe di una stessa famiglia —
 * quel che il telefono fa da solo, in secondo piano. Spenta di serie, funziona
 * solo col sottofondo acceso (è il servizio a guardare quando l'auto si
 * collega, e con lui si spegne), e non compare dove la build non la offre: la
 * variante per Google Play chiede alle app per l'auto di non partire da sole
 * (`rigaDellAuto`). Il testo dice la cosa com'è: Rymembra riparte da sola al
 * posto dell'ultima app usata, con la coda che aveva salvato.
 */
import { useCallback, useEffect, useState } from "react";

import { ipc, type InvitoLontano, type StatoLontano } from "../ipc";
import { t } from "../lingue";
import { noteDelSottofondo, rigaDellAuto, type SenzaRisparmio } from "../sottofondo";
import { Interruttore } from "./Interruttore";

/** Il comando di `SchedaDispositivi` che risponde col pannello. */
type Fai = (comando: () => Promise<StatoLontano | InvitoLontano | void>) => void;

/** La riga dell'interruttore, e quel che serve a farlo funzionare davvero. */
export function SottofondoTelefono({
  stato,
  fai,
  onErrore,
}: {
  stato: StatoLontano;
  fai: Fai;
  onErrore: (errore: unknown) => void;
}) {
  const [senzaRisparmio, setSenzaRisparmio] = useState<SenzaRisparmio>(null);

  const rileggi = useCallback(
    () =>
      ipc
        .telefonoSenzaRisparmioBatteria()
        .then(setSenzaRisparmio)
        // Un ponte che non risponde non è un errore da mostrare: la nota della
        // batteria resta semplicemente fuori.
        .catch(() => setSenzaRisparmio(null)),
    [],
  );

  // Lo si cambia nelle impostazioni di sistema, e da lì si torna senza un
  // risultato da aspettare: lo si rilegge quando l'app torna davanti.
  useEffect(() => {
    void rileggi();
    const torna = () => {
      if (document.visibilityState === "visible") void rileggi();
    };
    document.addEventListener("visibilitychange", torna);
    window.addEventListener("focus", torna);
    return () => {
      document.removeEventListener("visibilitychange", torna);
      window.removeEventListener("focus", torna);
    };
  }, [rileggi]);

  const note = noteDelSottofondo(stato.sottofondo, stato.attivo, senzaRisparmio);
  const auto = rigaDellAuto(stato.partenzaConAuto, stato.sottofondo);

  return (
    <>
      <Interruttore
        etichetta={t("settings.devices.background")}
        spiegazione={t("settings.devices.background.sub")}
        acceso={stato.sottofondo}
        onCambia={(acceso) => fai(() => ipc.lontanoSottofondo(acceso))}
      />
      {note.internet && (
        <p className="nota" role="status">
          {t("settings.devices.background.needsInternet")}
        </p>
      )}
      {note.batteria === "fuori" && (
        <p className="nota">{t("settings.devices.background.battery.out")}</p>
      )}
      {note.batteria === "dentro" && (
        <>
          <p className="nota">{t("settings.devices.background.battery.in")}</p>
          <div className="azioni">
            <button
              type="button"
              className="bottone btn-ghost"
              onClick={() => {
                ipc.telefonoApriRisparmioBatteria().catch(onErrore);
              }}
            >
              {t("settings.devices.background.battery.open")}
            </button>
          </div>
          <p className="nota">{t("settings.devices.background.makers")}</p>
        </>
      )}
      {auto !== "nascosta" && (
        <Interruttore
          etichetta={t("settings.devices.car")}
          spiegazione={t("settings.devices.car.sub")}
          acceso={stato.partiConAuto}
          impedito={auto === "serveIlSottofondo" ? t("settings.devices.car.needsBackground") : undefined}
          onCambia={(acceso) => fai(() => ipc.lontanoPartiConAuto(acceso))}
        />
      )}
    </>
  );
}
