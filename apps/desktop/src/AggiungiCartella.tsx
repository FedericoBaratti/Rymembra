/**
 * «Aggiungi una cartella»: dal disco o dal cloud, da un posto solo.
 *
 * # Perché una finestrella, e non il dialogo di sistema
 *
 * Il tasto «Aggiungi cartella…» apriva il dialogo di Windows, e il dialogo di
 * Windows di OneDrive conosce soltanto la cartella sincronizzata — quando c'è
 * — e di Google Drive, Dropbox, MEGA o un Nextcloud niente. Qui le sorgenti
 * stanno in fila, come linguette: «Questo PC» apre il dialogo di sempre, e
 * ogni servizio apre la sua strada — il consenso nel browser, o un modulo — e
 * poi le sue cartelle, da sfogliare come in un gestore di file. Una sola
 * entrata per tutti e quattro i posti da cui si arriva: le Impostazioni, lo
 * stato vuoto «nessuna cartella», il primo avvio, e «Ricollega» di un account.
 *
 * # Istantanea
 *
 * - Le linguette e i conti arrivano dal fornitore (`contiNuvola.ts`), letti
 *   all'avvio della finestra: aprendo non si aspetta niente.
 * - Le cartelle remote si chiedono una volta per dialogo e restano
 *   (`selettoreNuvola.ts`): risalire, tornare a una briciola, cambiare
 *   linguetta e tornare sono gratis, e una risposta arrivata tardi non scrive
 *   mai sopra quel che si guarda.
 * - Selezionare una linguetta non avvia niente: le frecce del gruppo scelgono
 *   la linguetta mentre ci passano sopra, e un consenso nel browser aperto da
 *   una freccia sarebbe la sorpresa peggiore. Il browser lo apre solo
 *   «Collega».
 *
 * # Le password
 *
 * Non restano nello stato dopo l'invio: il campo si svuota nel momento in cui
 * parte la richiesta. L'eccezione è la verifica in due passaggi di MEGA, che
 * chiede il codice **dopo** aver provato email e password: la password resta
 * in un riferimento — non nello stato, non nel campo — per quell'unico secondo
 * tentativo, e si cancella appena il collegamento riesce, fallisce per altro, o
 * la finestrella si chiude.
 *
 * # Le app
 *
 * Dalla 3.2 OneDrive, Google Drive e Dropbox si collegano con un'app che
 * l'utente registra da sé (`appNuvola.ts`). Una linguetta senza la sua app non
 * è spenta: si apre sul modulo che la configura, e salvata l'app sotto c'è
 * «Collega» — lo stesso pannello, senza smontarsi, così il fuoco passa dal
 * tasto «Salva» a quello che viene dopo. Con più app della stessa piattaforma
 * si sceglie con quale («Con l'app»). E un conto da ricollegare per colpa
 * dell'app — rifiutata, senza segreto, tolta — mostra prima l'app da sistemare
 * o da scegliere: ricollegarlo e basta riaprirebbe il browser per farsi dire
 * di no un'altra volta.
 */
import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as TastoReact,
} from "react";

import {
  appDellaPiattaforma,
  appPredefinita,
  piattaformaDi,
  rimedioDi,
  serveLApp,
} from "./appNuvola";
import { NOMI_SERVIZI, iconaServizio, schedaDi, type Scheda } from "./cartelleNuvola";
import { ConfermaGesto, type Domanda } from "./ConfermaGesto";
import { useContiNuvolaDellaFinestra } from "./contiNuvola";
import { Dialogo } from "./Dialogo";
import {
  eErroreIpc,
  ipc,
  testoErrore,
  type ContoNuvola,
  type ModuloCollega,
  type ServizioNuvola,
} from "./ipc";
import { t } from "./lingue";
import { AppServizio } from "./parti/AppServizio";
import { Icona } from "./parti/Icone";
import { Segmentato, type Voce } from "./parti/Segmentato";
import {
  NESSUN_ELENCO,
  PREIMPOSTAZIONI_WEBDAV,
  apribile,
  arrivato,
  chiaveElenco,
  chiesto,
  codiceCompleto,
  codicePulito,
  contiDella,
  contoDaMostrare,
  daChiedere,
  daDoveSiRisale,
  dentro,
  difettoIndirizzo,
  entra,
  fallito,
  inCima,
  inVirtuale,
  indirizzoDaMandare,
  linguetteDi,
  linkMegaPlausibile,
  nomeDiQui,
  nomiDalla,
  riprova,
  risali,
  sfogliabile,
  statoLinguetta,
  statoOfferta,
  tornaA,
  usabile,
  vista,
  type Briciole,
  type DifettoIndirizzo,
  type Elenchi,
  type Linguetta,
  type StatoOfferta,
} from "./selettoreNuvola";

/** Da dove viene la cartella: questo PC, o una linguetta di servizio. */
type Sorgente = "pc" | Scheda;

/**
 * L'ultima linguetta scelta, in questa finestra.
 *
 * Chi aggiunge tre cartelle di OneDrive di fila riapre la finestrella tre
 * volte, e ripartire ogni volta da «Questo PC» sarebbero tre clic per tornare
 * dove era. Non si salva: al prossimo avvio si riparte dal disco, che è la
 * risposta più comune.
 */
let ultimaSorgente: Sorgente = "pc";

/**
 * Perché una linguetta è spenta, detto. `undefined` se non lo è.
 *
 * Solo «in arrivo»: una linguetta senza la sua app si apre, e la configura.
 */
function ragioneSpenta(stato: StatoOfferta, servizio: string): string | undefined {
  return stato === "inArrivo" ? t("folders.add.soon", { servizio }) : undefined;
}

/** Il guasto di un collegamento è la richiesta del codice di MEGA? */
function chiedeIlCodice(guasto: unknown): boolean {
  return eErroreIpc(guasto) && guasto.code === "cloud.twoFactorRequired";
}

// ── il collegamento ─────────────────────────────────────────────────────────

/**
 * Un tentativo di collegamento, con la sua attesa.
 *
 * L'attesa del browser può durare tre minuti. In quel tempo chi guarda può
 * premere «Annulla», cambiare linguetta o chiudere la finestrella, e in tutti
 * e tre i casi la risposta che arriva dopo non deve fare niente: né aprire le
 * cartelle di un conto che non si aspetta più nessuno, né mostrare un guasto
 * in una finestrella che non c'è. Il numero del tentativo è quel che lo
 * decide; e uscire di scena con un'attesa in corso la ferma anche nel nucleo,
 * che altrimenti terrebbe aperta la porta del consenso fino alla sua scadenza.
 */
