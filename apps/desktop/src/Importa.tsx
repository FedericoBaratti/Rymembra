/**
 * Le due importazioni: dal vecchio database e da Spotify.
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
import { useEffect, useState } from "react";

import {
  ipc,
  testoErrore,
  type AnteprimaSpotify,
  type DiagnosticaSpotify,
  type EsitoImportazione,
  type EsitoSpotify,
} from "./ipc";

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
        className="finestrella"
        role="dialog"
        aria-modal="true"
        aria-label="Importa dal vecchio database"
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{fatto ? "Importazione conclusa" : "Cosa verrebbe importato"}</h2>
        <div className="percorso">{percorso}</div>

        {errore && <div className="errore">{errore}</div>}
        {inCorso && !rapporto && <p>Lettura del vecchio database…</p>}

        {rapporto && (
          <>
            <div className="rapporto">
              <Voce etichetta="Brani ritrovati" valore={rapporto.matched} />
              <Voce
                etichetta="Ascolti"
                valore={rapporto.playCountCarried}
                nota="conteggi fusi, mai abbassati"
              />
              <Voce etichetta="Voti" valore={rapporto.ratingsCarried} />
              <Voce etichetta="Preferiti" valore={rapporto.likedCarried} />
              <Voce etichetta="Righe di cronologia" valore={rapporto.historyRows} />
              <Voce etichetta="Playlist" valore={rapporto.playlists} />
              <Voce
                etichetta="Voci di playlist"
                valore={rapporto.playlistEntries}
                nota={
                  rapporto.playlistOrphans > 0
                    ? `${rapporto.playlistOrphans} saltate: il brano non c'è più`
                    : undefined
                }
              />
            </div>

            {rapporto.unmatched.length > 0 && (
              <details className="non-ritrovati">
                <summary>
                  {rapporto.unmatched.length} brani del vecchio database non sono
                  in libreria
                </summary>
                <p>
                  Le loro statistiche restano indietro. Di solito vuol dire che i
                  file sono stati spostati o cancellati: una scansione delle
                  cartelle giuste, poi di nuovo qui, li recupera.
                </p>
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
            {fatto ? "Chiudi" : "Annulla"}
          </button>
          {!fatto && (
            <button
              type="button"
              className="bottone primario"
              disabled={inCorso || piano === null}
              onClick={() => void esegui()}
            >
              {inCorso ? "Importazione…" : "Importa"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

// ── da Spotify ──────────────────────────────────────────────────────────────

/** Il nome del livello che ha risposto, in italiano. */
function nomeSorgente(sorgente: string): string {
  if (sorgente === "pathfinder") return "dal lettore web";
  if (sorgente === "embed") return "dalla pagina incorporabile";
  return "dai soli titolo e copertina";
}

/**
 * Perché non funziona.
 *
 * Vive dietro un `<details>` chiuso e non in mezzo alla schermata: questo
 * sottosistema dipende da punti interni di Spotify, e quando si rompe la
 * differenza fra «i cifrari sono scaduti» e «questo computer è offline» è
 * l'unica cosa che permette di ripararlo. Chi importa e basta non deve
 * leggerla.
 */
