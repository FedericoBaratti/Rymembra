/**
 * La pagina delle importazioni: dove vive un'operazione lunga.
 *
 * # Perché è una pagina e non un pannello
 *
 * Il pannello che mostrava la coda viveva dentro la settima di nove sezioni
 * delle impostazioni. Ma la coda non si ferma quando la finestrella si chiude,
 * non si ferma quando si cambia schermata e **non si ferma quando si chiude
 * l'applicazione**: cento brani sono un'ora, e al riavvio `desiderati` le
 * ritrova. Un'operazione che sopravvive a chi l'ha avviata non può abitare
 * dentro una pagina di configurazione, perché configurare è una cosa che si fa
 * una volta e guardare una coda è una cosa che si fa dieci volte.
 *
 * La regola che ne è uscita, e che varrà anche per il prossimo sottosistema
 * lungo: quel che è breve e transazionale sta in una finestrella; quel che dura
 * un'ora e sopravvive a chi l'ha avviato prende un indirizzo.
 *
 * # Perché non è una quinta voce nella barra
 *
 * Perché per quasi tutto il tempo non c'è niente da vedere, e una destinazione
 * vuota è una voce che si impara a saltare. `DESTINAZIONI` resta di quattro. Le
 * porte sono tre: il toast mentre qualcosa scende, il rimando in fondo a
 * «Importa da un servizio» nelle impostazioni, e la scorciatoia registrata in
 * `tastiera.ts`.
 *
 * # Lo stato non è qui
 *
 * `useImportazioni` sta in `App`, come prima. Questa è la superficie: se lo
 * stato fosse qui, uscire dalla pagina smonterebbe le sottoscrizioni e la coda
 * tornerebbe a scendere in silenzio — cioè il difetto che tutto questo esiste
 * per togliere.
 */
import { useEffect, useState } from "react";

import { ipc, testoErrore, type BranoScarico, type DaComprare } from "../ipc";
import { Avviso } from "../parti/Avvisi";
import { Icona } from "../parti/Icone";
import type { RigaImportazione, UsoImportazioni } from "../parti/Importazioni";
// Niente `PastigliaGradino` qui: il gradino racconta come un brano della fonte
// è stato **ritrovato in libreria**, che succede alla lettura del link. Quel che
// scende è quel che in libreria non c'era, e la sua incertezza è un'altra — il
// file scelto. Due incertezze diverse con la stessa pastiglia sarebbero una
// pastiglia che non vuol dire niente.
import {
  Scarto,
  Tacche,
  fiduciaDi,
  nomeAffidabilita,
  nomeNatura,
} from "../parti/Incertezza";
import { Intestazione } from "../parti/Intestazione";
import { Rapporto } from "../parti/Rapporto";
import { nomeLicenza, numero } from "../formato";
import { t } from "../lingue";
import { Trans } from "../lingue/Trans";

/** Quanti brani si prendono insieme: `procura::FILI`. Si dice una volta sola. */
const FILI = 3;

/**
 * Quante righe di «Da comprare» si chiedono.
 *
 * Un limite e non tutte, per la stessa ragione per cui ce l'ha
 * `importRapporti`: `desiderati` non cancella mai niente, quindi questo elenco
 * cresce per sempre. Duecento sono più di quante ne legga chiunque in una
 * sessione, e sono un elenco che si disegna in un fotogramma.
 */
const QUANTE_DA_COMPRARE = 200;

/** Cosa dice il sottotitolo, che è l'unico posto in cui i tre fili si nominano. */
function comeVa(importazioni: UsoImportazioni): string {
  const inAttesa = importazioni.stato?.conteggi.attesa ?? 0;
  if (importazioni.elenco.length === 0) {
    return t("imports.sub.empty");
  }
  if (importazioni.stato?.attiva === true) {
    return t("imports.sub.running", { n: FILI });
  }
  return inAttesa > 0 ? t("imports.sub.paused") : t("imports.sub.idle");
}

/**
 * L'intestazione, con i comandi della coda.
 *
 * Sta qui e non in `App.tsx` perché i tasti sono di questa pagina, e usa
 * `Intestazione` come tutte le altre perché una pagina che si disegna la testata
 * da sé è una pagina che si scosta dalle altre al primo ritocco della scala.
 */