function useCollegamento(onCollegato: (conto: ContoNuvola) => void) {
  const [inAttesa, setInAttesa] = useState(false);
  const [guasto, setGuasto] = useState<unknown>(null);
  const vivo = useRef(true);
  /** Il tentativo in corso; zero quando non ce n'è nessuno. */
  const tentativo = useRef(0);
  const prossimo = useRef(0);

  useEffect(() => {
    vivo.current = true;
    return () => {
      vivo.current = false;
      if (tentativo.current !== 0) {
        tentativo.current = 0;
        // Chi chiude non aspetta l'esito: un guasto qui non ha nessuno a cui
        // essere detto, e la porta del nucleo scade comunque da sé.
        ipc.nuvolaContoAnnulla().catch(() => undefined);
      }
    };
  }, []);

  /**
   * Prova a collegare, con l'app `appId` (`null`: la predefinita, o nessuna
   * per WebDAV e MEGA). Torna il guasto, o `null` se è andata o se non conta più.
   */
  const prova = async (
    servizio: ServizioNuvola,
    appId: string | null,
    modulo?: ModuloCollega,
  ): Promise<unknown> => {
    prossimo.current += 1;
    const questo = prossimo.current;
    tentativo.current = questo;
    setInAttesa(true);
    setGuasto(null);
    try {
      const conto = await ipc.nuvolaContoCollega(servizio, appId, modulo);
      if (!vivo.current || tentativo.current !== questo) return null;
      tentativo.current = 0;
      setInAttesa(false);
      onCollegato(conto);
      return null;
    } catch (e) {
      // Annullato, o la finestrella è andata: il nucleo risponde con un guasto
      // o con niente, e in tutti e due i casi qui non si dice nulla.
      if (!vivo.current || tentativo.current !== questo) return null;
      tentativo.current = 0;
      setInAttesa(false);
      setGuasto(e);
      return e;
    }
  };

  /** «Annulla» durante l'attesa del consenso. */
  const annulla = () => {
    if (tentativo.current === 0) return;
    tentativo.current = 0;
    setInAttesa(false);
    ipc.nuvolaContoAnnulla().catch(() => undefined);
  };

  return { inAttesa, guasto, prova, annulla };
}

/** Il nome e l'icona del servizio, in testa al pannello che lo collega. */
function TestaServizio({ servizio, nome }: { servizio: ServizioNuvola; nome: string }) {
  return (
    <div className="testa-collega">
      <Icona nome={iconaServizio(servizio)} dim={18} />
      <h3>{nome}</h3>
    </div>
  );
}

/** Il conto da ricollegare, e perché. */
function AvvisoRicollega({ conto }: { conto: ContoNuvola }) {
  return (
    <div className="avviso" data-livello="avviso">
      <Icona nome="i-alert" dim={15} />
      <div className="dentro">
        {t("folders.add.reconnect", { nome: conto.nome })}
        {conto.errore !== null && ` ${testoErrore(conto.errore)}`}
      </div>
    </div>
  );
}

/** Le proprietà dei tre pannelli che collegano. */
interface Collegare {
  linguetta: Linguetta;
  /** Il conto che il servizio non accetta più, se si sta ricollegando quello. */
  daRicollegare: ContoNuvola | null;
  onCollegato: (conto: ContoNuvola) => void;
  /**
   * Chiede conferma prima di un gesto che tocca altri: cambiare l'ID di
   * un'app che usano anche altri conti. La domanda la disegna la finestrella,
   * fuori dal suo riquadro: vedi `AppServizio.tsx`.
   */
  onChiedi: (domanda: Domanda) => void;
}

/**
 * OneDrive, Google Drive, Dropbox: il consenso nel browser.
 *
 * La frase dice prima che cosa succede — si apre il browser — e poi la cosa
 * che chi dà un consenso vuole sapere: che cosa sta concedendo. Solo la
 * lettura, ed è vero: gli scope chiesti non permettono altro.
 *
 * Prima del consenso, l'app (vedi «Le app» in testa). Tre facce dello stesso
 * pannello, che resta montato mentre passa dall'una all'altra:
 *
 * - **nessuna app** della piattaforma: il modulo che la registra;
 * - **l'app da rivedere** — il conto la dice rifiutata o senza segreto, o
 *   l'ha chiesto chi guarda —: il modulo che la modifica, coi passi aperti;
 * - **il consenso**: «Collega», con la scelta dell'app se ce n'è più d'una.
 */
