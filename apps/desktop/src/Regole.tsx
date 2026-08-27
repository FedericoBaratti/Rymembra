/**
 * L'editor delle regole di una playlist intelligente.
 *
 * # Perché l'anteprima è sempre accesa
 *
 * Perché una regola sbagliata non sembra sbagliata. «Anno maggiore di 2050»
 * è scritta bene, si salva senza errori e produce una playlist vuota sotto un
 * nome che prometteva il contrario; «artista contiene a» ne produce una da
 * milleduecento. Nessuna delle due dà un messaggio, e senza un numero che
 * cambia mentre si scrive ci si accorge dell'errore solo dopo aver chiuso la
 * finestrella e aperto la playlist.
 *
 * Il numero arriva dal nucleo — `playlist_regole_prova` — e non da un conteggio
 * fatto qui: le regole le valuta SQL, e una seconda valutazione scritta in
 * TypeScript sarebbe una seconda opinione destinata a divergere dalla prima.
 *
 * # Perché le regole storte non bloccano
 *
 * Cambiare il campo di una riga lascia l'operatore com'era, e per un istante
 * la coppia non sta in piedi — «anno» con «contiene». Il nucleo le **salta** e
 * le conta; qui si mostra quel conteggio come un avviso, non come un errore che
 * impedisce di salvare. Un editor che si blocca a metà di una modifica è un
 * editor in cui non si può cambiare idea.
 */
import { useCallback, useEffect, useState } from "react";

import {
  campiSmart,
  ipc,
  operatoriSmart,
  ordinamentiSmart,
  senzaValore,
  testoErrore,
  type AnteprimaRegole,
  type CampoSmart,
  type GenereSmart,
  type InsiemeSmart,
  type OperatoreSmart,
  type RegolaSmart,
} from "./ipc";
import { durata, nomeArtista, numero } from "./formato";
import { Icona } from "./parti/Icone";
import { t } from "./lingue";
import { Trans } from "./lingue/Trans";

/** Il genere di un campo, per sapere quali operatori mostrargli accanto. */
function genereDi(campo: CampoSmart): GenereSmart {
  return campiSmart().find((c) => c.chiave === campo)?.genere ?? "testo";
}

/** Una regola nuova, con l'operatore giusto per il campo. */
function regolaNuova(campo: CampoSmart = "artista"): RegolaSmart {
  const genere = genereDi(campo);
  const operatore = operatoriSmart()[genere][0]?.chiave ?? "contiene";
  return { campo, operatore, ...valoreDiSerie(genere, operatore) };
}

/**
 * Il valore di partenza per una coppia campo/operatore.
 *
 * Non è un dettaglio di comodo: una riga aggiunta con il valore sbagliato è una
 * riga che il nucleo scarta, e chi la vede scartata senza aver ancora scritto
 * niente pensa che l'editor sia rotto.
 */
function valoreDiSerie(
  genere: GenereSmart,
  operatore: OperatoreSmart,
): { testo: string | null; numero: number | null } {
  // Tutti e due i campi sempre presenti, con `null` per quello che non serve:
  // con `exactOptionalPropertyTypes` un campo assente e un campo `undefined`
  // sono cose diverse, e una fusione parziale lascerebbe in giro il valore
  // vecchio del tipo sbagliato — un numero dove il nucleo aspetta del testo.
  if (senzaValore(operatore)) return { testo: null, numero: null };
  if (genere === "testo") return { testo: "", numero: null };
  if (genere === "booleano") return { testo: null, numero: 1 };
  if (genere === "data") return { testo: null, numero: 30 };
  return { testo: null, numero: 0 };
}