export function TestaImportazioni({
  importazioni,
  onIncollaLink,
}: {
  importazioni: UsoImportazioni;
  onIncollaLink: () => void;
}) {
  const inAttesa = importazioni.stato?.conteggi.attesa ?? 0;
  const falliti = importazioni.stato?.conteggi.fallito ?? 0;
  const attiva = importazioni.stato?.attiva ?? false;

  return (
    <Intestazione
      titolo={t("imports.title")}
      sottotitolo={comeVa(importazioni)}
      azioni={
        <>
          {attiva ? (
            <button
              type="button"
              className="pillola btn-ghost"
              onClick={importazioni.annulla}
            >
              <Icona nome="i-pause" dim={15} />
              {t("imports.stop")}
            </button>
          ) : (
            inAttesa > 0 && (
              <button
                type="button"
                className="pillola btn-ghost"
                onClick={importazioni.riprendi}
              >
                <Icona nome="i-play" dim={15} />
                {t("imports.resume", { n: numero(inAttesa) })}
              </button>
            )
          )}
          {/* Rimette in fila solo i falliti, e solo a coda ferma: rimetterli
              mentre scende raddoppierebbe le righe. Gli introvabili non ci
              vanno — resterebbero introvabili, e riprovarli trasformerebbe
              «riprova» in «rifai tutto». */}
          {!attiva && falliti > 0 && (
            <button
              type="button"
              className="pillola btn-ghost"
              onClick={importazioni.riprovaFalliti}
            >
              <Icona nome="i-repeat" dim={15} />
              {t("imports.retryFailed", { n: falliti })}
            </button>
          )}
          <button
            type="button"
            className="pillola btn-accent"
            onClick={onIncollaLink}
          >
            <Icona nome="i-import" dim={15} />
            {t("imports.pasteLink")}
          </button>
        </>
      }
    />
  );
}

/** Cosa sta succedendo al brano in corso, in due parole. */
function nota(brano: BranoScarico): string {
  switch (brano.esito) {
    case "cerco":
      return t("imports.track.searching");
    case "prendo":
      return brano.frazione === null
        ? t("imports.track.fetching")
        : `${Math.round(brano.frazione * 100)}%`;
    case "fatto":
      return t("imports.track.done");
    case "introvabile":
      // Due ragioni, e vanno distinte: «non l'ho trovato» e «l'ho trovato e la
      // licenza non me lo lascia prendere» sono cose diverse per chi legge —
      // la seconda vuol dire che da qualche parte si ascolta.
      return brano.codice === "download.notPermitted"
        ? t("imports.track.notAllowed")
        : t("imports.track.nowhere");
    default:
      return brano.codice ?? t("imports.track.failed");
  }
}

/**
 * La riga in volo: quale brano scende adesso, e quale file è stato scelto.
 *
 * **Una per importazione, non tre.** `scarico:brano` è un evento solo e i fili
 * sono tre: tre righe interlacciate nello stesso riquadro darebbero un titolo
 * che salta. I fili si dicono una volta, nel sottotitolo della pagina.
 *
 * # Le tre cose che si dicono della scelta, e perché tutte e tre
 *
 * **Chi pubblica** e **con che titolo**, che è quel che dice se la scelta è
 * buona. **Sotto che licenza**, perché un file che entra in libreria senza che
 * nessuno dica a quali condizioni ci è entrato è un file che fra un anno
 * nessuno saprà se può condividere. E **che registrazione è** — studio, dal
 * vivo, un'altra versione — perché i cataloghi liberi sono fatti in gran parte
 * di concerti, e prendere un live per la versione in studio senza dirlo
 * sarebbe scrivere in libreria una cosa per un'altra.
 */
