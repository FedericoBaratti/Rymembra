/**
 * L'account Spotify intero, dal solo posto da cui si può prendere.
 *
 * # Una via sola, e perché
 *
 * L'archivio che Spotify consegna su richiesta. Il consenso OAuth alla Web API
 * c'era, funzionava, ed è stato tolto: quel che si può fare dei dati che quella
 * restituisce lo decide lo *Spotify Developer Policy*, e una libreria musicale
 * che tiene per anni le playlist e la cronologia di qualcuno non sta dentro
 * quei limiti. L'archivio no: è dell'utente per diritto (GDPR art. 20), e
 * portarselo dove vuole è esattamente ciò che quell'articolo gli riconosce.
 *
 * Il ragionamento per esteso, con quel che il confronto fra le due strade
 * diceva, sta in `parti/ArchivioSpotify.tsx`.
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
 * # La cosa che va detta prima, non dopo
 *
 * **I brani mancanti**: sono l'unica cosa che l'utente non può ricostruire
 * dopo, e vanno elencati per nome. Da qui finiscono nella coda, che li cerca
 * nei cataloghi liberi; quelli che nessun catalogo libero ha finiscono in «Da
 * comprare», che è la risposta onesta e non un guasto.
 */
import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";

import {
  ipc,
  mancantiAccount,
  testoErrore,
  totaliAccount,
  vuotoAccount,
  type AnteprimaAccount,
  type EsitoAccount,
  type ScelteAccount,
  type StatoAccount,
} from "./ipc";
import { useFinestrella } from "./finestrella";
import { ComeAvereLArchivio } from "./parti/ArchivioSpotify";
import { Avviso } from "./parti/Avvisi";
import { numero } from "./formato";
import { t } from "./lingue";
import { Trans } from "./lingue/Trans";