function PerBrowser({ linguetta, daRicollegare, onCollegato, onChiedi }: Collegare) {
  const { inAttesa, guasto, prova, annulla } = useCollegamento(onCollegato);
  const { app: registro, accogliApp } = useContiNuvolaDellaFinestra();
  const servizio = linguetta.offerto.servizio;
  const nome = linguetta.offerto.nome;
  const chiave = piattaformaDi(servizio);
  const piattaforma = registro?.piattaforme.find((p) => p.chiave === chiave) ?? null;
  const tutte = registro?.app ?? [];
  const sue = appDellaPiattaforma(tutte, chiave);
  const rimedio = rimedioDi(daRicollegare?.errore?.code);
  /** L'app scelta: per un conto da ricollegare la sua, se c'è ancora. */
  const [scelta, setScelta] = useState<string | null>(daRicollegare?.app ?? null);
  const appScelta = sue.find((a) => a.id === scelta) ?? appPredefinita(tutte, chiave);
  /**
   * Il modulo dell'app scelta è aperto. Da sé solo quando il guasto del conto
   * dice che è l'app a non andare: ricollegare senza toccarla darebbe lo stesso
   * guasto.
   */
  const [rivedi, setRivedi] = useState(rimedio === "correggiApp");
  /** Il fuoco nel modulo che compare: solo se l'ha aperto un tasto. */
  const [fuocoModulo, setFuocoModulo] = useState(false);
  const idScelta = useId();
  const collega = useRef<HTMLButtonElement>(null);
  const ferma = useRef<HTMLButtonElement>(null);
  const tastoRivedi = useRef<HTMLButtonElement>(null);
  /** Il fuoco segue il tasto che prende il posto di quello premuto. */
  const mosso = useRef(false);
  useLayoutEffect(() => {
    if (!mosso.current) return;
    (inAttesa ? ferma : collega).current?.focus();
  }, [inAttesa]);
  /**
   * Dove va il fuoco quando il modulo dell'app se ne va: su «Collega» dopo un
   * salvataggio — è il passo dopo —, sul tasto che l'aveva aperto dopo
   * «Annulla». Il tasto compare nello stesso disegno in cui il modulo sparisce,
   * quindi lo si cerca dopo ogni disegno finché non c'è.
   */
  const dopoIlModulo = useRef<"collega" | "rivedi" | null>(null);
  useLayoutEffect(() => {
    const dove = dopoIlModulo.current;
    if (dove === null) return;
    const nodo = (dove === "collega" ? collega : tastoRivedi).current;
    if (nodo === null) return;
    dopoIlModulo.current = null;
    nodo.focus();
  });

  // Nessuna app per questa piattaforma: prima si registra.
  if (piattaforma !== null && sue.length === 0) {
    return (
      <div className="collega-nuvola">
        <TestaServizio servizio={servizio} nome={nome} />
        {daRicollegare !== null && <AvvisoRicollega conto={daRicollegare} />}
        <p>{t("folders.add.needApp", { servizio: nome, piattaforma: piattaforma.nome })}</p>
        <AppServizio
          piattaforma={piattaforma}
          app={null}
          tutte={tutte}
          livello={4}
          onChiedi={onChiedi}
          onSalvata={(dopo, nuova) => {
            accogliApp(dopo);
            setScelta(nuova?.id ?? null);
            setRivedi(false);
            dopoIlModulo.current = "collega";
          }}
        />
      </div>
    );
  }

  // L'app da rivedere: rifiutata, senza segreto, o aperta da «Rivedi l'app».
  if (piattaforma !== null && rivedi && appScelta !== null) {
    return (
      <div className="collega-nuvola">
        <TestaServizio servizio={servizio} nome={nome} />
        {daRicollegare !== null && <AvvisoRicollega conto={daRicollegare} />}
        <AppServizio
          // Una chiave per app: passando a un'altra, il modulo riparte coi suoi valori.
          key={appScelta.id}
          piattaforma={piattaforma}
          app={appScelta}
          tutte={tutte}
          livello={4}
          // Per un segreto che manca i passi non servono — l'app è giusta, le
          // manca un campo —, a meno che non li abbia chiesti chi guarda.
          passiAperti={fuocoModulo || daRicollegare?.errore?.code !== "cloud.appSecretMissing"}
          fuoco={fuocoModulo ? "titolo" : undefined}
          onChiedi={onChiedi}
          onSalvata={(dopo) => {
            accogliApp(dopo);
            setRivedi(false);
            dopoIlModulo.current = "collega";
          }}
          onAnnulla={() => {
            setRivedi(false);
            dopoIlModulo.current = "rivedi";
          }}
        />
      </div>
    );
  }

  const appId = appScelta?.id ?? null;
  const daSistemare = guasto !== null && eErroreIpc(guasto) && serveLApp(guasto.code);

  return (
    <div className="collega-nuvola">
      <TestaServizio servizio={servizio} nome={nome} />
      {daRicollegare !== null && <AvvisoRicollega conto={daRicollegare} />}
      {rimedio === "scegliApp" && sue.length > 1 && (
        <p className="nota-collega">{t("folders.add.chooseApp")}</p>
      )}
      <p>{t("folders.add.browser.how", { servizio: nome })}</p>
      {sue.length > 1 && appScelta !== null && (
        <div className="con-app">
          <label htmlFor={idScelta}>{t("folders.add.withApp")}</label>
          <select
            id={idScelta}
            className="scegli-conto"
            value={appScelta.id}
            disabled={inAttesa}
            onChange={(e) => setScelta(e.target.value)}
          >
            {sue.map((a) => (
              <option key={a.id} value={a.id}>
                {a.predefinita ? t("folders.add.withApp.default", { nome: a.nome }) : a.nome}
              </option>
            ))}
          </select>
        </div>
      )}
      {guasto !== null && (
        <div className="errore" role="alert">
          {testoErrore(guasto)}
        </div>
      )}
      <div className="tasti-collega">
        {inAttesa ? (
          <>
            <span className="attesa-collega">{t("folders.add.waiting")}</span>
            <button
              ref={ferma}
              type="button"
              className="bottone"
              onClick={() => {
                mosso.current = true;
                annulla();
              }}
            >
              {t("common.cancel")}
            </button>
          </>
        ) : (
          <>
            <button
              ref={collega}
              type="button"
              className="bottone primario btn-accent"
              onClick={() => {
                mosso.current = true;
                void prova(servizio, appId);
              }}
            >
              <Icona nome={iconaServizio(servizio)} dim={15} />
              {t("folders.add.connect", { servizio: nome })}
            </button>
            {/* Sempre a portata, e in evidenza dopo un rifiuto: è l'uscita da un
                consenso che il fornitore nega, ed è la strada per correggere un
                passo saltato nella console. */}
            {appScelta !== null && piattaforma !== null && (
              <button
                ref={tastoRivedi}
                type="button"
                className={daSistemare ? "bottone" : "bottone btn-ghost"}
                onClick={() => {
                  setFuocoModulo(true);
                  setRivedi(true);
                }}
              >
                {t("folders.add.reviewApp", { nome: appScelta.nome })}
              </button>
            )}
          </>
        )}
      </div>
      {/* Microsoft e Dropbox, davanti a un indirizzo di ritorno non registrato
          o a un ID che non esiste, mostrano l'errore **nel browser** e non
          tornano: da qui si vede solo un'attesa che non finisce. Lo si dice. */}
      {inAttesa && <p className="nota-collega">{t("folders.add.waitingHint")}</p>}
      {/* Sempre montata, e vuota quando non c'è niente da dire: una regione viva
          che nasce insieme al suo testo non tutti i lettori la annunciano. */}
      <p className="solo-lettori" role="status">
        {inAttesa ? t("folders.add.waiting") : ""}
      </p>
      <p className="nota-collega">{t("folders.add.syncedHint", { servizio: nome })}</p>
    </div>
  );
}

/** Il messaggio di un difetto dell'indirizzo WebDAV. */
function fraseDifetto(difetto: DifettoIndirizzo): string {
  switch (difetto) {
    case "vuoto":
      return t("folders.add.webdav.bad.empty");
    case "segnaposto":
      return t("folders.add.webdav.bad.placeholder");
    case "credenziali":
      return t("errors.cloud.credentialsInAddress");
    case "http":
      return t("errors.cloud.insecureServer");
    case "nonHttps":
      return t("folders.add.webdav.bad.https");
  }
}

/**
 * WebDAV: indirizzo, utente, password per app.
 *
 * Le preimpostazioni scrivono la **forma** dell'indirizzo, coi pezzi da
 * sostituire fra graffe e il primo già selezionato: chi ha un Nextcloud sa il
 * nome del suo server, non sa dove sta la cartella WebDAV dentro di lui. Il
 * nome utente nel percorso lo mette il modulo, da quello scritto sotto.
 */