function RigaBrano({ brano, fonte }: { brano: BranoScarico; fonte: string }) {
  // Da un link di un catalogo non c'è niente da scegliere: il file è quello che
  // l'utente ha incollato. Dirlo è meglio che lasciare vuoto lo spazio in cui
  // le altre righe hanno un autore — e appianare l'asimmetria inventando una
  // confidenza sarebbe la prima bugia di un'interfaccia che se n'è vietata una
  // sola.
  const daUnLink = fonte !== "archivio-spotify" && fonte !== "file-playlist";
  const scelto = brano.scelto;

  return (
    <div className="importazione-brano list-row">
      <span className="titolo">
        {brano.artista !== null ? `${brano.artista} — ` : ""}
        {brano.titolo}
      </span>

      {scelto !== null ? (
        <>
          {/* L'autore davanti al titolo, perché è quel che dice se la scelta è
              buona; il titolo dietro, che è la conferma. Senza autore resta il
              titolo da solo — il catalogo non l'ha dato, e un trattino davanti
              al vuoto sembrerebbe un dato mancante invece di uno assente. */}
          <span className="candidato" title={scelto.titolo}>
            {scelto.autore !== null && scelto.autore}
            <span className="video">
              {scelto.autore !== null ? " · " : ""}
              {scelto.titolo}
            </span>
          </span>
          {/* La licenza **sempre**: è l'unica delle tre che non ha un caso
              normale da tacere. Ambra quando non è dichiarata, perché quello è
              il caso in cui il file **non** viene preso. */}
          <span
            className="pastiglia licenza"
            data-livello={scelto.licenza === "sconosciuta" ? "avviso" : "certo"}
          >
            {nomeLicenza(scelto.licenza)}
          </span>
          {nomeNatura(scelto.natura) !== null && (
            <span className="pastiglia natura" data-livello="avviso">
              {nomeNatura(scelto.natura)}
            </span>
          )}
          {nomeAffidabilita(scelto.affidabilita) !== null && (
            <span className="pastiglia ufficialita" data-livello="certo">
              {nomeAffidabilita(scelto.affidabilita)}
            </span>
          )}
          {/* Niente scarto quando una delle due durate non si sa: la pastiglia
              direbbe «0 s», cioè un combaciare che nessuno ha verificato. */}
          {scelto.scartoMs !== null && <Scarto scartoMs={scelto.scartoMs} />}
        </>
      ) : daUnLink && brano.esito === "prendo" ? (
        <span className="candidato">{t("imports.track.fromList")}</span>
      ) : (
        <span className="candidato" data-livello="nota">
          {nota(brano)}
        </span>
      )}

      {(scelto !== null || brano.esito === "prendo") && (
        <>
          <div className="barra">
            <div
              className="riempimento"
              style={{
                width:
                  brano.frazione !== null
                    ? `${Math.round(brano.frazione * 100)}%`
                    : "0%",
              }}
            />
          </div>
          <span className="percento">{nota(brano)}</span>
        </>
      )}
    </div>
  );
}

