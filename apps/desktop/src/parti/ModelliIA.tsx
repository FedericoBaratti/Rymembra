/**
 * I modelli di linguaggio: quali sono configurati, e quale usa lo Studio.
 *
 * # Perché è un componente suo e non un blocco dentro Impostazioni
 *
 * Per la stessa ragione di `Scrobbling`: ha uno stato che nessun altro guarda —
 * un elenco di profili, un modulo a metà, un menù di modelli appena letto — e
 * quello stato si rilegge dopo **ogni** gesto, perché tutti i comandi
 * restituiscono lo stato completo apposta. `Impostazioni` ha già sessantasette
 * prop e non ne deve prendere altre sei.
 *
 * # I quattro fornitori non si somigliano, e la schermata lo dice
 *
 * OpenRouter sta fuori: vuole una chiave, e ogni messaggio che gli si manda
 * esce da questo computer. Ollama e Bionic girano qui: non vogliono niente, ma
 * vogliono essere accesi — e se non lo sono l'errore lo dice, invece di
 * mandare a controllare il router. «Personalizzato» è lo stesso ramo dei due
 * locali con un indirizzo da scrivere a mano, e con l'unica regola che non si
 * può aggirare: in chiaro si parla solo con questa macchina.
 *
 * Nascondere questa differenza dietro quattro caselle identiche renderebbe
 * incomprensibile la volta in cui una delle quattro non funziona.
 *
 * # Perché la chiave si azzera dopo l'invio
 *
 * Perché da quel momento sta nel portachiavi del sistema, e tenerne una seconda
 * copia in memoria in un campo che nessuno guarda è una copia di troppo di un
 * segreto. È la stessa cosa che fa `Scrobbling` con il token di ListenBrainz.
 */
import { useCallback, useEffect, useState } from "react";

import type { FornitoreIa, ModelloIa, ProfiloIa, StatoIa } from "../ipc";
import { ipc } from "../ipc";
import { Icona } from "./Icone";
import { Segmentato } from "./Segmentato";
import { t, type Chiave } from "../lingue";
import { Trans } from "../lingue/Trans";

/** L'indirizzo che si propone scegliendo un fornitore. */
const DI_SERIE: Record<FornitoreIa, string> = {
  openrouter: "https://openrouter.ai/api/v1",
  ollama: "http://localhost:11434/v1",
  bionic: "http://localhost:1234/v1",
  custom: "",
};

/** Chi gira su questa macchina. */
const LOCALI: readonly FornitoreIa[] = ["ollama", "bionic"];

/** Chi non risponde senza una chiave. */
const CON_CHIAVE: readonly FornitoreIa[] = ["openrouter"];

/**
 * Come si legge una riga del menù dei modelli.
 *
 * # Perché non basta lo slug
 *
 * Perché con OpenRouter l'elenco è di centinaia di righe e uno slug non dice
 * niente di quel che serve a sceglierne una. Le due cose che decidono sono
 * quanto costa e quanto contesto regge, e si scoprivano dopo: dal 404 «This
 * model is unavailable for free», oppure da una risposta tagliata a metà.
 *
 * L'etichetta di un `<option>` dentro un `datalist` è quel che il browser
 * mostra accanto al valore, e il valore resta lo slug: chi sceglie legge il
 * nome, e quel che finisce nel campo è quel che il servizio vuole ricevere.
 */
function etichetta(modello: ModelloIa): string {
  const pezzi = [modello.nome ?? modello.id];
  if (modello.contesto !== null) {
    pezzi.push(t("ia.model.context", { k: Math.round(modello.contesto / 1000) }));
  }
  // «sconosciuto» non si scrive: per un modello locale il prezzo non esiste, e
  // una parola in più che dice «non lo so» è rumore su ogni riga del menù.
  if (modello.prezzo === "gratis") pezzi.push(t("ia.model.free"));
  if (modello.prezzo === "apagamento") pezzi.push(t("ia.model.paid"));
  return pezzi.join(" · ");
}

