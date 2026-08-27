/**
 * L'editor a battute: sincronizzare un testo premendo un tasto.
 *
 * # Perché esiste
 *
 * Perché nessun catalogo gratuito ha i testi di *tutte* le canzoni — quel
 * catalogo non esiste — e senza questa schermata la copertura si ferma dove si
 * ferma LRCLIB. Con questa, ogni brano è chiudibile: tre minuti di battute, e
 * quel che resta scoperto è solo quel che nessuno ha ancora sincronizzato.
 *
 * # Come si batte
 *
 * Si preme **Spazio** all'inizio di ogni riga mentre il brano suona. È il modo
 * più veloce che esista, e produce tempi sistematicamente in ritardo di due o
 * tre decimi di secondo: fra l'orecchio e il dito passa quello.
 *
 * La correzione non è un numero fisso — quel ritardo cambia da persona a
 * persona e con la stanchezza — ed è per questo che le battute non si salvano
 * come sono. Vanno al nucleo, che decodifica il brano, ne tira fuori gli
 * **attacchi** (dove il suono comincia davvero) e aggancia ogni battuta al più
 * vicino, correggendo le altre della latenza mediana misurata su quelle
 * agganciate. La regola sta in `aether_domain::testo::aggancia` e si prova
 * senza aprire una finestra; qui si raccolgono le pressioni di un tasto.
 *
 * # Tre momenti, e uno solo alla volta
 *
 * 1. **Le righe** — si incolla o si corregge il testo, una riga per verso.
 * 2. **Le battute** — si suona e si preme.
 * 3. **La revisione** — si guardano i tempi raddrizzati, si correggono a mano
 *    quelli che serve, si salva.
 *
 * Separati e non tutti in una schermata perché in ognuno dei tre la tastiera
 * vuol dire una cosa diversa: nel primo Spazio è uno spazio, nel secondo è una
 * battuta. Metterli insieme vorrebbe dire un editor in cui non si può scrivere.
 */
import { useCallback, useEffect, useRef, useState } from "react";

import { ipc, type Brano, type TestoBrano } from "./ipc";
import { durata } from "./formato";
import { t } from "./lingue";
import { Icona } from "./parti/Icone";
import { posizioneAdesso } from "./riproduzione";

/** Di quanto si torna indietro quando si riparte da una riga. */
const RINCORSA_MS = 2000;

/** Di quanto sposta una freccia, e di quanto con lo Shift premuto. */
const PASSO_FINE = 10;
const PASSO_GROSSO = 100;

/** In quale dei quattro momenti si è. */
type Momento = "righe" | "battute" | "rivedi" | "dono";