/** Una riga: una playlist, un album o un brano, e come sta scendendo. */
function Riga({
  riga,
  brano,
  rapportoAperto,
  onScarta,
  onRapporto,
  onChiudiRapporto,
}: {
  riga: RigaImportazione;
  /** Il brano in corso, se è di questa importazione. */
  brano: BranoScarico | null;
  rapportoAperto: boolean;
  onScarta: (sourceId: string) => void;
  onRapporto: (sourceId: string) => void;
  onChiudiRapporto: () => void;
}) {
  const percento =
    riga.totale > 0 ? Math.round((riga.fatti / riga.totale) * 100) : 100;
  const conclusa = riga.attesa === 0;
  // Un'importazione che non ha lasciato righe in coda era già tutta in
  // libreria. È l'unico esito andato perfettamente, ed era l'unico che non
  // lasciava traccia: chi premeva «Importa» vedeva la finestrella chiudersi e
  // nient'altro.
  const nullaDaScaricare = riga.totale === 0;

  return (
    <li className="importazione" data-conclusa={conclusa || undefined}>
      <div className="che-cosa">
        <span className="titolo" title={riga.titolo}>
          {riga.titolo}
        </span>
        <span className="genere">
          {riga.genere}
          {riga.brani !== null &&
            t("imports.row.tracks", { n: numero(riga.brani) })}
        </span>
        {/* Senza rapporto in sessione le tacche si **omettono**, non si mettono
            a «completo»: di un'importazione ritrovata al riavvio non sappiamo
            quale livello aveva risposto. */}
        {riga.sorgente !== null && (
          <>
            <Tacche fiducia={fiduciaDi(riga.sorgente)} />
            <span className="provenienza">{riga.provenienza ?? ""}</span>
          </>
        )}
        <span className="conteggio">
          {nullaDaScaricare
            ? t("imports.row.nothing")
            : `${numero(riga.fatti)} / ${numero(riga.totale)}`}
        </span>
        {!nullaDaScaricare && (
          <div
            className="barra"
            role="progressbar"
            aria-label={t("imports.row.progress", { titolo: riga.titolo })}
            aria-valuenow={percento}
            aria-valuemin={0}
            aria-valuemax={100}
          >
            <div className="riempimento" style={{ width: `${percento}%` }} />
          </div>
        )}
        {conclusa && (
          <button
            type="button"
            className="bottone minuto btn-ghost"
            onClick={() =>
              rapportoAperto ? onChiudiRapporto() : onRapporto(riga.sourceId)
            }
          >
            {rapportoAperto
              ? t("imports.row.closeReport")
              : t("imports.row.openReport")}
          </button>
        )}
        {/* Il × solo a coda vuota: togliere dall'elenco qualcosa che sta ancora
            scendendo nasconderebbe una cosa in corso, che è il difetto che
            questa pagina esiste per togliere. */}
        {conclusa && (
          <button
            type="button"
            className="tasto icon-btn"
            aria-label={t("imports.row.remove", { titolo: riga.titolo })}
            onClick={() => onScarta(riga.sourceId)}
          >
            <Icona nome="i-x" dim={12} />
          </button>
        )}
      </div>

      <div className="note">
        {riga.giaInLibreria !== null && riga.giaInLibreria > 0 && (
          <span>{t("imports.row.already", { n: riga.giaInLibreria })}</span>
        )}
        {riga.playlist !== null && (
          <span>{t("imports.row.playlist", { nome: riga.playlist })}</span>
        )}
        {/* Ambra, perché un gesto le ripara: «Riprova i N non riusciti». */}
        {riga.falliti > 0 && (
          <span data-livello="avviso">
            {t("imports.row.failed", { n: riga.falliti })}
          </span>
        )}
        {/* Grigi, e non rossi: cercato-e-non-c'è è **Info** nel catalogo. Non è
            un guasto, ed è terminale: la riga lo dice invece di offrire un
            tasto morto. */}
        {riga.introvabili > 0 && (
          <span data-livello="nota">
            {t("imports.row.nowhere", { n: riga.introvabili })}
          </span>
        )}
        {nullaDaScaricare && (
          <span data-livello="esito">{t("imports.row.allThere")}</span>
        )}
      </div>

      {brano !== null && <RigaBrano brano={brano} fonte={riga.servizio} />}

      {rapportoAperto && (
        <Rapporto sourceId={riga.sourceId} onChiudi={onChiudiRapporto} />
      )}
    </li>
  );
}

/**
 * Quel che resta da comprare.
 *
 * # Perché questa sezione esiste, e cosa sostituisce
 *
 * Sostituisce lo scaricamento che non si può fare. Fino a poco fa i brani che
 * la coda non trovava diventavano un numero grigio in fondo a una riga —
 * «3 introvabili» — e lì finiva: un'informazione vera, inutile, e con
 * un'implicazione sbagliata, perché suonava come un guasto.
 *
 * Non lo è. Nessun catalogo libero ha il catalogo commerciale, e non è un
 * limite da aggirare: è il motivo per cui questa applicazione non scarica più
 * da dove non deve. La cosa utile che può fare, e che fa qui, è dire **dove**
 * prendere quel brano — in posti dove chi l'ha fatto viene pagato.
 *
 * # Perché i tre negozi in quest'ordine
 *
 * **Bandcamp** per primo perché la quota che arriva all'artista è la più alta.
 * **Qobuz** perché consegna file — FLAC, che entrano in libreria e restano — e
 * non un abbonamento che scade. **Discogs** per ultimo perché trova le cose
 * fuori catalogo, che sono esattamente quelle che nessun catalogo libero aveva.
 *
 * # Perché è chiusa di serie
 *
 * Perché la pagina è il posto della coda, e questa è la parte che si va a
 * cercare quando la coda ha finito. Quel che si va a cercare non deve stare
 * davanti a quel che si guarda — la stessa regola della storia qui sotto.
 *
 * L'elenco si chiede **all'apertura**, non al montaggio: può essere lungo come
 * la libreria di qualcuno, e chiederlo a ogni visita della pagina per mostrarlo
 * in una sezione che di norma resta chiusa sarebbe una lettura buttata via.
 */
