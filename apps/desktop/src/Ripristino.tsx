/**
 * Il ripristino dal backup su Drive.
 *
 * È la schermata che si guarda dopo aver reinstallato il computer, ed è
 * costruita attorno a quel momento: si vede per esteso cosa tornerebbe indietro
 * prima che torni, si conferma con un tasto che dice cosa fa, e quel che non si
 * può rimettere si **elenca** invece di sparire.
 *
 * # Perché è quasi una copia di `Riordino.tsx`
 *
 * Deliberatamente. Sono le due sole schermate di Aether che cambiano qualcosa
 * senza poterlo disfare con un tasto, e non esiste un runner di test per il
 * TypeScript: la sola verifica strutturale possibile è che le due si possano
 * leggere come un diff. Una terza forma inventata da capo qui sarebbe una cosa
 * in più da rivedere riga per riga.
 *
 * # Cosa non fa
 *
 * Non tocca niente finché non si preme il tasto, e non si fida del piano che ha
 * mostrato: `nuvolaRipristina` riscarica e ricalcola. Fra il momento in cui si
 * guarda e quello in cui si conferma può essere finita una scansione.
 */
import { listen } from "@tauri-apps/api/event";
import { useEffect, useState } from "react";

import {
  ipc,
  testoErrore,
  type AvanzamentoNuvola,
  type CambioBrano,
  type EsitoRipristino,
  type PianoRipristino,
} from "./ipc";

/** Come si legge una fase dell'avanzamento. */
const FASI: Record<AvanzamentoNuvola["cosa"], string> = {
  metadati: "metadati",
  skin: "skin",
  bozze: "bozze dello Studio",
};

/** Un brano, nella forma in cui la chiave lo sa descrivere. */
function nomina(brano: CambioBrano): string {
  const pezzi = [brano.titolo, brano.artista, brano.album].filter(
    (p) => p.length > 0,
  );
  return pezzi.length > 0 ? pezzi.join(" · ") : "(senza tag)";
}

/** Il delta di un brano, nella forma «ascolti 3 → 17, voto — → ★★★★». */
function delta(brano: CambioBrano): string {
  const parti: string[] = [];
  if (brano.ascoltiDopo !== brano.ascoltiPrima) {
    parti.push(`ascolti ${brano.ascoltiPrima} → ${brano.ascoltiDopo}`);
  }
  if (brano.votoDopo !== brano.votoPrima) {
    const stelle = (n: number) => (n > 0 ? "★".repeat(n) : "—");
    parti.push(`voto ${stelle(brano.votoPrima)} → ${stelle(brano.votoDopo)}`);
  }
  if (brano.preferitoDopo) parti.push("preferito");
  return parti.join(", ");
}

/** La data di un backup, come si legge. */
function quando(ms: number): string {
  if (ms <= 0) return "data sconosciuta";
  return new Date(ms).toLocaleString();
}