/** Tutto acceso: chi preme «importa il mio account» vuole il suo account. */
const TUTTO: ScelteAccount = {
  playlist: true,
  preferiti: true,
  album: true,
  artisti: true,
  cronologia: true,
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
      <span className="conteggio">{numero(valore)}</span>
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
  // A zero la casella resta **visibile** e spenta, e dice perché. Toglierla
  // farebbe sparire una riga fra cinque senza spiegazione — chi sa di avere
  // degli album salvati e non trova la riga «Album salvati» conclude che
  // l'applicazione non li importa, non che l'archivio non li portava.
  const vuota = quanti === 0;
  return (
    <label className="riga-opzione">
      <div className="che-cosa">
        <div className="etichetta">
          {etichetta} <span className="conteggio">{numero(quanti)}</span>
        </div>
        <div className="spiegazione">
          {vuota ? t("account.empty.hint") : spiegazione}
        </div>
      </div>
      <input
        type="checkbox"
        checked={acceso && !vuota}
        disabled={vuota}
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
  const [errore, setErrore] = useState<string | null>(null);
  const [inCorso, setInCorso] = useState(false);
  const [dimenticati, setDimenticati] = useState<number | null>(null);
  const finestrella = useFinestrella<HTMLDivElement>(onChiudi);

  // Lo stato è la prima cosa: decide che cosa mostrare — le istruzioni per
  // ottenere l'archivio, o direttamente quel che è già in cella da un giro
  // precedente della stessa sessione.
  const caricaStato = () => {
    ipc
      .accountStato()
      .then((s) => {
        setStato(s);
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
      filters: [{ name: t("account.file"), extensions: ["zip"] }],
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
        ref={finestrella}
        /* Larghezza normale e non `larga`: qui dentro c'è testo da leggere, e
           una riga di prosa lunga novecento pixel si legge peggio — l'occhio
           perde il capo della riga successiva. `larga` serve al ripristino, che
           mostra percorsi che non si possono accorciare. */
        className="finestrella glass-modal"
        role="dialog"
        aria-modal="true"
        aria-label={t("account.aria")}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{anteprima ? t("account.title.preview") : t("account.title")}</h2>

        {!anteprima && (
          <>
            <ComeAvereLArchivio onApriArchivio={() => void apriArchivio()} />

            {/* Sotto le istruzioni e non sopra: è l'unico modo di disfare, e
                chi lo cerca lo cerca quando ha già importato — cioè quando ha
                già letto tutto il resto una volta e non lo rilegge. */}
            {stato !== null && stato.ascoltiImportati > 0 && (
              <section className="scheda">
                <header>
                  <h3>{t("account.already")}</h3>
                  <span className="nota-testa">
                    {t("account.already.rows", { n: stato.ascoltiImportati })}
                  </span>
                </header>
                <p className="nota">
                  <Trans
                    k="account.already.note"
                    v={{
                      non: <strong>{t("account.already.note.not")}</strong>,
                    }}
                  />
                </p>
                {dimenticati !== null && (
                  <p className="nota">
                    {t("account.forgotten", { n: dimenticati })}
                  </p>
                )}
                <div className="azioni">
                  <button
                    type="button"
                    className="bottone btn-ghost"
                    onClick={() => void dimentica()}
                  >
                    {t("account.forget")}
                  </button>
                </div>
              </section>
            )}
          </>
        )}

        {inCorso && !anteprima && <p>{t("account.opening")}</p>}

        {errore && <div className="errore">{errore}</div>}

        {anteprima && (
          <>
            <div className="scheda-anteprima">
              <div className="che-cosa">
                <div className="titolo">
                  {anteprima.profilo ?? t("account.unnamed")}
                </div>
                <div className="nota">
                  {anteprima.provenienza === "archivio"
                    ? t("account.from.archive")
                    : t("account.from.earlier")}
                </div>
              </div>
            </div>

            {/* Prima delle scelte: sono le cose che spiegano un numero più
                basso di quel che uno si aspetta, e leggerle dopo vorrebbe dire
                credere per qualche secondo che l'importazione abbia sbagliato. */}
            {/* Nota, e con un tasto: dei due zip che Spotify manda, questo è
                quello senza la cronologia estesa. Non è un guasto — è l'altro
                file, che arriva settimane dopo — e il gesto che ripara è
                aprirlo quando arriva, sopra questo. */}
            {anteprima.cronologia === 0 && anteprima.playlist > 0 && (
              <Avviso
                livello="nota"
                azione={
                  <button
                    type="button"
                    className="bottone minuto btn-ghost"
                    disabled={inCorso}
                    onClick={() => void apriArchivio()}
                  >
                    {t("account.otherArchive.cta")}
                  </button>
                }
              >
                {t("account.otherArchive")}
              </Avviso>
            )}
            {/* Nota: il resto è stato letto, e non c'è niente da premere. I nomi
                oltre i tre stanno in un `<details>`: un archivio con quaranta
                file illeggibili darebbe quaranta nomi in mezzo alla frase. */}
            {anteprima.illeggibili.length > 0 && (
              <Avviso livello="nota">
                {t("account.unreadable", { n: anteprima.illeggibili.length })}
                {anteprima.illeggibili.length <= 3 ? (
                  <span className="quali">
                    {anteprima.illeggibili.map((f) => f.nome).join(" · ")}
                  </span>
                ) : (
                  <details>
                    <summary>{t("account.unreadable.which")}</summary>
                    <ul className="quali-elenco">
                      {anteprima.illeggibili.map((f) => (
                        <li key={f.nome}>{f.nome}</li>
                      ))}
                    </ul>
                  </details>
                )}
              </Avviso>
            )}
            {/* Blocco: non c'è niente da importare, e continuare non porta da
                nessuna parte. La frase dice l'unica cosa che si può fare. */}
            {niente && (
              <Avviso livello="blocco">
                <Trans
                  k="account.nothing"
                  v={{
                    a: <code>Playlist1.json</code>,
                    b: <code>Streaming_History_Audio</code>,
                  }}
                />
              </Avviso>
            )}

            <div className="scelte-account">
              <Casella
                etichetta={t("account.pick.playlists")}
                spiegazione={t("account.pick.playlists.hint", {
                  n: anteprima.braniInPlaylist,
                })}
                acceso={scelte.playlist}
                quanti={anteprima.playlist}
                onCambia={(v) => setScelte({ ...scelte, playlist: v })}
              />
              <Casella
                etichetta={t("account.pick.liked")}
                spiegazione={t("account.pick.liked.hint")}
                acceso={scelte.preferiti}
                quanti={anteprima.preferiti}
                onCambia={(v) => setScelte({ ...scelte, preferiti: v })}
              />
              <Casella
                etichetta={t("account.pick.albums")}
                spiegazione={t("account.pick.albums.hint")}
                acceso={scelte.album}
                quanti={anteprima.album}
                onCambia={(v) => setScelte({ ...scelte, album: v })}
              />
              <Casella
                etichetta={t("account.pick.artists")}
                spiegazione={t("account.pick.artists.hint")}
                acceso={scelte.artisti}
                quanti={anteprima.artisti}
                onCambia={(v) => setScelte({ ...scelte, artisti: v })}
              />
              <Casella
                etichetta={t("account.pick.history")}
                spiegazione={t("account.pick.history.hint")}
                acceso={scelte.cronologia}
                quanti={anteprima.cronologia}
                onCambia={(v) => setScelte({ ...scelte, cronologia: v })}
              />
            </div>

            {inCorso && !piano && <p>{t("account.planning")}</p>}

            {piano && totali && (
              <>
                <div className="rapporto">
                  <Voce
                    etichetta={t("account.found")}
                    valore={totali.ritrovati}
                  />
                  {/* Il conteggio **distinto**, non la somma per elenco: lo
                      stesso brano che manca da tre playlist è un brano che non
                      hai, non tre. La somma resta nella nota, perché è quella
                      che descrive la coda — lì una riga per sorgente c'è
                      davvero, ed è quel che permette di dire da dove manca. */}
                  <Voce
                    etichetta={t("account.missing")}
                    valore={mancanti.length}
                    nota={
                      totali.inCoda > 0
                        ? t("account.missing.queue", { n: totali.inCoda })
                        : undefined
                    }
                  />
                  <Voce
                    etichetta={t("account.toLike")}
                    valore={piano.likedMarked}
                    nota={t("account.toLike.note")}
                  />
                  <Voce
                    etichetta={t("account.toWrite")}
                    valore={piano.historyRows}
                    nota={
                      piano.historySkipped.duplicates +
                        piano.historySkipped.tooShort +
                        piano.historySkipped.notInLibrary >
                      0
                        ? t("account.skipped", {
                            doppioni: piano.historySkipped.duplicates,
                            brevi: piano.historySkipped.tooShort,
                            fuori: piano.historySkipped.notInLibrary,
                          })
                        : undefined
                    }
                  />
                  <Voce
                    etichetta={t("account.artistsLink")}
                    valore={piano.artistsLinked}
                  />
                  <Voce
                    etichetta={t("account.albumsLink")}
                    valore={piano.albumIdsWritten}
                  />
                </div>

                {/* Nota: è il nucleo che le rifiuta per una ragione sua — vedi
                    `import_account` — e non c'è un tasto che le faccia entrare.
                    Un ambra qui prometterebbe che ci sia. */}
                {piano.rejectedPlaylists.length > 0 && (
                  <Avviso livello="nota">
                    {t("account.rejected", {
                      n: piano.rejectedPlaylists.length,
                    })}{" "}
                    <span className="quali">
                      {piano.rejectedPlaylists
                        .map((p) => `«${p.name}»`)
                        .join(" · ")}
                    </span>
                  </Avviso>
                )}

                {/* L'elenco per nome, e non un numero: sono l'unica cosa che
                    non si può ricostruire dopo. */}
                {mancanti.length > 0 && (
                  <details className="mancanti-account">
                    <summary>
                      {t("account.missingList", { n: mancanti.length })}
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
                        {t("account.andMore", { n: mancanti.length - 200 })}
                      </p>
                    )}
                  </details>
                )}
              </>
            )}
          </>
        )}

        <div className="azioni">
          <button
            type="button"
            className="bottone btn-ghost"
            onClick={onChiudi}
          >
            {anteprima ? t("common.cancel") : t("account.close")}
          </button>
          {anteprima && (
            <button
              type="button"
              className="bottone"
              disabled={inCorso || piano === null || niente}
              onClick={() => void importa()}
            >
              {inCorso ? t("account.importing") : t("account.import")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
