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
  CAMPI_SMART,
  OPERATORI_SMART,
  ORDINAMENTI_SMART,
  ipc,
  senzaValore,
  testoErrore,
  type AnteprimaRegole,
  type CampoSmart,
  type GenereSmart,
  type InsiemeSmart,
  type OperatoreSmart,
  type RegolaSmart,
} from "./ipc";
import { durata } from "./formato";
import { Icona } from "./parti/Icone";

/** Il genere di un campo, per sapere quali operatori mostrargli accanto. */
function genereDi(campo: CampoSmart): GenereSmart {
  return CAMPI_SMART.find((c) => c.chiave === campo)?.genere ?? "testo";
}

/** Una regola nuova, con l'operatore giusto per il campo. */
function regolaNuova(campo: CampoSmart = "artista"): RegolaSmart {
  const genere = genereDi(campo);
  const operatore = OPERATORI_SMART[genere][0]?.chiave ?? "contiene";
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

const VUOTO: InsiemeSmart = {
  combinazione: "tutte",
  regole: [],
  limite: null,
  ordinamento: "scaffale",
};

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
  const [insieme, setInsieme] = useState<InsiemeSmart>(VUOTO);
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
        setInsieme(lette ?? VUOTO);
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
    const ammessi = OPERATORI_SMART[genere];
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

  const salva = () => {
    const pulito = nome.trim();
    if (pulito.length === 0) {
      setErrore("Serve un nome.");
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
        className="finestrella larga"
        role="dialog"
        aria-modal="true"
        aria-label="Regole della playlist"
        onClick={(e) => e.stopPropagation()}
      >
        <h2>
          {playlist === null
            ? "Nuova playlist intelligente"
            : `Regole di «${playlist.name}»`}
        </h2>
        <p className="nota">
          Una playlist intelligente non contiene brani: contiene una domanda. Il
          suo contenuto è la libreria filtrata <strong>adesso</strong>, e cambia
          da sé quando aggiungi musica o ascolti qualcosa — senza che nessuno la
          tocchi.
        </p>

        {errore && <div className="errore">{errore}</div>}

        {playlist === null && (
          <label>
            Nome
            <input
              type="text"
              value={nome}
              placeholder="Ascoltati poco nel 2019"
              onChange={(e) => setNome(e.target.value)}
            />
          </label>
        )}

        <div className="riga-combinazione">
          <span>Prendi i brani per cui</span>
          <select
            value={insieme.combinazione}
            onChange={(e) =>
              setInsieme((p) => ({
                ...p,
                combinazione: e.target.value as InsiemeSmart["combinazione"],
              }))
            }
          >
            <option value="tutte">valgono tutte</option>
            <option value="qualsiasi">ne vale almeno una</option>
          </select>
          <span>queste condizioni:</span>
        </div>

        <ul className="regole">
          {insieme.regole.map((regola, indice) => {
            const genere = genereDi(regola.campo);
            return (
              <li className="regola" key={indice}>
                <select
                  aria-label="Campo"
                  value={regola.campo}
                  onChange={(e) =>
                    cambiaCampo(indice, e.target.value as CampoSmart)
                  }
                >
                  {CAMPI_SMART.map((c) => (
                    <option key={c.chiave} value={c.chiave}>
                      {c.etichetta}
                    </option>
                  ))}
                </select>

                <select
                  aria-label="Operatore"
                  value={regola.operatore}
                  onChange={(e) => {
                    const operatore = e.target.value as OperatoreSmart;
                    cambia(indice, {
                      operatore,
                      ...valoreDiSerie(genere, operatore),
                    });
                  }}
                >
                  {OPERATORI_SMART[genere].map((o) => (
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
                    aria-label="Valore"
                    value={regola.testo ?? ""}
                    onChange={(e) => cambia(indice, { testo: e.target.value })}
                  />
                ) : genere === "booleano" ? (
                  <select
                    aria-label="Valore"
                    value={String(regola.numero ?? 1)}
                    onChange={(e) =>
                      cambia(indice, { numero: Number(e.target.value) })
                    }
                  >
                    <option value="1">sì</option>
                    <option value="0">no</option>
                  </select>
                ) : (
                  <input
                    type="number"
                    aria-label="Valore"
                    value={regola.numero ?? 0}
                    onChange={(e) =>
                      cambia(indice, { numero: Number(e.target.value) })
                    }
                  />
                )}

                <button
                  type="button"
                  className="tasto icon-btn"
                  aria-label="Togli questa condizione"
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

        <div className="azioni">
          <button
            type="button"
            className="bottone btn-ghost"
            onClick={() =>
              setInsieme((p) => ({ ...p, regole: [...p.regole, regolaNuova()] }))
            }
          >
            <Icona nome="i-plus" dim={15} />
            Aggiungi una condizione
          </button>
        </div>

        <div className="riga-ordine">
          <label>
            In ordine di
            <select
              value={insieme.ordinamento}
              onChange={(e) =>
                setInsieme((p) => ({
                  ...p,
                  ordinamento: e.target.value as InsiemeSmart["ordinamento"],
                }))
              }
            >
              {ORDINAMENTI_SMART.map((o) => (
                <option key={o.chiave} value={o.chiave}>
                  {o.etichetta}
                </option>
              ))}
            </select>
          </label>
          <label>
            Al massimo
            <input
              type="number"
              min={0}
              placeholder="tutti"
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
          <div className="anteprima-regole">
            <div className="quanti">
              {anteprima.prendeNiente ? (
                <strong>Nessun brano</strong>
              ) : (
                <>
                  <strong>{anteprima.quanti.toLocaleString("it")}</strong>{" "}
                  {anteprima.quanti === 1 ? "brano" : "brani"}
                </>
              )}
              {insieme.limite != null &&
                insieme.limite > 0 &&
                anteprima.quanti > insieme.limite && (
                  <span className="nota">
                    {" "}
                    · ne entrano {insieme.limite.toLocaleString("it")}
                  </span>
                )}
            </div>

            {anteprima.prendeTutto && (
              <p className="avviso">
                Nessuna condizione: questa playlist conterrebbe{" "}
                <strong>tutta la libreria</strong>. È un risultato legittimo e
                quasi mai quello voluto.
              </p>
            )}
            {anteprima.prendeNiente && (
              <p className="avviso">
                «Ne vale almeno una» senza nessuna condizione non è mai vero:
                questa playlist resterebbe vuota per sempre.
              </p>
            )}
            {insieme.regole.some((r) => !completa(r)) && (
              <p className="nota">
                Una condizione senza valore non conta ancora: scrivi cosa
                cercare e il numero qui sopra si aggiorna.
              </p>
            )}
            {anteprima.scartate > 0 && (
              <p className="avviso">
                {anteprima.scartate === 1
                  ? "Una condizione non sta in piedi ed è stata saltata"
                  : `${anteprima.scartate} condizioni non stanno in piedi e sono state saltate`}
                : succede quando il campo e l&apos;operatore non vanno
                d&apos;accordo — «anno» con «contiene», per dire.
              </p>
            )}

            {anteprima.primi.length > 0 && (
              <ol className="primi">
                {anteprima.primi.map((b) => (
                  <li key={b.id}>
                    <span className="nome">{b.title}</span>
                    <span className="autore">{b.artist}</span>
                    <span className="durata">{durata(b.durationMs)}</span>
                  </li>
                ))}
              </ol>
            )}
          </div>
        )}

        <div className="in-fondo">
          <button
            type="button"
            className="bottone btn-ghost"
            disabled={salvando}
            onClick={onChiudi}
          >
            Annulla
          </button>
          <button
            type="button"
            className="bottone primario btn-accent"
            disabled={salvando || nome.trim().length === 0}
            onClick={salva}
          >
            {salvando ? "Salvo…" : playlist === null ? "Crea" : "Salva"}
          </button>
        </div>
      </div>
    </div>
  );
}