function PerWebdav({ linguetta, daRicollegare, onCollegato }: Collegare) {
  const { inAttesa, guasto, prova } = useCollegamento(onCollegato);
  const [preimpostata, setPreimpostata] = useState<string | null>(null);
  const [indirizzo, setIndirizzo] = useState("");
  // Ricollegando, l'utente c'è già: il nome del conto è `utente@server`. Solo
  // l'utente, e non l'indirizzo: il nome porta il server ma non il percorso
  // (`/remote.php/dav/files/…`), e un indirizzo a metà sarebbe un errore al
  // primo tentativo. Per il percorso ci sono le preimpostazioni qui sotto.
  const [utente, setUtente] = useState(() => {
    const chiocciola = daRicollegare?.nome.lastIndexOf("@") ?? -1;
    return daRicollegare === null || chiocciola < 0 ? "" : daRicollegare.nome.slice(0, chiocciola);
  });
  const [password, setPassword] = useState("");
  /** Il difetto dell'indirizzo, detto dopo un invio: non mentre si scrive. */
  const [difetto, setDifetto] = useState<DifettoIndirizzo | null>(null);
  const campo = useRef<HTMLInputElement>(null);
  const idDifetto = useId();
  const nome = linguetta.offerto.nome;

  const scegli = (chiave: string, forma: string) => {
    setPreimpostata(chiave);
    setIndirizzo(forma);
    setDifetto(null);
    // Dopo il disegno, quando il campo ha il valore nuovo: si seleziona il
    // primo segnaposto, così il primo tasto lo sostituisce.
    requestAnimationFrame(() => {
      const nodo = campo.current;
      if (nodo === null) return;
      nodo.focus();
      const da = forma.indexOf("{server}");
      if (da >= 0) nodo.setSelectionRange(da, da + "{server}".length);
    });
  };

  const invia = () => {
    if (inAttesa) return;
    const dove = indirizzoDaMandare(indirizzo, utente);
    const male = difettoIndirizzo(dove);
    setDifetto(male);
    if (male !== null) {
      campo.current?.focus();
      return;
    }
    const segreto = password;
    // Via dallo stato prima ancora che la richiesta parta.
    setPassword("");
    void prova("webdav", null, { indirizzo: dove, utente: utente.trim(), password: segreto });
  };

  return (
    <form
      className="collega-nuvola"
      onSubmit={(e) => {
        e.preventDefault();
        invia();
      }}
    >
      <TestaServizio servizio="webdav" nome={nome} />
      {daRicollegare !== null && <AvvisoRicollega conto={daRicollegare} />}
      <div className="preimpostazioni" role="group" aria-label={t("folders.add.webdav.presets")}>
        {PREIMPOSTAZIONI_WEBDAV.map((p) => (
          <button
            key={p.chiave}
            type="button"
            className="bottone minuto"
            aria-pressed={preimpostata === p.chiave}
            data-active={preimpostata === p.chiave || undefined}
            disabled={inAttesa}
            onClick={() => scegli(p.chiave, p.forma)}
          >
            {p.nome}
          </button>
        ))}
      </div>
      <label>
        {t("folders.add.webdav.address")}
        <input
          ref={campo}
          type="url"
          className="campo field-input"
          value={indirizzo}
          placeholder="https://"
          autoComplete="url"
          spellCheck={false}
          disabled={inAttesa}
          aria-invalid={difetto !== null || undefined}
          aria-describedby={difetto !== null ? idDifetto : undefined}
          onChange={(e) => {
            setIndirizzo(e.target.value);
            setPreimpostata(null);
          }}
        />
      </label>
      {difetto !== null && (
        <p id={idDifetto} className="difetto-campo">
          {fraseDifetto(difetto)}
        </p>
      )}
      <div className="campi-affiancati">
        <label>
          {t("folders.add.webdav.user")}
          <input
            type="text"
            className="campo field-input"
            value={utente}
            autoComplete="username"
            spellCheck={false}
            disabled={inAttesa}
            onChange={(e) => setUtente(e.target.value)}
          />
        </label>
        <label>
          {t("folders.add.webdav.password")}
          <input
            type="password"
            className="campo field-input"
            value={password}
            autoComplete="current-password"
            disabled={inAttesa}
            onChange={(e) => setPassword(e.target.value)}
          />
        </label>
      </div>
      <p className="nota-collega">{t("folders.add.webdav.note")}</p>
      {guasto !== null && (
        <div className="errore" role="alert">
          {testoErrore(guasto)}
        </div>
      )}
      <div className="tasti-collega">
        <button
          type="submit"
          className="bottone primario btn-accent"
          disabled={inAttesa || utente.trim() === "" || password === ""}
        >
          <Icona nome="i-server" dim={15} />
          {inAttesa ? t("folders.add.trying") : t("folders.add.connect", { servizio: nome })}
        </button>
      </div>
      <p className="solo-lettori" role="status">
        {inAttesa ? t("folders.add.trying") : ""}
      </p>
    </form>
  );
}

/**
 * MEGA: l'account (email, password e, se serve, il codice) oppure un link.
 *
 * Due strade dentro la stessa linguetta, perché per chi ha in mano un link
 * condiviso «MEGA» è il posto dove cercarlo, e non deve accorgersi che dal lato
 * del nucleo sono due servizi (`mega` e `megalink`).
 */
