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
import { useEffect, useState } from "react";

import {
  ipc,
  testoErrore,
  type AvanzamentoNuvola,
  type CambioBrano,
  type EsitoRipristino,
  type PianoRipristino,
} from "./ipc";
import { t } from "./lingue";
import { Trans } from "./lingue/Trans";
import { dataOra } from "./formato";
import { useAscolto } from "./pagine";

/** Come si legge una fase dell'avanzamento. */
function fasi(): Record<AvanzamentoNuvola["cosa"], string> {
  return {
    metadati: t("restore.phase.metadati"),
    skin: t("restore.phase.skin"),
    bozze: t("restore.phase.bozze"),
  };
}

/** Un brano, nella forma in cui la chiave lo sa descrivere. */
function nomina(brano: CambioBrano): string {
  const pezzi = [brano.titolo, brano.artista, brano.album].filter(
    (p) => p.length > 0,
  );
  return pezzi.length > 0 ? pezzi.join(" · ") : t("restore.noTags");
}

/** Il delta di un brano, nella forma «ascolti 3 → 17, voto — → ★★★★». */
function delta(brano: CambioBrano): string {
  const parti: string[] = [];
  if (brano.ascoltiDopo !== brano.ascoltiPrima) {
    parti.push(
      t("restore.delta.plays", {
        prima: brano.ascoltiPrima,
        dopo: brano.ascoltiDopo,
      }),
    );
  }
  if (brano.votoDopo !== brano.votoPrima) {
    const stelle = (n: number) => (n > 0 ? "★".repeat(n) : "—");
    parti.push(
      t("restore.delta.rating", {
        prima: stelle(brano.votoPrima),
        dopo: stelle(brano.votoDopo),
      }),
    );
  }
  if (brano.preferitoDopo) parti.push(t("restore.delta.liked"));
  return parti.join(", ");
}

