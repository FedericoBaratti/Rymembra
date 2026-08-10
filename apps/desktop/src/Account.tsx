/**
 * L'account Spotify intero, in una schermata sola.
 *
 * # Perché una sola, e non due
 *
 * Perché le due vie — il consenso OAuth e l'archivio che Spotify manda per
 * posta — producono di là lo **stesso** valore, e da quel punto in giù il codice
 * è uno solo. Due schermate sarebbero due copie della stessa cosa che divergono
 * il giorno in cui qualcuno ne aggiusta una: la scelta della via è un
 * interruttore in cima, non un altro posto in cui andare.
 *
 * # Tre tempi, come per ogni operazione che non si disfa
 *
 * Anteprima → piano → conferma, la stessa forma di `Importa` e `Ripristino`. Qui
 * pesa più che altrove, perché quel che sta per succedere è la scrittura meno
 * reversibile dell'applicazione: decine di migliaia di righe di cronologia in
 * mezzo a quelle vere. Il piano è l'importazione vera dentro una transazione
 * abbandonata, quindi i numeri che mostra sono quelli che si otterranno — non
 * una previsione che poi può smentirsi.
 *
 * # Le due cose che vanno dette prima, non dopo
 *
 * **I brani mancanti**: sono l'unica cosa che l'utente non può ricostruire dopo,
 * e vanno elencati per nome.
 *
 * **Il Premium**: dal febbraio 2026 un'applicazione Spotify in Development Mode
 * smette di funzionare quando chi l'ha registrata perde l'abbonamento, e Spotify
 * non manda nessun avviso. Se lo si scopre al primo guasto, lo si scopre come
 * «Aether non funziona».
 */
import { useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";

import {
  ipc,
  mancantiAccount,
  testoErrore,
  totaliAccount,
  vuotoAccount,
  type AnteprimaAccount,
  type AvanzamentoAccount,
  type EsitoAccount,
  type ScelteAccount,
  type StatoAccount,
} from "./ipc";

/** Tutto acceso: chi preme «importa il mio account» vuole il suo account. */
const TUTTO: ScelteAccount = {
  playlist: true,
  preferiti: true,
  album: true,
  artisti: true,
  cronologia: true,
};

/** Come si legge una fase dell'avanzamento. */
const FASI: Record<string, string> = {
  profilo: "Chi sei",
  preferiti: "Brani che ti piacciono",
  album: "Album salvati",
  artisti: "Artisti seguiti",
  playlist: "Playlist",
  cronologia: "Ascolti recenti",
};

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
      <span className="conteggio">{valore.toLocaleString("it")}</span>
      {nota && <small>{nota}</small>}
    </div>
  );
}

/** Una casella di scelta, con la sua ragione sotto. */
function Casella({
  etichetta,
  spiegazione,
  acceso,
  quanti,
  onCambia,
}: {
  etichetta: string;
  spiegazione: string;
  acceso: boolean;
  quanti: number;
  onCambia: (valore: boolean) => void;
}) {
  return (
    <label className="riga-opzione">
      <div className="che-cosa">
        <div className="etichetta">
          {etichetta} <span className="conteggio">{quanti.toLocaleString("it")}</span>
        </div>
        <div className="spiegazione">{spiegazione}</div>
      </div>
      <input
        type="checkbox"
        checked={acceso}
        disabled={quanti === 0}
        onChange={(e) => onCambia(e.target.checked)}
      />
    </label>
  );
}