function DaComprareSezione({ quanti }: { quanti: number }) {
  const [aperta, setAperta] = useState(false);
  const [righe, setRighe] = useState<DaComprare[] | null>(null);
  const [errore, setErrore] = useState<string | null>(null);

  useEffect(() => {
    if (!aperta || righe !== null) return;
    let annullato = false;
    ipc
      .daComprare(QUANTE_DA_COMPRARE)
      .then((r) => {
        if (!annullato) setRighe(r);
      })
      .catch((e: unknown) => {
        if (!annullato) setErrore(testoErrore(e));
      });
    return () => {
      annullato = true;
    };
  }, [aperta, righe]);

  return (
    <details
      className="da-comprare"
      onToggle={(e) => setAperta(e.currentTarget.open)}
    >
      <summary>{t("buy.title", { n: quanti })}</summary>

      <p className="nota">{t("buy.note")}</p>

      {errore !== null && <Avviso livello="blocco">{errore}</Avviso>}

      {righe !== null && (
        <ul className="elenco-da-comprare">
          {righe.map((riga) => {
            // Artista e titolo insieme: cercare il solo titolo su Bandcamp dà
            // trenta cover, e chi ha aperto questa sezione voleva quel brano.
            const cosa =
              riga.artista !== ""
                ? `${riga.artista} ${riga.titolo}`
                : riga.titolo;
            return (
              <li key={`${riga.artista}|${riga.titolo}`} className="list-row">
                <span className="titolo">
                  {riga.artista !== "" ? `${riga.artista} — ` : ""}
                  {riga.titolo}
                </span>
                {riga.album !== "" && (
                  <span className="genere">{riga.album}</span>
                )}
                {/* Il perché, e non solo il fatto: «la licenza non lo consente»
                    vuol dire che quel brano da qualche parte si **ascolta**, ed
                    è una cosa diversa da «non l'ho trovato». */}
                {riga.motivo === "download.notPermitted" && (
                  <span className="pastiglia licenza" data-livello="nota">
                    {t("buy.listenOnly")}
                  </span>
                )}
                <span className="provenienza">{riga.provenienza}</span>
                <span className="negozi">
                  {(["bandcamp", "qobuz", "discogs"] as const).map(
                    (negozio) => (
                      <button
                        key={negozio}
                        type="button"
                        className="bottone minuto btn-ghost"
                        /* Il nome del negozio è tutto quel che si legge sul
                           tasto, e su duecento righe sono seicento tasti che si
                           chiamano alla stessa maniera: l'etichetta dice anche
                           che cosa si va a cercare. */
                        aria-label={t("buy.searchOn", { cosa, negozio })}
                        onClick={() =>
                          void ipc.cercaDoveComprare(negozio, cosa)
                        }
                      >
                        {negozio}
                      </button>
                    ),
                  )}
                </span>
              </li>
            );
          })}
        </ul>
      )}

      {righe !== null && righe.length === QUANTE_DA_COMPRARE && (
        <p className="nota">{t("buy.mostRecent", { n: QUANTE_DA_COMPRARE })}</p>
      )}
    </details>
  );
}

/**
 * Il corpo della pagina.
 *
 * Rende `empty-state` quando non c'è niente: un pannello dentro una scheda
 * poteva non rendere nulla, una pagina raggiungibile no — chi ci arriva da una
 * scorciatoia e trova il vuoto non sa se è vuota o rotta.
 */