function PerMega({ linguetta, daRicollegare, onCollegato }: Collegare) {
  /** La password del primo tentativo, per il secondo col codice: vedi il commento in testa. */
  const inVolo = useRef("");
  const { inAttesa, guasto, prova } = useCollegamento((conto) => {
    inVolo.current = "";
    onCollegato(conto);
  });
  useEffect(
    () => () => {
      inVolo.current = "";
    },
    [],
  );
  const conLink = linguetta.link;
  const statoAccount = linguetta.offerto.servizio === "mega" ? statoOfferta(linguetta.offerto) : "inArrivo";
  const statoLink = conLink === null ? "inArrivo" : statoOfferta(conLink);
  const [via, setVia] = useState<"account" | "link">(
    daRicollegare?.servizio === "megalink" || statoAccount !== "pronto" ? "link" : "account",
  );
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  /** Il primo tentativo ha chiesto il codice: la password è in `inVolo`. */
  const [serveCodice, setServeCodice] = useState(false);
  const [codice, setCodice] = useState("");
  const [link, setLink] = useState("");
  const [linkStorto, setLinkStorto] = useState(false);
  const campoCodice = useRef<HTMLInputElement>(null);
  const idCodice = useId();
  const idLink = useId();

  useLayoutEffect(() => {
    if (serveCodice) campoCodice.current?.focus();
  }, [serveCodice]);

  const ricomincia = () => {
    inVolo.current = "";
    setServeCodice(false);
    setCodice("");
  };

  const accedi = async () => {
    if (inAttesa) return;
    const segreto = serveCodice ? inVolo.current : password;
    if (email.trim() === "" || segreto === "") return;
    if (serveCodice && !codiceCompleto(codice)) {
      campoCodice.current?.focus();
      return;
    }
    inVolo.current = segreto;
    setPassword("");
    const guastoNuovo = await prova("mega", null, {
      utente: email.trim(),
      password: segreto,
      ...(serveCodice ? { codice: codicePulito(codice) } : {}),
    });
    if (guastoNuovo === null) return;
    if (chiedeIlCodice(guastoNuovo)) {
      // La password resta in `inVolo` per il tentativo col codice.
      setServeCodice(true);
      setCodice("");
    } else {
      // Un altro guasto — password sbagliata, codice scaduto — vuol dire
      // ricominciare: la password si riscrive, e non resta in memoria.
      ricomincia();
    }
  };

  const apriLink = () => {
    if (inAttesa) return;
    if (!linkMegaPlausibile(link)) {
      setLinkStorto(true);
      return;
    }
    setLinkStorto(false);
    void prova("megalink", null, { link: link.trim() });
  };

  return (
    <div className="collega-nuvola">
      <TestaServizio servizio="mega" nome={linguetta.offerto.nome} />
      {daRicollegare !== null && <AvvisoRicollega conto={daRicollegare} />}
      <Segmentato
        voci={[
          {
            chiave: "account",
            etichetta: t("folders.add.mega.account"),
            spenta: ragioneSpenta(statoAccount, "MEGA"),
          },
          {
            chiave: "link",
            etichetta: t("folders.add.mega.link"),
            spenta: ragioneSpenta(statoLink, "MEGA"),
          },
        ]}
        scelta={via}
        onScegli={(scelta) => {
          if (!inAttesa) setVia(scelta);
        }}
        etichetta={t("folders.add.mega.how")}
        classe="minuto"
      />
      {via === "account" ? (
        <form
          className="modulo-collega"
          onSubmit={(e) => {
            e.preventDefault();
            void accedi();
          }}
        >
          <label>
            {t("folders.add.mega.email")}
            <input
              type="email"
              className="campo field-input"
              value={email}
              autoComplete="username"
              spellCheck={false}
              disabled={inAttesa || serveCodice}
              onChange={(e) => setEmail(e.target.value)}
            />
          </label>
          {serveCodice ? (
            <>
              <label>
                {t("folders.add.mega.code")}
                <input
                  ref={campoCodice}
                  type="text"
                  className="campo mono field-input"
                  value={codice}
                  inputMode="numeric"
                  autoComplete="one-time-code"
                  maxLength={7}
                  spellCheck={false}
                  disabled={inAttesa}
                  aria-describedby={idCodice}
                  onChange={(e) => setCodice(e.target.value)}
                />
              </label>
              <p id={idCodice} className="nota-collega">
                {t("errors.cloud.twoFactorRequired")}
              </p>
            </>
          ) : (
            <label>
              {t("folders.add.mega.password")}
              <input
                type="password"
                className="campo field-input"
                value={password}
                autoComplete="current-password"
                disabled={inAttesa}
                onChange={(e) => setPassword(e.target.value)}
              />
            </label>
          )}
          {/* La richiesta del codice non è un guasto: è il passo dopo, e la
              dice già la riga sotto il campo. In rosso sembrerebbe un errore
              di chi ha scritto la password giusta. */}
          {guasto !== null && !chiedeIlCodice(guasto) && (
            <div className="errore" role="alert">
              {testoErrore(guasto)}
            </div>
          )}
          <div className="tasti-collega">
            <button
              type="submit"
              className="bottone primario btn-accent"
              disabled={
                inAttesa ||
                email.trim() === "" ||
                (serveCodice ? !codiceCompleto(codice) : password === "")
              }
            >
              <Icona nome="i-nuvola" dim={15} />
              {inAttesa ? t("folders.add.trying") : t("folders.add.connect", { servizio: "MEGA" })}
            </button>
            {serveCodice && (
              <button type="button" className="bottone" disabled={inAttesa} onClick={ricomincia}>
                {t("folders.add.mega.restart")}
              </button>
            )}
          </div>
          <p className="nota-collega">{t("folders.add.mega.note")}</p>
        </form>
      ) : (
        <form
          className="modulo-collega"
          onSubmit={(e) => {
            e.preventDefault();
            apriLink();
          }}
        >
          <label>
            {t("folders.add.mega.linkLabel")}
            <input
              type="url"
              className="campo field-input"
              value={link}
              placeholder="https://mega.nz/folder/…#…"
              spellCheck={false}
              disabled={inAttesa}
              aria-invalid={linkStorto || undefined}
              aria-describedby={idLink}
              onChange={(e) => {
                setLink(e.target.value);
                setLinkStorto(false);
              }}
            />
          </label>
          <p id={idLink} className={linkStorto ? "difetto-campo" : "nota-collega"}>
            {linkStorto ? t("errors.cloud.linkInvalid") : t("folders.add.mega.linkHint")}
          </p>
          {guasto !== null && (
            <div className="errore" role="alert">
              {testoErrore(guasto)}
            </div>
          )}
          <div className="tasti-collega">
            <button
              type="submit"
              className="bottone primario btn-accent"
              disabled={inAttesa || link.trim() === ""}
            >
              <Icona nome="i-nuvola" dim={15} />
              {inAttesa ? t("folders.add.trying") : t("folders.add.mega.open")}
            </button>
          </div>
        </form>
      )}
      <p className="solo-lettori" role="status">
        {inAttesa ? t("folders.add.trying") : ""}
      </p>
    </div>
  );
}

/** Il pannello che collega un servizio: il browser, o il suo modulo. */
function Collega(proprieta: Collegare) {
  if (proprieta.linguetta.scheda === "webdav") return <PerWebdav {...proprieta} />;
  if (proprieta.linguetta.scheda === "mega") return <PerMega {...proprieta} />;
  return <PerBrowser {...proprieta} />;
}

// ── lo sfoglio ──────────────────────────────────────────────────────────────

/** Il conto con cui si sfoglia, e il modo di cambiarlo o di aggiungerne un altro. */
function BarraConto({
  conti,
  conto,
  onScegli,
  onUnAltro,
}: {
  conti: readonly ContoNuvola[];
  conto: ContoNuvola;
  onScegli: (conto: string) => void;
  onUnAltro: () => void;
}) {
  return (
    <div className="conto-scelto">
      <Icona nome={iconaServizio(conto.servizio)} dim={16} />
      {conti.length > 1 ? (
        <select
          className="scegli-conto"
          aria-label={t("folders.add.account")}
          value={conto.conto}
          onChange={(e) => onScegli(e.target.value)}
        >
          {conti.map((c) => (
            <option key={c.conto} value={c.conto}>
              {sfogliabile(c) ? c.nome : t("folders.add.account.reconnect", { nome: c.nome })}
            </option>
          ))}
        </select>
      ) : (
        <span className="nome-conto">
          <span className="solo-lettori">{t("folders.add.account")}: </span>
          {conto.nome}
        </span>
      )}
      <button type="button" className="bottone minuto btn-ghost" onClick={onUnAltro}>
        <Icona nome="i-plus" dim={13} />
        {t("folders.add.otherAccount")}
      </button>
    </div>
  );
}

/**
 * Le cartelle di un conto: le briciole, l'elenco, e la tastiera.
 *
 * L'elenco è una fila di bottoni con **una** fermata di tabulazione: le frecce
 * si muovono dentro, Invio (o → ) entra, Backspace (o ←) risale. Senza, un drive
 * con duecento cartelle in cima costerebbe duecento Tab per arrivare a «Usa
 * questa cartella».
 */
