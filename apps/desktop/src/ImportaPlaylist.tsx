/**
 * Portare dentro una playlist da un file M3U, PLS o XSPF.
 *
 * # Perché c'è un piano prima
 *
 * Perché reimportare **sostituisce**: la playlist che un file aveva creato con
 * lo stesso nome viene svuotata e riscritta, che è l'unica delle tre scelte
 * possibili — accodare, rinominare, sostituire — che si può fare due volte
 * senza pentirsene. Ma cancella, e quel che cancella va detto prima di farlo,
 * non dopo. Una playlist fatta qui con quel nome invece non si tocca mai: il
 * file le entra accanto, «Serata (file)», e il piano lo dice (D-11).
 *
 * # Il piano è quello del nome nel campo
 *
 * Tutte e due le cose dipendono dal nome, quindi il piano si rifà quando il
 * campo cambia, e «Importa» resta spento finché il piano a schermo non è quello
 * del nome scritto (R13c-2, `pianoFilePlaylist.ts`).
 *
 * # I mancanti si dicono con il percorso
 *
 * Un brano di un M3U che qui non c'è **non** finisce nella coda, al contrario
 * di quel che succede importando un archivio o un link. La differenza è nella
 * domanda: quelli sono elenchi di canzoni che l'utente vuole avere, un file M3U
 * è la fotografia di una libreria che qualcuno aveva già. Andare a cercare nei
 * cataloghi un file che esiste sul computer di chi ha esportato la playlist è
 * una risposta a una domanda che nessuno ha fatto — e il percorso, che si
 * mostra, è l'unica cosa utile per andarselo a prendere dov'è davvero.
 */
import { useEffect, useState } from "react";

import { Dialogo } from "./Dialogo";
import { ATTESA_PIANO_MS, importabile } from "./pianoFilePlaylist";
import { vinceLUltima } from "./ultima";
import { ipc, testoErrore, type EsitoFilePlaylist } from "./ipc";
import { Avviso } from "./parti/Avvisi";
import { Icona } from "./parti/Icone";
import { numero } from "./formato";
import { t } from "./lingue";
import { Trans } from "./lingue/Trans";