export function SchermataImportazioni({
  importazioni,
  onIncollaLink,
}: {
  importazioni: UsoImportazioni;
  /** Apre la finestrella in cui si incolla un link. */
  onIncollaLink: () => void;
}) {
  const { elenco, stato, brano, errore, rientro, storia, scarta } =
    importazioni;
  // Uno alla volta, e non un `Set`: due rapporti aperti insieme sono due pile
  // di numeri simili nella stessa colonna, che è il modo di leggere quello
  // sbagliato.
  const [rapportoAperto, setRapportoAperto] = useState<string | null>(null);

  const inAttesa = stato?.conteggi.attesa ?? 0;
  const attiva = stato?.attiva ?? false;
  const fatti = stato?.fatti ?? 0;
  const totale = fatti + inAttesa;

  return (
    <div className="importazioni-pagina">
      {/* Gli avvisi prima dell'elenco, sempre: leggerli dopo vorrebbe dire
          credere per qualche secondo che l'importazione abbia sbagliato. */}

      {errore !== null && <Avviso livello="blocco">{errore}</Avviso>}

      {/* Il viaggio di ritorno: l'unico verde del flusso, e l'unica cosa andata
          meglio di quanto sembrava. Senza dirlo, chi guarda vede una playlist
          riempirsi da sola qualche secondo dopo la fine. */}
      {rientro !== null && rientro.vociRimesse > 0 && (
        <Avviso livello="esito">
          <Trans
            k="imports.back"
            n={{ n: rientro.vociRimesse }}
            v={{ quanti: <strong>{numero(rientro.vociRimesse)}</strong> }}
          />
        </Avviso>
      )}

      {elenco.length === 0 ? (
        <div className="vuoto empty-state">
          <span className="empty-icon" aria-hidden="true">
            <Icona nome="i-import" dim={30} />
          </span>
          <h2>{t("imports.empty.title")}</h2>
          <p>
            <Trans
              k="imports.empty.p1"
              v={{
                a: <span className="mono">archive.org</span>,
                b: <span className="mono">audius.co</span>,
              }}
            />
          </p>
          <p className="nota">{t("imports.empty.p2")}</p>
          <button type="button" className="bottone" onClick={onIncollaLink}>
            {t("imports.pasteLink")}
          </button>
        </div>
      ) : (
        <>
          {totale > 0 && (
            <div className="riepilogo-coda">
              <span className="stat-number">
                {numero(fatti)} / {numero(totale)}
              </span>
              <div
                className="barra"
                role="progressbar"
                aria-label={t("imports.state.progress")}
                aria-valuenow={fatti}
                aria-valuemin={0}
                aria-valuemax={totale}
              >
                <div
                  className="riempimento"
                  // Ferma e grigia, non assente: una barra che sparisce dice
                  // «finito».
                  data-ferma={!attiva || undefined}
                  style={{ width: `${Math.round((fatti / totale) * 100)}%` }}
                />
              </div>
              <span className="stato">
                {attiva
                  ? t("imports.state.running")
                  : t("imports.state.paused", { n: numero(inAttesa) })}
              </span>
            </div>
          )}

          <ul className="elenco-importazioni">
            {elenco.map((riga) => (
              <Riga
                key={riga.sourceId}
                riga={riga}
                brano={
                  brano !== null && brano.sorgenteId === riga.sourceId
                    ? brano
                    : null
                }
                rapportoAperto={rapportoAperto === riga.sourceId}
                onScarta={scarta}
                onRapporto={setRapportoAperto}
                onChiudiRapporto={() => setRapportoAperto(null)}
              />
            ))}
          </ul>
        </>
      )}

      {/* Sopra la storia e sotto la coda: è la conclusione di quel che la coda
          ha fatto, non un archivio di sessioni passate. */}
      {(stato?.conteggi.introvabile ?? 0) > 0 && (
        <DaComprareSezione quanti={stato?.conteggi.introvabile ?? 0} />
      )}

      {/* Chiusa di serie: la pagina è il posto della coda, e la storia è quel
          che si va a cercare — quel che si va a cercare non deve stare davanti
          a quel che si guarda. */}
      {storia.length > 0 && (
        <details className="storia-importazioni">
          <summary>{t("imports.history", { n: storia.length })}</summary>
          <ul className="elenco-importazioni magro">
            {storia.map((r) => (
              <li key={r.sourceId} className="list-row">
                <span className="titolo">{r.title}</span>
                <span className="genere">{r.kind}</span>
                <span className="conteggio">
                  {r.missing > 0
                    ? t("imports.history.missing", { n: numero(r.missing) })
                    : t("imports.history.allThere")}
                </span>
                <button
                  type="button"
                  className="bottone minuto btn-ghost"
                  onClick={() => setRapportoAperto(r.sourceId)}
                >
                  {t("imports.row.openReport")}
                </button>
              </li>
            ))}
          </ul>
          {/* Il rapporto di una conclusa di prima si apre qui sotto: quella
              riga non ha una `<li>` della coda a cui attaccarsi. */}
          {rapportoAperto !== null &&
            storia.some((r) => r.sourceId === rapportoAperto) && (
              <Rapporto
                sourceId={rapportoAperto}
                onChiudi={() => setRapportoAperto(null)}
              />
            )}
        </details>
      )}
    </div>
  );
}
