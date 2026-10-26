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
 * Un millisecondo. Erano dieci, e la ragione scritta qui era giusta finché il
 * cursore serviva a una cosa sola: mettere d'accordo il testo sincronizzato con
 * la voce, dove dieci millisecondi sono il limite di quel che l'occhio
 * distingue. Con «Ascolta insieme» lo stesso numero mette d'accordo **due
 * casse nella stessa stanza**, e lì l'orecchio sente lo spostamento
 * dell'immagine stereo per pochi millisecondi: a passi di dieci il punto
 * giusto cadeva quasi sempre fra due tacche. Mille tacche per mezzo secondo
 * sono troppe da trascinare con precisione, ed è per questo che il cursore
 * della calibrazione ha accanto i due tasti da un millisecondo; le frecce
 * della tastiera fanno lo stesso passo.
 */
const PASSO_LATENZA = 1;

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
  return (
    <>
      {/* `titolo-gruppo` e non una classe nuova: è già il modo in cui questo
          albero intitola un gruppo dentro una scheda — lo fa l'elenco delle
          scorciatoie in `Impostazioni` — e un cursore senza titolo, in una
          scheda che parla di elenchi di uscite, sembrerebbe un residuo. */}
      <div className="titolo-gruppo">{t("settings.output.latency.title")}</div>
      <CursoreLatenza
        limite={LATENZA_MASSIMA_MS}
        etichetta={t("settings.output.latency.title")}
        onErrore={onErrore}
      />
    </>
  );
}

/**
 * Il cursore della latenza dell'uscita corrente, da solo.
 *
 * # Perché è uscito da `Latenza`
 *
 * Perché adesso lo usano due posti: la scheda dell'uscita qui sopra e la
 * calibrazione di «Ascolta insieme» (`parti/Insieme.tsx`). È lo **stesso**
 * numero — il comando `latenza` lo scrive sull'uscita aperta adesso — e due
 * cursori scritti a parte avrebbero due idee diverse di quando scrivere, cioè
 * due valori diversi a schermo per la stessa preferenza.
 *
 * Quel che cambia fra i due è quanto è lunga la corsa (`limite`) e se accanto
 * ci sono i due tasti da un millisecondo (`conPassi`): la calibrazione si fa a
 * orecchio, un colpo alla volta, mentre si ascolta — e un cursore di seicento
 * tacche trascinato col mouse salta di cinque alla volta.
 */
export function CursoreLatenza({
  limite,
  etichetta,
  conPassi = false,
  onErrore,
}: {
  /** Fin dove arriva, in ms, nei due versi. Mai oltre `LATENZA_MASSIMA_MS`. */
  limite: number;
  /** Il nome del cursore, per chi ascolta lo schermo. */
  etichetta: string;
  /** I due tasti «un millisecondo in meno / in più». */
  conPassi?: boolean | undefined;
  onErrore: (e: unknown) => void;
}) {
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

  // Un valore scritto fuori dalla corsa di questo cursore — mezzo secondo
  // dichiarato nelle Impostazioni, aperta poi la calibrazione che arriva a
  // trecento — si disegna al bordo: il numero accanto resta quello vero, e il
  // primo gesto lo riporta dentro.
  const dove = Math.min(limite, Math.max(-limite, trascinato ?? scritta ?? 0));

  const rilascia = () => {
    if (trascinato === null) return;
    // Solo se è davvero cambiato, come per la dissolvenza: toccare il cursore e
    // rimetterlo dov'era è un gesto frequente e non deve costare una scrittura.
    if (trascinato !== scritta) ipc.latenza(trascinato).catch(onErrore);
    setTrascinato(null);
  };

  /**
   * Un millisecondo da un tasto: si scrive subito, non al rilascio.
   *
   * Non c'è niente da rilasciare — il gesto è il clic — e chi calibra preme,
   * ascolta, e preme di nuovo: aspettare un evento che non arriva lascerebbe
   * il valore a metà strada. La risposta del comando è il numero **rimasto
   * scritto**, che il nucleo taglia ai suoi limiti, ed è quello che si adotta.
   */
  const passo = (verso: 1 | -1) => {
    const nuovo = Math.min(limite, Math.max(-limite, (scritta ?? 0) + verso * PASSO_LATENZA));
    if (nuovo === scritta) return;
    ipc.latenza(nuovo).then(setScritta).catch(onErrore);
  };

  // Il cursore va da −limite a +limite, quindi l'avanzamento per il tracciato
  // si misura da sinistra e non dal valore: a zero il riempimento sta a metà.
  const avanzamento = ((dove + limite) / (2 * limite)) * 100;
  const mostrato = trascinato ?? scritta ?? 0;
  const valore =
    mostrato === 0
      ? t("settings.output.latency.none")
      : t("settings.output.latency.ms", { n: mostrato });

  return (
    <div className="latenza">
      {conPassi && (
        <button
          type="button"
          className="bottone minuto btn-ghost"
          aria-label={t("together.calibrate.less")}
          title={t("together.calibrate.less")}
          disabled={scritta === null || scritta <= -limite}
          onClick={() => passo(-1)}
        >
          −1
        </button>
      )}
      <input
        type="range"
        className="scorrimento range-accent"
        min={-limite}
        max={limite}
        step={PASSO_LATENZA}
        value={dove}
        style={{ "--avanzamento": `${avanzamento}%` } as CSSProperties}
        aria-label={etichetta}
        aria-valuetext={valore}
        onChange={(e) => setTrascinato(Number(e.target.value))}
        onPointerUp={rilascia}
        onKeyUp={rilascia}
        onBlur={rilascia}
      />
      {conPassi && (
        <button
          type="button"
          className="bottone minuto btn-ghost"
          aria-label={t("together.calibrate.more")}
          title={t("together.calibrate.more")}
          disabled={scritta === null || scritta >= limite}
          onClick={() => passo(1)}
        >
          +1
        </button>
      )}
      <span className="valore">{valore}</span>
    </div>
  );
}
