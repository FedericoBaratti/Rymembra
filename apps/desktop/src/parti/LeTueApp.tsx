/**
 * Le app dell'utente nelle Impostazioni: la scheda «Le tue app», e la scelta
 * dell'app Google del backup su Drive.
 *
 * # Perché una scheda sua, accanto agli account
 *
 * Un'app non è un account: è il permesso, registrato dall'utente presso
 * Microsoft, Google o Dropbox, con cui Rymembra chiede a quegli account di
 * entrare. Una sola può servirne molti — due account di Google Drive e il
 * backup — e cambiarla li tocca tutti. Mischiate nelle righe dei conti, le app
 * sembrerebbero una proprietà di ciascun conto, e il giorno in cui se ne
 * cambia l'ID non si capirebbe perché si spengono anche gli altri. Qui ogni
 * riga dice chi la usa, e il gesto che li tocca lo chiede prima, nominandoli.
 *
 * # Perché si legge da sé
 *
 * Le app stanno in `FornitoreContiNuvola` (vedi `contiNuvola.ts`), che ascolta
 * `nuvola:app` per tutta la finestra: come `SchedaContiNuvola`. Per lo stesso
 * motivo questi componenti si montano solo nelle sezioni che li mostrano —
 * la tela dello Studio disegna le Impostazioni senza quel fornitore.
 *
 * # Il fuoco
 *
 * Il modulo si apre al posto della riga (o del tasto) che l'ha chiesto, e
 * prende il fuoco sul titolo; chiuso, il fuoco torna a quel tasto — o, per
 * un'app appena registrata, al suo «Modifica», che è il posto in cui adesso
 * sta. Togliendo un'app, al tasto che ne aggiunge una della stessa piattaforma:
 * la riga su cui si era non c'è più.
 */
import { useId, useLayoutEffect, useRef, useState } from "react";

import { appDellaPiattaforma, chiLaUsa } from "../appNuvola";
import type { Domanda } from "../ConfermaGesto";
import { useContiNuvolaDellaFinestra } from "../contiNuvola";
import {
  ipc,
  testoErrore,
  type AppUtente,
  type ChiavePiattaforma,
  type StatoNuvola,
} from "../ipc";
import { t, type Chiave } from "../lingue";
import { Annuncio } from "./Annuncio";
import { AppServizio, fraseGuastoApp } from "./AppServizio";
import { Icona } from "./Icone";

/** A che cosa serve un'app di quella piattaforma, accanto al suo nome. */
const PER: Readonly<Record<ChiavePiattaforma, Chiave>> = {
  microsoft: "settings.cloudApps.for.microsoft",
  google: "settings.cloudApps.for.google",
  dropbox: "settings.cloudApps.for.dropbox",
};

/** Chi usa un'app, in una frase: il backup a parole, i conti per nome. */
function fraseUso(app: AppUtente): string {
  const chi = app.usataDa.map((uso) =>
    uso.conto === null ? t("settings.cloudApps.usedBy.backup") : uso.nome,
  );
  return chi.length === 0
    ? t("settings.cloudApps.unused")
    : t("settings.cloudApps.usedBy", { chi: chi.join(", ") });
}

/** Il modulo aperto: per un'app che c'è (`app`), o per una nuova di quella piattaforma. */
interface Aperto {
  piattaforma: ChiavePiattaforma;
  app: string | null;
}

/**
 * Il cuore della scheda «Le tue app»: per ogni piattaforma le sue app, coi
 * gesti, e il modo di aggiungerne. Vedi il commento in testa.
 */