function Sfoglia({
  conto,
  briciole,
  onBriciole,
  elenchi,
  onRiprova,
}: {
  conto: string;
  briciole: Briciole;
  onBriciole: (dopo: Briciole) => void;
  elenchi: Elenchi;
  onRiprova: (chiave: string) => void;
}) {
  const chiave = chiaveElenco(conto, dentro(briciole));
  const ora = vista(elenchi, chiave);
  const cartelle = ora.tipo === "elenco" ? ora.cartelle : [];
  const [attivo, setAttivo] = useState(0);
  const area = useRef<HTMLDivElement>(null);
  const bottoni = useRef<(HTMLButtonElement | null)[]>([]);
  /**
   * Dove deve andare il fuoco quando l'elenco nuovo c'è: la cartella da cui si
   * è risaliti, la prima (`0`), o da nessuna parte (`null`).
   *
   * Si parte da `0`: subito dopo un collegamento il tasto «Collega» è sparito e
   * il fuoco è caduto, e la prima cartella è il posto giusto dove ritrovarlo.
   * Aprendo la finestrella invece il fuoco sta sulle linguette, e la regola di
   * sotto — si sposta solo se è libero — lo lascia lì.
   */
  const fuocoVerso = useRef<string | 0 | null>(0);
  const qui = nomeDiQui(briciole);

  const vai = (dopo: Briciole) => {
    if (dopo === briciole) return;
    fuocoVerso.current = daDoveSiRisale(briciole, dopo) ?? 0;
    setAttivo(0);
    onBriciole(dopo);
  };

  /*
   * Il fuoco dopo un passo. In un effetto di disposizione e non passivo: il
   * tasto premuto — una briciola, una cartella — è appena sparito, il fuoco è
   * caduto, e la finestrella lo riporterebbe sulla sua radice prima di un
   * effetto passivo. Si sposta solo se è rimasto qui dentro o è caduto:
   * chi nel frattempo è andato altrove con Tab ci resta.
   */
  useLayoutEffect(() => {
    const voluto = fuocoVerso.current;
    if (voluto === null) return;
    const adesso = document.activeElement;
    const libero =
      adesso === null ||
      adesso === document.body ||
      (area.current !== null &&
        (area.current.contains(adesso) || adesso === area.current.closest("[role='dialog']")));
    if (ora.tipo === "elenco") {
      fuocoVerso.current = null;
      const trovato = typeof voluto === "string" ? ora.cartelle.findIndex((c) => c.id === voluto) : 0;
      const indice = Math.max(0, trovato);
      setAttivo(indice);
      if (libero) bottoni.current[indice]?.focus();
      return;
    }
    // Nessun elenco ancora (o nessuno affatto): il fuoco aspetta sull'area,
    // e la cartella lo prende quando arriva.
    if (libero) area.current?.focus({ preventScroll: true });
    if (ora.tipo !== "attesa") fuocoVerso.current = null;
  }, [chiave, ora]);

  const muovi = (indice: number) => {
    const quante = cartelle.length;
    if (quante === 0) return;
    const dove = Math.min(quante - 1, Math.max(0, indice));
    setAttivo(dove);
    bottoni.current[dove]?.focus();
  };

  const suTastoCartella = (e: TastoReact<HTMLButtonElement>, indice: number) => {
    const cartella = cartelle[indice];
    let fatto = true;
    if (e.key === "ArrowDown") muovi(indice + 1);
    else if (e.key === "ArrowUp") muovi(indice - 1);
    else if (e.key === "Home") muovi(0);
    else if (e.key === "End") muovi(cartelle.length - 1);
    else if (e.key === "ArrowRight" && cartella !== undefined) vai(entra(briciole, cartella));
    else if (e.key === "ArrowLeft") vai(risali(briciole));
    else fatto = false;
    if (fatto) {
      // Fermato, non solo impedito: le frecce sono anche scorciatoie del
      // lettore appese a `window`. È la regola di `fuoco.ts`.
      e.preventDefault();
      e.stopPropagation();
    }
  };

  const annuncio =
    ora.tipo === "attesa"
      ? t("folders.add.loadingFolders")
      : ora.tipo === "vuota"
        ? t("folders.add.empty")
        : ora.tipo === "elenco"
          ? t("folders.add.count", { n: ora.cartelle.length, nome: qui })
          : "";

  return (
    <div
      ref={area}
      className="sfoglia-nuvola"
      tabIndex={-1}
      onKeyDown={(e) => {
        // Backspace risale da qualunque punto dello sfoglio — briciole,
        // elenco, l'area stessa — ma non da un campo, dove cancella.
        if (e.key !== "Backspace" || e.target instanceof HTMLInputElement) return;
        e.preventDefault();
        vai(risali(briciole));
      }}
    >
      <nav className="briciole-nuvola" aria-label={t("folders.add.path")}>
        <ol>
          {briciole.map((passo, indice) => (
            <li key={`${indice}:${passo.id ?? ""}`}>
              {indice === briciole.length - 1 ? (
                <span className="briciola" aria-current="location">
                  {passo.nome}
                </span>
              ) : (
                <button
                  type="button"
                  className="briciola"
                  onClick={() => vai(tornaA(briciole, indice))}
                >
                  {passo.nome}
                </button>
              )}
            </li>
          ))}
        </ol>
      </nav>

      {ora.tipo === "attesa" && <p className="attesa-sfoglia">{t("folders.add.loadingFolders")}</p>}
      {ora.tipo === "guasto" && (
        <div className="errore" role="alert">
          <span>{testoErrore(ora.guasto)}</span>
          <button type="button" className="bottone minuto" onClick={() => onRiprova(chiave)}>
            {t("common.retry")}
          </button>
        </div>
      )}
      {ora.tipo === "vuota" && (
        <p className="vuota-sfoglia">
          {t(inVirtuale(briciole) ? "folders.add.sharedEmpty" : "folders.add.empty")}
        </p>
      )}
      {ora.tipo === "elenco" && (
        <ul className="cartelle-remote" aria-label={t("folders.add.subfolders", { nome: qui })}>
          {ora.cartelle.map((cartella, indice) => (
            <li key={cartella.id}>
              <button
                ref={(nodo) => {
                  bottoni.current[indice] = nodo;
                }}
                type="button"
                className="cartella-remota"
                tabIndex={indice === Math.min(attivo, ora.cartelle.length - 1) ? 0 : -1}
                onFocus={() => setAttivo(indice)}
                onClick={() => vai(entra(briciole, cartella))}
                onKeyDown={(e) => suTastoCartella(e, indice)}
              >
                <Icona nome="i-folder" dim={15} />
                <span className="nome">{cartella.nome}</span>
                <Icona nome="i-chev-r" dim={14} />
              </button>
            </li>
          ))}
        </ul>
      )}
      <p className="solo-lettori" role="status">
        {annuncio}
      </p>
    </div>
  );
}

// ── la finestrella ──────────────────────────────────────────────────────────