export function Sincronizza({
  brano,
  iniziale,
  onChiudi,
  onSalvato,
  onErrore,
}: {
  brano: Brano;
  /** Il testo da cui partire: quel che il pannello aveva, o niente. */
  iniziale: string;
  onChiudi: () => void;
  onSalvato: (testo: TestoBrano) => void;
  onErrore: (e: unknown) => void;
}) {
  const [momento, setMomento] = useState<Momento>("righe");
  const [grezzo, setGrezzo] = useState(iniziale);
  const [righe, setRighe] = useState<string[]>([]);
  const [tempi, setTempi] = useState<number[]>([]);
  const [indice, setIndice] = useState(0);
  const [raddrizzo, setRaddrizzo] = useState(false);
  const [salvando, setSalvando] = useState(false);
  // Il dono ha tre stati e non due: «non ancora», «sto mandando» e «mandato».
  // Il secondo dura secondi — la prova di lavoro di LRCLIB è calcolo, non
  // attesa — e senza un modo di dirlo sembrerebbe che il pulsante è rotto.
  const [donando, setDonando] = useState(false);
  const [donato, setDonato] = useState(false);
  // Se le richieste al catalogo sono spente, l'offerta non si fa: proporre di
  // mandare qualcosa a un servizio che si è deciso di non interpellare è una
  // domanda a cui la risposta è già stata data.
  const [reteAccesa, setReteAccesa] = useState(false);
  // La riga su cui si sta battendo, per tenerla in vista senza ridisegnare
  // l'elenco a ogni fotogramma.
  const corrente = useRef<HTMLLIElement | null>(null);

  useEffect(() => {
    corrente.current?.scrollIntoView({ block: "center", behavior: "smooth" });
  }, [indice]);

  // ── il momento delle righe ────────────────────────────────────────────────

  const cominciaABattere = useCallback(() => {
    // Le righe vuote in mezzo sono pause fra le strofe e si tengono: hanno un
    // loro tempo, e toglierle attaccherebbe l'ultima riga di una strofa alla
    // prima della successiva. Quelle in testa e in coda no.
    const pulite = grezzo.split("\n").map((riga) => riga.trim());
    while (pulite.length > 0 && pulite[0] === "") pulite.shift();
    while (pulite.length > 0 && pulite[pulite.length - 1] === "") pulite.pop();
    if (pulite.length === 0) return;
    setRighe(pulite);
    setTempi([]);
    setIndice(0);
    setMomento("battute");
  }, [grezzo]);

  // ── il momento delle battute ──────────────────────────────────────────────

  const batti = useCallback(() => {
    if (indice >= righe.length) return;
    // `posizioneAdesso` e non la posizione iscritta: qui non si disegna a venti
    // fotogrammi al secondo, si legge una volta in risposta a un tasto. È
    // esattamente il caso per cui quella funzione esiste.
    const adesso = posizioneAdesso();
    setTempi((prima) => [...prima.slice(0, indice), adesso]);
    setIndice((prima) => prima + 1);
  }, [indice, righe.length]);

  const rifai = useCallback(() => {
    if (indice === 0) return;
    const precedente = indice - 1;
    setIndice(precedente);
    setTempi((prima) => prima.slice(0, precedente));
    const da = Math.max(0, (tempi[precedente] ?? 0) - RINCORSA_MS);
    ipc.vaiA(da).catch(onErrore);
  }, [indice, tempi, onErrore]);

  const finisci = useCallback(() => {
    setRaddrizzo(true);
    ipc
      .pausa()
      .catch(() => {})
      .finally(() => {
        ipc
          .testoAggancia(brano.id, tempi)
          .then((raddrizzati) => {
            setTempi(raddrizzati);
            setMomento("rivedi");
          })
          .catch(onErrore)
          .finally(() => setRaddrizzo(false));
      });
  }, [brano.id, tempi, onErrore]);

  // La tastiera vale solo mentre si batte: negli altri due momenti Spazio è uno
  // spazio e le frecce muovono un cursore.
  useEffect(() => {
    if (momento !== "battute") return;
    const alTasto = (evento: KeyboardEvent) => {
      if (evento.key === " ") {
        evento.preventDefault();
        batti();
      } else if (evento.key === "Backspace") {
        evento.preventDefault();
        rifai();
      } else if (evento.key === "Enter" && indice >= righe.length) {
        evento.preventDefault();
        finisci();
      }
    };
    window.addEventListener("keydown", alTasto);
    return () => window.removeEventListener("keydown", alTasto);
  }, [momento, batti, rifai, finisci, indice, righe.length]);

  // ── il momento della revisione ────────────────────────────────────────────

  const sposta = useCallback((quale: number, quanto: number) => {
    setTempi((prima) =>
      prima.map((ms, i) => (i === quale ? Math.max(0, ms + quanto) : ms)),
    );
  }, []);

  const salva = useCallback(() => {
    setSalvando(true);
    ipc
      .testoSalva(
        brano.id,
        righe.map((testo, i) => ({ ms: tempi[i] ?? 0, testo })),
      )
      .then((salvato) => {
        // Il pannello dietro si aggiorna subito, prima ancora che questa
        // finestra si chiuda. Il lavoro è finito e salvato; quel che resta è
        // un'offerta, e un'offerta non deve tenere in ostaggio il risultato.
        onSalvato(salvato);
        return ipc
          .testiStato()
          .then((stato) => setReteAccesa(stato.rete))
          .catch(() => setReteAccesa(false))
          .then(() => setMomento("dono"));
      })
      .catch(onErrore)
      .finally(() => setSalvando(false));
  }, [brano.id, righe, tempi, onSalvato, onErrore]);

  /* Restituire. È un gesto separato dal salvataggio per una ragione sola: si
     salva sempre, quindi tutto quel che sta attaccato al salvataggio è
     automatico, e mandare il proprio lavoro a un servizio pubblico non deve
     succedere per inerzia. Chi non preme qui non ha mandato niente, e il `.lrc`
     è comunque sul disco. */
  const dona = useCallback(() => {
    setDonando(true);
    ipc
      .testoPubblica(brano.id)
      .then(() => setDonato(true))
      .catch(onErrore)
      .finally(() => setDonando(false));
  }, [brano.id, onErrore]);

  const battute = indice >= righe.length && righe.length > 0;

  return (
    <div className="velo scuro">
      <div
        className="finestrella larga sincronizza glass-modal"
        role="dialog"
        aria-modal="true"
        aria-label={t("sync.aria")}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>{t("sync.aria")}</h2>
        <div className="percorso">
          {brano.title} · {brano.artist}
        </div>

        {momento === "righe" && (
          <>
            <p className="nota">{t("sync.lines.help")}</p>
            <textarea
              className="righe-da-battere"
              value={grezzo}
              rows={14}
              spellCheck={false}
              onChange={(e) => setGrezzo(e.target.value)}
              aria-label={t("sync.lines.aria")}
            />
            <div className="azioni">
              <button type="button" className="bottone btn-ghost" onClick={onChiudi}>
                {t("common.cancel")}
              </button>
              <button
                type="button"
                className="bottone primario btn-accent"
                disabled={grezzo.trim() === ""}
                onClick={cominciaABattere}
              >
                {t("sync.lines.start")}
              </button>
            </div>
          </>
        )}

        {momento === "battute" && (
          <>
            <p className="nota">
              {battute ? t("sync.tap.done") : t("sync.tap.help")}
            </p>
            <ol className="elenco-battute">
              {righe.map((riga, i) => (
                <li
                  key={`${i}-${riga}`}
                  ref={i === indice ? corrente : undefined}
                  data-attesa={i === indice || undefined}
                  data-fatta={i < indice || undefined}
                >
                  <span className="quando">
                    {i < indice ? durata(tempi[i] ?? 0) : "—"}
                  </span>
                  <span className="cosa">{riga === "" ? " " : riga}</span>
                </li>
              ))}
            </ol>
            <div className="azioni">
              <button
                type="button"
                className="bottone btn-ghost"
                onClick={() => ipc.alterna().catch(onErrore)}
              >
                <Icona nome="i-play" dim={15} />
                {t("sync.tap.play")}
              </button>
              <button
                type="button"
                className="bottone btn-ghost"
                disabled={indice === 0}
                onClick={rifai}
              >
                {t("sync.tap.undo")}
                <kbd className="scorciatoia">⌫</kbd>
              </button>
              <button
                type="button"
                className="bottone primario btn-accent"
                disabled={!battute || raddrizzo}
                onClick={finisci}
              >
                {raddrizzo ? t("sync.tap.straightening") : t("sync.tap.finish")}
              </button>
            </div>
            <p className="nota">{t("sync.tap.key")}</p>
          </>
        )}

        {momento === "rivedi" && (
          <>
            <p className="nota">{t("sync.review.help")}</p>
            <ol className="elenco-battute rivedi">
              {righe.map((riga, i) => (
                <li key={`${i}-${riga}`}>
                  <span className="quando">{durata(tempi[i] ?? 0)}</span>
                  <span className="cosa">{riga === "" ? " " : riga}</span>
                  <span className="ritocco">
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      title={t("sync.review.earlier")}
                      onClick={() => sposta(i, -PASSO_GROSSO)}
                    >
                      −{PASSO_GROSSO}
                    </button>
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      title={t("sync.review.earlier.fine")}
                      onClick={() => sposta(i, -PASSO_FINE)}
                    >
                      −{PASSO_FINE}
                    </button>
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      title={t("sync.review.later.fine")}
                      onClick={() => sposta(i, PASSO_FINE)}
                    >
                      +{PASSO_FINE}
                    </button>
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      title={t("sync.review.later")}
                      onClick={() => sposta(i, PASSO_GROSSO)}
                    >
                      +{PASSO_GROSSO}
                    </button>
                    <button
                      type="button"
                      className="bottone minuto btn-ghost"
                      title={t("sync.review.hear")}
                      onClick={() =>
                        ipc
                          .vaiA(Math.max(0, (tempi[i] ?? 0) - 1000))
                          .then(() => ipc.riprendi())
                          .catch(onErrore)
                      }
                    >
                      <Icona nome="i-play" dim={12} />
                    </button>
                  </span>
                </li>
              ))}
            </ol>
            <div className="azioni">
              <button
                type="button"
                className="bottone btn-ghost"
                onClick={() => {
                  setIndice(0);
                  setTempi([]);
                  setMomento("battute");
                }}
              >
                {t("sync.review.again")}
              </button>
              <button
                type="button"
                className="bottone btn-ghost"
                onClick={onChiudi}
              >
                {t("common.cancel")}
              </button>
              <button
                type="button"
                className="bottone primario btn-accent"
                disabled={salvando}
                onClick={salva}
              >
                {salvando ? t("sync.review.saving") : t("sync.review.save")}
              </button>
            </div>
            {/* Dove finisce, detto prima di premere: un file accanto alla
                musica lo leggono anche gli altri lettori, e chi non lo vuole
                deve poterlo sapere adesso e non dopo. */}
            <p className="nota">{t("sync.review.where")}</p>
          </>
        )}

        {momento === "dono" && (
          <div className="dono">
            <p className="fatto">
              <Icona nome="i-check" dim={16} />
              {t("sync.done.saved")}
            </p>

            {/* Senza rete verso il catalogo non c'è offerta: si dice che è
                fatto e si chiude, senza far intravedere una porta chiusa. */}
            {reteAccesa && !donato && (
              <>
                <p className="nota">{t("sync.done.offer")}</p>
                {/* Cosa esce di qui, per esteso e prima di premere: il titolo
                    che il catalogo userà per ritrovarlo, e le righe con i loro
                    tempi. Non c'è niente d'altro nell'invio — nessun percorso,
                    nessun identificativo, niente sul dispositivo. */}
                <ul className="cosa-va">
                  <li>
                    {brano.title} · {brano.artist}
                    {brano.album ? ` · ${brano.album}` : ""}
                  </li>
                  <li>{t("sync.done.lines", { n: righe.length })}</li>
                </ul>
                <p className="nota">{t("sync.done.slow")}</p>
              </>
            )}

            {donato && <p className="nota">{t("sync.done.thanks")}</p>}

            <div className="azioni">
              {reteAccesa && !donato && (
                <button
                  type="button"
                  className="bottone btn-ghost"
                  disabled={donando}
                  onClick={dona}
                >
                  {donando ? t("sync.done.giving") : t("sync.done.give")}
                </button>
              )}
              {/* Chiudere è il pulsante primario, anche qui. Chi arriva a
                  questa schermata ha già ottenuto quel che voleva; il dono è la
                  strada in più, non quella diritta. */}
              <button
                type="button"
                className="bottone primario btn-accent"
                onClick={onChiudi}
              >
                {t("common.close")}
              </button>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
