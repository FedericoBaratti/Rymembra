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
import { type CSSProperties, useCallback, useEffect, useState } from "react";

import {
  LATENZA_MASSIMA_MS,
  ipc,
  type DispositivoAudio,
  type StatoRiproduzione,
} from "../ipc";
import { t } from "../lingue";
import { useAscolto } from "../pagine";

/** Il valore del gruppo di scelte che vuol dire «quella di sistema». */
const SISTEMA = "";

/**
 * Di quanto sposta una tacca del cursore della latenza.
 *
 * Dieci millisecondi. Sotto, una tacca non si sente — dieci millisecondi sono
 * già il limite di quel che l'orecchio distingue su uno sfasamento fra immagine
 * e suono — e cento tacche per mezzo secondo sono una corsa ragionevole col
 * dito o con le frecce.
 */
const PASSO_LATENZA = 10;

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
      <Latenza onErrore={onErrore} />
    </>
  );
}

/**
 * Di quanto la catena d'uscita ritarda il suono, dichiarato a mano.
 *
 * # Perché un cursore e non una misura
 *
 * Perché la misura esiste già e copre solo un pezzo. Il motore legge da `cpal`
 * quanto vale il buffer del dispositivo — una decina di millisecondi — e lo
 * compensa da sé; quel che resta è tutto ciò che `cpal` non vede: il mixer di
 * sistema, il driver, la conversione digitale-analogica, e su un'uscita senza
 * fili la radio, che da sola vale più di tutto il resto insieme. Nessuna API
 * dice quel numero, e indovinarlo per tutti vorrebbe dire sbagliarlo per
 * ciascuno.
 *
 * Si regola guardando l'effetto: si apre un brano con un testo sincronizzato e si
 * sposta il cursore finché la riga si accende quando la voce attacca.
 *
 * # Perché sta dentro la scheda dell'uscita
 *
 * Perché **è** una proprietà dell'uscita, non del lettore: cambiando scheda
 * cambia, e trovarla accanto all'elenco delle uscite è trovarla dove si era
 * appena cambiata la cosa che l'ha fatta sbagliare. Per la stessa ragione non
 * viaggia nel profilo, come `player.output`.
 *
 * # Perché si gestisce da sé
 *
 * Come la scheda che la contiene: il valore vero sta nello stato della
 * riproduzione, e passarlo per prop vorrebbe dire allungare di uno le
 * sessantasette di `Impostazioni`.
 */
function Latenza({ onErrore }: { onErrore: (e: unknown) => void }) {
  const [scritta, setScritta] = useState<number | null>(null);
  const [trascinato, setTrascinato] = useState<number | null>(null);

  useEffect(() => {
    let annullato = false;
    ipc
      .riproduzioneStato()
      .then((stato) => {
        if (!annullato) setScritta(stato.latenzaMs);
      })
      .catch(() => {
        /* Senza scheda audio lo stato non si legge, e non c'è niente da
           correggere: il cursore resta a zero invece di mostrare un errore rosso
           accanto alla riga che già dice che non c'è nessuna uscita. */
        if (!annullato) setScritta(0);
      });
    return () => {
      annullato = true;
    };
  }, []);

  // Il valore vero arriva da qui in poi: il comando scrive, rilegge e manda lo
  // stato, quindi questo evento porta il numero **rimasto scritto** e non quello
  // chiesto. Serve anche quando il cursore non l'ha mosso nessuno — una
  // riapertura del dispositivo rimanda lo stato — e per questo non basta la
  // risposta del comando.
  useAscolto<StatoRiproduzione>("riproduzione:stato", (stato) =>
    setScritta(stato.latenzaMs),
  );

  const dove = trascinato ?? scritta ?? 0;

  const rilascia = () => {
    if (trascinato === null) return;
    // Solo se è davvero cambiato, come per la dissolvenza: toccare il cursore e
    // rimetterlo dov'era è un gesto frequente e non deve costare una scrittura.
    if (trascinato !== scritta) ipc.latenza(trascinato).catch(onErrore);
    setTrascinato(null);
  };

  // Il cursore va da −massimo a +massimo, quindi l'avanzamento per il tracciato
  // si misura da sinistra e non dal valore: a zero il riempimento sta a metà.
  const avanzamento =
    ((dove + LATENZA_MASSIMA_MS) / (2 * LATENZA_MASSIMA_MS)) * 100;
  const etichetta =
    dove === 0
      ? t("settings.output.latency.none")
      : t("settings.output.latency.ms", { n: dove });

  return (
    <>
      {/* `titolo-gruppo` e non una classe nuova: è già il modo in cui questo
          albero intitola un gruppo dentro una scheda — lo fa l'elenco delle
          scorciatoie in `Impostazioni` — e un cursore senza titolo, in una
          scheda che parla di elenchi di uscite, sembrerebbe un residuo. */}
      <div className="titolo-gruppo">{t("settings.output.latency.title")}</div>
      <div className="latenza">
        <input
          type="range"
          className="scorrimento range-accent"
          min={-LATENZA_MASSIMA_MS}
          max={LATENZA_MASSIMA_MS}
          step={PASSO_LATENZA}
          value={dove}
          style={{ "--avanzamento": `${avanzamento}%` } as CSSProperties}
          aria-label={t("settings.output.latency.title")}
          aria-valuetext={etichetta}
          onChange={(e) => setTrascinato(Number(e.target.value))}
          onPointerUp={rilascia}
          onKeyUp={rilascia}
          onBlur={rilascia}
        />
        <span className="valore">{etichetta}</span>
      </div>
      <p className="nota">{t("settings.output.latency.hint")}</p>
    </>
  );
}
