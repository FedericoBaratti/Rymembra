/**
 * Lo Skin Studio: un solo spazio di lavoro.
 *
 * # La tesi
 *
 * Una skin è un documento di dati validato: `parse_skin` lo legge,
 * `check_skin` avvisa, `compile_skin` lo trasforma in CSS deterministico.
 * Tutto quel che serve a un editor esiste già nel nucleo — i token, le parti,
 * gli effetti col loro costo, gli stati, gli inneschi, i campi di un
 * fotogramma coi loro estremi — e arriva qui col registro. Lo Studio non
 * aggiunge potere: rende visibile un contratto.
 *
 * # Com'è fatto
 *
 * Come un editor di disegno, e non più come quattro viste separate: a
 * sinistra quel che si può toccare (le parti e lo scafale, oppure le
 * variabili), al centro l'applicazione vera con dati finti, a destra
 * l'ispettore di quel che si è scelto, sotto un cassetto per la timeline, i
 * problemi e il codice. La chat è un cassetto a destra.
 *
 * # Le tre regole, ancora
 *
 * 1. Non si può scrivere quel che il formato non accetta: ogni controllo ha la
 *    forma di un tipo del nucleo. Il posto dove si scrive liberamente è uno
 *    solo, il codice, e lì risponde il parser.
 * 2. L'anteprima è l'applicazione: le pagine vere, e «Prova sull'app» per
 *    indossarla sulla finestra vera.
 * 3. Il costo si vede mentre lo si spende: ogni parte porta il suo, e la
 *    finestra il totale.
 */
import { salvaCon } from "../dialoghi";
import { Activity, useCallback, useEffect, useMemo, useReducer, useRef, useState } from "react";
import { flushSync } from "react-dom";

import { ConfermaGesto, type Domanda } from "../ConfermaGesto";
import { ipc, testoErrore, type OperazioneIa, type Registro, type Skin } from "../ipc";
import { t } from "../lingue";
import { Avviso } from "../parti/Avvisi";
import { Icona } from "../parti/Icone";
import { haAnnulloProprio, prendeITasti, type Bersaglio } from "./annullo";
import { BarraStudio } from "./BarraStudio";
import { Centro } from "./Centro";
import { Chat } from "./Chat";
import {
  ContestoAzioniStudio,
  ContestoStatoStudio,
  type AzioniDelloStudio,
  type StatoDelloStudio,
} from "./contesto";
import { Dock } from "./Dock";
import { Ispezione } from "./Ispezione";
import { Livelli } from "./Livelli";
import { creaMittente, type Mittente } from "./mittente";
import { daSalvare, iniziale, riduci, testo, type SchedaSinistra } from "./negozio";
import { leggi } from "./patch";
import { daFotografare, poiSeSalvato } from "./salvataggio";
import { Schede } from "./Schede";
import { usePacchetto } from "./usaPacchetto";
import { Variabili } from "./Variabili";

/** Quanto aspetta il salvataggio della bozza dopo l'ultima modifica. */
const RESPIRO_SALVATAGGIO = 800;


/** Il bersaglio di un tasto, per le due regole di `annullo.ts`. */
function bersaglio(dove: EventTarget | null): Bersaglio | null {
  if (!(dove instanceof HTMLElement)) return null;
  return {
    tag: dove.tagName,
    tipo: dove instanceof HTMLInputElement ? dove.type : null,
    modificabile: dove.isContentEditable,
  };
}

