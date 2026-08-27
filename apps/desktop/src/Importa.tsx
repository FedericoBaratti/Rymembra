/**
 * Le due importazioni: dal vecchio database e da un link.
 *
 * # Due tempi, e perché non uno
 *
 * `piano_importazione` non scrive niente: apre il vecchio database in sola
 * lettura e dice cosa porterebbe. Solo dopo, e su un secondo clic, `importa`
 * esegue. Il motivo non è la prudenza generica — è che il numero che conta è
 * quello dei brani **non ritrovati**, e chi importa deve poterlo leggere prima
 * di decidere, non scoprirlo dopo in un riepilogo.
 *
 * L'operazione è idempotente dal lato del nucleo (le chiavi sono uniche e la
 * fusione delle statistiche è commutativa), quindi una seconda importazione non
 * raddoppia niente. Resta comunque un'operazione da capire prima di fare.
 *
 * Quella da Spotify sta qui e non in un file suo perché è lo **stesso**
 * concetto con la stessa forma — anteprima, piano, conferma — e sono le stesse
 * due schermate. Ha un passo in più davanti, il link, e per il resto il piano
 * dice le stesse cose: quanti brani ci sono già e quali no.
 */
import { useEffect, useRef, useState } from "react";

import {
  eErroreIpc,
  ipc,
  testoErrore,
  type AnteprimaImport,
  type DiagnosticaImport,
  type EsitoImportazione,
  type EsitoImport,
} from "./ipc";
import { Avviso, AvvisoErrore } from "./parti/Avvisi";
import { nomeSorgente } from "./parti/Importazioni";
import { fiduciaDi, nomeFonte, PastigliaGradino, Tacche } from "./parti/Incertezza";
import { LetturaLink, useLetturaLink } from "./parti/LetturaLink";
import { t } from "./lingue";
import { Trans } from "./lingue/Trans";

/** Una riga del rapporto: etichetta, numero, e il perché quando serve. */
function Voce({
  etichetta,
  valore,
  nota,
}: {
  etichetta: string;
  valore: number;
  nota?: string | undefined;
}) {
  return (
    <div className="voce-rapporto">
      <span>{etichetta}</span>
      <span className="conteggio">{valore}</span>
      {nota && <small>{nota}</small>}
    </div>
  );
}