/** Vedi il commento in testa. */
export function AggiungiCartella({
  onQuestoPC,
  onUsa,
  onChiudi,
}: {
  /**
   * Il dialogo di sistema. `true` se si è scelta una cartella: allora la
   * finestrella ha finito; chi ha annullato il dialogo resta qui.
   */
  onQuestoPC: () => Promise<boolean>;
  /**
   * Aggiunge la cartella del cloud scelta. Lancia se non ci riesce: il guasto
   * resta qui, con la scelta ancora in mano.
   */
  onUsa: (conto: string, id: string, nomi: string[]) => Promise<void>;
  onChiudi: () => void;
}) {
  const { servizi, conti, guasto, ricarica, accogli } = useContiNuvolaDellaFinestra();
  const [sorgente, setSorgente] = useState<Sorgente>(ultimaSorgente);
  /** Il conto scelto in ogni linguetta: tornandoci, si ritrova quello. */
  const [scelti, setScelti] = useState<Partial<Record<Scheda, string>>>({});
  /** La linguetta su cui si sta collegando un altro account. */
  const [unAltro, setUnAltro] = useState<Scheda | null>(null);
  /** Le briciole di ogni conto: tornando a un conto, si è dove si era. */
  const [briciole, setBriciole] = useState<ReadonlyMap<string, Briciole>>(() => new Map());
  const [elenchi, setElenchi] = useState<Elenchi>(NESSUN_ELENCO);
  const [cercaPC, setCercaPC] = useState(false);
  const [aggiungendo, setAggiungendo] = useState(false);
  const [guastoUsa, setGuastoUsa] = useState<unknown>(null);
  /** La domanda prima di un gesto su un'app: vedi `Collegare.onChiedi`. */
  const [domanda, setDomanda] = useState<Domanda | null>(null);
  const vivo = useRef(true);
  const idPerche = useId();

  useEffect(() => {
    vivo.current = true;
    return () => {
      vivo.current = false;
    };
  }, []);

  // La lettura dell'avvio è fallita, o non è ancora tornata: si chiede di
  // nuovo aprendo. `ricarica` non ne apre una seconda se ce n'è già una. Solo
  // all'apertura, e per questo senza dipendenze: dopo, «Riprova» è di chi
  // guarda, e un guasto non deve far ripartire la domanda da solo.
  const servizi0 = useRef(servizi);
  useEffect(() => {
    if (servizi0.current === null) ricarica();
  }, [ricarica]);

  const linguette = servizi === null ? [] : linguetteDi(servizi);
  const linguetta = sorgente === "pc" ? null : (linguette.find((l) => l.scheda === sorgente) ?? null);
  const contiQui = sorgente === "pc" || conti === null ? [] : contiDella(conti, sorgente);
  const conto = sorgente === "pc" ? null : contoDaMostrare(contiQui, scelti[sorgente] ?? null);
  // Aperta anche da configurare: il pannello che collega mostra prima l'app.
  const aperta = linguetta !== null && apribile(linguetta);
  const collegaAltro = sorgente !== "pc" && unAltro === sorgente;
  const sfogliato =
    aperta && conto !== null && sfogliabile(conto) && !collegaAltro ? conto : null;
  const bricioleQui =
    sfogliato === null || linguetta === null
      ? null
      : (briciole.get(sfogliato.conto) ?? inCima(linguetta.offerto.nome));
  const chiaveQui =
    sfogliato === null || bricioleQui === null
      ? null
      : chiaveElenco(sfogliato.conto, dentro(bricioleQui));
  const cartellaQui = bricioleQui === null ? null : dentro(bricioleQui);
  const contoQui = sfogliato?.conto ?? null;

  /*
   * Chiede le cartelle di dove si è, se non le ha già. La risposta si scrive
   * al suo posto nell'archivio, mai nella vista: vedi `selettoreNuvola.ts`.
   */
  useEffect(() => {
    if (contoQui === null || chiaveQui === null) return;
    if (!daChiedere(elenchi, chiaveQui)) return;
    const k = chiaveQui;
    setElenchi((prima) => chiesto(prima, k));
    ipc.nuvolaCartelle(contoQui, cartellaQui).then(
      (trovate) => {
        if (vivo.current) setElenchi((prima) => arrivato(prima, k, trovate));
      },
      (e: unknown) => {
        if (vivo.current) setElenchi((prima) => fallito(prima, k, e));
      },
    );
  }, [contoQui, cartellaQui, chiaveQui, elenchi]);

  const scegliSorgente = (dove: Sorgente) => {
    ultimaSorgente = dove;
    setSorgente(dove);
    setGuastoUsa(null);
  };

  const scegliPC = () => {
    setCercaPC(true);
    onQuestoPC().then(
      (scelta) => {
        if (!vivo.current) return;
        setCercaPC(false);
        if (scelta) onChiudi();
      },
      () => {
        if (vivo.current) setCercaPC(false);
      },
    );
  };

  const collegato = (nuovo: ContoNuvola) => {
    accogli(nuovo);
    setScelti((prima) => ({ ...prima, [schedaDi(nuovo.servizio)]: nuovo.conto }));
    setUnAltro(null);
  };

  const puoUsare = bricioleQui !== null && usabile(bricioleQui);
  const usa = () => {
    // `aria-disabled` e non `disabled` (il tasto resta raggiungibile e dice
    // perché): il clic arriva lo stesso, e dentro «Shared with me» mandava la
    // cartella virtuale, che il servizio non ha.
    if (sfogliato === null || bricioleQui === null || aggiungendo || !puoUsare) return;
    const id = dentro(bricioleQui);
    if (id === null) return;
    setAggiungendo(true);
    setGuastoUsa(null);
    onUsa(sfogliato.conto, id, nomiDalla(bricioleQui)).then(
      () => {
        if (vivo.current) onChiudi();
      },
      (e: unknown) => {
        if (!vivo.current) return;
        setGuastoUsa(e);
        setAggiungendo(false);
      },
    );
  };

  const voci: Voce<Sorgente>[] = [
    { chiave: "pc", etichetta: t("folders.add.pc") },
    ...linguette.map((l) => ({
      chiave: l.scheda,
      etichetta: l.offerto.nome,
      spenta: ragioneSpenta(statoLinguetta(l), l.offerto.nome),
    })),
  ];

  /** Il corpo di una linguetta di servizio. */
  const corpoServizio = () => {
    if (linguetta === null) {
      // I servizi non sono arrivati: il guasto, se c'è, lo dice la riga sopra.
      return guasto === null ? <p className="attesa-sfoglia">{t("folders.add.loading")}</p> : null;
    }
    if (!aperta) {
      return (
        <p className="nota-collega">
          {ragioneSpenta(statoLinguetta(linguetta), linguetta.offerto.nome)}
        </p>
      );
    }
    if (conti === null) {
      return guasto === null ? <p className="attesa-sfoglia">{t("folders.add.loading")}</p> : null;
    }
    if (collegaAltro || conto === null) {
      return (
        <>
          {collegaAltro && contiQui.length > 0 && (
            <button
              type="button"
              className="bottone minuto btn-ghost torna-cartelle"
              onClick={() => setUnAltro(null)}
            >
              <Icona nome="i-chev-l" dim={13} />
              {t("folders.add.backToFolders")}
            </button>
          )}
          <Collega
            key={`${linguetta.scheda}:nuovo`}
            linguetta={linguetta}
            daRicollegare={null}
            onCollegato={collegato}
            onChiedi={setDomanda}
          />
        </>
      );
    }
    return (
      <>
        <BarraConto
          conti={contiQui}
          conto={conto}
          onScegli={(scelto) => setScelti((prima) => ({ ...prima, [linguetta.scheda]: scelto }))}
          onUnAltro={() => setUnAltro(linguetta.scheda)}
        />
        {sfogliato !== null && bricioleQui !== null ? (
          <Sfoglia
            key={sfogliato.conto}
            conto={sfogliato.conto}
            briciole={bricioleQui}
            onBriciole={(dopo) =>
              setBriciole((prima) => new Map(prima).set(sfogliato.conto, dopo))
            }
            elenchi={elenchi}
            onRiprova={(k) => setElenchi((prima) => riprova(prima, k))}
          />
        ) : (
          <Collega
            key={`${linguetta.scheda}:${conto.conto}`}
            linguetta={linguetta}
            daRicollegare={conto}
            onCollegato={collegato}
            onChiedi={setDomanda}
          />
        )}
      </>
    );
  };

  return (
    <>
      <Dialogo
        titolo={t("folders.add.title")}
        classe="finestrella media aggiungi-cartella glass-modal"
        onChiudi={onChiudi}
        chiudibile={!aggiungendo}
        occupato={aggiungendo}
      >
        <Segmentato
          voci={voci}
          scelta={sorgente}
          onScegli={scegliSorgente}
          etichetta={t("folders.add.sources")}
          classe="sorgenti-cartella"
        />
        {servizi === null && guasto !== null && (
          <div className="errore" role="alert">
            <span>{testoErrore(guasto)}</span>
            <button type="button" className="bottone minuto" onClick={ricarica}>
              {t("common.retry")}
            </button>
          </div>
        )}

        <div className="corpo-aggiungi">
          {sorgente === "pc" ? (
            <div className="collega-nuvola">
              <TestaServizioPC />
              <p>{t("folders.add.pc.body")}</p>
              <div className="tasti-collega">
                <button
                  type="button"
                  className="bottone primario btn-accent"
                  disabled={cercaPC}
                  onClick={scegliPC}
                >
                  <Icona nome="i-folder" dim={15} />
                  {cercaPC ? t("folders.add.pc.choosing") : t("folders.add.pc.choose")}
                </button>
              </div>
              <p className="nota-collega">{t("folders.add.pc.drop")}</p>
            </div>
          ) : (
            corpoServizio()
          )}
        </div>

        {guastoUsa !== null && (
          <div className="errore" role="alert">
            {testoErrore(guastoUsa)}
          </div>
        )}
        {sfogliato !== null && !puoUsare && (
          <p id={idPerche} className="nota-collega">
            {t(
              bricioleQui !== null && inVirtuale(bricioleQui)
                ? "folders.add.pickShared"
                : "folders.add.pickInside",
            )}
          </p>
        )}
        <div className="tasti-finestrella">
          <button type="button" className="bottone" disabled={aggiungendo} onClick={onChiudi}>
            {t("common.cancel")}
          </button>
          {sfogliato !== null && (
            <button
              type="button"
              className="bottone primario btn-accent"
              /* Spento ma raggiungibile in cima al drive e in una cartella
                 virtuale: la ragione è la riga qui sopra, e la legge chi ci
                 arriva col tabulatore. */
              aria-disabled={!puoUsare || undefined}
              aria-describedby={!puoUsare ? idPerche : undefined}
              disabled={aggiungendo}
              onClick={usa}
            >
              {aggiungendo ? t("folders.add.adding") : t("folders.add.use")}
            </button>
          )}
        </div>
        <p className="solo-lettori" role="status">
          {aggiungendo ? t("folders.add.adding") : cercaPC ? t("folders.add.pc.choosing") : ""}
        </p>
      </Dialogo>
      {/* Accanto alla finestrella e non dentro: vedi «Le conferme» in
          `AppServizio.tsx`. Si apre sopra, e la finestrella sotto diventa inerte. */}
      {domanda !== null && (
        <ConfermaGesto domanda={domanda} onChiudi={() => setDomanda(null)} />
      )}
    </>
  );
}