/** La data di un backup, come si legge. */
function quando(ms: number): string {
  return ms <= 0 ? t("restore.unknownDate") : dataOra(ms);
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

  useAscolto<AvanzamentoNuvola>("nuvola:avanzamento", setAvanzamento);

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
        className="finestrella larga glass-modal"
        role="dialog"
        aria-modal="true"
        aria-label={t("restore.aria")}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{esito ? t("restore.done.title") : t("restore.aria")}</h2>
        {piano?.cEUnBackup && (
          <div className="percorso">
            {t("restore.backupOf", { quando: quando(piano.generatoMs) })}
          </div>
        )}

        {errore && <div className="errore">{errore}</div>}

        {avanzamento && (
          <>
            <div className="avanzamento">
              <div style={{ width: `${percentuale}%` }} />
            </div>
            <div className="conteggio">
              {fasi()[avanzamento.cosa]} {avanzamento.fatti} /{" "}
              {avanzamento.totale}
            </div>
          </>
        )}

        {!piano && !esito && inCorso && <p>{t("restore.downloading")}</p>}

        {esito && (
          <>
            <div className="rapporto">
              <div className="voce-rapporto">
                <span>{t("restore.tracks")}</span>
                <span className="conteggio">{esito.brani}</span>
              </div>
              <div className="voce-rapporto">
                <span>{t("restore.playlists")}</span>
                <span className="conteggio">{esito.playlist}</span>
              </div>
              <div className="voce-rapporto">
                <span>{t("restore.folders")}</span>
                <span className="conteggio">{esito.cartelle}</span>
              </div>
              <div className="voce-rapporto">
                <span>{t("restore.skins")}</span>
                <span className="conteggio">{esito.skin}</span>
              </div>
              <div className="voce-rapporto">
                <span>{t("restore.drafts")}</span>
                <span className="conteggio">{esito.bozze}</span>
              </div>
            </div>
            {esito.mancanti.length > 0 && (
              <details className="non-ritrovati" open>
                <summary>
                  {t("restore.missing", { n: esito.mancanti.length })}
                </summary>
                <p>{t("restore.missing.note")}</p>
                <ul>
                  {esito.mancanti.slice(0, 100).map((m) => (
                    <li key={m}>{m}</li>
                  ))}
                </ul>
              </details>
            )}
            {esito.cartelle > 0 && (
              <p>
                <Trans
                  k="restore.foldersBack"
                  v={{
                    scansiona: <strong>{t("restore.foldersBack.cta")}</strong>,
                  }}
                />
              </p>
            )}
          </>
        )}

        {piano && !esito && !piano.cEUnBackup && <p>{t("restore.noBackup")}</p>}

        {piano && !esito && piano.cEUnBackup && (
          <>
            <div className="rapporto">
              <div className="voce-rapporto">
                <span>{t("restore.plan.tracks")}</span>
                <span className="conteggio">{piano.braniDaAggiornare}</span>
              </div>
              <div className="voce-rapporto">
                <span>{t("restore.plan.same")}</span>
                <span className="conteggio">{piano.braniInvariati}</span>
              </div>
              <div className="voce-rapporto">
                <span>{t("restore.plan.playlists")}</span>
                <span className="conteggio">{piano.playlist.length}</span>
              </div>
              <div className="voce-rapporto">
                <span>{t("restore.plan.skins")}</span>
                <span className="conteggio">
                  {piano.skinDaInstallare.length}
                </span>
              </div>
              <div className="voce-rapporto">
                <span>{t("restore.plan.drafts")}</span>
                <span className="conteggio">
                  {piano.bozzeDaScrivere.length}
                </span>
              </div>
            </div>

            {piano.vuoto && <p>{t("restore.plan.empty")}</p>}

            {piano.assentiTotale > 0 && (
              <details className="non-ritrovati">
                <summary>
                  {t("restore.absent", { n: piano.assentiTotale })}
                </summary>
                <p>
                  <Trans
                    k="restore.absent.note"
                    v={{ non: <strong>{t("restore.absent.note.not")}</strong> }}
                  />
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
                    {t("restore.shownFirst", {
                      quanti: piano.assenti.length,
                      totale: piano.assentiTotale,
                    })}
                  </p>
                )}
              </details>
            )}

            {piano.playlist.length > 0 && (
              <details className="non-ritrovati" open>
                <summary>
                  {t("restore.playlists.n", { n: piano.playlist.length })}
                </summary>
                <ul>
                  {piano.playlist.map((p) => (
                    <li key={p.nome}>
                      {t("restore.playlist.line", {
                        nome: p.nome,
                        cosa: p.daCreare
                          ? t("restore.playlist.create")
                          : t("restore.playlist.update"),
                      })}
                      {p.automatica
                        ? t("restore.playlist.smart")
                        : t("restore.playlist.count", {
                            qui: p.braniQui,
                            backup: p.braniNelBackup,
                          })}
                    </li>
                  ))}
                </ul>
              </details>
            )}

            {piano.cartelle.length > 0 && (
              <details className="non-ritrovati" open>
                <summary>
                  {t("restore.foldersToAdd", { n: piano.cartelle.length })}
                </summary>
                <ul>
                  {piano.cartelle.map((c) => (
                    <li key={c.percorso}>
                      {c.percorso}
                      {c.esiste ? "" : t("restore.folderGone")}
                    </li>
                  ))}
                </ul>
              </details>
            )}

            {(piano.skinDaInstallare.length > 0 ||
              piano.bozzeDaScrivere.length > 0) && (
              <details className="non-ritrovati">
                <summary>
                  {t("restore.skinsAndDrafts", {
                    skin: piano.skinDaInstallare.length,
                    bozze: piano.bozzeDaScrivere.length,
                  })}
                </summary>
                <p>
                  {t("restore.keptAsIs", {
                    skin: piano.skinPresenti,
                    bozze: piano.bozzePresenti,
                  })}
                </p>
                <ul>
                  {piano.skinDaInstallare.map((s) => (
                    <li key={`skin-${s}`}>
                      {t("restore.skinItem", { nome: s })}
                    </li>
                  ))}
                  {piano.bozzeDaScrivere.map((b) => (
                    <li key={`bozza-${b}`}>
                      {t("restore.draftItem", { nome: b })}
                    </li>
                  ))}
                </ul>
                {piano.skinAttiva !== null && (
                  <p>{t("restore.activeSkin", { nome: piano.skinAttiva })}</p>
                )}
              </details>
            )}

            {piano.cambi.length > 0 && (
              <details className="spostamenti" open>
                <summary>
                  {t("restore.tracksToUpdate", { n: piano.braniDaAggiornare })}
                </summary>
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
                    {t("restore.shownAllUpdated", {
                      quanti: piano.cambi.length,
                      totale: piano.braniDaAggiornare,
                    })}
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
            {esito ? t("common.close") : t("restore.doNothing")}
          </button>
          {!esito && (
            <button
              type="button"
              className="bottone primario"
              disabled={inCorso || !piano || piano.vuoto}
              onClick={() => void esegui()}
            >
              {inCorso ? t("restore.running") : t("restore.go")}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