export function Importa({
  percorso,
  onChiudi,
  onImportato,
}: {
  percorso: string;
  onChiudi: () => void;
  onImportato: () => void;
}) {
  const [piano, setPiano] = useState<EsitoImportazione | null>(null);
  const [fatto, setFatto] = useState<EsitoImportazione | null>(null);
  const [errore, setErrore] = useState<string | null>(null);
  const [inCorso, setInCorso] = useState(true);

  // L'anteprima parte da sola: aprire questa schermata è già la richiesta di
  // vedere cosa c'è, e un secondo clic per ottenerlo sarebbe un passaggio a
  // vuoto. Il piano non scrive niente, quindi chiederlo due volte — come fa
  // StrictMode in sviluppo — non ha conseguenze.
  useEffect(() => {
    let annullato = false;
    ipc
      .pianoImportazione(percorso)
      .then((p) => {
        if (!annullato) setPiano(p);
      })
      .catch((e: unknown) => {
        if (!annullato) setErrore(testoErrore(e));
      })
      .finally(() => {
        if (!annullato) setInCorso(false);
      });
    return () => {
      annullato = true;
    };
  }, [percorso]);

  const esegui = async () => {
    setInCorso(true);
    setErrore(null);
    try {
      setFatto(await ipc.importa(percorso));
      onImportato();
    } catch (e) {
      setErrore(testoErrore(e));
    } finally {
      setInCorso(false);
    }
  };

  const rapporto = fatto ?? piano;

  return (
    <div className="velo scuro" onClick={onChiudi}>
      <div
        className="finestrella glass-modal"
        role="dialog"
        aria-modal="true"
        aria-label={t("legacy.aria")}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{fatto ? t("legacy.done") : t("legacy.preview")}</h2>
        <div className="percorso">{percorso}</div>

        {errore && <div className="errore">{errore}</div>}
        {inCorso && !rapporto && <p>{t("legacy.reading")}</p>}

        {rapporto && (
          <>
            <div className="rapporto">
              <Voce etichetta={t("legacy.found")} valore={rapporto.matched} />
              <Voce
                etichetta={t("legacy.plays")}
                valore={rapporto.playCountCarried}
                nota={t("legacy.plays.note")}
              />
              <Voce
                etichetta={t("legacy.ratings")}
                valore={rapporto.ratingsCarried}
              />
              <Voce
                etichetta={t("legacy.liked")}
                valore={rapporto.likedCarried}
              />
              <Voce
                etichetta={t("legacy.historyRows")}
                valore={rapporto.historyRows}
              />
              <Voce
                etichetta={t("legacy.playlists")}
                valore={rapporto.playlists}
              />
              <Voce
                etichetta={t("legacy.entries")}
                valore={rapporto.playlistEntries}
                nota={
                  rapporto.playlistOrphans > 0
                    ? t("legacy.orphans", { n: rapporto.playlistOrphans })
                    : undefined
                }
              />
            </div>

            {rapporto.unmatched.length > 0 && (
              <details className="non-ritrovati">
                <summary>
                  {t("legacy.unmatched", { n: rapporto.unmatched.length })}
                </summary>
                <p>{t("legacy.unmatched.note")}</p>
                <ul>
                  {rapporto.unmatched.map((etichetta) => (
                    <li key={etichetta}>{etichetta}</li>
                  ))}
                </ul>
              </details>
            )}
          </>
        )}

        <div className="tasti-finestrella">
          <button type="button" className="bottone" onClick={onChiudi}>
            {fatto ? t("common.close") : t("common.cancel")}
          </button>
          {!fatto && (
            <button
              type="button"
              className="bottone primario"
              disabled={inCorso || piano === null}
              onClick={() => void esegui()}
            >
              {inCorso ? t("legacy.importing") : t("legacy.import")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

// ── da un link: uno dei cataloghi liberi ────────────────────────────────────

/**
 * Che aspetto ha un link che Aether sa leggere.
 *
 * Esempi veri e non descrizioni: chi ha negli appunti qualcosa che a questi non
 * somiglia lo vede prima di incollare, invece di scoprirlo dal messaggio di una
 * lettura fallita.
 *
 * Sono tre e non uno perché sono tre posti diversi, e la differenza non è
 * cosmetica: dall'Internet Archive si può tenere una copia, da Jamendo no —
 * i loro termini vietano esplicitamente la cache e l'accesso offline.
 */
const ESEMPI: readonly string[] = [
  "https://archive.org/details/…",
  "https://www.jamendo.com/track/…",
  "https://audius.co/…",
];

/**
 * Perché non funziona.
 *
 * Vive dietro un `<details>` chiuso e non in mezzo alla schermata: chi importa
 * e basta non deve leggerlo. Ma quando qualcosa non va, la differenza fra «il
 * catalogo non risponde» e «questo computer è offline» è l'unica cosa che
 * permette di ripararlo, e senza un posto dove guardarla resta «non funziona».
 *
 * # Perché li mostra tutti, adesso
 *
 * Il pannello di prima mostrava solo la metà che riguardava il servizio da cui
 * si era partiti, perché le due metà parlavano di cose incomparabili — cifrari
 * TOTP di qua, presenza di un binario di là. Adesso i cataloghi rispondono alla
 * **stessa** domanda ciascuno, e un elenco di tre righe uguali si legge tutto
 * insieme più in fretta di quanto si sceglierebbe quale guardare.
 *
 * # E perché tocca la rete
 *
 * Perché la domanda è «rispondono adesso», e nessuna risposta cablata la
 * risolve. È l'unico posto dell'applicazione in cui aprire un pannello fa
 * partire delle richieste, ed è giusto che sia questo: è l'unico che si apre
 * per saperlo.
 */
function Diagnosi() {
  const [dati, setDati] = useState<DiagnosticaImport | null>(null);
  const [errore, setErrore] = useState<string | null>(null);

  const chiedi = () => {
    if (dati) return;
    ipc
      .importDiagnostica()
      .then(setDati)
      .catch((e: unknown) => setErrore(testoErrore(e)));
  };

  return (
    <details className="diagnosi-servizio" onToggle={chiedi}>
      <summary>{t("link.why")}</summary>
      {errore && <div className="errore">{errore}</div>}
      {dati && (
        <>
          <ul className="cataloghi-stato">
            {dati.cataloghi.map((c) => (
              <li key={c.nome} data-risponde={c.risponde || undefined}>
                <span className="nome">{nomeFonte(c.nome)}</span>
                <span className="che-fa">
                  {c.risponde ? t("link.answering") : t("link.notAnswering")}
                  {" · "}
                  {c.consegna ? t("link.canDownload") : t("link.listenOnly")}
                </span>
              </li>
            ))}
          </ul>
          {dati.cataloghi.every((c) => !c.risponde) ? (
            <p>{t("link.allDown")}</p>
          ) : (
            <p>{t("link.someUp")}</p>
          )}
        </>
      )}
    </details>
  );
}

/* Lo scaricamento dei desiderati non si mostra più qui.
 *
 * Stava in questo file, dentro la finestrella, e moriva con lei: la coda gira
 * su un filo del nucleo e **non** si ferma quando la finestrella si chiude,
 * quindi chi chiudeva si ritrovava un'applicazione che scaricava in silenzio.
 * Adesso sta in `parti/Importazioni.tsx`, montato in `App` e mostrato in
 * Impostazioni › Da Spotify, dove sopravvive alla chiusura e mette una riga per
 * importazione invece di una barra sola per tutte.
 */

/**
 * L'importazione da un link di un catalogo libero.
 *
 * # Una casella sola, e perché non tre
 *
 * Perché chi incolla non deve scegliere un catalogo prima di sapere che cosa ha
 * negli appunti. Il riconoscimento del link lo fa il nucleo, e i lettori
 * producono lo stesso tipo: da qui in poi non c'è nessuna differenza fra le
 * strade, tranne una — da un link il file di ogni brano **è già noto**, quindi
 * la coda non si rimette a cercarlo.
 *
 * Un link che non è di un catalogo riconosciuto non viene tentato: il nucleo
 * risponde `download.unrecognizedUrl` senza fare **nessuna** richiesta. Non è
 * prudenza generica — è la regola che tiene questa applicazione dentro i
 * termini di chi le dà la musica: si bussa dove si sa di poter bussare.
 *
 * # Tre passi, e nessuno è cosmetico
 *
 * 1. **Anteprima** — cosa c'è dietro il link. È l'unico passo che tocca la
 *    rete, e dice anche *quale* livello ha risposto: se ha risposto l'ultimo,
 *    di quel contenuto si sanno solo il titolo e la copertina, e la cosa va
 *    vista prima di importare, non dedotta da un rapporto con zero brani.
 * 2. **Piano** — quanti brani ci sono già in libreria e quali no. È l'ultimo
 *    momento in cui si può ancora andare a cercare i mancanti sul disco e
 *    rifare una scansione, per lo stesso motivo scritto in `import_legacy.rs`.
 * 3. **Conferma** — scrive.
 *
 * Il contenuto letto non viaggia mai fin qui: tutti e tre i comandi ricevono il
 * **link**, e di là c'è una cella. Il secondo e il terzo passo quindi non
 * ripagano la rete.
 *
 * # E perché il terzo passo chiude
 *
 * Perché quello che comincia dopo la conferma — la ricerca dei mancanti nei
 * cataloghi — dura molto più di questa finestrella e non ha bisogno di lei: gira
 * su un filo del nucleo e si racconta in Impostazioni, una riga per
 * importazione. Tenere aperto un riepilogo davanti a quell'elenco vorrebbe dire
 * un tasto «Chiudi» fra l'utente e il link successivo, che è precisamente il
 * gesto che si voleva rendere facile.
 *
 * # E perché c'è «Riprova»
 *
 * Per la stessa cella. Una lettura caduta all'ultimo livello — perché in quel
 * momento la rete singhiozzava — resterebbe altrimenti *la* risposta di quel
 * link per tutto il tempo in cui l'applicazione è aperta, e riaprire questa
 * finestrella non cambierebbe niente. `forza` la salta.
 */
export function ImportaLink({
  onChiudi,
  onImportato,
}: {
  onChiudi: () => void;
  /** Riceve il rapporto: è quel che fa comparire la riga nell'elenco. */
  onImportato: (esito: EsitoImport) => void;
}) {
  const [url, setUrl] = useState("");
  const [anteprima, setAnteprima] = useState<AnteprimaImport | null>(null);
  const [piano, setPiano] = useState<EsitoImport | null>(null);
  const [creaPlaylist, setCreaPlaylist] = useState(true);
  const [errore, setErrore] = useState<unknown>(null);
  const [inCorso, setInCorso] = useState(false);
  /**
   * La rete è in mezzo, adesso.
   *
   * Distinto da `inCorso`, che copre tutti e tre i tempi: il piano si rifà a
   * ogni cambio della casella e **non tocca la rete** — il contenuto è già letto
   * di là. Accendere la barra della lettura anche lì vorrebbe dire mostrare un
   * avanzamento di pagine per un'operazione che di pagine non ne ha, e che dura
   * meno del tempo di comparire.
   */
  const [leggendo, setLeggendo] = useState(false);
  const avanzamento = useLetturaLink(leggendo);

  /**
   * Il fuoco, nei tre momenti in cui si sposta da sé.
   *
   * All'apertura sta nel campo del link — ci pensa `autoFocus`. Quando arriva
   * l'anteprima **il campo non c'è più**: senza questo il fuoco cadrebbe sul
   * `<body>`, e chi naviga con la tastiera dovrebbe ritabulare dall'inizio del
   * documento per raggiungere «Importa», che è a due centimetri dai suoi occhi.
   *
   * Alla chiusura torna dov'era. Conta di più adesso che prima: con `Ctrl+l`
   * questa finestrella si apre da qualunque pagina, e chi l'ha aperta da un
   * elenco vuole ritrovarsi nell'elenco.
   */
  const primario = useRef<HTMLButtonElement>(null);
  const chiAveva = useRef<HTMLElement | null>(null);
  useEffect(() => {
    chiAveva.current =
      document.activeElement instanceof HTMLElement
        ? document.activeElement
        : null;
    return () => chiAveva.current?.focus();
  }, []);
  useEffect(() => {
    if (anteprima !== null) primario.current?.focus();
  }, [anteprima]);

  /**
   * Legge il link.
   *
   * `forza` è quel che c'è dietro «Riprova»: senza, il nucleo ritroverebbe in
   * cella la lettura di prima e la ridarebbe identica, e riprovare non
   * riproverebbe niente. Una rilettura fallita **non** cancella l'anteprima già
   * ottenuta, perché nemmeno di là la cella si svuota: quel che si vede
   * continua a descrivere quel che si importerebbe.
   */
  const guarda = async (forza = false) => {
    if (!url.trim() || inCorso) return;
    setInCorso(true);
    setLeggendo(true);
    setErrore(null);
    setPiano(null);
    try {
      const a = await ipc.importAnteprima(url.trim(), forza);
      setAnteprima(a);
      // Un brano solo non diventa una playlist di uno: la casella parte spenta,
      // e chi la vuole lo stesso può accenderla.
      setCreaPlaylist(a.genere !== "brano");
    } catch (e) {
      setErrore(e);
    } finally {
      setInCorso(false);
      setLeggendo(false);
    }
  };

  // Il piano si rifà a ogni cambio della casella: non tocca la rete — il
  // contenuto è già letto di là — e mostrare un piano calcolato con l'altra
  // scelta sarebbe mostrare qualcosa che l'importazione poi non fa.
  useEffect(() => {
    if (!anteprima) return;
    let annullato = false;
    setInCorso(true);
    ipc
      .importPiano(url.trim(), creaPlaylist)
      .then((p) => {
        if (!annullato) setPiano(p);
      })
      .catch((e: unknown) => {
        if (!annullato) setErrore(e);
      })
      .finally(() => {
        if (!annullato) setInCorso(false);
      });
    return () => {
      annullato = true;
    };
    // `url` non può cambiare mentre `anteprima` è attivo — l'input è
    // nascosto — ma resta fra le dipendenze per robustezza: se la UI
    // cambiasse, il piano dovrebbe rifarsi con l'indirizzo nuovo.
  }, [anteprima, creaPlaylist, url]);

  /**
   * Scrive, e poi se ne va.
   *
   * `onImportato` prima di `onChiudi`: è la consegna dell'importazione a chi la
   * seguirà — l'elenco in Impostazioni — e farla dopo la chiusura vorrebbe dire
   * consegnarla da un componente che non c'è più. Su errore non si chiude
   * niente: il messaggio va letto dove si è premuto.
   */
  const esegui = async () => {
    setInCorso(true);
    setErrore(null);
    try {
      onImportato(await ipc.importEsegui(url.trim(), creaPlaylist));
      onChiudi();
    } catch (e) {
      setErrore(e);
      setInCorso(false);
    }
  };

  // Il piano *è* il rapporto: non ce n'è più un secondo dopo la conferma,
  // perché dopo la conferma questa finestrella non c'è più.
  const rapporto = piano;
  const monco = anteprima?.troncato ?? null;
  // Ha risposto solo il terzo livello: di questo contenuto si sanno il titolo e
  // la copertina, e i brani sono zero. Importare adesso creerebbe una playlist
  // vuota e nient'altro — cioè assomiglierebbe a un successo.
  const degradato = anteprima?.sorgente === "oembed";
  /**
   * Il nucleo ha rifiutato perché la playlist di destinazione è automatica.
   *
   * È l'unico guasto di questa finestrella che ha una **causa visibile sullo
   * schermo**: la casella «Crea la playlist». Mostrarlo in cima con gli altri
   * vorrebbe dire far cercare all'utente quale delle cose che ha davanti l'abbia
   * provocato; attaccarlo alla casella lo dice senza spiegarlo.
   *
   * Non c'è uno scavalco. Un flag di forzatura non esiste nel nucleo, ed è
   * deliberato: le righe messe a mano in una playlist automatica sparirebbero al
   * primo ricalcolo delle regole, quindi «fallo lo stesso» sarebbe un tasto che
   * promette una cosa che si disfa da sola.
   */
  const playlistAutomatica =
    eErroreIpc(errore) && errore.code === "library.playlistIsSmart";

  /** Rilegge il link saltando la cella. Vedi `guarda`. */
  const riprova = (
    <button
      type="button"
      className="bottone"
      disabled={inCorso}
      onClick={() => void guarda(true)}
    >
      {inCorso ? t("link.retrying") : t("common.retry")}
    </button>
  );

  return (
    <div className="velo scuro" onClick={onChiudi}>
      <div
        className="finestrella glass-modal"
        role="dialog"
        aria-modal="true"
        aria-label={t("link.aria")}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{anteprima ? t("legacy.preview") : t("link.title")}</h2>

        {!anteprima && (
          <>
            <p>
              <Trans
                k="link.intro"
                v={{ solo: <strong>{t("link.intro.only")}</strong> }}
              />
            </p>
            <label>
              {t("link.label")}
              <input
                type="url"
                className="campo field-input"
                autoFocus
                placeholder={ESEMPI[0]}
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void guarda();
                }}
              />
            </label>
            {/* Sotto il campo e non dentro il segnaposto: nel segnaposto ce ne
                starebbe uno solo, e il fatto che siano tre posti diversi — con
                regole diverse su cosa si può tenere — è precisamente la cosa
                che va vista prima di incollare. */}
            <ul className="esempi-link">
              {ESEMPI.map((esempio) => (
                <li key={esempio} className="mono">
                  {esempio}
                </li>
              ))}
            </ul>
          </>
        )}

        {anteprima && (
          <div className="scheda-anteprima">
            {anteprima.copertina && (
              /* Un `data:` già scaricato dal nucleo, non un indirizzo del
                 catalogo: la finestra non parla mai con nessuno di loro, e la
                 politica dei contenuti resta quella di prima. */
              <img src={anteprima.copertina} alt="" />
            )}
            <div className="che-cosa">
              <div className="titolo">{anteprima.titolo}</div>
              {anteprima.autore && (
                <div className="autore">{anteprima.autore}</div>
              )}
              {/* Le tacche prima della frase, e `aria-hidden`: chi legge con lo
                  schermo sente già «dai soli titolo e copertina», che è la
                  stessa cosa detta meglio. Servono a chi guarda, per vedere in
                  un colpo che questa lettura vale meno della precedente. */}
              <div className="nota provenienza">
                <Tacche fiducia={fiduciaDi(anteprima.sorgente)} />
                <span>
                  {t("link.summary", {
                    fonte: nomeFonte(anteprima.fonte),
                    genere: anteprima.genere,
                    brani: t("link.tracks", { n: anteprima.brani }),
                    sorgente: nomeSorgente(anteprima.sorgente),
                  })}
                </span>
              </div>
              {/* Il numero che rende onesta questa schermata: quanti di quei
                  brani si possono davvero tenere. Un elenco di venti di cui tre
                  si prendono e diciassette si ascoltano e basta è una cosa da
                  sapere **prima** di confermare, non da scoprire dalla coda che
                  si riempie di righe introvabili. */}
              {anteprima.brani > 0 && (
                <div className="nota licenza-riga">
                  {anteprima.scaricabili === anteprima.brani ? (
                    t("link.allDownloadable")
                  ) : anteprima.scaricabili === 0 ? (
                    <Trans
                      k="link.noneDownloadable"
                      v={{
                        nessuno: (
                          <strong>{t("link.noneDownloadable.head")}</strong>
                        ),
                      }}
                    />
                  ) : (
                    <Trans
                      k="link.someDownloadable"
                      n={{ totale: anteprima.brani }}
                      v={{ quanti: <strong>{anteprima.scaricabili}</strong> }}
                    />
                  )}
                </div>
              )}
              {/* L'attribuzione **si mostra**, sempre: nominare il catalogo da
                  cui viene quel che si sta per portare dentro non è un
                  dettaglio legale da tenere in un file di licenze, è una
                  condizione d'uso, e nasconderla vorrebbe dire usare il
                  catalogo senza rispettarne i termini. Composta qui e non dal
                  nucleo perché è testo dell'interfaccia — la riga gemella che
                  finisce **nei tag** di un file scaricato la scrive
                  `prelievo.rs`, e quella non segue la lingua attiva. */}
              <div className="nota attribuzione">
                {anteprima.autore === null
                  ? t("link.attribution", { fonte: nomeFonte(anteprima.fonte) })
                  : t("link.attribution.by", {
                      autore: anteprima.autore,
                      fonte: nomeFonte(anteprima.fonte),
                    })}
              </div>
            </div>
          </div>
        )}

        {/* Il rifiuto della playlist automatica **non** compare qui: è attaccato
            alla casella che lo causa, più in basso. Un blocco in cima che parla
            di una casella a mezzo schermo di distanza è un blocco che si legge
            senza capire a cosa si riferisca. */}
        {errore !== null && !playlistAutomatica && (
          <AvvisoErrore
            errore={errore}
            {...(url.trim() !== ""
              ? { onRiprova: () => void guarda(true) }
              : {})}
          />
        )}
        {/* Anche con un'anteprima in mano, quando è quella magra: è proprio il
            caso in cui sapere *dove* si è fermata la lettura serve a qualcosa. */}
        {(errore !== null || degradato) && <Diagnosi />}
        {/* Era `<p>Lettura in corso…</p>`, e per una playlist da trecento brani
            erano duecento richieste dietro una frase che non cambiava mai: chi
            aspettava non poteva distinguere una lettura lunga da una impiantata,
            e l'unica risposta possibile era chiudere e rifare da capo la cosa
            che stava quasi finendo. */}
        {leggendo && <LetturaLink avanzamento={avanzamento} />}

        {/* Blocco e non avviso, ed era ambra come tutto il resto: importare
            adesso creerebbe una playlist **vuota**, cioè qualcosa che dal di
            fuori assomiglia a un successo. Il primario è spento, e l'unica cosa
            da fare — rileggere il link saltando la cella — sta dentro il
            riquadro invece che in fondo alla finestrella accanto ad «Annulla». */}
        {degradato && (
          <Avviso livello="blocco" azione={riprova}>
            <Trans
              k="link.degraded"
              v={{
                titolo: <strong>{t("link.degraded.title")}</strong>,
                copertina: <strong>{t("link.degraded.cover")}</strong>,
              }}
            />
          </Avviso>
        )}

        {/* Avviso e non blocco: importare adesso porta dentro qualcosa di
            buono, solo non tutto. I due numeri in `stat-number` perché la
            differenza fra 297 e 300 e quella fra 30 e 300 sono due notizie
            diverse, e in mezzo a una frase si leggono uguali. */}
        {monco && anteprima && (
          <Avviso livello="avviso" azione={riprova}>
            <Trans
              k="link.truncated"
              n={{
                fonte: nomeFonte(anteprima.fonte),
                mancanti: monco.attesi - monco.letti,
              }}
              v={{
                attesi: <span className="stat-number">{monco.attesi}</span>,
                letti: <span className="stat-number">{monco.letti}</span>,
              }}
            />
          </Avviso>
        )}

        {anteprima && !degradato && (
          <>
            <label className="scelta">
              <input
                type="checkbox"
                checked={creaPlaylist}
                onChange={(e) => setCreaPlaylist(e.target.checked)}
              />
              <span>
                {t("link.createPlaylist", { nome: anteprima.titolo })}
                {piano?.playlistReplaced && <em>{t("link.replaces")}</em>}
              </span>
            </label>
            {/* Attaccato alla casella, e con l'unica uscita che esiste davvero:
                importare i brani senza toccare la playlist. Spegnere la casella
                rifà il piano da sé — l'effetto la guarda — quindi il tasto fa
                una cosa sola e quella cosa si vede subito. */}
            {playlistAutomatica && (
              <Avviso
                livello="blocco"
                azione={
                  <button
                    type="button"
                    className="bottone minuto btn-ghost"
                    onClick={() => setCreaPlaylist(false)}
                  >
                    {t("link.smartEscape")}
                  </button>
                }
              >
                {testoErrore(errore)}
              </Avviso>
            )}
          </>
        )}

        {rapporto && (
          <>
            <div className="rapporto">
              <Voce etichetta={t("link.inLibrary")} valore={rapporto.matched} />
              {/* La scomposizione esce dalla `nota` e diventa quattro pastiglie.
                  Era una frase — «12 esatti, 3 con un altro album, 1 a titolo
                  ripulito» — che elencava sempre tutti e tre i gradini, zeri
                  compresi, e non diceva quale fosse il più solido. Le pastiglie
                  portano il grado nel colore, spiegano al passaggio del
                  puntatore, e i gradini a zero **non compaiono**: una voce
                  fissa a zero è una voce che si impara a non leggere.

                  L'ISRC compare qui e non nella frase di prima perché nella
                  frase non c'era: `matchedIsrc` esisteva nel rapporto e non
                  usciva da nessuna parte, cioè l'unico gradino senza margine di
                  dubbio era l'unico che non si vedeva. */}
              {rapporto.matched > 0 && (
                <div className="scomposizione">
                  {rapporto.matchedIsrc > 0 && (
                    <span>
                      <PastigliaGradino gradino="isrc" /> {rapporto.matchedIsrc}
                    </span>
                  )}
                  {rapporto.matchedExact > 0 && (
                    <span>
                      <PastigliaGradino gradino="esatta" />{" "}
                      {rapporto.matchedExact}
                    </span>
                  )}
                  {rapporto.matchedByTitle > 0 && (
                    <span>
                      <PastigliaGradino gradino="artistaTitolo" />{" "}
                      {rapporto.matchedByTitle}
                    </span>
                  )}
                  {rapporto.matchedStripped > 0 && (
                    <span>
                      <PastigliaGradino gradino="ripulito" />{" "}
                      {rapporto.matchedStripped}
                    </span>
                  )}
                </div>
              )}
              <Voce
                etichetta={t("link.notFound")}
                valore={rapporto.missing}
                nota={t("link.notFound.note")}
              />
              {rapporto.playlistId !== null && (
                <Voce
                  etichetta={t("link.entries")}
                  valore={rapporto.playlistEntries}
                  nota={t(
                    rapporto.playlistCreated
                      ? "link.entries.new"
                      : "link.entries.replaced",
                    { nome: rapporto.playlistName ?? "" },
                  )}
                />
              )}
              {/* Solo quando ce n'è: da un catalogo quella colonna resta
                  vuota, e una riga fissa a zero è una riga che si impara a non
                  leggere — la stessa ragione per cui l'ISRC qui sotto compare a
                  condizione. */}
              {rapporto.spotifyAlbumIdsWritten > 0 && (
                <Voce
                  etichetta={t("link.spotifyIds")}
                  valore={rapporto.spotifyAlbumIdsWritten}
                  nota={t("link.spotifyIds.note")}
                />
              )}
              {/* Solo quando ce n'è: oggi Spotify non manda più l'ISRC, e una
                  riga fissa a zero è una riga che si impara a non leggere. */}
              {rapporto.isrcWritten > 0 && (
                <Voce
                  etichetta={t("link.isrc")}
                  valore={rapporto.isrcWritten}
                />
              )}
            </div>

            {/* L'elenco dei mancanti sta qui perché è l'ultimo momento per
                accorgersi che i file ci sono già in una cartella non
                sorvegliata, e risparmiarsi di riscaricarli.

                Non è più l'**unico** momento, ed è la differenza che il quarto
                tempo ha portato: il rapporto adesso si salva, e la pagina
                Importazioni lo riapre tre giorni dopo. Finché moriva con questa
                finestrella, chi chiudeva senza copiare questi nomi li perdeva —
                e la finestrella si chiude da sé alla conferma, perché il gesto
                da rendere facile era il link successivo. */}
            {rapporto.missingTracks.length > 0 && (
              <details className="non-ritrovati">
                <summary>{t("link.missing", { n: rapporto.missing })}</summary>
                <p>
                  <Trans
                    k="link.missing.p1"
                    v={{
                      daComprare: (
                        <strong>{t("settings.import.how.p2.toBuy")}</strong>
                      ),
                    }}
                  />
                </p>
                <p>{t("link.missing.p2")}</p>
                {/* La frase che rende innocua la chiusura automatica. Il difetto
                    non era la finestrella che si chiude: era che chiudere
                    costava questo elenco, e nessuno lo diceva. */}
                <p>
                  <Trans
                    k="link.missing.p3"
                    v={{
                      importazioni: (
                        <strong>{t("settings.queue.empty.link")}</strong>
                      ),
                    }}
                  />
                </p>
                <ul>
                  {rapporto.missingTracks.map((b) => (
                    <li key={`${b.position}-${b.title}`}>
                      {b.artist ? `${b.artist} — ` : ""}
                      {b.title}
                    </li>
                  ))}
                </ul>
              </details>
            )}
          </>
        )}

        <div className="tasti-finestrella">
          <button type="button" className="bottone" onClick={onChiudi}>
            {t("common.cancel")}
          </button>
          {anteprima ? (
            <button
              type="button"
              className="bottone primario"
              ref={primario}
              // `degradato`: importare zero brani riesce, ed è il guasto
              // peggiore possibile qui perché assomiglia in tutto a un
              // successo. Meglio un tasto spento e una frase che lo spiega.
              disabled={inCorso || piano === null || degradato}
              onClick={() => void esegui()}
            >
              {inCorso ? t("legacy.importing") : t("legacy.import")}
            </button>
          ) : (
            <button
              type="button"
              className="bottone primario"
              disabled={inCorso || url.trim() === ""}
              onClick={() => void guarda()}
            >
              {inCorso ? t("link.retrying") : t("link.look")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