/**
 * La regola è stata **finita di scrivere**.
 *
 * # Incompleta non è sbagliata
 *
 * Una riga appena aggiunta ha il campo del valore vuoto: per il nucleo è una
 * regola che non sta in piedi, e la scarta — giustamente, perché «artista
 * contiene ""» selezionerebbe tutto sotto un nome che promette il contrario.
 * Ma dirlo a chi ha appena premuto «aggiungi una condizione» e non ha ancora
 * digitato niente è dirgli che l'editor è rotto.
 *
 * Perciò le righe incomplete non vengono nemmeno mandate: non contano
 * nell'anteprima e non finiscono su disco. Quel che il nucleo continua a
 * scartare — e a contare — è la regola **malformata**: campo e operatore che
 * non vanno d'accordo, cioè un errore vero.
 */
function completa(regola: RegolaSmart): boolean {
  if (senzaValore(regola.operatore)) return true;
  if (genereDi(regola.campo) === "testo") {
    return (regola.testo ?? "").trim().length > 0;
  }
  return regola.numero != null;
}

/** Solo le regole finite: è quel che si manda al nucleo, sempre. */
function scritte(insieme: InsiemeSmart): InsiemeSmart {
  return { ...insieme, regole: insieme.regole.filter(completa) };
}

/**
 * Quanti brani dell'anteprima si mostrano.
 *
 * Il nucleo ne manda venti; qui se ne disegnano sei. Non è un troncamento per
 * risparmiare: è la differenza fra un assaggio — che si legge in un colpo
 * d'occhio e dice «sì, sono quelli giusti» — e un elenco da sfogliare, che in
 * questa finestrella vorrebbe una barra di scorrimento dentro un'altra.
 */
const ASSAGGIO = 6;

const VUOTO: InsiemeSmart = {
  combinazione: "tutte",
  regole: [],
  limite: null,
  ordinamento: "scaffale",
};

/**
 * Un insieme senza righe ne prende una, vuota.
 *
 * # Perché non si apre sul vuoto
 *
 * Un editor di condizioni che si apre senza condizioni mostra il proprio
 * soggetto come uno spazio bianco con un tasto in mezzo, e fa peggio: nessuna
 * condizione **è** «tutta la libreria», quindi l'anteprima ha ragione a dirlo e
 * la finestrella finisce per avvisare di uno stato in cui si è messa da sola,
 * prima che si sia toccato niente.
 *
 * La riga di partenza non è un valore di serie che finisce su disco: è vuota,
 * quindi `completa` la scarta e `scritte` non la manda — né all'anteprima né al
 * salvataggio. Chi non la vuole la toglie, e allora «prende tutto» torna a
 * essere una notizia vera.
 */
function conUnaRiga(insieme: InsiemeSmart): InsiemeSmart {
  if (insieme.regole.length > 0) return insieme;
  return { ...insieme, regole: [regolaNuova()] };
}