export function ImportaPlaylist({
  percorso,
  onChiudi,
  onImportato,
  onErrore,
}: {
  percorso: string;
  onChiudi: () => void;
  onImportato: (esito: EsitoFilePlaylist) => void;
  onErrore: (e: unknown) => void;
}) {
  const [rapporto, setRapporto] = useState<EsitoFilePlaylist | null>(null);
  const [nome, setNome] = useState("");
  /** Il nome per cui è stato fatto il piano a schermo: `null` finché non c'è. */
  const [nomeDelPiano, setNomeDelPiano] = useState<string | null>(null);
  const [errore, setErrore] = useState<string | null>(null);
  const [inCorso, setInCorso] = useState(true);
  /** Si sta rifacendo il piano per un nome nuovo. */
  const [ripianificando, setRipianificando] = useState(false);
  // Fra due piani chiesti a pochi tasti di distanza conta l'ultimo chiesto, non
  // l'ultimo arrivato: vedi `ultima.ts`.
  const [turno] = useState(vinceLUltima);
  // Si apre con tutti i comandi spenti, perché il piano non è ancora arrivato:
  // il fuoco va sulla radice, che è l'unico posto che c'è in quell'istante, e
  // l'Escape funziona anche durante la lettura del file. Tutto da `Dialogo`.

  useEffect(() => {
    let annullato = false;
    setInCorso(true);
    ipc
      .playlistFilePiano(percorso)
      .then((piano) => {
        if (annullato) return;
        setRapporto(piano.rapporto);
        setNome(piano.nome);
        setNomeDelPiano(piano.nome);
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

  // Il piano si rifà quando il nome cambia, dopo un attimo senza tasti: con il
  // nome di una playlist che c'è già cambia quel che «Importa» farebbe.
  useEffect(() => {
    if (nomeDelPiano === null) return;
    if (nome === nomeDelPiano || nome.trim().length === 0) {
      // Una risposta per un nome di prima, ancora in viaggio, non deve più
      // arrivare a schermo.
      turno();
      setRipianificando(false);
      return;
    }
    const mio = turno();
    setRipianificando(true);
    const attesa = window.setTimeout(() => {
      ipc
        .playlistFilePiano(percorso, nome)
        .then((piano) => {
          if (!mio()) return;
          setRapporto(piano.rapporto);
          setNomeDelPiano(piano.nome);
          setErrore(null);
        })
        .catch((e: unknown) => {
          if (mio()) setErrore(testoErrore(e));
        })
        .finally(() => {
          if (mio()) setRipianificando(false);
        });
    }, ATTESA_PIANO_MS);
    return () => window.clearTimeout(attesa);
  }, [nome, nomeDelPiano, percorso, turno]);

  const conferma = () => {
    setInCorso(true);
    setErrore(null);
    ipc
      .playlistFileImporta(percorso, nome)
      .then((esito) => {
        onImportato(esito);
        onChiudi();
      })
      .catch((e: unknown) => {
        setErrore(testoErrore(e));
        setInCorso(false);
        onErrore(e);
      });
  };

  const vuota = rapporto !== null && rapporto.matchedByPath + rapporto.matchedByTags === 0;
  /** Il piano a schermo è quello del nome nel campo. */
  const pianoDelNome = nomeDelPiano !== null && nomeDelPiano === nome;

  return (
    // Il velo resta spento mentre legge e mentre importa, com'era; Escape
    // invece chiude anche durante la lettura del file, che è un gesto
    // deliberato. Il titolo dice il passo: il nome resta quello di sempre.
    <Dialogo
      titolo={t("plfile.title")}
      etichetta={t("plfile.aria")}
      classe="finestrella glass-modal"
      onChiudi={onChiudi}
      veloChiude={!inCorso}
    >
      <div className="percorso">{percorso}</div>

      {errore && (
        <div className="errore" role="alert">
          {errore}
        </div>
      )}

      {rapporto === null ? (
        <p className="nota">
          {inCorso ? t("plfile.reading") : t("plfile.nothing")}
        </p>
      ) : (
        <>
          <label>
            {t("plfile.name")}
            <input
              type="text"
              value={nome}
              onChange={(e) => setNome(e.target.value)}
            />
          </label>

          <dl className="numeri">
            <div>
              <dt>{t("plfile.rows")}</dt>
              <dd className="stat-number">{numero(rapporto.entries)}</dd>
            </div>
            <div>
              <dt>{t("plfile.byPath")}</dt>
              <dd className="stat-number">
                {numero(rapporto.matchedByPath)}
              </dd>
            </div>
            <div>
              <dt>{t("plfile.byTags")}</dt>
              <dd className="stat-number">
                {numero(rapporto.matchedByTags)}
              </dd>
            </div>
            <div>
              <dt>{t("plfile.notInLib")}</dt>
              <dd className="stat-number">
                {numero(rapporto.missing.length)}
              </dd>
            </div>
          </dl>

          {/* Finché il piano non è quello del nome scritto, gli avvisi qui
              sotto parlerebbero di un altro nome: si dice che si sta
              ricontrollando, e «Importa» aspetta. */}
          {!pianoDelNome && nome.trim().length > 0 && (
            <p className="nota">{t("plfile.replanning")}</p>
          )}

          {/* Avviso: qualcosa viene distrutto, e un gesto lo evita. Il gesto
              è cambiare il nome nel campo qui sopra, quindi non c'è un tasto
              da mettere dentro il riquadro — la frase indica dove. Il nome è
              quello del piano, non quello che si sta scrivendo. */}
          {pianoDelNome && rapporto.replaced && (
            <Avviso livello="avviso">
              <Trans
                k="plfile.replaced"
                v={{
                  nome: <strong>{rapporto.name}</strong>,
                  sostituita: <strong>{t("plfile.replaced.word")}</strong>,
                }}
              />
            </Avviso>
          )}

          {/* Nota: non si distrugge niente. La playlist di qui resta com'è, e
              il file entra accanto con un nome suo (D-11). */}
          {pianoDelNome && rapporto.beside !== null && (
            <Avviso livello="nota">
              <Trans
                k="plfile.beside"
                v={{
                  esistente: <strong>{rapporto.beside}</strong>,
                  nome: <strong>{rapporto.name}</strong>,
                }}
              />
            </Avviso>
          )}

          {/* Blocco: importare produrrebbe una playlist vuota, e il primario è
              spento. Era ambra accanto all'altro, e i due dicevano cose
              opposte con lo stesso colore — «attento» e «non si può». */}
          {vuota && <Avviso livello="blocco">{t("plfile.empty")}</Avviso>}

          {/* Nota: righe che non si sono capite sono un fatto del file, non
              un guasto di Rymembra, e non c'è niente da fare. */}
          {rapporto.unreadable > 0 && (
            <Avviso livello="nota">
              {t("plfile.unreadable", { n: rapporto.unreadable })}
            </Avviso>
          )}

          {rapporto.missing.length > 0 && (
            <details className="mancanti-account">
              <summary>
                {t("plfile.missing", { n: rapporto.missing.length })}
              </summary>
              <ul>
                {rapporto.missing.slice(0, 50).map((m) => (
                  <li key={m.path}>
                    <span className="nome">
                      {m.title ?? m.path.split(/[\\/]/).pop()}
                    </span>
                    {m.artist !== null && (
                      <span className="autore">{m.artist}</span>
                    )}
                    {/* Il percorso e non il titolo: è l'unica cosa che serve
                        per andarselo a prendere dov'è davvero. */}
                    <span className="percorso" title={m.path}>
                      {m.path}
                    </span>
                  </li>
                ))}
              </ul>
              {rapporto.missing.length > 50 && (
                <p className="nota">
                  {t("plfile.andMore", { n: rapporto.missing.length - 50 })}
                </p>
              )}
            </details>
          )}
        </>
      )}

      <div className="tasti-finestrella">
        <button
          type="button"
          className="bottone btn-ghost"
          disabled={inCorso}
          onClick={onChiudi}
        >
          {t("common.cancel")}
        </button>
        <button
          type="button"
          className="bottone primario btn-accent"
          // `vuota`: la playlist verrebbe creata e sarebbe vuota, cioè un
          // successo apparente. Il riquadro qui sopra è un blocco e questo
          // tasto deve dirlo anche lui, o il blocco è solo un colore. E spento
          // finché il piano non è quello del nome scritto (R13c-2).
          disabled={
            rapporto === null ||
            !importabile({
              inCorso: inCorso || ripianificando,
              nome,
              nomeDelPiano,
              ritrovati: rapporto.matchedByPath + rapporto.matchedByTags,
            })
          }
          onClick={conferma}
        >
          <Icona nome="i-import" dim={15} />
          {inCorso ? t("plfile.working") : t("legacy.import")}
        </button>
      </div>
    </Dialogo>
  );
}