export function Studio({
  id,
  onEsci,
  onInstallata,
  onModelli,
  onProva,
  onErrore,
}: {
  id: string;
  onEsci: () => void;
  /** «Salva e usa» ha installato la skin: la finestra la deve indossare. */
  onInstallata: (id: string) => void;
  /** La chat non ha un modello: si va a configurarlo. */
  onModelli: () => void;
  /**
   * «Prova sull'app»: la finestra vera indossa la bozza compilata. Il ritorno
   * lo gestisce `App`, che tiene la pillola fuori dallo Studio nascosto.
   */
  onProva: (prova: { skin: Skin; sorgente: string; id: string }) => void;
  onErrore: (e: unknown) => void;
}) {
  const [s, manda] = useReducer(riduci, id, iniziale);
  const [registro, setRegistro] = useState<Registro | null>(null);
  const attuale = useRef(s);
  attuale.current = s;
  /**
   * Dove sta la bozza **adesso**, prima ancora che il negozio lo sappia.
   *
   * Il salvataggio può spostarla — il documento ha cambiato `id` — e chi
   * viene subito dopo nella stessa catena (la prova, «Salva e usa»,
   * l'istantanea) deve già cercarla lì: `attuale` si aggiorna al disegno
   * dopo, cioè troppo tardi per un `await` che riparte nello stesso giro.
   */
  const idBozza = useRef(id);

  // ── il registro e il documento ──────────────────────────────────────────
  //
  // Una volta sola anche il registro: è statico, e tornando visibile da
  // «Prova sull'app» gli effetti ripartono — chiederlo di nuovo era un giro
  // nel nucleo per una risposta che non può essere cambiata.
  //
  // Se uno dei due non arriva, lo Studio non ha niente da mostrare: fino alla
  // 3.2 restava su «Caricamento…» per sempre, senza tasti, sopra una finestra
  // `inert` e con le scorciatoie dell'app spente — l'unica via era chiudere
  // Rymembra. Il guasto resta qui, e la schermata d'attesa diventa una
  // schermata d'errore con «Riprova» (`tentativo` rilancia le due richieste)
  // e «Chiudi lo Studio».
  const [guastoApertura, setGuastoApertura] = useState<{ errore: unknown } | null>(null);
  const [tentativo, setTentativo] = useState(0);
  const registroChiesto = useRef(false);
  useEffect(() => {
    if (registroChiesto.current) return;
    registroChiesto.current = true;
    ipc
      .studioRegistro()
      .then(setRegistro)
      .catch((e: unknown) => {
        // Si riprova alla prossima volta che lo Studio torna visibile, o a
        // «Riprova».
        registroChiesto.current = false;
        setGuastoApertura({ errore: e });
        onErrore(e);
      });
  }, [onErrore, tentativo]);

  // Una volta sola: lo Studio vive sotto `<Activity>`, e tornando visibile
  // dopo «Prova sull'app» gli effetti ripartono. Rileggere il documento lì
  // butterebbe la storia — e, se il salvataggio non era arrivato, il lavoro.
  useEffect(() => {
    if (attuale.current.caricato) return;
    let vivo = true;
    ipc
      .studioDocumento(id)
      .then((letto) => {
        if (vivo && !attuale.current.caricato) manda({ tipo: "apri", testo: letto });
      })
      .catch((e: unknown) => {
        if (vivo) setGuastoApertura({ errore: e });
        onErrore(e);
      });
    return () => {
      vivo = false;
    };
  }, [id, onErrore, tentativo]);

  // ── la validazione: l'ultimo vince ─────────────────────────────────────
  //
  // Il mittente nasce nell'effetto e muore con lui: `ferma` è per sempre, e
  // un mittente tenuto in `useMemo` e fermato dalla pulizia di un effetto
  // resterebbe fermo quando l'effetto riparte — in `StrictMode`, e ogni volta
  // che lo Studio torna visibile — e la tela smetterebbe di aggiornarsi.
  const mittente = useRef<Mittente | null>(null);
  const adesso = testo(s);
  useEffect(() => {
    const nuovo = creaMittente({
      // Con l'id della bozza di quel momento: il foglio si compila sotto di
      // lui, e la tela lo scrive in `data-skin` (`Validazione.radice`).
      valida: (testo) => ipc.studioValida(testo, idBozza.current),
      arrivato: (per, esito) => manda({ tipo: "validato", testo: per, esito }),
      guasto: onErrore,
    });
    mittente.current = nuovo;
    if (attuale.current.caricato) nuovo.manda(testo(attuale.current));
    return () => {
      nuovo.ferma();
      mittente.current = null;
    };
  }, [onErrore]);
  // Anche quando cambia l'id della bozza: il testo è lo stesso, ma le
  // risorse adesso stanno in un'altra cartella, e il foglio le deve chiedere
  // lì.
  useEffect(() => {
    if (s.caricato) mittente.current?.manda(adesso);
  }, [adesso, s.caricato, s.id]);

  // ── la bozza si salva da sé ─────────────────────────────────────────────
  //
  // Anche se non è valida — un documento a metà è un lavoro in corso — e
  // anche quando si torna all'originale: la bozza su disco deve dire quel che
  // si vede, o riaprendo si ritroverebbe una versione annullata.
  //
  // I salvataggi vanno in fila. Il nucleo sposta la bozza quando il documento
  // cambia `id`, e due salvataggi in volo insieme — quello automatico e un
  // Ctrl+S — partivano dallo stesso id: il secondo trovava la cartella nuova
  // già occupata dal primo e riscriveva quella vecchia. In fila, ognuno parte
  // da dove l'altro ha lasciato la bozza (`idBozza`).
  const fila = useRef<Promise<void>>(Promise.resolve());
  /** L'id che il documento dichiarava l'ultima volta che è stato rifiutato. */
  const rifiutoDetto = useRef<string | null>(null);
  const salva = useCallback((): Promise<void> => {
    const giro = fila.current.then(async () => {
      const ora = attuale.current;
      if (!daSalvare(ora)) return;
      const quale = testo(ora);
      const da = idBozza.current;
      const esito = await ipc.studioSalva(da, quale, id);
      if (esito.id !== da) {
        idBozza.current = esito.id;
        manda({ tipo: "rinominato", id: esito.id });
      }
      manda({ tipo: "salvato", testo: quale });
      // Un id già preso si dice una volta, non a ogni salvataggio: fino alla
      // 3.0 non si diceva affatto, e la bozza restava sotto il nome vecchio
      // senza che nessuno sapesse perché.
      if (esito.rinomina === null) {
        rifiutoDetto.current = null;
      } else {
        const dichiarato = String(leggi(quale)?.["id"] ?? "");
        if (rifiutoDetto.current !== dichiarato) {
          rifiutoDetto.current = dichiarato;
          onErrore(esito.rinomina);
        }
      }
    });
    // La fila non si ferma su un guasto: il prossimo salvataggio riparte.
    fila.current = giro.catch(() => undefined);
    return giro;
  }, [id, onErrore]);

  useEffect(() => {
    if (!daSalvare(s)) return;
    const attesa = window.setTimeout(() => void salva().catch(onErrore), RESPIRO_SALVATAGGIO);
    return () => window.clearTimeout(attesa);
  }, [s, salva, onErrore]);
  // E smontandosi, o nascondendosi sotto `<Activity>`: un'altra skin aperta
  // durante la prova rimonta lo Studio (`key` in `App`), e il respiro qui
  // sopra si spegne con lui — l'ultima modifica restava indietro. `salva`
  // legge dai ref, quindi lavora anche a componente smontato.
  useEffect(
    () => () => {
      if (daSalvare(attuale.current)) void salva().catch(onErrore);
    },
    [salva, onErrore],
  );

  // ── quel che gli editor tengono in mano ─────────────────────────────────
  //
  // L'editor del codice di una parte applica il suo testo dopo mezzo secondo,
  // e solo se è JSON: fino alla 3.2 «Esci», Ctrl+S o «Prova» partivano senza
  // le ultime battute, e un testo a metà spariva con l'editor (R14-3). Chi
  // salva per un gesto svuota prima gli editor, dentro un `flushSync`, così
  // il negozio — e `attuale`, che `salva` legge — ha già il loro testo.
  const sospesi = useRef(new Set<() => void>());
  const registraSospeso = useCallback((svuota: () => void) => {
    sospesi.current.add(svuota);
    return () => {
      sospesi.current.delete(svuota);
    };
  }, []);
  const svuotaSospesi = useCallback(() => {
    flushSync(() => {
      for (const svuota of sospesi.current) svuota();
    });
  }, []);
  /** La domanda di «Esci» con un codice che non si è potuto applicare. */
  const [domandaUscita, setDomandaUscita] = useState<Domanda | null>(null);

  /**
   * Esce solo a salvataggio riuscito: niente si perde chiudendo.
   *
   * Fino alla 3.0 usciva comunque — `finally` — e un salvataggio fallito
   * (disco pieno, cartella senza permessi) diventava un messaggio sopra uno
   * Studio che non c'era più, col lavoro dentro.
   *
   * Un codice di parte che non è JSON non arriva al documento, e uscendo se
   * ne andrebbe: lo si chiede prima, nominando le parti.
   */
  const esci = useCallback(() => {
    svuotaSospesi();
    const sospese = Object.keys(attuale.current.codiceInSospeso).map((chiave) => chiave.split("/")[1] ?? chiave);
    if (sospese.length === 0) {
      void poiSeSalvato(salva, onEsci, onErrore);
      return;
    }
    setDomandaUscita({
      titolo: t("studio3.code.pending.title"),
      descrizione: t("studio3.code.pending.what", { parti: [...new Set(sospese)].join(", ") }),
      conferma: t("studio3.code.pending.leave"),
      fai: () => void poiSeSalvato(salva, onEsci, onErrore),
    });
  }, [salva, onEsci, onErrore, svuotaSospesi]);

  // ── le scritture ────────────────────────────────────────────────────────
  //
  // Come operazioni, e il testo lo calcola il riduttore: vedi l'azione
  // `modifica` in `negozio.ts`. Calcolarlo qui da `attuale.current` voleva
  // dire che due scritture nello stesso gestore partivano dallo stesso testo.
  const scrivi = useCallback(
    (percorso: readonly string[], valore: unknown, etichetta: string) => {
      const togli = valore === null || valore === undefined;
      manda({
        tipo: "modifica",
        operazioni: [{ togli, percorso: [...percorso], valore: togli ? undefined : valore }],
        etichetta,
        gruppo: percorso.join("."),
        adesso: Date.now(),
      });
    },
    [],
  );
  const scriviTesto = useCallback((nuovo: string, etichetta: string, gruppo = "testo") => {
    manda({ tipo: "scrivi", testo: nuovo, etichetta, gruppo, adesso: Date.now() });
  }, []);
  const scriviMolti = useCallback(
    (operazioni: readonly OperazioneIa[], etichetta: string, gruppo?: string) => {
      manda({
        tipo: "modifica",
        operazioni,
        etichetta,
        gruppo: gruppo ?? operazioni.map((o) => o.percorso.join(".")).join("|"),
        adesso: Date.now(),
      });
    },
    [],
  );

  // ── i file del pacchetto ────────────────────────────────────────────────
  const { voci: file, rileggi: rileggiFile } = usePacchetto(s.id, onErrore);

  // ── le azioni della barra ───────────────────────────────────────────────
  // Tutte con `idBozza` e non con l'id del negozio: vengono dopo un
  // salvataggio che può aver spostato la bozza. Vedi `idBozza`.
  /** Il testo dell'ultima istantanea «manuale» presa in questa sessione. */
  const ultimaManuale = useRef<string | null>(null);
  const istantanea = useCallback(
    async (causa: "manuale" | "salvata" | "esportata") => {
      const quale = testo(attuale.current);
      await ipc.studioIstantanea(idBozza.current, quale, causa);
      if (causa === "manuale") ultimaManuale.current = quale;
      // L'elenco del pannello «Istantanee» si rilegge: vedi `fotografata`.
      manda({ tipo: "fotografata" });
    },
    [],
  );
  /**
   * L'istantanea di Ctrl+S e della chat in agent: solo se il testo non è
   * quello dell'ultima manuale. Vedi `daFotografare`.
   */
  const istantaneaManuale = useCallback(async () => {
    const quale = testo(attuale.current);
    if (!daFotografare(quale, ultimaManuale.current)) return;
    // Segnata prima dell'attesa: due Ctrl+S ravvicinati non ne prendono due
    // uguali. Se non riesce, la prossima ci riprova.
    const prima = ultimaManuale.current;
    ultimaManuale.current = quale;
    try {
      await istantanea("manuale");
    } catch (e) {
      ultimaManuale.current = prima;
      throw e;
    }
  }, [istantanea]);

  const salvaEUsa = useCallback(async () => {
    svuotaSospesi();
    const ora = attuale.current;
    try {
      await salva();
      await istantanea("salvata");
      const voce = await ipc.skinInstallaSorgente(testo(ora), idBozza.current);
      onInstallata(voce.id);
    } catch (e) {
      onErrore(e);
    }
  }, [salva, istantanea, onInstallata, onErrore, svuotaSospesi]);

  const esporta = useCallback(async () => {
    svuotaSospesi();
    const ora = attuale.current;
    try {
      await salvaCon(
        {
          defaultPath: `${idBozza.current}.ryskin`,
          filters: [{ name: "Rymembra skin", extensions: ["ryskin"] }],
          tipo: "application/zip",
        },
        async (dove) => {
          await istantanea("esportata");
          await ipc.studioEsporta(idBozza.current, testo(ora), dove);
        },
      );
    } catch (e) {
      onErrore(e);
    }
  }, [istantanea, onErrore, svuotaSospesi]);

  const prova = useCallback(async () => {
    svuotaSospesi();
    const ora = attuale.current;
    try {
      // Prima si salva: la prova usa le risorse della bozza su disco, e chi
      // torna deve ritrovare quel che ha provato. Poi si compila sotto l'id
      // della bozza, dove le risorse stanno davvero.
      await salva();
      const sorgente = testo(ora);
      const bozza = idBozza.current;
      const skin = await ipc.studioCompilaApp(sorgente, bozza);
      onProva({ skin, sorgente, id: bozza });
    } catch (e) {
      onErrore(e);
    }
  }, [salva, onProva, onErrore, svuotaSospesi]);

  /**
   * I gesti della barra che salvano e installano, uno alla volta.
   *
   * Un doppio clic su «Prova» con la compilazione non ancora in memoria
   * arrivava due volte a `iniziaProva`, e la seconda prendeva la bozza già
   * indossata per «la skin di prima»; su «Salva e usa» faceva due istantanee e
   * due installazioni (R14-8). Il ref ferma il secondo clic dello stesso giro,
   * lo stato spegne i tasti finché il primo non ha finito.
   */
  const gestoInVolo = useRef(false);
  const [inVolo, setInVolo] = useState(false);
  const unoAllaVolta = useCallback(async (fai: () => Promise<void>) => {
    if (gestoInVolo.current) return;
    gestoInVolo.current = true;
    setInVolo(true);
    try {
      await fai();
    } finally {
      gestoInVolo.current = false;
      setInVolo(false);
    }
  }, []);

  // ── la tastiera ─────────────────────────────────────────────────────────
  useEffect(() => {
    const ascolta = (e: KeyboardEvent) => {
      const ora = attuale.current;
      const mod = e.ctrlKey || e.metaKey;
      const tasto = e.key.toLowerCase();
      const dove = bersaglio(e.target);
      // Dentro un campo di testo il campo ha il suo annullo: quel che si sta
      // scrivendo lì — un nome, un messaggio alla chat — non è il documento.
      // Un cursore, una casella o un elenco invece un annullo non ce l'hanno,
      // e il fuoco ci resta dopo il gesto: lì Ctrl+Z è dello Studio. Vedi
      // `annullo.ts`. L'editor di codice passa il suo Ctrl+Z allo Studio da
      // sé (`codice.ts`).
      if (mod && !e.altKey && !haAnnulloProprio(dove)) {
        if (tasto === "z" && !e.shiftKey) {
          e.preventDefault();
          manda({ tipo: "annulla" });
          return;
        }
        if ((tasto === "z" && e.shiftKey) || tasto === "y") {
          e.preventDefault();
          manda({ tipo: "ripeti" });
          return;
        }
      }
      if (mod && tasto === "s") {
        e.preventDefault();
        svuotaSospesi();
        void salva()
          .then(istantaneaManuale)
          .catch(onErrore);
        return;
      }
      if (mod && tasto === "k") {
        e.preventDefault();
        // Disegnato subito: questo è un ascoltatore nativo, e React
        // disegnerebbe il cambio di colonna dopo. Partendo da Variabili il
        // campo dei Livelli era ancora dentro un `<Activity mode="hidden">`,
        // cioè `display: none`, e il fuoco non ci andava.
        flushSync(() => manda({ tipo: "imposta", campi: { sinistra: "livelli" } }));
        document.querySelector<HTMLInputElement>(".studio3 .livelli-studio .cerca-livelli input")?.focus();
        return;
      }
      if (mod || e.altKey || prendeITasti(dove)) return;
      if (e.key === "Escape") {
        manda({ tipo: "scegli", selezione: null });
        return;
      }
      if (tasto === "i") {
        e.preventDefault();
        manda({ tipo: "imposta", campi: { sonda: !ora.sonda } });
        return;
      }
      if (tasto === "f") {
        e.preventDefault();
        manda({ tipo: "rigioca" });
        return;
      }
      const schede = ["stile", "stati", "movimento", "codice"] as const;
      const numero = Number.parseInt(e.key, 10);
      const scheda = schede[numero - 1];
      if (scheda !== undefined) {
        e.preventDefault();
        manda({ tipo: "imposta", campi: { ispettore: scheda } });
      }
    };
    window.addEventListener("keydown", ascolta);
    return () => window.removeEventListener("keydown", ascolta);
  }, [salva, istantaneaManuale, onErrore, svuotaSospesi]);

  const documento = useMemo(() => leggi(adesso), [adesso]);
  const parti = useMemo(
    () => new Map((registro?.parts ?? []).map((p) => [p.name, p])),
    [registro],
  );
  // Le due metà del contesto, ognuna stabile finché non cambia davvero:
  // vedi `contesto.ts`.
  const azioni = useMemo<AzioniDelloStudio | null>(
    () =>
      registro === null
        ? null
        : { manda, registro, parti, scrivi, scriviMolti, scriviTesto, rileggiFile, registraSospeso, onErrore },
    [registro, parti, scrivi, scriviMolti, scriviTesto, rileggiFile, registraSospeso, onErrore],
  );
  const statoDelloStudio = useMemo<StatoDelloStudio>(
    () => ({ s, documento, file }),
    [s, documento, file],
  );

  if (azioni === null || !s.caricato) {
    if (guastoApertura !== null) {
      // Niente da salvare — il documento non c'è, o non si può toccare senza
      // registro — ma «Chiudi» passa da `esci` lo stesso: è la strada che non
      // perde niente anche nel caso che qui non si vede.
      return (
        <div className="studio3 caricando">
          <Avviso
            livello="blocco"
            azione={
              <>
                <button
                  type="button"
                  className="bottone primario btn-accent"
                  onClick={() => {
                    setGuastoApertura(null);
                    setTentativo((n) => n + 1);
                  }}
                >
                  {t("common.retry")}
                </button>
                <button type="button" className="bottone" onClick={esci}>
                  {t("crash.studio.close")}
                </button>
              </>
            }
          >
            <strong>{t("studio3.open.failed")}</strong> {testoErrore(guastoApertura.errore)}
          </Avviso>
        </div>
      );
    }
    return (
      <div className="studio3 caricando" role="status">
        <Icona nome="i-skin" dim={22} />
        <span>{t("common.loading")}</span>
      </div>
    );
  }

  return (
    <ContestoAzioniStudio value={azioni}>
      <ContestoStatoStudio value={statoDelloStudio}>
        <div
          className="studio3"
          data-dock={s.dock !== null || undefined}
          data-chat={s.chat || undefined}
        >
          <BarraStudio
            inVolo={inVolo}
            onEsci={esci}
            onSalvaEUsa={() => void unoAllaVolta(salvaEUsa)}
            onEsporta={() => void unoAllaVolta(esporta)}
            onProva={() => void unoAllaVolta(prova)}
          />
          <aside className="studio3-sinistra" aria-label={t("studio3.left.aria")}>
            <Schede<SchedaSinistra>
              etichetta={t("studio3.left.aria")}
              voci={[
                ["livelli", t("studio3.left.layers")],
                ["variabili", t("studio3.left.variables")],
              ]}
              scelta={s.sinistra}
              onScegli={(sinistra) => manda({ tipo: "imposta", campi: { sinistra } })}
            />
            {/* Tutte e due sotto `<Activity>`: passando dall'una all'altra si
                ritrovano le sezioni aperte, la ricerca e lo scorrimento. */}
            <Activity mode={s.sinistra === "livelli" ? "visible" : "hidden"}>
              <Livelli />
            </Activity>
            <Activity mode={s.sinistra === "variabili" ? "visible" : "hidden"}>
              <Variabili />
            </Activity>
          </aside>
          <main className="studio3-centro">
            <Centro />
          </main>
          <aside className="studio3-destra" aria-label={t("studio3.right.aria")}>
            <Ispezione />
          </aside>
          {s.dock !== null && <Dock />}
          {s.chat && (
            <div className="studio3-chat">
              <Chat
                sorgente={adesso}
                onSorgente={(nuovo) => scriviTesto(nuovo, t("studio3.step.chat"), "chat")}
                onIstantanea={() => void istantaneaManuale().catch(onErrore)}
                onErrore={onErrore}
                onChiudi={() => manda({ tipo: "imposta", campi: { chat: false } })}
                // Anche questa strada smonta lo Studio: la stessa regola di
                // «Esci», o quel che il respiro del salvataggio non aveva
                // ancora scritto andrebbe perso.
                onVaiAImpostazioni={() => {
                  svuotaSospesi();
                  void poiSeSalvato(salva, onModelli, onErrore);
                }}
              />
            </div>
          )}
          {domandaUscita !== null && (
            <ConfermaGesto domanda={domandaUscita} onChiudi={() => setDomandaUscita(null)} />
          )}
        </div>
      </ContestoStatoStudio>
    </ContestoAzioniStudio>
  );
}