/** Il modulo, mentre lo si compila. */
type Bozza = {
  /** `null` mentre si crea un profilo nuovo. */
  id: string | null;
  nome: string;
  fornitore: FornitoreIa;
  urlBase: string;
  modello: string;
};

/** Una bozza vuota, col fornitore scelto e il suo indirizzo. */
function vuota(fornitore: FornitoreIa): Bozza {
  return {
    id: null,
    nome: "",
    fornitore,
    urlBase: DI_SERIE[fornitore],
    modello: "",
  };
}

/** Un profilo esistente, aperto nel modulo. */
function da(profilo: ProfiloIa): Bozza {
  return {
    id: profilo.id,
    nome: profilo.nome,
    fornitore: profilo.fornitore,
    urlBase: profilo.urlBase,
    modello: profilo.modello,
  };
}

export function ModelliIA({
  onErrore,
  onNotizia,
}: {
  onErrore: (e: unknown) => void;
  /** Una cosa andata bene da dire: non è un errore e non va sul canale rosso. */
  onNotizia: (testo: string) => void;
}) {
  const [stato, setStato] = useState<StatoIa | null>(null);
  const [bozza, setBozza] = useState<Bozza>(() => vuota("openrouter"));
  const [chiave, setChiave] = useState("");
  const [modelli, setModelli] = useState<ModelloIa[]>([]);
  /**
   * Il filtro dei gratuiti, spento di serie.
   *
   * Compare solo dove ha un senso, cioè dove esiste un prezzo: sui due locali
   * non c'è niente da filtrare, e una casella che non fa niente è peggio di una
   * casella che manca.
   */
  const [soloGratis, setSoloGratis] = useState(false);
  const [inVolo, setInVolo] = useState(false);

  useEffect(() => {
    ipc.iaProfili().then(setStato).catch(onErrore);
  }, [onErrore]);

  /**
   * Esegue un comando e ne prende lo stato di ritorno.
   *
   * Tutti i comandi restituiscono lo stato completo, quindi non serve mai una
   * seconda chiamata per rileggerlo — che è anche l'unico modo di non avere un
   * istante in cui la schermata mostra quel che c'era prima.
   */
  const esegui = useCallback(
    (azione: () => Promise<StatoIa>) => {
      setInVolo(true);
      azione()
        .then(setStato)
        .catch(onErrore)
        .finally(() => setInVolo(false));
    },
    [onErrore],
  );

  /** Cambiare fornitore riscrive l'indirizzo, se non l'ha toccato nessuno. */
  const scegliFornitore = useCallback((fornitore: FornitoreIa) => {
    setBozza((prima) => ({
      ...prima,
      fornitore,
      // Un indirizzo scritto a mano non si perde cambiando idea sul fornitore;
      // uno lasciato com'era segue il fornitore, che è quel che ci si aspetta.
      urlBase: Object.values(DI_SERIE).includes(prima.urlBase)
        ? DI_SERIE[fornitore]
        : prima.urlBase,
    }));
    setModelli([]);
  }, []);

  /** Legge l'elenco dei modelli, che è anche la prova di connessione. */
  const prova = useCallback(
    (id: string) => {
      setInVolo(true);
      ipc
        .iaModelli(id)
        .then((trovati: ModelloIa[]) => {
          setModelli(trovati);
          onNotizia(
            trovati.length > 0
              ? t("ia.tested", { n: trovati.length })
              : t("ia.tested.none"),
          );
        })
        .catch(onErrore)
        .finally(() => setInVolo(false));
    },
    [onErrore, onNotizia],
  );

  const salva = useCallback(() => {
    setInVolo(true);
    ipc
      .iaSalvaProfilo(
        {
          id: bozza.id,
          nome: bozza.nome,
          fornitore: bozza.fornitore,
          urlBase: bozza.urlBase,
          modello: bozza.modello,
        },
        // `null` lascia stare la chiave che c'è: chi modifica il modello di un
        // profilo non deve reincollare il segreto.
        chiave === "" ? null : chiave,
      )
      .then((s: StatoIa) => {
        setStato(s);
        setChiave("");
        const salvato = s.profili.find((p) => p.nome === bozza.nome);
        setBozza(vuota(bozza.fornitore));
        setModelli([]);
        onNotizia(t("ia.saved"));
        // E si prova subito. «Prova» esiste solo dopo il salvataggio — la
        // chiave sta nel portachiavi sotto il nome del profilo — quindi il
        // gesto successivo era comunque quello, ed è quello che nel diario di
        // una sessione vera compare quattro volte di fila intervallato da
        // altrettanti errori. Farlo da soli è la differenza fra scoprire adesso
        // che quel modello non esiste e scoprirlo alla prima domanda.
        if (salvato !== undefined) prova(salvato.id);
      })
      .catch(onErrore)
      .finally(() => setInVolo(false));
  }, [bozza, chiave, onErrore, onNotizia, prova]);

  const profili = stato?.profili ?? [];
  const locale = LOCALI.includes(bozza.fornitore);
  const vuoleChiave = CON_CHIAVE.includes(bozza.fornitore);
  /**
   * Se in questo elenco c'è qualcosa da filtrare.
   *
   * Si guarda l'elenco e non il fornitore: è l'unico dei due che sappia se un
   * prezzo esiste. Un endpoint personalizzato davanti a OpenRouter dichiara i
   * prezzi come lui; Ollama non li dichiara mai, chiunque lo serva.
   */
  const conPrezzi = modelli.some((m) => m.prezzo !== "sconosciuto");
  const mostrati =
    soloGratis && conPrezzi
      ? modelli.filter((m) => m.prezzo === "gratis")
      : modelli;
  // Un profilo senza modello non può chiamare niente, e senza indirizzo
  // nemmeno: il bottone lo dice prima invece di far scoprire un errore dopo.
  const completa = bozza.urlBase.trim() !== "" && bozza.modello.trim() !== "";

  return (
    <>
      <p className="nota">{t("ia.p1")}</p>
      <p className="nota">
        <Trans k="ia.p2" v={{ locali: <strong>{t("ia.p2.local")}</strong> }} />
      </p>
      <p className="nota">{t("ia.keyring")}</p>

      {/* ── i profili salvati ── */}
      {profili.length > 0 && (
        <ul className="elenco-profili">
          {profili.map((profilo) => {
            const attivo = profilo.id === stato?.attivo;
            return (
              <li key={profilo.id} className="profilo-ia" data-active={attivo || undefined}>
                <div className="che-cosa">
                  <div className="etichetta">
                    {profilo.nome}
                    {attivo && (
                      <span className="quante">{t("ia.inUse")}</span>
                    )}
                  </div>
                  <div className="spiegazione">
                    {t(`ia.provider.${profilo.fornitore}` as Chiave)} ·{" "}
                    {profilo.modello} · {profilo.urlBase}
                    {profilo.conChiave ? ` · ${t("ia.hasKey")}` : ""}
                  </div>
                </div>
                <div className="azioni">
                  {!attivo && (
                    <button
                      type="button"
                      className="bottone btn-ghost"
                      disabled={inVolo}
                      onClick={() => esegui(() => ipc.iaScegliProfilo(profilo.id))}
                    >
                      {t("ia.use")}
                    </button>
                  )}
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    disabled={inVolo}
                    onClick={() => {
                      setBozza(da(profilo));
                      setChiave("");
                      setModelli([]);
                    }}
                  >
                    {t("ia.edit")}
                  </button>
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    disabled={inVolo}
                    onClick={() => esegui(() => ipc.iaEliminaProfilo(profilo.id))}
                  >
                    {t("ia.delete")}
                  </button>
                </div>
              </li>
            );
          })}
        </ul>
      )}

      {/* ── il modulo ── */}
      <h3 className="titoletto">
        {bozza.id === null ? t("ia.form.new") : t("ia.form.edit")}
      </h3>

      <Segmentato
        classe="minuto"
        etichetta={t("ia.provider")}
        scelta={bozza.fornitore}
        onScegli={scegliFornitore}
        voci={[
          { chiave: "openrouter", etichetta: t("ia.provider.openrouter") },
          { chiave: "ollama", etichetta: t("ia.provider.ollama") },
          { chiave: "bionic", etichetta: t("ia.provider.bionic") },
          { chiave: "custom", etichetta: t("ia.provider.custom") },
        ]}
      />

      <p className="nota">
        {locale ? t("ia.provider.localNote") : t("ia.provider.remoteNote")}
      </p>

      <div className="azioni">
        <input
          type="text"
          className="campo field-input"
          placeholder={t("ia.name")}
          value={bozza.nome}
          onChange={(e) => setBozza({ ...bozza, nome: e.target.value })}
        />
        <input
          type="text"
          className="campo field-input"
          placeholder={t("ia.url")}
          value={bozza.urlBase}
          onChange={(e) => setBozza({ ...bozza, urlBase: e.target.value })}
        />
      </div>

      <div className="azioni">
        <input
          type="text"
          className="campo field-input"
          placeholder={t("ia.model")}
          list="modelli-ia"
          value={bozza.modello}
          onChange={(e) => setBozza({ ...bozza, modello: e.target.value })}
        />
        {/*
         * Un `datalist` e non un `<select>`: i nomi noti sono un suggerimento,
         * non il vocabolario. Un modello appena pubblicato, o uno servito da un
         * endpoint che non elenca niente, deve poter essere scritto a mano.
         */}
        <datalist id="modelli-ia">
          {mostrati.map((modello) => (
            <option key={modello.id} value={modello.id} label={etichetta(modello)} />
          ))}
        </datalist>
        <input
          type="password"
          className="campo field-input"
          placeholder={vuoleChiave ? t("ia.key") : t("ia.key.optional")}
          value={chiave}
          onChange={(e) => setChiave(e.target.value)}
        />
      </div>

      {/* Solo dove c'è un prezzo da guardare: su Ollama la casella filtrerebbe
          un elenco in cui tutto costa lo stesso, cioè niente. */}
      {conPrezzi && (
        <label className="filtro-modelli">
          <input
            type="checkbox"
            checked={soloGratis}
            onChange={(e) => setSoloGratis(e.target.checked)}
          />
          {t("ia.model.onlyFree")}
        </label>
      )}

      <div className="azioni">
        <button
          type="button"
          className="bottone primario btn-accent"
          disabled={!completa || inVolo}
          onClick={salva}
        >
          <Icona nome="i-ia" dim={15} />
          {inVolo ? t("ia.saving") : t("ia.save")}
        </button>
        {bozza.id !== null && (
          <>
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo}
              onClick={() => prova(bozza.id ?? "")}
            >
              {inVolo ? t("ia.testing") : t("ia.test")}
            </button>
            <button
              type="button"
              className="bottone btn-ghost"
              disabled={inVolo}
              onClick={() => {
                setBozza(vuota(bozza.fornitore));
                setChiave("");
                setModelli([]);
              }}
            >
              {t("ia.form.cancel")}
            </button>
          </>
        )}
      </div>

      {/*
       * «Prova» esiste solo su un profilo già salvato, e non è una limitazione
       * che si poteva evitare: la chiave sta nel portachiavi sotto il nome del
       * profilo, e provare prima di salvare vorrebbe dire mandare il segreto
       * attraverso l'IPC per una richiesta sola — cioè il solo posto in cui non
       * passa mai.
       */}
      {bozza.id === null && <p className="nota">{t("ia.test.afterSave")}</p>}
    </>
  );
}