export function Account({
  onChiudi,
  onImportato,
}: {
  onChiudi: () => void;
  onImportato: (esito: EsitoAccount) => void;
}) {
  const [stato, setStato] = useState<StatoAccount | null>(null);
  const [anteprima, setAnteprima] = useState<AnteprimaAccount | null>(null);
  const [scelte, setScelte] = useState<ScelteAccount>(TUTTO);
  const [piano, setPiano] = useState<EsitoAccount | null>(null);
  const [avanzamento, setAvanzamento] = useState<AvanzamentoAccount | null>(null);
  const [clientId, setClientId] = useState("");
  const [errore, setErrore] = useState<string | null>(null);
  const [inCorso, setInCorso] = useState(false);
  const [dimenticati, setDimenticati] = useState<number | null>(null);

  // Lo stato è la prima cosa: decide che cosa mostrare — la casella del client
  // id, il tasto «Collega», o direttamente quel che è già in cella da una
  // sessione precedente della stessa finestra.
  const caricaStato = () => {
    ipc
      .accountStato()
      .then((s) => {
        setStato(s);
        setClientId(s.clientId ?? "");
        if (s.caricato) setAnteprima(s.caricato);
      })
      .catch((e: unknown) => setErrore(testoErrore(e)));
  };
  const primoGiro = useRef(true);
  useEffect(() => {
    if (!primoGiro.current) return;
    primoGiro.current = false;
    caricaStato();
  }, []);

  // L'evento che nel resto dell'applicazione sarebbe rimasto orfano. Leggere un
  // account da duecento playlist sono duecento richieste: senza qualcosa che
  // avanzi, chi guarda non ha modo di distinguere «sta lavorando» da «si è
  // piantato».
  useEffect(() => {
    const promessa = listen<AvanzamentoAccount>("account:avanzamento", (e) =>
      setAvanzamento(e.payload),
    );
    return () => {
      void promessa.then((stacca) => stacca());
    };
  }, []);

  // Il piano si rifà a ogni cambio delle caselle: non tocca la rete — quel che
  // c'è da importare è già letto di là — e mostrare un piano calcolato con
  // scelte diverse sarebbe mostrare qualcosa che l'importazione poi non fa.
  useEffect(() => {
    if (!anteprima) return;
    let annullato = false;
    setInCorso(true);
    ipc
      .accountPiano(scelte)
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
  }, [anteprima, scelte]);

  const salvaClientId = async () => {
    setErrore(null);
    try {
      setStato(await ipc.accountCredenziali(clientId.trim()));
    } catch (e) {
      setErrore(testoErrore(e));
    }
  };

  const collega = async () => {
    setInCorso(true);
    setErrore(null);
    try {
      setStato(await ipc.accountCollega());
    } catch (e) {
      setErrore(testoErrore(e));
    } finally {
      setInCorso(false);
    }
  };

  const scollega = async () => {
    setErrore(null);
    try {
      setStato(await ipc.accountScollega());
      setAnteprima(null);
      setPiano(null);
    } catch (e) {
      setErrore(testoErrore(e));
    }
  };

  const leggi = async () => {
    setInCorso(true);
    setErrore(null);
    setPiano(null);
    setAvanzamento(null);
    try {
      setAnteprima(await ipc.accountLeggi());
    } catch (e) {
      setErrore(testoErrore(e));
    } finally {
      setInCorso(false);
      setAvanzamento(null);
    }
  };

  /**
   * Apre uno degli zip che Spotify manda.
   *
   * Ne manda due, in momenti diversi: i dati dell'account in qualche giorno, la
   * cronologia estesa fino a trenta. Se ne può aprire uno solo — quel che manca
   * resta vuoto — e questa è la ragione per cui l'anteprima elenca i file letti:
   * è l'unico modo di capire quale dei due si è appena aperto.
   */
  const apriArchivio = async () => {
    const scelta = await open({
      multiple: false,
      filters: [{ name: "Archivio Spotify", extensions: ["zip"] }],
    });
    if (typeof scelta !== "string") return;
    setInCorso(true);
    setErrore(null);
    setPiano(null);
    try {
      setAnteprima(await ipc.archivioApri(scelta));
    } catch (e) {
      setErrore(testoErrore(e));
    } finally {
      setInCorso(false);
    }
  };

  /**
   * Scrive, e poi se ne va.
   *
   * `onImportato` prima di `onChiudi`, come in `Importa`: è la consegna a chi
   * seguirà l'importazione, e farla dopo la chiusura vorrebbe dire consegnarla
   * da un componente che non c'è più. Su errore non si chiude niente: il
   * messaggio va letto dove si è premuto.
   */
  const importa = async () => {
    setInCorso(true);
    setErrore(null);
    try {
      onImportato(await ipc.accountImporta(scelte));
      onChiudi();
    } catch (e) {
      setErrore(testoErrore(e));
      setInCorso(false);
    }
  };

  const dimentica = async () => {
    setErrore(null);
    try {
      setDimenticati(await ipc.cronologiaDimenticaImportati());
      caricaStato();
    } catch (e) {
      setErrore(testoErrore(e));
    }
  };

  const totali = piano ? totaliAccount(piano) : null;
  const mancanti = piano ? mancantiAccount(piano) : [];
  const niente = anteprima !== null && vuotoAccount(anteprima);

  return (
    <div className="velo scuro" onClick={onChiudi}>
      <div
        /* Larghezza normale e non `larga`: qui dentro c'è testo da leggere, e
           una riga di prosa lunga novecento pixel si legge peggio — l'occhio
           perde il capo della riga successiva. `larga` serve al riordino, che
           mostra percorsi che non si possono accorciare. */
        className="finestrella"
        role="dialog"
        aria-modal="true"
        aria-label="Importa il tuo account Spotify"
        onClick={(e) => e.stopPropagation()}
      >
        <h2>
          {anteprima ? "Cosa verrebbe importato" : "Il tuo account Spotify"}
        </h2>

        {!anteprima && (
          <>
            <p className="nota">
              Due strade, e portano nello stesso posto. L&apos;
              <strong>archivio</strong> non chiede niente a nessuno ma ci mette
              giorni ad arrivare, e contiene <strong>tutti</strong> i tuoi
              ascolti. Il <strong>collegamento</strong> è immediato e porta
              l&apos;ISRC — con cui i brani si ritrovano meglio — ma degli
              ascolti dà solo gli ultimi cinquanta.
            </p>

            {/* ── via B: l'archivio ── */}
            <section className="scheda">
              <header>
                <h3>Dall&apos;archivio</h3>
                <span className="nota-testa">niente account, niente chiavi</span>
              </header>
              <p className="nota">
                Chiedi i tuoi dati su{" "}
                <code>spotify.com/account/privacy</code>: spunta anche{" "}
                <em>«Cronologia di streaming estesa»</em> se vuoi gli ascolti di
                tutti gli anni. Arrivano per email in due zip separati — apri
                pure uno solo, quel che manca resta vuoto.
              </p>
              <div className="azioni">
                <button
                  type="button"
                  className="bottone btn-ghost"
                  disabled={inCorso}
                  onClick={() => void apriArchivio()}
                >
                  Scegli l&apos;archivio…
                </button>
              </div>
            </section>

            {/* ── via A: il consenso ── */}
            <section className="scheda">
              <header>
                <h3>Collegando l&apos;account</h3>
                <span className="nota-testa">
                  {stato?.collegato ? "collegato" : "serve un Client ID"}
                </span>
              </header>

              {!stato?.collegato && (
                <>
                  <p className="nota">
                    Serve un&apos;applicazione tua su{" "}
                    <code>developer.spotify.com/dashboard</code>, con{" "}
                    <code>http://127.0.0.1</code> come <em>Redirect URI</em> —
                    senza porta. Aether non ne porta una dentro apposta: in
                    Development Mode ognuna accetta <strong>cinque</strong>{" "}
                    utenti, e una chiave nel programma la esaurirebbero i primi
                    cinque che lo installano.
                  </p>
                  {/* Prima del collegamento e non dopo il primo guasto: quando
                      l'abbonamento scade, l'applicazione smette di funzionare e
                      Spotify non avvisa nessuno. */}
                  <p className="nota">
                    Da febbraio 2026 chi registra l&apos;applicazione deve avere{" "}
                    <strong>Spotify Premium attivo</strong>: se scade, smette di
                    funzionare senza avviso. L&apos;archivio non ha questa
                    dipendenza.
                  </p>
                  <label>
                    Client ID
                    <input
                      type="text"
                      className="campo"
                      placeholder="32 caratteri esadecimali"
                      value={clientId}
                      onChange={(e) => setClientId(e.target.value)}
                      onBlur={() => void salvaClientId()}
                    />
                  </label>
                </>
              )}

              <div className="azioni">
                {stato?.collegato ? (
                  <>
                    <button
                      type="button"
                      className="bottone"
                      disabled={inCorso}
                      onClick={() => void leggi()}
                    >
                      {inCorso ? "Lettura…" : "Leggi il mio account"}
                    </button>
                    <button
                      type="button"
                      className="bottone btn-ghost"
                      disabled={inCorso}
                      onClick={() => void scollega()}
                    >
                      Scollega
                    </button>
                  </>
                ) : (
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    disabled={inCorso || clientId.trim() === ""}
                    onClick={() => void collega()}
                  >
                    {inCorso ? "Aspetto il consenso…" : "Collega l'account"}
                  </button>
                )}
              </div>
              {inCorso && !stato?.collegato && (
                <p className="nota">
                  Ho aperto il consenso nel browser di sistema. Hai tre minuti.
                </p>
              )}
            </section>

            {/* Sta qui e non fra le scelte: è l'unico modo di disfare, e chi
                lo cerca lo cerca quando ha già importato. */}
            {stato !== null && stato.ascoltiImportati > 0 && (
              <section className="scheda">
                <header>
                  <h3>Ascolti già importati</h3>
                  <span className="nota-testa">
                    {stato.ascoltiImportati.toLocaleString("it")} righe
                  </span>
                </header>
                <p className="nota">
                  Restano distinguibili da quelli veri, e si possono togliere. I
                  conteggi d&apos;ascolto però <strong>non scendono</strong>:
                  quel numero è l&apos;unica cosa in libreria che non si può
                  ricostruire, e una funzione che lo abbassa prima o poi lo
                  abbassa quando non doveva.
                </p>
                {dimenticati !== null && (
                  <p className="nota">
                    Tolte {dimenticati.toLocaleString("it")} righe.
                  </p>
                )}
                <div className="azioni">
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    onClick={() => void dimentica()}
                  >
                    Dimentica gli ascolti importati
                  </button>
                </div>
              </section>
            )}
          </>
        )}

        {avanzamento && (
          <p className="nota">
            {FASI[avanzamento.fase] ?? avanzamento.fase}
            {avanzamento.totali !== null && avanzamento.totali > 1
              ? ` ${avanzamento.fatti + 1}/${avanzamento.totali}`
              : "…"}
            {avanzamento.nome && ` — ${avanzamento.nome}`}
          </p>
        )}

        {errore && <div className="errore">{errore}</div>}

        {anteprima && (
          <>
            <div className="anteprima-spotify">
              <div className="che-cosa">
                <div className="titolo">
                  {anteprima.profilo ?? "Account Spotify"}
                </div>
                <div className="nota">
                  {anteprima.provenienza === "archivio"
                    ? "dall'archivio"
                    : "dal collegamento"}
                  {anteprima.premium === false &&
                    " · questo account non ha Premium"}
                </div>
              </div>
            </div>

            {/* Prima delle scelte: sono le cose che spiegano un numero più
                basso di quel che uno si aspetta, e leggerle dopo vorrebbe dire
                credere per qualche secondo che l'importazione abbia sbagliato. */}
            {!anteprima.cronologiaCompleta && anteprima.cronologia > 0 && (
              <div className="avviso-monco">
                Della cronologia il collegamento dà solo gli{" "}
                <strong>ultimi {anteprima.cronologia}</strong> ascolti: è tutto
                quel che Spotify espone da lì. Gli anni precedenti stanno
                soltanto nell&apos;archivio.
              </div>
            )}
            {anteprima.senzaContenuto.length > 0 && (
              <div className="avviso-monco">
                Di {anteprima.senzaContenuto.length} playlist Spotify non dà più
                i brani:{" "}
                {anteprima.senzaContenuto.length === 1
                  ? "è una che segui"
                  : "sono quelle che segui"}{" "}
                e non possiedi. Le altre ci sono tutte.
              </div>
            )}
            {anteprima.troncati.length > 0 && (
              <div className="avviso-monco">
                Questi elenchi sono arrivati a metà:{" "}
                {anteprima.troncati.join(", ")}. Verrebbe importato meno di
                quello che hai.
              </div>
            )}
            {anteprima.illeggibili.length > 0 && (
              <div className="avviso-monco">
                {anteprima.illeggibili.length} file dell&apos;archivio non si
                sono aperti:{" "}
                {anteprima.illeggibili.map((f) => f.nome).join(", ")}. Il resto
                è stato letto lo stesso.
              </div>
            )}
            {niente && (
              <div className="avviso-monco">
                Qui dentro non c&apos;è niente da importare. Se è un archivio,
                può essere l&apos;altro dei due che Spotify manda: cerca quello
                con dentro <code>Playlist1.json</code> o{" "}
                <code>Streaming_History_Audio</code>.
              </div>
            )}

            <div className="scelte-account">
              <Casella
                etichetta="Playlist"
                spiegazione={`${anteprima.braniInPlaylist.toLocaleString("it")} brani in tutto. Una playlist qui per ognuna di là; reimportare sostituisce invece di accodare.`}
                acceso={scelte.playlist}
                quanti={anteprima.playlist}
                onCambia={(v) => setScelte({ ...scelte, playlist: v })}
              />
              <Casella
                etichetta="Brani che ti piacciono"
                spiegazione="Diventano preferiti qui. Non una playlist: Aether ha già il suo posto per i preferiti."
                acceso={scelte.preferiti}
                quanti={anteprima.preferiti}
                onCambia={(v) => setScelte({ ...scelte, preferiti: v })}
              />
              <Casella
                etichetta="Album salvati"
                spiegazione="Attaccano l'identificativo Spotify ai dischi che hai già, così le edizioni dello stesso album si fondono."
                acceso={scelte.album}
                quanti={anteprima.album}
                onCambia={(v) => setScelte({ ...scelte, album: v })}
              />
              <Casella
                etichetta="Artisti seguiti"
                spiegazione="Solo per quelli che hai già in libreria: gli altri sparirebbero alla prima scansione."
                acceso={scelte.artisti}
                quanti={anteprima.artisti}
                onCambia={(v) => setScelte({ ...scelte, artisti: v })}
              />
              <Casella
                etichetta="Ascolti"
                spiegazione="Restano marcati come importati, e si possono togliere. Contano solo quelli sentiti per almeno metà brano, la stessa regola di quando suona Aether."
                acceso={scelte.cronologia}
                quanti={anteprima.cronologia}
                onCambia={(v) => setScelte({ ...scelte, cronologia: v })}
              />
            </div>

            {inCorso && !piano && <p>Calcolo di quel che succederebbe…</p>}

            {piano && totali && (
              <>
                <div className="rapporto">
                  <Voce etichetta="Brani ritrovati" valore={totali.ritrovati} />
                  {/* Il conteggio **distinto**, non la somma per elenco: lo
                      stesso brano che manca da tre playlist è un brano che non
                      hai, non tre. La somma resta nella nota, perché è quella
                      che descrive la coda — lì una riga per sorgente c'è
                      davvero, ed è quel che permette di dire da dove manca. */}
                  <Voce
                    etichetta="Brani che non hai"
                    valore={mancanti.length}
                    nota={
                      totali.inCoda > 0
                        ? `${totali.inCoda} righe in coda di scaricamento`
                        : undefined
                    }
                  />
                  <Voce
                    etichetta="Preferiti da segnare"
                    valore={piano.likedMarked}
                    nota="quelli già segnati non si contano"
                  />
                  <Voce
                    etichetta="Ascolti da scrivere"
                    valore={piano.historyRows}
                    nota={
                      piano.historySkipped.duplicates +
                        piano.historySkipped.tooShort +
                        piano.historySkipped.notInLibrary >
                      0
                        ? `fuori: ${piano.historySkipped.duplicates} doppioni, ${piano.historySkipped.tooShort} troppo brevi, ${piano.historySkipped.notInLibrary} non in libreria`
                        : undefined
                    }
                  />
                  <Voce
                    etichetta="Artisti da collegare"
                    valore={piano.artistsLinked}
                  />
                  <Voce
                    etichetta="Album da collegare"
                    valore={piano.albumIdsWritten}
                  />
                </div>

                {piano.rejectedPlaylists.length > 0 && (
                  <div className="avviso-monco">
                    {piano.rejectedPlaylists.length} playlist verrebbero
                    saltate:{" "}
                    {piano.rejectedPlaylists
                      .map((p) => `«${p.name}»`)
                      .join(", ")}
                    . Le altre entrano lo stesso.
                  </div>
                )}

                {/* L'elenco per nome, e non un numero: sono l'unica cosa che
                    non si può ricostruire dopo. */}
                {mancanti.length > 0 && (
                  <details className="mancanti-account">
                    <summary>
                      {mancanti.length.toLocaleString("it")}{" "}
                      {mancanti.length === 1 ? "brano" : "brani"} che non hai
                    </summary>
                    <ul>
                      {mancanti.slice(0, 200).map((b) => (
                        <li key={`${b.artist ?? ""}|${b.title}`}>
                          {b.artist ? `${b.artist} — ` : ""}
                          {b.title}
                        </li>
                      ))}
                    </ul>
                    {mancanti.length > 200 && (
                      <p className="nota">
                        …e altri {(mancanti.length - 200).toLocaleString("it")}.
                      </p>
                    )}
                  </details>
                )}
              </>
            )}
          </>
        )}

        <div className="azioni">
          <button type="button" className="bottone btn-ghost" onClick={onChiudi}>
            {anteprima ? "Annulla" : "Chiudi"}
          </button>
          {anteprima && (
            <button
              type="button"
              className="bottone"
              disabled={inCorso || piano === null || niente}
              onClick={() => void importa()}
            >
              {inCorso ? "Importazione…" : "Importa"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
