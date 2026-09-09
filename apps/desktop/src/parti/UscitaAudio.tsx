/**
 * Da quale scheda audio esce il suono.
 *
 * # Perché si gestisce da sé
 *
 * Per la stessa ragione di `SchedaTesti`: `Impostazioni` riceve già
 * sessantasette prop, e un elenco che cambia da solo — quando qualcuno attacca
 * delle cuffie — ridisegnerebbe l'intera schermata a ogni cavo mosso se
 * passasse da `App`. Quel che serve a questa scheda lo chiede lei quando viene
 * montata, cioè quando qualcuno apre le Impostazioni, e lo lascia andare quando
 * si smonta.
 *
 * # Perché non c'è un tasto «aggiorna»
 *
 * Perché un tasto «aggiorna» è la confessione che l'elenco può essere vecchio.
 * Il nucleo rilegge le uscite ogni paio di secondi e manda
 * `riproduzione:dispositivi` **solo quando cambiano**: attaccare delle cuffie
 * le fa comparire da sé, staccarle le fa spegnere. La prima lettura la chiede
 * questa scheda, e la richiesta pungola anche il sorvegliante — così quel che
 * si vede all'apertura è di adesso e non di due secondi fa.
 *
 * # Perché la scelta assente resta nell'elenco
 *
 * Perché toglierla racconterebbe una bugia. Staccato il DAC, una riga in meno
 * lascia il pallino su «predefinito di sistema» — cioè dice che la preferenza è
 * stata dimenticata — mentre invece è ancora scritta e tornerà buona appena si
 * riattacca il cavo. Il nucleo la manda comunque, con `presente: false`, e qui
 * si disegna spenta.
 */
import { useCallback, useEffect, useState } from "react";

import { ipc, type DispositivoAudio } from "../ipc";
import { t } from "../lingue";
import { useAscolto } from "../pagine";

/** Il valore del gruppo di scelte che vuol dire «quella di sistema». */
const SISTEMA = "";

export function SchedaUscita({
  onErrore,
}: {
  /** Scegliere un'uscita apre un dispositivo: se non si apre, va detto. */
  onErrore: (e: unknown) => void;
}) {
  const [dispositivi, setDispositivi] = useState<DispositivoAudio[] | null>(
    null,
  );

  useEffect(() => {
    let annullato = false;
    ipc
      .dispositiviAudio()
      .then((letti) => {
        if (!annullato) setDispositivi(letti);
      })
      .catch(() => {
        /* Un elenco che non si legge resta vuoto, e la scheda lo dice con la
           riga «nessuna uscita collegata»: un errore rosso nelle Impostazioni
           per un'enumerazione fallita sarebbe rumore, e la risposta pratica —
           «non c'è niente da scegliere» — è la stessa. */
        if (!annullato) setDispositivi([]);
      });
    return () => {
      annullato = true;
    };
  }, []);

  // L'elenco che cambia da solo: `useAscolto` apre l'ascolto una volta e lo
  // chiude allo smontaggio, quindi qui non serve la guardia di `annullato` —
  // quella copre la prima lettura, che è una promessa e può tornare tardi.
  useAscolto<DispositivoAudio[]>("riproduzione:dispositivi", setDispositivi);

  const scegli = useCallback(
    (id: string) => {
      // Nessun aggiornamento ottimista: il comando riapre il dispositivo e
      // rimanda l'elenco con il segno di «attivo» già spostato. Segnarlo qui
      // vorrebbe dire mostrare per un istante una scelta che potrebbe non
      // essersi aperta — ed è proprio l'istante in cui il suono non c'è.
      ipc.scegliDispositivoAudio(id === SISTEMA ? null : id).catch(onErrore);
    },
    [onErrore],
  );

  const elenco = dispositivi ?? [];
  // Il pallino sta su «sistema» finché una riga non si dichiara scelta. È il
  // nucleo a dirlo — vedi `DispositivoAudio.scelto` — e non un confronto fatto
  // qui: `attivo` dice dove il suono **esce**, che con un cavo staccato non è
  // dove si era chiesto che uscisse.
  const scelto = elenco.find((d) => d.scelto)?.id ?? SISTEMA;

  return (
    <>
      {elenco.length === 0 ? (
        <p className="nota">{t("settings.output.none")}</p>
      ) : (
        <ul className="uscite">
          <li>
            <label>
              <input
                type="radio"
                name="uscita-audio"
                value={SISTEMA}
                checked={scelto === SISTEMA}
                onChange={() => scegli(SISTEMA)}
              />
              <span className="nome">{t("settings.output.system")}</span>
              {/* Quale sia adesso il predefinito va detto: «predefinito di
                  sistema» da solo non dice da dove uscirà il suono, ed è
                  esattamente la domanda che porta qualcuno in questa
                  schermata. */}
              {elenco.some((d) => d.predefinito) && (
                <span className="stato">
                  {t("settings.output.systemNow", {
                    nome: elenco.find((d) => d.predefinito)?.nome ?? "",
                  })}
                </span>
              )}
            </label>
          </li>
          {elenco.map((dispositivo) => (
            <li key={dispositivo.id} className={dispositivo.presente ? "" : "assente"}>
              <label>
                <input
                  type="radio"
                  name="uscita-audio"
                  value={dispositivo.id}
                  checked={scelto === dispositivo.id}
                  onChange={() => scegli(dispositivo.id)}
                />
                <span className="nome">{dispositivo.nome}</span>
                {dispositivo.attivo && (
                  <span className="stato in-uso">
                    {t("settings.output.active")}
                  </span>
                )}
                {!dispositivo.presente && (
                  <span className="stato">{t("settings.output.absent")}</span>
                )}
              </label>
            </li>
          ))}
        </ul>
      )}
      <p className="nota">{t("settings.output.hint")}</p>
    </>
  );
}