export function Ripristino({
  onChiudi,
  onFatto,
}: {
  onChiudi: () => void;
  onFatto: () => void;
}) {
  const [piano, setPiano] = useState<PianoRipristino | null>(null);
  const [esito, setEsito] = useState<EsitoRipristino | null>(null);
  const [errore, setErrore] = useState<string | null>(null);
  const [inCorso, setInCorso] = useState(true);
  const [avanzamento, setAvanzamento] = useState<AvanzamentoNuvola | null>(null);

  useEffect(() => {
    const promessa = listen<AvanzamentoNuvola>(
      "nuvola:avanzamento",
      (evento) => setAvanzamento(evento.payload),
    );
    return () => {
      void promessa.then((stop) => stop());
    };
  }, []);

  useEffect(() => {
    let annullato = false;
    ipc
      .nuvolaPianoRipristino()
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
  }, []);

  const esegui = async () => {
    setInCorso(true);
    setErrore(null);
    try {
      setEsito(await ipc.nuvolaRipristina());
      onFatto();
    } catch (e) {
      setErrore(testoErrore(e));
    } finally {
      setInCorso(false);
      setAvanzamento(null);
    }
  };

  const percentuale =
    avanzamento && avanzamento.totale > 0
      ? Math.round((avanzamento.fatti / avanzamento.totale) * 100)
      : 0;

  return (
    <div className="velo scuro" onClick={inCorso ? undefined : onChiudi}>
      <div
        className="finestrella larga"
        role="dialog"
        aria-modal="true"
        aria-label="Ripristina dal backup"
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{esito ? "Ripristino concluso" : "Ripristina dal backup"}</h2>
        {piano?.cEUnBackup && (
          <div className="percorso">Backup del {quando(piano.generatoMs)}</div>
        )}

        {errore && <div className="errore">{errore}</div>}

        {avanzamento && (
          <>
            <div className="avanzamento">
              <div style={{ width: `${percentuale}%` }} />
            </div>
            <div className="conteggio">
              {FASI[avanzamento.cosa]} {avanzamento.fatti} /{" "}
              {avanzamento.totale}
            </div>
          </>
        )}

        {!piano && !esito && inCorso && <p>Scarico il backup…</p>}

        {esito && (
          <>
            <div className="rapporto">
              <div className="voce-rapporto">
                <span>Brani ripristinati</span>
                <span className="conteggio">{esito.brani}</span>
              </div>
              <div className="voce-rapporto">
                <span>Playlist</span>
                <span className="conteggio">{esito.playlist}</span>
              </div>
              <div className="voce-rapporto">
                <span>Cartelle sorvegliate aggiunte</span>
                <span className="conteggio">{esito.cartelle}</span>
              </div>
              <div className="voce-rapporto">
                <span>Skin installate</span>
                <span className="conteggio">{esito.skin}</span>
              </div>
              <div className="voce-rapporto">
                <span>Bozze dello Studio</span>
                <span className="conteggio">{esito.bozze}</span>
              </div>
            </div>
            {esito.mancanti.length > 0 && (
              <details className="non-ritrovati" open>
                <summary>
                  {esito.mancanti.length} file non erano su Drive
                </summary>
                <p>
                  Succede quando un salvataggio si è interrotto fra i metadati e
                  i pacchetti. Il resto è stato ripristinato: il prossimo
                  salvataggio li rimanda su.
                </p>
                <ul>
                  {esito.mancanti.slice(0, 100).map((m) => (
                    <li key={m}>{m}</li>
                  ))}
                </ul>
              </details>
            )}
            {esito.cartelle > 0 && (
              <p>
                Sono tornate delle cartelle sorvegliate.{" "}
                <strong>Fai una scansione</strong>: i brani che erano lì dentro
                rientrano in libreria e ritrovano le loro statistiche.
              </p>
            )}
          </>
        )}

        {piano && !esito && !piano.cEUnBackup && (
          <p>
            Su Drive non c&apos;è ancora nessun backup. Collega un account dalle
            Impostazioni e lascia passare un salvataggio.
          </p>
        )}

        {piano && !esito && piano.cEUnBackup && (
          <>
            <div className="rapporto">
              <div className="voce-rapporto">
                <span>Brani da aggiornare</span>
                <span className="conteggio">{piano.braniDaAggiornare}</span>
              </div>
              <div className="voce-rapporto">
                <span>Brani già a posto</span>
                <span className="conteggio">{piano.braniInvariati}</span>
              </div>
              <div className="voce-rapporto">
                <span>Playlist da scrivere</span>
                <span className="conteggio">{piano.playlist.length}</span>
              </div>
              <div className="voce-rapporto">
                <span>Skin da installare</span>
                <span className="conteggio">
                  {piano.skinDaInstallare.length}
                </span>
              </div>
              <div className="voce-rapporto">
                <span>Bozze da scrivere</span>
                <span className="conteggio">{piano.bozzeDaScrivere.length}</span>
              </div>
            </div>

            {piano.vuoto && (
              <p>
                Non c&apos;è niente da ripristinare: quello che sta nel backup è
                già qui.
              </p>
            )}

            {piano.assentiTotale > 0 && (
              <details className="non-ritrovati">
                <summary>
                  {piano.assentiTotale} brani del backup non hanno un file qui
                </summary>
                <p>
                  Il ripristino <strong>non li ricrea</strong>: una riga senza
                  file non si può aprire, e la scansione successiva la
                  toglierebbe. Sono elencati perché tu sappia cosa andare a
                  ricercare. I nomi sono nella forma normalizzata dei tag.
                </p>
                <ul>
                  {piano.assenti.map((b) => (
                    <li key={`${b.artista}|${b.titolo}|${b.album}`}>
                      {nomina(b)}
                    </li>
                  ))}
                </ul>
                {piano.assentiTotale > piano.assenti.length && (
                  <p>
                    Mostrati i primi {piano.assenti.length} di{" "}
                    {piano.assentiTotale}.
                  </p>
                )}
              </details>
            )}

            {piano.playlist.length > 0 && (
              <details className="non-ritrovati" open>
                <summary>{piano.playlist.length} playlist</summary>
                <ul>
                  {piano.playlist.map((p) => (
                    <li key={p.nome}>
                      «{p.nome}» — {p.daCreare ? "da creare" : "da aggiornare"}
                      {p.automatica
                        ? ", automatica: riceve le regole, non i brani"
                        : `, ${p.braniQui} brani su ${p.braniNelBackup} presenti qui`}
                    </li>
                  ))}
                </ul>
              </details>
            )}

            {piano.cartelle.length > 0 && (
              <details className="non-ritrovati" open>
                <summary>
                  {piano.cartelle.length} cartelle sorvegliate da aggiungere
                </summary>
                <ul>
                  {piano.cartelle.map((c) => (
                    <li key={c.percorso}>
                      {c.percorso}
                      {c.esiste ? "" : " — non esiste più su questo disco"}
                    </li>
                  ))}
                </ul>
              </details>
            )}

            {(piano.skinDaInstallare.length > 0 ||
              piano.bozzeDaScrivere.length > 0) && (
              <details className="non-ritrovati">
                <summary>
                  {piano.skinDaInstallare.length} skin e{" "}
                  {piano.bozzeDaScrivere.length} bozze
                </summary>
                <p>
                  Quelle che ci sono già non vengono toccate:{" "}
                  {piano.skinPresenti} skin e {piano.bozzePresenti} bozze restano
                  come sono.
                </p>
                <ul>
                  {piano.skinDaInstallare.map((s) => (
                    <li key={`skin-${s}`}>skin «{s}»</li>
                  ))}
                  {piano.bozzeDaScrivere.map((b) => (
                    <li key={`bozza-${b}`}>bozza «{b}»</li>
                  ))}
                </ul>
                {piano.skinAttiva !== null && (
                  <p>La skin attiva diventerà «{piano.skinAttiva}».</p>
                )}
              </details>
            )}

            {piano.cambi.length > 0 && (
              <details className="spostamenti" open>
                <summary>{piano.braniDaAggiornare} brani da aggiornare</summary>
                <ul>
                  {piano.cambi.map((b) => (
                    <li key={`${b.artista}|${b.titolo}|${b.album}`}>
                      <span className="da">{nomina(b)}</span>
                      <span className="freccia" aria-hidden="true">
                        ↦
                      </span>
                      <span className="a">{delta(b)}</span>
                    </li>
                  ))}
                </ul>
                {piano.braniDaAggiornare > piano.cambi.length && (
                  <p>
                    Mostrati i primi {piano.cambi.length} di{" "}
                    {piano.braniDaAggiornare}. Verranno aggiornati tutti.
                  </p>
                )}
              </details>
            )}
          </>
        )}

        <div className="tasti-finestrella">
          <button
            type="button"
            className="bottone"
            disabled={inCorso}
            onClick={onChiudi}
          >
            {esito ? "Chiudi" : "Non fare niente"}
          </button>
          {!esito && (
            <button
              type="button"
              className="bottone primario"
              disabled={inCorso || !piano || piano.vuoto}
              onClick={() => void esegui()}
            >
              {inCorso ? "Ripristino…" : "Ripristina"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