export function LeTueApp({ onChiedi }: { onChiedi: (domanda: Domanda) => void }) {
  const { app: registro, guasto, ricarica, accogliApp } = useContiNuvolaDellaFinestra();
  const [aperto, setAperto] = useState<Aperto | null>(null);
  /** L'app che si sta togliendo: i suoi tasti si spengono finché il nucleo non risponde. */
  const [togliendo, setTogliendo] = useState<string | null>(null);
  const [guastoTogli, setGuastoTogli] = useState<unknown>(null);
  /** Quel che si dice a chi ascolta lo schermo dopo un gesto andato bene. */
  const [detto, setDetto] = useState<string | null>(null);
  const radice = useRef<HTMLDivElement>(null);
  const base = useId();

  /**
   * Dove deve tornare il fuoco: il `data-fuoco` di un tasto. Lo si cerca dopo
   * ogni disegno finché non c'è, perché il tasto compare nello stesso disegno
   * in cui il modulo se ne va — o in quello dopo, quando arrivano le app nuove.
   */
  const fuocoA = useRef<string | null>(null);
  useLayoutEffect(() => {
    const dove = fuocoA.current;
    if (dove === null) return;
    const nodo = radice.current?.querySelector<HTMLElement>(`[data-fuoco="${dove}"]`);
    if (nodo === null || nodo === undefined) return;
    fuocoA.current = null;
    nodo.focus();
  });

  /*
   * Il modulo aperto adesso, per chi finisce dopo: un salvataggio che torna
   * mentre se n'è aperto un altro ha in mano il disegno di quando è partito.
   */
  const apertoOra = useRef(aperto);
  apertoOra.current = aperto;

  /**
   * Chiude il modulo `di` — un'app, o `null` per quello nuovo della
   * `piattaforma` — se è ancora quello aperto. Mentre un'app si salvava se ne
   * poteva aprire un'altra, e il salvataggio, finendo, chiudeva quella e
   * buttava via quel che ci si stava scrivendo.
   */
  const chiudi = (fuoco: string, piattaforma: ChiavePiattaforma, di: string | null) => {
    const ora = apertoOra.current;
    if (ora === null || ora.piattaforma !== piattaforma || ora.app !== di) return;
    fuocoA.current = fuoco;
    setAperto(null);
  };

  const togli = (app: AppUtente) => {
    const chi = chiLaUsa(app);
    const pezzi = [t("settings.cloudApps.remove.confirm.body")];
    if (chi.conti.length > 0) {
      pezzi.push(t("settings.cloudApps.remove.confirm.accounts", { conti: chi.conti.join(", ") }));
    }
    if (chi.backup) pezzi.push(t("settings.cloudApps.remove.confirm.backup"));
    onChiedi({
      titolo: t("settings.cloudApps.remove.confirm.title", { nome: app.nome }),
      descrizione: pezzi.join(" "),
      conferma: t("settings.cloudApps.remove.confirm.ok"),
      fai: () => {
        setTogliendo(app.id);
        setGuastoTogli(null);
        ipc
          .nuvolaAppTogli(app.id)
          .then((dopo) => {
            fuocoA.current = `nuova:${app.piattaforma}`;
            accogliApp(dopo);
            setDetto(t("settings.cloudApps.removed", { nome: app.nome }));
          })
          .catch((e: unknown) => setGuastoTogli(e))
          .finally(() => setTogliendo(null));
      },
    });
  };

  if (registro === null) {
    // Le app arrivano insieme ai conti: il guasto, se c'è, è lo stesso.
    return guasto === null ? null : (
      <div className="errore">
        <span>{testoErrore(guasto)}</span>
        <button type="button" className="bottone minuto" onClick={ricarica}>
          {t("common.retry")}
        </button>
      </div>
    );
  }

  return (
    <div ref={radice} className="le-tue-app">
      <p className="nota">{t("settings.cloudApps.body")}</p>
      {guastoTogli !== null && (
        <div className="errore" role="alert">
          {fraseGuastoApp(guastoTogli)}
        </div>
      )}
      {registro.piattaforme.map((piattaforma) => {
        const sue = appDellaPiattaforma(registro.app, piattaforma.chiave);
        const idTitolo = `${base}-${piattaforma.chiave}`;
        const nuovaAperta = aperto?.piattaforma === piattaforma.chiave && aperto.app === null;
        return (
          <section
            key={piattaforma.chiave}
            className="piattaforma-app"
            aria-labelledby={idTitolo}
          >
            <h3 className="titoletto" id={idTitolo}>
              {piattaforma.nome}{" "}
              <span className="per-app">{t(PER[piattaforma.chiave])}</span>
            </h3>
            {sue.length === 0 && !nuovaAperta && (
              <p className="niente empty-state">{t("settings.cloudApps.none")}</p>
            )}
            {sue.length > 0 && (
              <ul className="elenco-app">
                {sue.map((app) => {
                  const idNome = `${base}-${app.id}`;
                  if (aperto?.app === app.id) {
                    return (
                      <li key={app.id} className="app-aperta">
                        <AppServizio
                          piattaforma={piattaforma}
                          app={app}
                          tutte={registro.app}
                          livello={4}
                          fuoco="titolo"
                          onChiedi={onChiedi}
                          onSalvata={(dopo, salvata) => {
                            accogliApp(dopo);
                            setDetto(t("cloudApp.saved", { nome: salvata?.nome ?? app.nome }));
                            chiudi(`modifica:${app.id}`, piattaforma.chiave, app.id);
                          }}
                          onAnnulla={() => chiudi(`modifica:${app.id}`, piattaforma.chiave, app.id)}
                        />
                      </li>
                    );
                  }
                  return (
                    <li key={app.id} className="app-nuvola" data-manca={!app.segretoSalvato || undefined}>
                      <div className="che-app">
                        <div className="nome-app" id={idNome}>
                          {app.nome}
                          {/* «Predefinita» solo dove c'è una scelta: con un'app
                              sola, dirlo non distingue niente. */}
                          {sue.length > 1 && app.predefinita && (
                            <span className="pastiglia-app">{t("settings.cloudApps.default")}</span>
                          )}
                        </div>
                        <div className="id-app">
                          <span className="solo-lettori">{t("settings.cloudApps.idLabel")}: </span>
                          <code>{app.clientId}</code>
                        </div>
                        <div className="uso-app">{fraseUso(app)}</div>
                        {!app.segretoSalvato && (
                          <div className="manca-app">
                            <Icona nome="i-alert" dim={13} />
                            <span>{t("settings.cloudApps.secretMissing")}</span>
                          </div>
                        )}
                      </div>
                      {/* I tasti dicono di chi sono con `aria-describedby`,
                          come quelli dei conti: due «Modifica» di fila,
                          sentiti senza vederli, sono due tasti uguali. */}
                      <div className="tasti-conto">
                        <button
                          type="button"
                          className={
                            app.segretoSalvato
                              ? "bottone minuto btn-ghost"
                              : "bottone minuto primario btn-accent"
                          }
                          data-fuoco={`modifica:${app.id}`}
                          aria-describedby={idNome}
                          disabled={togliendo !== null}
                          onClick={() => setAperto({ piattaforma: piattaforma.chiave, app: app.id })}
                        >
                          {t("settings.cloudApps.edit")}
                        </button>
                        <button
                          type="button"
                          className="bottone minuto btn-ghost"
                          aria-describedby={idNome}
                          disabled={togliendo !== null}
                          onClick={() => togli(app)}
                        >
                          {t("settings.cloudApps.remove")}
                        </button>
                      </div>
                    </li>
                  );
                })}
              </ul>
            )}
            {nuovaAperta ? (
              <AppServizio
                piattaforma={piattaforma}
                app={null}
                tutte={registro.app}
                livello={4}
                fuoco="titolo"
                onChiedi={onChiedi}
                onSalvata={(dopo, salvata) => {
                  accogliApp(dopo);
                  if (salvata !== null) setDetto(t("cloudApp.saved", { nome: salvata.nome }));
                  chiudi(
                    salvata === null ? `nuova:${piattaforma.chiave}` : `modifica:${salvata.id}`,
                    piattaforma.chiave,
                    null,
                  );
                }}
                onAnnulla={() => chiudi(`nuova:${piattaforma.chiave}`, piattaforma.chiave, null)}
              />
            ) : (
              <div className="azioni">
                <button
                  type="button"
                  className="bottone btn-ghost"
                  data-fuoco={`nuova:${piattaforma.chiave}`}
                  onClick={() => setAperto({ piattaforma: piattaforma.chiave, app: null })}
                >
                  <Icona nome="i-plus" dim={15} />
                  {sue.length === 0
                    ? t("settings.cloudApps.add", { piattaforma: piattaforma.nome })
                    : t("settings.cloudApps.addAnother", { piattaforma: piattaforma.nome })}
                </button>
              </div>
            )}
          </section>
        );
      })}
      <Annuncio testo={detto} />
    </div>
  );
}