function Diagnosi() {
  const [dati, setDati] = useState<DiagnosticaSpotify | null>(null);
  const [errore, setErrore] = useState<string | null>(null);

  const chiedi = () => {
    if (dati) return;
    ipc
      .spotifyDiagnostica()
      .then(setDati)
      .catch((e: unknown) => setErrore(testoErrore(e)));
  };

  return (
    <details className="diagnosi-spotify" onToggle={chiedi}>
      <summary>Perché non funziona?</summary>
      {errore && <div className="errore">{errore}</div>}
      {dati && (
        <>
          <p>
            {dati.strettaDiMano === null
              ? "Il collegamento a Spotify funziona: il problema è nel link, o in questo contenuto."
              : `Spotify non risponde: ${dati.strettaDiMano}`}
          </p>
          <p>
            {dati.cifrari} cifrari disponibili (v
            {dati.versioniCifrari.join(", v")}).
            {dati.fileConfig === "letto"
              ? " Le costanti sono state lette dal file qui sotto."
              : dati.fileConfig === "illeggibile"
                ? ` Il file qui sotto non si è capito (${dati.fileConfigErrore ?? "?"}) e sono state usate quelle compilate dentro.`
                : " Sono quelle compilate dentro l'applicazione."}
          </p>
          <p>
            Quando Spotify le cambia, si riparano scrivendo il file qui sotto —
            senza aggiornare Aether.
          </p>
          <div className="percorso">{dati.percorsoConfig}</div>
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
 * L'importazione da un link di Spotify.
 *
 * # Tre passi, e nessuno è cosmetico
 *
 * 1. **Anteprima** — cosa c'è dietro il link. È l'unico passo che tocca la
 *    rete, e dice anche *quale* dei tre livelli ha risposto: se ha risposto il
 *    terzo, di quel contenuto si sanno solo il titolo e la copertina, e la cosa
 *    va vista prima di importare, non dedotta da un rapporto con zero brani.
 * 2. **Piano** — quanti brani ci sono già in libreria e quali no. È l'ultimo
 *    momento in cui si può ancora andare a cercare i mancanti sul disco e
 *    rifare una scansione, per lo stesso motivo scritto in `import_legacy.rs`.
 * 3. **Conferma** — scrive.
 *
 * Il contenuto letto non viaggia mai fin qui: tutti e tre i comandi ricevono il
 * **link**, e di là c'è una cella indicizzata dall'URI. Il secondo e il terzo
 * passo quindi non ripagano la rete.
 *
 * # E perché il terzo passo chiude
 *
 * Perché quello che comincia dopo la conferma — lo scaricamento dei mancanti da
 * YouTube — dura molto più di questa finestrella e non ha bisogno di lei: gira
 * su un filo del nucleo e si racconta in Impostazioni › Da Spotify, una riga per
 * importazione. Tenere aperto un riepilogo davanti a quell'elenco vorrebbe dire
 * un tasto «Chiudi» fra l'utente e il link successivo, che è precisamente il
 * gesto che si voleva rendere facile.
 *

 * # E perché c'è «Riprova»
 *
 * Per la stessa cella. Una lettura caduta al terzo livello — perché in quel
 * momento la rete singhiozzava — resterebbe altrimenti *la* risposta di quel
 * link per tutto il tempo in cui l'applicazione è aperta, e riaprire questa
 * finestrella non cambierebbe niente. `forza` la salta.
 */
export function ImportaSpotify({
  onChiudi,
  onImportato,
}: {
  onChiudi: () => void;
  /** Riceve il rapporto: è quel che fa comparire la riga nell'elenco. */
  onImportato: (esito: EsitoSpotify) => void;
}) {
  const [url, setUrl] = useState("");
  const [anteprima, setAnteprima] = useState<AnteprimaSpotify | null>(null);
  const [piano, setPiano] = useState<EsitoSpotify | null>(null);
  const [creaPlaylist, setCreaPlaylist] = useState(true);
  const [errore, setErrore] = useState<string | null>(null);
  const [inCorso, setInCorso] = useState(false);

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
    setErrore(null);
    setPiano(null);
    try {
      const a = await ipc.spotifyAnteprima(url.trim(), forza);
      setAnteprima(a);
      // Un brano solo non diventa una playlist di uno: la casella parte spenta,
      // e chi la vuole lo stesso può accenderla.
      setCreaPlaylist(a.genere !== "brano");
    } catch (e) {
      setErrore(testoErrore(e));
    } finally {
      setInCorso(false);
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
      .spotifyPiano(url.trim(), creaPlaylist)
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
      onImportato(await ipc.spotifyImporta(url.trim(), creaPlaylist));
      onChiudi();
    } catch (e) {
      setErrore(testoErrore(e));
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

  /** Rilegge il link saltando la cella. Vedi `guarda`. */
  const riprova = (
    <button
      type="button"
      className="bottone"
      disabled={inCorso}
      onClick={() => void guarda(true)}
    >
      {inCorso ? "Lettura…" : "Riprova"}
    </button>
  );

  return (
    <div className="velo scuro" onClick={onChiudi}>
      <div
        className="finestrella"
        role="dialog"
        aria-modal="true"
        aria-label="Importa da Spotify"
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{anteprima ? "Cosa verrebbe importato" : "Importa da Spotify"}</h2>

        {!anteprima && (
          <>
            <p>
              Incolla il link di un brano, di un album o di una playlist
              pubblica. Non serve un account: Aether cerca in libreria i brani
              che ci sono già e scarica gli altri <strong>da YouTube</strong>,
              preferendo i canali ufficiali. Da Spotify si prendono solo i nomi.
            </p>
            <label>
              Link
              <input
                type="url"
                className="campo"
                autoFocus
                placeholder="https://open.spotify.com/playlist/…"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter") void guarda();
                }}
              />
            </label>
          </>
        )}

        {anteprima && (
          <div className="anteprima-spotify">
            {anteprima.copertina && (
              /* Un `data:` già scaricato dal nucleo, non un indirizzo di
                 Spotify: la finestra non parla mai con Spotify, e la politica
                 dei contenuti resta quella di prima. */
              <img src={anteprima.copertina} alt="" />
            )}
            <div className="che-cosa">
              <div className="titolo">{anteprima.titolo}</div>
              {anteprima.autore && (
                <div className="autore">{anteprima.autore}</div>
              )}
              <div className="nota">
                {anteprima.genere} · {anteprima.brani}{" "}
                {anteprima.brani === 1 ? "brano" : "brani"} ·{" "}
                {nomeSorgente(anteprima.sorgente)}
              </div>
            </div>
          </div>
        )}

        {errore && (
          <div className="errore">
            <span>{errore}</span>
            {url.trim() !== "" && riprova}
          </div>
        )}
        {/* Anche con un'anteprima in mano, quando è quella magra: è proprio il
            caso in cui sapere *dove* si è fermata la lettura serve a qualcosa. */}
        {(errore || degradato) && <Diagnosi />}
        {inCorso && !rapporto && <p>Lettura da Spotify…</p>}

        {degradato && (
          <div className="avviso-monco">
            Di questo link si sono ottenuti solo il <strong>titolo</strong> e la{" "}
            <strong>copertina</strong>: l&apos;elenco dei brani non è arrivato.
            Importarlo adesso creerebbe una playlist vuota. Può essere Spotify
            che ha cambiato qualcosa, o un momento storto della rete.
            {riprova}
          </div>
        )}

        {monco && (
          <div className="avviso-monco">
            Spotify ne dichiara <strong>{monco.attesi}</strong> ma ne ha mandati{" "}
            <strong>{monco.letti}</strong>. Importando adesso, i{" "}
            {monco.attesi - monco.letti} che mancano resterebbero fuori.
          </div>
        )}

        {anteprima && !degradato && (
          <label className="scelta">
            <input
              type="checkbox"
              checked={creaPlaylist}
              onChange={(e) => setCreaPlaylist(e.target.checked)}
            />
            <span>
              Crea la playlist «{anteprima.titolo}»
              {piano?.playlistReplaced && (
                <em> — esiste già, e il suo contenuto verrebbe sostituito</em>
              )}
            </span>
          </label>
        )}

        {rapporto && (
          <>
            <div className="rapporto">
              <Voce
                etichetta="Brani già in libreria"
                valore={rapporto.matched}
                nota={
                  rapporto.matched > 0
                    ? `${rapporto.matchedExact} esatti, ${rapporto.matchedByTitle} con un altro album, ${rapporto.matchedStripped} a titolo ripulito`
                    : undefined
                }
              />
              <Voce
                etichetta="Brani non trovati"
                valore={rapporto.missing}
                nota="verranno scaricati da YouTube"
              />
              {rapporto.playlistId !== null && (
                <Voce
                  etichetta="Voci in playlist"
                  valore={rapporto.playlistEntries}
                  nota={
                    rapporto.playlistCreated
                      ? `«${rapporto.playlistName ?? ""}», nuova`
                      : `«${rapporto.playlistName ?? ""}», contenuto sostituito`
                  }
                />
              )}
              <Voce
                etichetta="Identificativi Spotify scritti"
                valore={rapporto.spotifyAlbumIdsWritten}
                nota="fondono le edizioni dello stesso album già in libreria"
              />
              {/* Solo quando ce n'è: oggi Spotify non manda più l'ISRC, e una
                  riga fissa a zero è una riga che si impara a non leggere. */}
              {rapporto.isrcWritten > 0 && (
                <Voce
                  etichetta="Codici ISRC scritti"
                  valore={rapporto.isrcWritten}
                />
              )}
            </div>

            {/* L'elenco dei mancanti sta qui e non dopo: è l'ultimo momento per
                accorgersi che i file ci sono già in una cartella non
                sorvegliata, e risparmiarsi di riscaricarli. Dopo la conferma
                non c'è un «dopo» in questa finestrella — c'è l'elenco delle
                importazioni in Impostazioni. */}
            {rapporto.missingTracks.length > 0 && (
              <details className="non-ritrovati">
                <summary>
                  {rapporto.missing}{" "}
                  {rapporto.missing === 1 ? "brano non è" : "brani non sono"} in
                  libreria
                </summary>
                <p>
                  Confermando, questi si scaricano da YouTube uno per uno, con i
                  tag di Spotify scritti sopra. Se invece i file ci sono già ma
                  in una cartella non sorvegliata, conviene aggiungerla e rifare
                  una scansione — poi di nuovo qui: si scaricherà solo quel che
                  manca davvero.
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
            Annulla
          </button>
          {anteprima ? (
            <button
              type="button"
              className="bottone primario"
              // `degradato`: importare zero brani riesce, ed è il guasto
              // peggiore possibile qui perché assomiglia in tutto a un
              // successo. Meglio un tasto spento e una frase che lo spiega.
              disabled={inCorso || piano === null || degradato}
              onClick={() => void esegui()}
            >
              {inCorso ? "Importazione…" : "Importa"}
            </button>
          ) : (
            <button
              type="button"
              className="bottone primario"
              disabled={inCorso || url.trim() === ""}
              onClick={() => void guarda()}
            >
              {inCorso ? "Lettura…" : "Guarda"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