/** La testa di «Questo PC»: come quella di un servizio, con la cartella. */
function TestaServizioPC() {
  return (
    <div className="testa-collega">
      <Icona nome="i-folder" dim={18} />
      <h3>{t("folders.add.pc")}</h3>
    </div>
  );
}

/**
 * «Ricollega» di un account delle Impostazioni: lo stesso pannello che
 * collega, da solo.
 *
 * Lo stesso e non un tasto che apre il browser dalla riga, perché MEGA e
 * WebDAV hanno un modulo — e la verifica in due passaggi — che la riga non sa
 * disegnare: due strade per la stessa cosa sarebbero due cose da tenere
 * d'accordo. È anche il posto in cui un conto spento per colpa dell'app
 * mostra prima l'app da sistemare o da scegliere (vedi `PerBrowser`).
 *
 * Il conto si segue mentre la finestrella è aperta: scrivere il segreto che
 * mancava riapre i conti da soli (`nuvola_app_salva`), e chi l'ha appena
 * scritto non deve trovarsi davanti un «Collega» per un conto che è già
 * tornato.
 */
export function RicollegaConto({
  conto,
  onChiudi,
  onFatto,
}: {
  conto: ContoNuvola;
  onChiudi: () => void;
  /** Il conto è tornato: chi ha aperto lo dice. */
  onFatto: (conto: ContoNuvola) => void;
}) {
  const { servizi, conti, guasto, ricarica, accogli } = useContiNuvolaDellaFinestra();
  const [domanda, setDomanda] = useState<Domanda | null>(null);
  /** Il conto com'è adesso: quello aperto, finché l'elenco non dice altro. */
  const adesso = conti?.find((c) => c.conto === conto.conto) ?? conto;
  // «Collegato» o «elenco» e non soltanto «non da ricollegare»: un conto che
  // si riapre e non risponde (`errore`) non è tornato, e dirlo lo sarebbe.
  const tornato =
    conto.stato === "daRicollegare" &&
    (adesso.stato === "collegato" || adesso.stato === "elenco");
  // Le funzioni di chi ha aperto in un riferimento: l'effetto deve partire
  // quando il conto torna, non quando cambia chi lo ha aperto.
  const avvisa = useRef({ onFatto, onChiudi });
  avvisa.current = { onFatto, onChiudi };
  /** Già detto e già chiuso: dal consenso appena dato, o da qui. Una volta sola. */
  const finito = useRef(false);
  useEffect(() => {
    if (!tornato || finito.current) return;
    finito.current = true;
    avvisa.current.onFatto(adesso);
    avvisa.current.onChiudi();
    // Solo `tornato`, e apposta: `adesso` cambia a ogni voce elencata, e
    // conta soltanto il momento in cui il conto torna.
  }, [tornato]);
  const linguetta =
    linguetteDi(servizi ?? []).find((l) => l.scheda === schedaDi(conto.servizio)) ?? null;
  return (
    <>
      <Dialogo
        titolo={t("folders.reconnect.title", { servizio: NOMI_SERVIZI[conto.servizio] })}
        classe="finestrella media glass-modal"
        onChiudi={onChiudi}
      >
        {linguetta !== null ? (
          <Collega
            linguetta={linguetta}
            daRicollegare={adesso}
            onCollegato={(collegato) => {
              finito.current = true;
              accogli(collegato);
              onFatto(collegato);
              onChiudi();
            }}
            onChiedi={setDomanda}
          />
        ) : guasto !== null ? (
          <div className="errore" role="alert">
            <span>{testoErrore(guasto)}</span>
            <button type="button" className="bottone minuto" onClick={ricarica}>
              {t("common.retry")}
            </button>
          </div>
        ) : (
          <p className="attesa-sfoglia">{t("folders.add.loading")}</p>
        )}
        <div className="tasti-finestrella">
          <button type="button" className="bottone" onClick={onChiudi}>
            {t("common.cancel")}
          </button>
        </div>
      </Dialogo>
      {domanda !== null && (
        <ConfermaGesto domanda={domanda} onChiudi={() => setDomanda(null)} />
      )}
    </>
  );
}