/**
 * L'app Google del backup su Drive.
 *
 * # Tre casi
 *
 * - **Nessuna app Google**: il modulo che la registra, qui, con la frase che
 *   dice perché. Un'app Google nuova, col backup senza app, il backup la
 *   prende da sé (`nuvola_app_salva`): la si nomina subito come sua, senza
 *   aspettare lo stato del backup che lo conferma — fra le due cose ci
 *   sarebbe un istante con «Nessuna» scelta, per un'app che non può che
 *   essere quella.
 * - **App Google, ma il backup senza** (quella che usava è stata tolta): la
 *   scelta, con «Nessuna» davanti finché non se ne sceglie una.
 * - **Il backup ha la sua app**: la scelta, se ce n'è più d'una, o il suo nome.
 *   E se all'app manca il segreto, lo si dice con il modo di scriverlo.
 *
 * # Cambiare app
 *
 * La cartella privata del backup su Drive (`drive.appdata`) è dell'app che
 * l'ha scritta: con un'altra i backup di prima possono non vedersi, e la
 * sincronia fra PC non ritrova i suoi dati. Lo si chiede prima, sempre che il
 * backup avesse già un'app; e il controllo resta su quella di adesso finché la
 * risposta non è sì.
 */
export function AppDelBackup({
  nuvola,
  onScegli,
  onChiedi,
  onGestisci,
}: {
  nuvola: StatoNuvola | null;
  /** Sceglie l'app del backup: `nuvolaBackupApp`. */
  onScegli: (scelta: string | null) => void;
  onChiedi: (domanda: Domanda) => void;
  /** Porta alla scheda «Le tue app», in «Cartelle e scansione». */
  onGestisci: () => void;
}) {
  const { app: registro, accogliApp } = useContiNuvolaDellaFinestra();
  /** Il modulo dell'app del backup è aperto, per scriverle il segreto. */
  const [modifica, setModifica] = useState(false);
  const tastoSegreto = useRef<HTMLButtonElement>(null);
  const tastoGestisci = useRef<HTMLButtonElement>(null);
  /**
   * Dove va il fuoco quando un modulo se ne va: al tasto che l'aveva aperto
   * dopo «Annulla»; dopo un salvataggio — il modulo sparisce con l'avviso che
   * lo conteneva — alla riga dell'app, che è quel che è cambiato.
   */
  const dopoIlModulo = useRef<"segreto" | "gestisci" | null>(null);
  /** L'app appena registrata qui, che il backup prende da sé: vedi il commento in testa. */
  const [adottata, setAdottata] = useState<string | null>(null);
  const idScelta = useId();
  useLayoutEffect(() => {
    const dove = dopoIlModulo.current;
    if (dove === null) return;
    const nodo = (dove === "segreto" ? tastoSegreto : tastoGestisci).current;
    if (nodo === null) return;
    dopoIlModulo.current = null;
    nodo.focus();
  });

  if (registro === null || nuvola === null) return null;
  const google = registro.piattaforme.find((p) => p.chiave === "google");
  if (google === undefined) return null;
  const sue = appDellaPiattaforma(registro.app, "google");
  const legata = sue.find((a) => a.id === (nuvola.app ?? adottata)) ?? null;

  if (sue.length === 0) {
    return (
      <div className="app-del-backup">
        <p className="nota">{t("settings.cloud.noCreds")}</p>
        <AppServizio
          piattaforma={google}
          app={null}
          tutte={registro.app}
          onChiedi={onChiedi}
          onSalvata={(dopo, salvata) => {
            dopoIlModulo.current = "gestisci";
            setAdottata(salvata?.id ?? null);
            accogliApp(dopo);
          }}
        />
      </div>
    );
  }

  const scegli = (scelta: string | null) => {
    if (scelta === (legata?.id ?? null)) return;
    if (legata === null) {
      onScegli(scelta);
      return;
    }
    onChiedi({
      titolo: t("settings.cloud.app.confirm.title"),
      descrizione: t("settings.cloud.app.confirm.body"),
      conferma: t("settings.cloud.app.confirm.ok"),
      fai: () => onScegli(scelta),
    });
  };

  return (
    <div className="app-del-backup">
      {legata === null && <p className="nota">{t("settings.cloud.chooseApp")}</p>}
      <div className="azioni">
        {sue.length > 1 || legata === null ? (
          <>
            <label htmlFor={idScelta}>{t("settings.cloud.app")}</label>
            <select
              id={idScelta}
              className="scegli-conto"
              value={legata?.id ?? ""}
              disabled={nuvola.inCorso}
              onChange={(e) => scegli(e.target.value === "" ? null : e.target.value)}
            >
              {legata === null && <option value="">{t("settings.cloud.app.none")}</option>}
              {sue.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.nome}
                </option>
              ))}
            </select>
          </>
        ) : (
          <span className="oppure">
            {t("settings.cloud.app")}: <strong>{legata.nome}</strong>
          </span>
        )}
        <button
          ref={tastoGestisci}
          type="button"
          className="bottone minuto btn-ghost"
          onClick={onGestisci}
        >
          {t("settings.cloud.app.manage")}
        </button>
      </div>
      {legata !== null &&
        !legata.segretoSalvato &&
        (modifica ? (
          <AppServizio
            key={legata.id}
            piattaforma={google}
            app={legata}
            tutte={registro.app}
            fuoco="segreto"
            onChiedi={onChiedi}
            onSalvata={(dopo) => {
              dopoIlModulo.current = "gestisci";
              accogliApp(dopo);
              setModifica(false);
            }}
            onAnnulla={() => {
              dopoIlModulo.current = "segreto";
              setModifica(false);
            }}
          />
        ) : (
          <div className="avviso" data-livello="avviso">
            <Icona nome="i-alert" dim={15} />
            <div className="dentro">
              <p>{t("settings.cloud.app.secretMissing", { nome: legata.nome })}</p>
              <div className="azioni">
                <button
                  ref={tastoSegreto}
                  type="button"
                  className="bottone minuto"
                  onClick={() => setModifica(true)}
                >
                  {t("settings.cloud.app.writeSecret")}
                </button>
              </div>
            </div>
          </div>
        ))}
    </div>
  );
}