export function Regole({
  /** La playlist da modificare, o `null` per crearne una nuova. */
  playlist,
  onChiudi,
  onFatto,
  onErrore,
}: {
  playlist: { id: number; name: string } | null;
  onChiudi: () => void;
  onFatto: () => void;
  onErrore: (e: unknown) => void;
}) {
  const [nome, setNome] = useState(playlist?.name ?? "");
  const [insieme, setInsieme] = useState<InsiemeSmart>(() => conUnaRiga(VUOTO));
  const [anteprima, setAnteprima] = useState<AnteprimaRegole | null>(null);
  const [errore, setErrore] = useState<string | null>(null);
  const [salvando, setSalvando] = useState(false);
  const [caricato, setCaricato] = useState(playlist === null);

  // Le regole di una playlist che c'è già. Finché non arrivano non si mostra
  // l'insieme vuoto come se fosse suo: sarebbe «nessuna condizione», cioè
  // tutta la libreria, e chi legge lo prenderebbe per il contenuto vero.
  useEffect(() => {
    if (playlist === null) return;
    let annullato = false;
    ipc
      .playlistRegole(playlist.id)
      .then((lette) => {
        if (annullato) return;
        setInsieme(conUnaRiga(lette ?? VUOTO));
        setCaricato(true);
      })
      .catch((e: unknown) => {
        if (!annullato) setErrore(testoErrore(e));
      });
    return () => {
      annullato = true;
    };
  }, [playlist]);

  // L'anteprima, a ogni modifica. Nessun ritardo artificiale: la query gira su
  // indici che la libreria ha già, e un `debounce` qui vorrebbe dire un numero
  // che resta indietro di mezzo secondo su quel che si sta scrivendo.
  const prova = useCallback(
    (quali: InsiemeSmart) => {
      ipc
        .playlistRegoleProva(quali)
        .then(setAnteprima)
        .catch((e: unknown) => setErrore(testoErrore(e)));
    },
    [],
  );

  useEffect(() => {
    if (caricato) prova(scritte(insieme));
  }, [insieme, caricato, prova]);

  const cambia = (indice: number, come: Partial<RegolaSmart>) => {
    setInsieme((prima) => ({
      ...prima,
      regole: prima.regole.map((r, i) => (i === indice ? { ...r, ...come } : r)),
    }));
  };

  /** Cambiare campo rifà anche operatore e valore: vedi `valoreDiSerie`. */
  const cambiaCampo = (indice: number, campo: CampoSmart) => {
    const genere = genereDi(campo);
    const attuale = insieme.regole[indice]?.operatore;
    const ammessi = operatoriSmart()[genere];
    const operatore =
      attuale && ammessi.some((o) => o.chiave === attuale)
        ? attuale
        : (ammessi[0]?.chiave ?? "contiene");
    setInsieme((prima) => ({
      ...prima,
      regole: prima.regole.map((r, i) =>
        i === indice
          ? { campo, operatore, ...valoreDiSerie(genere, operatore) }
          : r,
      ),
    }));
  };

  /** C'è una riga che si sta ancora scrivendo: cambia cosa vale la pena dire. */
  const incomplete = insieme.regole.some((r) => !completa(r));

  const salva = () => {
    const pulito = nome.trim();
    if (pulito.length === 0) {
      setErrore(t("rules.needName"));
      return;
    }
    setSalvando(true);
    setErrore(null);
    // `scritte` anche qui: una regola col valore vuoto salvata su disco è una
    // regola che il nucleo scarta a ogni apertura, cioè una riga che riapparirà
    // nell'editor senza fare mai niente.
    const daSalvare = scritte(insieme);
    const promessa =
      playlist === null
        ? ipc.playlistCreaSmart(pulito, daSalvare)
        : ipc.playlistRegoleScrivi(playlist.id, daSalvare);
    promessa
      .then(() => onFatto())
      .catch((e: unknown) => {
        setErrore(testoErrore(e));
        setSalvando(false);
        // Anche fuori: un guasto del database non riguarda solo questa
        // finestrella, e chi la chiude senza leggere non deve perderlo.
        onErrore(e);
      });
  };

  return (
    <div className="velo scuro" onClick={salvando ? undefined : onChiudi}>
      <div
        className="finestrella media editor-regole glass-modal"
        role="dialog"
        aria-modal="true"
        aria-label={t("rules.aria")}
        onClick={(e) => e.stopPropagation()}
      >
        <h2>
          {playlist === null
            ? t("rules.title.new")
            : t("rules.title.edit", { nome: playlist.name })}
        </h2>
        {/* Prosa e non `.nota`: è il sottotitolo del titolo qui sopra, non un
            richiamo. Il riquadro d'accento faceva della spiegazione la cosa più
            forte della finestrella — e con una skin rossa la faceva somigliare
            alla fascia d'errore, che è l'unica altra cosa colorata qui dentro. */}
        <p className="sottotitolo">
          <Trans
            k="rules.intro"
            v={{ adesso: <strong>{t("rules.intro.now")}</strong> }}
          />
        </p>

        {errore && <div className="errore">{errore}</div>}

        {playlist === null && (
          <label>
            {t("rules.name")}
            <input
              type="text"
              value={nome}
              placeholder={t("rules.name.hint")}
              onChange={(e) => setNome(e.target.value)}
            />
          </label>
        )}

        <div className="riga-combinazione">
          <span>{t("rules.take")}</span>
          <select
            value={insieme.combinazione}
            onChange={(e) =>
              setInsieme((p) => ({
                ...p,
                combinazione: e.target.value as InsiemeSmart["combinazione"],
              }))
            }
          >
            <option value="tutte">{t("rules.all")}</option>
            <option value="qualsiasi">{t("rules.any")}</option>
          </select>
          <span>{t("rules.conditions")}</span>
        </div>

        <ul className="regole">
          {insieme.regole.map((regola, indice) => {
            const genere = genereDi(regola.campo);
            return (
              <li className="regola" key={indice}>
                <select
                  aria-label={t("rules.field")}
                  value={regola.campo}
                  onChange={(e) =>
                    cambiaCampo(indice, e.target.value as CampoSmart)
                  }
                >
                  {campiSmart().map((c) => (
                    <option key={c.chiave} value={c.chiave}>
                      {c.etichetta}
                    </option>
                  ))}
                </select>

                <select
                  aria-label={t("rules.operator")}
                  value={regola.operatore}
                  onChange={(e) => {
                    const operatore = e.target.value as OperatoreSmart;
                    cambia(indice, {
                      operatore,
                      ...valoreDiSerie(genere, operatore),
                    });
                  }}
                >
                  {operatoriSmart()[genere].map((o) => (
                    <option key={o.chiave} value={o.chiave}>
                      {o.etichetta}
                    </option>
                  ))}
                </select>

                {senzaValore(regola.operatore) ? (
                  <span className="senza-valore">—</span>
                ) : genere === "testo" ? (
                  <input
                    type="text"
                    aria-label={t("rules.value")}
                    value={regola.testo ?? ""}
                    onChange={(e) => cambia(indice, { testo: e.target.value })}
                  />
                ) : genere === "booleano" ? (
                  <select
                    aria-label={t("rules.value")}
                    value={String(regola.numero ?? 1)}
                    onChange={(e) =>
                      cambia(indice, { numero: Number(e.target.value) })
                    }
                  >
                    <option value="1">{t("rules.yes")}</option>
                    <option value="0">{t("rules.no")}</option>
                  </select>
                ) : (
                  <input
                    type="number"
                    aria-label={t("rules.value")}
                    value={regola.numero ?? 0}
                    onChange={(e) =>
                      cambia(indice, { numero: Number(e.target.value) })
                    }
                  />
                )}

                <button
                  type="button"
                  className="tasto icon-btn"
                  aria-label={t("rules.remove")}
                  onClick={() =>
                    setInsieme((p) => ({
                      ...p,
                      regole: p.regole.filter((_, i) => i !== indice),
                    }))
                  }
                >
                  <Icona nome="i-x" dim={14} />
                </button>
              </li>
            );
          })}
        </ul>

        {/* L'azione che porta avanti la finestrella, e non un tasto scolorito:
            aggiungere una condizione è **il** verbo di questo editor, mentre
            «Crea» è solo la fine. Lasciarlo in tinta smorta e tenere l'accento
            sul salvataggio metteva il risalto sull'unico tasto che qui non si
            deve premere per primo. */}
        <div className="azioni">
          <button
            type="button"
            className="bottone aggiungi-regola"
            onClick={() =>
              setInsieme((p) => ({ ...p, regole: [...p.regole, regolaNuova()] }))
            }
          >
            <Icona nome="i-plus" dim={15} />
            {t("rules.add")}
          </button>
        </div>

        {/*
         * Il risultato: come si presenta, quanto è grande, e i primi.
         *
         * `In ordine di` e `Al massimo` stanno **qui** e non su fra le
         * condizioni: non dicono quali brani entrano, dicono come esce quel che
         * è entrato. In mezzo all'editor spezzavano la frase — condizioni,
         * due tendine di tutt'altro, e solo dopo il numero che le condizioni
         * producono.
         */}
        <div className="anteprima-regole">
          <div className="riga-ordine">
            <label>
              {t("rules.orderBy")}
              <select
                value={insieme.ordinamento}
                onChange={(e) =>
                  setInsieme((p) => ({
                    ...p,
                    ordinamento: e.target.value as InsiemeSmart["ordinamento"],
                  }))
                }
              >
                {ordinamentiSmart().map((o) => (
                  <option key={o.chiave} value={o.chiave}>
                    {o.etichetta}
                  </option>
                ))}
              </select>
            </label>
            <label>
              {t("rules.atMost")}
              <input
                type="number"
                min={0}
                placeholder={t("rules.atMost.all")}
                value={insieme.limite ?? ""}
                onChange={(e) =>
                  setInsieme((p) => ({
                    ...p,
                    limite:
                      e.target.value.trim() === ""
                        ? null
                        : Math.max(0, Number(e.target.value)),
                  }))
                }
              />
            </label>
          </div>

          {anteprima && (
            <>
              <div className="quanti">
                {anteprima.prendeNiente ? (
                  <strong>{t("rules.none")}</strong>
                ) : (
                  <>
                    <strong>{numero(anteprima.quanti)}</strong>{" "}
                    {t("rules.count.unit", { n: anteprima.quanti })}
                  </>
                )}
                {insieme.limite != null &&
                  insieme.limite > 0 &&
                  anteprima.quanti > insieme.limite && (
                    <span className="nota">
                      {t("rules.fitting", { n: insieme.limite })}
                    </span>
                  )}
              </div>

              {/* «Prende tutto» solo a righe finite. Mentre se ne sta scrivendo
                  una, il nucleo non la conta — è giusto — e quindi le regole in
                  corso d'opera prendono davvero tutta la libreria: dirlo col
                  riquadro giallo vorrebbe dire allarmare chi sta digitando per
                  una condizione che sta arrivando. Lo dice la riga sotto, che
                  non ha un riquadro perché non c'è niente da rimediare. */}
              {anteprima.prendeTutto && !incomplete && (
                <p className="avviso">
                  <Trans
                    k="rules.takesAll"
                    v={{
                      tutta: <strong>{t("rules.takesAll.emphasis")}</strong>,
                    }}
                  />
                </p>
              )}
              {anteprima.prendeNiente && (
                <p className="avviso">{t("rules.takesNothing")}</p>
              )}
              {incomplete && <p className="nota">{t("rules.incomplete")}</p>}
              {anteprima.scartate > 0 && (
                <p className="avviso">
                  {t("rules.skipped", { n: anteprima.scartate })}
                </p>
              )}

              {/* Un assaggio, non un elenco da sfogliare: sei righe che ci
                  stanno tutte. Il nucleo ne manda venti e le venti stavano in
                  un riquadro alto centonovanta con la sua barra di scorrimento
                  — cioè uno scorrimento dentro una finestrella che ne ha già
                  uno suo, e proprio sotto le condizioni, che sono la parte che
                  finiva spinta fuori dallo schermo. */}
              {anteprima.primi.length > 0 && (
                <ol className="primi">
                  {anteprima.primi.slice(0, ASSAGGIO).map((b) => (
                    <li key={b.id}>
                      <span className="nome">{b.title}</span>
                      <span className="autore">{nomeArtista(b.artist)}</span>
                      <span className="durata">{durata(b.durationMs)}</span>
                    </li>
                  ))}
                </ol>
              )}
            </>
          )}
        </div>

        <div className="tasti-finestrella">
          <button
            type="button"
            className="bottone"
            disabled={salvando}
            onClick={onChiudi}
          >
            {t("common.cancel")}
          </button>
          <button
            type="button"
            className="bottone primario"
            disabled={salvando || nome.trim().length === 0}
            onClick={salva}
          >
            {salvando
              ? t("rules.saving")
              : playlist === null
                ? t("addTo.create")
                : t("common.save")}
          </button>
        </div>
      </div>
    </div>
  );
}
